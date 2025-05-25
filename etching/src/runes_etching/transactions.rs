use std::borrow::Cow;
use std::str::FromStr;

use bitcoin::{Address, Amount, PublicKey, Transaction, Txid};
use candid::{CandidType, Deserialize};
use ic_canister_log::log;
use ic_stable_structures::storable::Bound;
use ic_stable_structures::Storable;

use crate::runes_etching::constants::POSTAGE;
use crate::runes_etching::error::{CallError, Reason};
use crate::runes_etching::etching_state::{init_etching_account_info, mutate_state, read_state};
use crate::runes_etching::fee_calculator::{
  check_allowance, select_utxos, transfer_etching_fees, FIXED_COMMIT_TX_VBYTES, INPUT_SIZE_VBYTES,
};
use crate::runes_etching::fees::Fees;
use crate::runes_etching::transactions::EtchingStatus::{SendCommitFailed, SendCommitSuccess};
use crate::runes_etching::wallet::builder::EtchingTransactionArgs;
use crate::runes_etching::wallet::{CreateCommitTransactionArgsV2, Runestone};
use crate::runes_etching::{
  management, EtchingArgs, InternalEtchingArgs, LogoParams, Nft, OrdResult, OrdTransactionBuilder,
  SignCommitTransactionArgs, Utxo,
};
use crate::DEFAULT_ETCHING_FEE_IN_ICP;
use common::logs::INFO;
use ordinals::{Etching, SpacedRune, Terms};
use serde::Serialize;

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct SendEtchingRequest {
  pub etching_args: InternalEtchingArgs,
  pub txs: Vec<Transaction>,
  pub err_info: Option<CallError>,
  pub commit_at: u64,
  pub reveal_at: u64,
  pub script_out_address: String,
  pub status: EtchingStatus,
}

impl From<SendEtchingRequest> for SendEtchingInfo {
  fn from(value: SendEtchingRequest) -> Self {
    let err_info = match value.err_info {
      None => "".to_string(),
      Some(e) => e.to_string(),
    };
    SendEtchingInfo {
      etching_args: value.etching_args.clone().into(),
      err_info,
      commit_txid: value.txs[0].compute_txid().to_string(),
      reveal_txid: value.txs[1].compute_txid().to_string(),
      time_at: value.commit_at,
      script_out_address: value.script_out_address,
      status: value.status,
      receiver: value.etching_args.premine_receiver,
    }
  }
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, CandidType)]
pub struct SendEtchingInfo {
  pub etching_args: EtchingArgs,
  pub commit_txid: String,
  pub reveal_txid: String,
  pub err_info: String,
  pub time_at: u64,
  pub script_out_address: String,
  pub status: EtchingStatus,
  pub receiver: String,
}

impl Storable for SendEtchingRequest {
  fn to_bytes(&self) -> std::borrow::Cow<[u8]> {
    let mut bytes = vec![];
    let _ = ciborium::ser::into_writer(self, &mut bytes);
    Cow::Owned(bytes)
  }

  fn from_bytes(bytes: std::borrow::Cow<[u8]>) -> Self {
    let dire = ciborium::de::from_reader(bytes.as_ref()).expect("failed to decode Directive");
    dire
  }

  const BOUND: Bound = Bound::Unbounded;
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq, CandidType)]
pub enum EtchingStatus {
  Initial,
  SendCommitSuccess,
  SendCommitFailed,
  SendRevealSuccess,
  SendRevealFailed,
  Final,
}

pub fn find_commit_remain_fee(t: &Transaction) -> Option<Utxo> {
  if t.output.len() > 1 {
    let r = t.output.last().cloned().unwrap();
    let utxo = Utxo {
      id: t.compute_txid(),
      index: (t.output.len() - 1) as u32,
      amount: r.value,
    };
    Some(utxo)
  } else {
    None
  }
}

pub async fn etching_rune(
  fee_rate: u64,
  args: &InternalEtchingArgs,
) -> anyhow::Result<(SendEtchingRequest, u64)> {
  let (_commit_tx_size, reveal_size) =
    estimate_tx_vbytes(args.rune_name.as_str(), args.logo.clone()).await?;
  let icp_fee_amt = read_state(|s| s.etching_fee).unwrap_or(DEFAULT_ETCHING_FEE_IN_ICP);
  let allowance = check_allowance(icp_fee_amt).await?;
  let vins = select_utxos(fee_rate, reveal_size as u64 + FIXED_COMMIT_TX_VBYTES)?;
  log!(INFO, "selected fee utxos: {:?}", vins);
  let commit_size = vins.len() as u64 * INPUT_SIZE_VBYTES + FIXED_COMMIT_TX_VBYTES;
  let fee = Fees {
    commit_fee: Amount::from_sat(commit_size * fee_rate),
    reveal_fee: Amount::from_sat(reveal_size as u64 * fee_rate + POSTAGE * 2),
  };
  let result = generate_etching_transactions(fee, vins.clone(), args)
    .await
    .map_err(|e| {
      mutate_state(|s| {
        for in_utxo in vins.clone() {
          s.etching_fee_utxos
            .push(&in_utxo)
            .expect("retire utxo failed");
        }
      });
      e
    })?;
  let mut send_res = SendEtchingRequest {
    etching_args: args.clone(),
    txs: result.txs.clone(),
    err_info: None,
    commit_at: ic_cdk::api::time(),
    reveal_at: 0,
    script_out_address: result.script_out_address.clone(),
    status: SendCommitSuccess,
  };
  if let Err(e) = management::send_etching(&result.txs[0]).await {
    send_res.status = SendCommitFailed;
    send_res.err_info = Some(e);
  }
  //修改fee utxo列表
  if send_res.status == SendCommitSuccess {
    //insert_utxo
    if let Some(u) = find_commit_remain_fee(&send_res.txs.first().cloned().unwrap()) {
      let _ = mutate_state(|s| s.etching_fee_utxos.push(&u));
    }
  } else {
    mutate_state(|s| {
      for in_utxo in vins {
        s.etching_fee_utxos
          .push(&in_utxo)
          .expect("retire utxo failed1");
      }
    });
  }
  Ok((send_res, allowance))
}

pub async fn generate_etching_transactions(
  fees: Fees,
  vins: Vec<Utxo>,
  args: &InternalEtchingArgs,
) -> anyhow::Result<BuildEtchingTxsResult> {
  let etching_account = init_etching_account_info().await;
  let sender = Address::from_str(etching_account.address.as_str())
    .unwrap()
    .assume_checked();

  let mut builder = OrdTransactionBuilder::p2tr(
    PublicKey::from_str(etching_account.pubkey.as_str()).unwrap(),
    sender.clone(),
  );
  let space_rune = SpacedRune::from_str(&args.rune_name).unwrap();
  let symbol = match args.symbol.clone() {
    None => None,
    Some(s) => {
      let cs: Vec<char> = s.chars().collect();
      cs.first().cloned()
    }
  };
  let terms = match args.terms {
    Some(t) => Some(Terms {
      amount: Some(t.amount),
      cap: Some(t.cap),
      height: t.height,
      offset: t.offset,
    }),
    None => None,
  };
  let etching = Etching {
    rune: Some(space_rune.rune.clone()),
    divisibility: args.divisibility,
    premine: args.premine,
    spacers: Some(space_rune.spacers),
    symbol,
    terms,
    turbo: args.turbo,
  };

  let mut inscription = Nft::new(None, None, args.logo.clone());
  inscription.pointer = Some(vec![]);
  inscription.rune = Some(
    etching
      .rune
      .ok_or(anyhow::anyhow!("Invalid etching data; rune is missing"))?
      .commitment(),
  );
  let commit_tx = builder
    .build_commit_transaction_with_fixed_fees(CreateCommitTransactionArgsV2 {
      inputs: vins.clone(),
      inscription,
      txin_script_pubkey: sender.script_pubkey(),
      fees,
    })
    .await?;
  let signed_commit_tx = builder
    .sign_commit_transaction(
      commit_tx.unsigned_tx,
      SignCommitTransactionArgs {
        inputs: vins,
        txin_script_pubkey: sender.script_pubkey(),
      },
    )
    .await?;
  let pointer = if args.premine.is_some() {
    Some(1)
  } else {
    None
  };
  // make runestone
  let runestone = Runestone {
    etching: Some(etching),
    edicts: vec![],
    mint: None,
    pointer,
  };

  let receipient = args.premine_receiver.clone();
  let receipient = Address::from_str(receipient.as_str())
    .unwrap()
    .assume_checked();
  let reveal_transaction = builder
    .build_etching_transaction(EtchingTransactionArgs {
      input: Utxo {
        id: signed_commit_tx.compute_txid(),
        index: 0,
        amount: commit_tx.reveal_balance,
      },
      recipient_address: receipient,
      redeem_script: commit_tx.redeem_script,
      runestone,
      derivation_path: None,
    })
    .await?;
  Ok(BuildEtchingTxsResult {
    txs: vec![signed_commit_tx, reveal_transaction],
    script_out_address: commit_tx.script_out_address,
  })
}

pub struct BuildEtchingTxsResult {
  pub txs: Vec<Transaction>,
  pub script_out_address: String,
}
pub async fn estimate_tx_vbytes(
  rune_name: &str,
  logo: Option<LogoParams>,
) -> OrdResult<(usize, usize)> {
  let fees = Fees {
    commit_fee: Amount::from_sat(1000),
    reveal_fee: Amount::from_sat(20000),
  };
  let sender = Address::from_str("bc1qyelgkxpfhfjrg6hg8hlr9t4dzn7n88eajxfy5c")
    .unwrap()
    .assume_checked();
  let vins = vec![Utxo {
    id: Txid::from_str("13a0ea6d76b710a1a9cdf2d8ce37c53feaaf985386f14ba3e65c544833c00a47").unwrap(),
    index: 1,
    amount: Amount::from_sat(1122),
  }];
  let space_rune = SpacedRune::from_str(rune_name).unwrap();

  let etching = Etching {
    rune: Some(space_rune.rune.clone()),
    divisibility: Some(2),
    premine: Some(1000000),
    spacers: Some(space_rune.spacers),
    symbol: Some('$'),
    terms: Some(Terms {
      amount: Some(100000),
      cap: Some(10000),
      height: (None, None),
      offset: (None, None),
    }),
    turbo: true,
  };

  let mut inscription = Nft::new(
    Some("text/plain;charset=utf-8".as_bytes().to_vec()),
    Some(etching.rune.unwrap().to_string().as_bytes().to_vec()),
    logo,
  );
  inscription.pointer = Some(vec![]);
  inscription.rune = Some(
    etching
      .rune
      .ok_or(anyhow::anyhow!("Invalid etching data; rune is missing"))
      .unwrap()
      .commitment(),
  );
  let mut builder = OrdTransactionBuilder::p2tr(
    PublicKey::from_str("02eec672e95d002ac6d1e8ba97a2faa9d94c6162e2f20988984106ba6265020453")
      .unwrap(), //TODO
    sender.clone(),
  );
  let commit_tx = builder
    .estimate_commit_transaction(CreateCommitTransactionArgsV2 {
      inputs: vins.clone(),
      inscription,
      txin_script_pubkey: sender.script_pubkey(),
      fees,
    })
    .await?;
  let runestone = Runestone {
    etching: Some(etching),
    edicts: vec![],
    mint: None,
    pointer: Some(1),
  };
  let reveal_transaction = builder
    .build_etching_transaction(EtchingTransactionArgs {
      input: Utxo {
        id: commit_tx.unsigned_tx.compute_txid(),
        index: 0,
        amount: commit_tx.reveal_balance,
      },
      recipient_address: sender,
      redeem_script: commit_tx.redeem_script,
      runestone,
      derivation_path: None,
    })
    .await?;
  Ok((commit_tx.unsigned_tx.vsize(), reveal_transaction.vsize()))
}

pub async fn internal_etching(args: EtchingArgs) -> Result<String, String> {
  let fee_rate = read_state(|s| {
    let high = s.bitcoin_fee_rate.high;
    if high == 0 {
      5
    } else {
      high
    }
  });
  let _ = SpacedRune::from_str(args.rune_name.as_str()).map_err(|e| e.to_string())?;
  args.check().map_err(|e| e.to_string())?;
  let internal_args: InternalEtchingArgs = args;
  let r = etching_rune(fee_rate, &internal_args).await;
  match r {
    Ok((sr, allowance)) => {
      if sr.status == SendCommitSuccess {
        let commit_tx_id = sr.txs[0].compute_txid().to_string();
        mutate_state(|s| s.pending_etching_requests.insert(commit_tx_id.clone(), sr));
        let r = transfer_etching_fees(allowance as u128).await;
        log!(INFO, "transfer etching fee result: {:?}", r);
        Ok(commit_tx_id)
      } else {
        Err(
          sr.err_info
            .unwrap_or(CallError {
              method: "send commit tx".to_string(),
              reason: Reason::QueueIsFull,
            })
            .to_string(),
        )
      }
    }
    Err(e) => Err(e.to_string()),
  }
}
