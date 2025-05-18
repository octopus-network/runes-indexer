use std::collections::BTreeMap;
use std::time::Duration;

use candid::CandidType;
use ic_canister_log::log;
use ic_cdk::api::management_canister::bitcoin::BitcoinNetwork;
use runes_indexer_interface::GetEtchingResult;
use serde::Deserialize;
use thiserror::Error;

use crate::runes_etching::management::get_bitcoin_balance;
use crate::runes_etching::transactions::EtchingStatus::{
  Final, Initial, SendCommitFailed, SendRevealFailed, SendRevealSuccess,
};
use crate::runes_etching::transactions::{EtchingStatus, SendEtchingRequest};

use crate::runes_etching::etching_state::{mutate_state, no_initial, read_state};
use crate::runes_etching::management::send_etching;
use crate::{MIN_NANOS, SEC_NANOS};
use common::logs::INFO;

#[derive(Debug, Eq, PartialEq, Error, CandidType, Deserialize)]
enum OrdError {
  #[error("params: {0}")]
  Params(String),
  #[error("overflow")]
  Overflow,
  #[error("wrong block hash: {0}")]
  WrongBlockHash(String),
  #[error("wrong block merkle root: {0}")]
  WrongBlockMerkleRoot(String),
  #[error("index error: {0}")]
  Index(#[from] MintError),
  #[error("rpc error: {0}")]
  Rpc(#[from] RpcError),
  #[error("recoverable reorg at height {height} with depth {depth}")]
  Recoverable { height: u32, depth: u32 },
  #[error("unrecoverable reorg")]
  Unrecoverable,
  #[error("outpoint not found")]
  OutPointNotFound,
  #[error("not enough confirmations")]
  NotEnoughConfirmations,
}

#[derive(Debug, Clone, Error, Eq, PartialEq, CandidType, Deserialize)]
pub enum RpcError {
  #[error("IO error occured while calling {0} onto {1} due to {2}.")]
  Io(String, String, String),
  #[error("Decoding response of {0} from {1} failed due to {2}.")]
  Decode(String, String, String),
  #[error("Received an error of endpoint {0} from {1}: {2}.")]
  Endpoint(String, String, String),
}

#[derive(Debug, Clone, Error, Eq, PartialEq, CandidType, Deserialize)]
pub enum MintError {
  #[error("limited to {0} mints")]
  Cap(u128),
  #[error("mint ended on block {0}")]
  End(u64),
  #[error("mint starts on block {0}")]
  Start(u64),
  #[error("not mintable")]
  Unmintable,
}

fn check_time(confirmation_blocks: u32, req_time: u64) -> bool {
  let now = ic_cdk::api::time();
  let network = read_state(|s| s.btc_network);
  let wait_time = finalization_time_estimate(confirmation_blocks, network);
  let check_timeline = req_time + (wait_time.as_nanos() as u64);
  let check_time_window = Duration::from_secs(21600).as_nanos() as u64;
  check_timeline < now && now < check_timeline + check_time_window
}

fn finalization_time_estimate(min_confirmations: u32, network: BitcoinNetwork) -> Duration {
  Duration::from_nanos(
    min_confirmations as u64
      * match network {
        BitcoinNetwork::Mainnet => 7 * MIN_NANOS,
        BitcoinNetwork::Testnet => MIN_NANOS,
        BitcoinNetwork::Regtest => SEC_NANOS,
      },
  )
}

pub async fn handle_etching_result_task(f: impl Fn(String) -> Option<GetEtchingResult>) {
  if no_initial() {
    return;
  }
  if read_state(|s| s.pending_etching_requests.is_empty()) {
    return;
  }
  let network = read_state(|s| s.btc_network);
  let kvs = read_state(|s| {
    s.pending_etching_requests
      .iter()
      .collect::<BTreeMap<String, SendEtchingRequest>>()
  });
  for (k, mut req) in kvs {
    match req.status.clone() {
      EtchingStatus::SendCommitSuccess => {
        if !check_time(4, req.commit_at) {
          continue;
        }
        let balance = get_bitcoin_balance(network, &req.script_out_address, 6)
          .await
          .unwrap_or_default();
        if balance == 0 {
          continue;
        }
        let r = send_etching(&req.txs[1]).await;
        if r.is_err() {
          req.status = SendRevealFailed;
          req.err_info = r.err();
        } else {
          req.status = SendRevealSuccess
        }
        req.reveal_at = ic_cdk::api::time();
        mutate_state(|s| s.pending_etching_requests.insert(k, req));
      }
      SendRevealSuccess => {
        if !check_time(1, req.reveal_at) {
          continue;
        }
        let tx = req.txs[1].compute_txid().to_string();
        let rune = f(tx.clone());
        match rune {
          None => {}
          Some(resp) => {
            mutate_state(|s| {
              req.status = Final;
              s.finalized_etching_requests.insert(k.clone(), req);
            });
            mutate_state(|s| s.pending_etching_requests.remove(&k));
            log!(
              INFO,
              "Etching result:  {}.{}, {}",
              tx,
              resp.rune_id.clone(),
              resp.confirmations
            );
          }
        }
      }
      Final | SendCommitFailed | SendRevealFailed | Initial => {}
    }
  }
}
