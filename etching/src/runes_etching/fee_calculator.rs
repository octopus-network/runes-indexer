use std::str::FromStr;

use crate::runes_etching::constants::POSTAGE;
use crate::runes_etching::etching_state::mutate_state;
use crate::runes_etching::Utxo;
use anyhow::anyhow;
use candid::{Nat, Principal};
use common::logs::INFO;
use ic_canister_log::log;
use ic_cdk::{caller, id};
use icrc_ledger_client_cdk::{CdkRuntime, ICRC1Client};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc2::allowance::{Allowance, AllowanceArgs};
use icrc_ledger_types::icrc2::transfer_from::TransferFromArgs;
use num_traits::ToPrimitive;

const ICP_LEDGER_CANISTER_ID: &str = "ryjl3-tyaaa-aaaaa-aaaba-cai";
pub const INPUT_SIZE_VBYTES: u64 = 74;
pub const OUTPUT_SIZE_VBYTES: u64 = 31;
pub const TX_OVERHEAD_VBYTES: u64 = 21;
pub const FIXED_COMMIT_TX_VBYTES: u64 = OUTPUT_SIZE_VBYTES * 2 + TX_OVERHEAD_VBYTES;
pub const MAX_LOGO_CONTENT_SIZE: usize = 65536 * 2; //64K*2
pub fn select_utxos(fee_rate: u64, fixed_size: u64) -> anyhow::Result<Vec<Utxo>> {
  let mut selected_utxos: Vec<Utxo> = vec![];
  let mut selected_amount = 0u64;
  let mut estimate_size = fixed_size;
  mutate_state(|s| loop {
    if selected_amount >= fee_rate * estimate_size + POSTAGE * 2 {
      return Ok(selected_utxos);
    }
    let u = s.etching_fee_utxos.pop();
    match u {
      None => {
        return Err(anyhow!("InsufficientFunds"));
      }
      Some(utxo) => {
        selected_amount += utxo.amount.to_sat();
        selected_utxos.push(utxo);
        estimate_size += INPUT_SIZE_VBYTES;
      }
    }
  })
}

pub async fn check_allowance(fee_amt: u64) -> anyhow::Result<u64> {
  let allowance = allowance(caller()).await?;
  let allx = allowance.allowance.0.to_u64().unwrap_or_default();
  log!(
    INFO,
    "query allowance result: {}, {}",
    caller().to_text(),
    allx
  );
  if allx < fee_amt {
    return Err(anyhow!(format!(
      "InsufficientFee: required: {}, provided: {}",
      fee_amt, allx
    )));
  }
  if allowance.expires_at.is_some() {
    return Err(anyhow!("allowance is expired".to_string()));
  }
  Ok(allx)
}

pub async fn transfer_etching_fees(amount: u128) -> anyhow::Result<()> {
  let canister = Principal::from_str(ICP_LEDGER_CANISTER_ID).unwrap();
  let client = ICRC1Client {
    runtime: CdkRuntime,
    ledger_canister_id: canister,
  };
  let fee = client
    .fee()
    .await
    .map_err(|e| anyhow!(format!("Failed to get icrc fee, error: {:?}", e).to_string(),))?;
  let user = Account {
    owner: caller(),
    subaccount: None,
  };
  let transfer_amount = Nat::from(amount) - fee;
  let result = client
    .transfer_from(TransferFromArgs {
      spender_subaccount: None,
      from: user,
      to: Account {
        owner: id(),
        subaccount: None,
      },
      amount: transfer_amount.clone(),
      fee: None,
      memo: None,
      created_at_time: Some(ic_cdk::api::time()),
    })
    .await
    .map_err(|(code, msg)| {
      anyhow!(format!(
        "cannot transfer_icp transaction: {} (reject_code = {})",
        msg, code
      ))
    })?;

  match result {
    Ok(_) => Ok(()),
    Err(e) => Err(anyhow!(format!("transfer fee error{:?}", e))),
  }
}

pub async fn allowance(acct: Principal) -> anyhow::Result<Allowance> {
  let canister = Principal::from_str(ICP_LEDGER_CANISTER_ID).unwrap();
  let allowance: (Allowance,) = ic_cdk::call(
    canister,
    "icrc2_allowance",
    (AllowanceArgs {
      account: Account {
        owner: acct,
        subaccount: None,
      },
      spender: Account {
        owner: ic_cdk::id(),
        subaccount: None,
      },
    },),
  )
  .await
  .map_err(|e| anyhow!(e.1))?;
  Ok(allowance.0)
}

#[test]
pub fn test() {
  let pr =
    Principal::from_str("njasx-txxdl-dmy3b-rjiqe-bwpih-cqhsx-4352l-z3qic-uuumr-jbges-rqe").unwrap();
  println!("{}", pr.to_text());
}
