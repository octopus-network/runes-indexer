use crate::logs::CRITICAL;
use candid::{self, CandidType, Principal};
use ic_canister_log::log;

type BlockHeight = u32;

#[derive(CandidType)]
pub struct NewBlockRequest {
  pub block_height: BlockHeight,
  pub block_hash: String,
  pub block_timestamp: u64,
  pub tx_ids: Vec<String>,
}

pub async fn notify_new_block(
  canister_id: Principal,
  block_height: u32,
  block_hash: String,
  block_timestamp: u64,
  tx_ids: Vec<String>,
) -> crate::Result<()> {
  let req = NewBlockRequest {
    block_height,
    block_hash,
    block_timestamp,
    tx_ids,
  };

  if let Err(e) = ic_cdk::call::<_, ()>(canister_id, "new_block_detected", (req,)).await {
    log!(CRITICAL, "failed to notify new block: {:?}", e);
  }
  Ok(())
}
