use crate::index::entry::Entry;
use crate::index::{CRITICAL, INFO};
use bitcoin::block::BlockHash;
use ic_canister_log::log;
use ic_cdk::api::management_canister::bitcoin::BitcoinNetwork;
use std::fmt::{self, Display, Formatter};

#[derive(Debug, PartialEq)]
pub(crate) enum Error {
  Recoverable { height: u32, depth: u32 },
  Unrecoverable,
  Retry,
}

impl Display for Error {
  fn fmt(&self, f: &mut Formatter) -> fmt::Result {
    match self {
      Self::Recoverable { height, depth } => {
        write!(f, "{depth} block deep reorg detected at height {height}")
      }
      Self::Unrecoverable => write!(f, "unrecoverable reorg detected"),
      Self::Retry => write!(f, "retry reorg detected"),
    }
  }
}

impl std::error::Error for Error {}

pub fn get_max_recoverable_reorg_depth(network: BitcoinNetwork) -> u32 {
  match network {
    BitcoinNetwork::Regtest => 6,
    BitcoinNetwork::Testnet => 64,
    BitcoinNetwork::Mainnet => 6,
  }
}

pub struct Reorg {}

impl Reorg {
  pub(crate) async fn detect_reorg(
    network: BitcoinNetwork,
    index_prev_blockhash: Option<BlockHash>,
    bitcoind_prev_blockhash: BlockHash,
    height: u32,
  ) -> Result<(), Error> {
    match index_prev_blockhash {
      Some(index_prev_blockhash) if index_prev_blockhash == bitcoind_prev_blockhash => Ok(()),
      Some(index_prev_blockhash) if index_prev_blockhash != bitcoind_prev_blockhash => {
        for depth in 1..=get_max_recoverable_reorg_depth(network) {
          let check_height = height.checked_sub(depth).ok_or_else(|| {
            log!(CRITICAL, "Height overflow at depth {}", depth);
            Error::Unrecoverable
          })?;

          let index_block_hash = crate::index::mem_block_hash(check_height).ok_or_else(|| {
            log!(
              CRITICAL,
              "Missing index block hash at height {}",
              check_height
            );
            Error::Unrecoverable
          })?;

          match crate::bitcoin_api::get_block_hash(network, check_height).await {
            Ok(Some(bitcoin_block_hash)) => {
              if index_block_hash == bitcoin_block_hash {
                log!(
                  INFO,
                  "Found common ancestor at height {} (depth {})",
                  check_height,
                  depth
                );
                return Err(Error::Recoverable { height, depth });
              }
            }
            _ => {
              log!(
                INFO,
                "The block hash at height {} is not found. Retrying.",
                check_height
              );
              return Err(Error::Retry);
            }
          }
        }

        log!(
          CRITICAL,
          "No common ancestor found within recoverable depth"
        );
        Err(Error::Unrecoverable)
      }
      _ => Ok(()),
    }
  }

  pub fn handle_reorg(height: u32, depth: u32) {
    log!(
      INFO,
      "rolling back state after reorg of depth {depth} at height {height}"
    );

    for h in (height - depth + 1..height).rev() {
      log!(INFO, "rolling back change record at height {h}");
      if let Some(change_record) = crate::index::mem_get_change_record(h) {
        change_record
          .removed_outpoints
          .iter()
          .for_each(|(outpoint, rune_balances, height)| {
            crate::index::mem_insert_outpoint_to_rune_balances(
              outpoint.store(),
              rune_balances.clone(),
            );
            crate::index::mem_insert_outpoint_to_height(outpoint.store(), *height);
          });
        change_record.added_outpoints.iter().for_each(|outpoint| {
          crate::index::mem_remove_outpoint_to_rune_balances(outpoint.store());
          crate::index::mem_remove_outpoint_to_height(outpoint.store());
        });
        change_record.burned.iter().for_each(|(rune_id, amount)| {
          let mut entry = crate::index::mem_get_rune_id_to_rune_entry(rune_id.store()).unwrap();
          entry.burned = *amount;
          crate::index::mem_insert_rune_id_to_rune_entry(rune_id.store(), entry);
          log!(
            INFO,
            "resetting burned for rune_id: {} to {}",
            rune_id,
            amount
          );
        });
        change_record.mints.iter().for_each(|(rune_id, amount)| {
          let mut entry = crate::index::mem_get_rune_id_to_rune_entry(rune_id.store()).unwrap();
          entry.mints = *amount;
          crate::index::mem_insert_rune_id_to_rune_entry(rune_id.store(), entry);
          log!(
            INFO,
            "resetting mints for rune_id: {} to {}",
            rune_id,
            amount
          );
        });
        change_record
          .added_runes
          .iter()
          .for_each(|(rune, rune_id, txid)| {
            crate::index::mem_remove_rune_to_rune_id(rune.store());
            crate::index::mem_remove_rune_id_to_rune_entry(rune_id.store());
            crate::index::mem_remove_transaction_id_to_rune(txid.store());
            log!(INFO, "removing rune_id: {}", rune_id);
          });
      }
      crate::index::mem_remove_change_record(h);
      crate::index::mem_remove_statistic_runes(h);
      crate::index::mem_remove_statistic_reserved_runes(h);
      crate::index::mem_remove_block_header(h);
    }

    log!(
      INFO,
      "successfully rolled back state to height {}",
      height - depth,
    );
  }

  pub(crate) fn prune_change_record(network: BitcoinNetwork, height: u32) {
    if height >= get_max_recoverable_reorg_depth(network) {
      let h = height - get_max_recoverable_reorg_depth(network);
      log!(INFO, "clearing change record at height {h}");
      crate::index::mem_prune_change_record(h);
      crate::index::mem_prune_statistic_runes(h);
      crate::index::mem_prune_statistic_reserved_runes(h);
      crate::index::mem_prune_block_header(h);
    }
  }
}
