use self::rune_updater::RuneUpdater;
use super::*;
use crate::index::reorg::Reorg;
use crate::logs::{CRITICAL, INFO};
use crate::timestamp;
use candid::Principal;

mod rune_updater;

pub(crate) struct BlockData {
  pub(crate) header: Header,
  pub(crate) txdata: Vec<(Transaction, Txid)>,
}

impl From<Block> for BlockData {
  fn from(block: Block) -> Self {
    BlockData {
      header: block.header,
      txdata: block
        .txdata
        .into_iter()
        .map(|transaction| {
          let txid = transaction.compute_txid();
          (transaction, txid)
        })
        .collect(),
    }
  }
}

pub fn update_index(network: BitcoinNetwork, subscribers: Vec<Principal>) -> Result {
  ic_cdk_timers::set_timer(std::time::Duration::from_secs(10), move || {
    ic_cdk::spawn(async move {
      let (height, index_prev_blockhash) = crate::index::next_block(network);
      match crate::bitcoin_api::get_block_hash(network, height).await {
        Ok(Some(block_hash)) => match crate::rpc::get_block(block_hash).await {
          Ok(block) => {
            match Reorg::detect_reorg(
              network,
              index_prev_blockhash,
              block.header.prev_blockhash,
              height,
            )
            .await
            {
              Ok(()) => {
                let txids: Vec<String> = block
                  .txdata
                  .iter()
                  .map(|(_, txid)| txid.to_string())
                  .collect();
                let block_timestamp = block.header.time as u64;
                if let Err(e) = index_block(height, block).await {
                  log!(
                    CRITICAL,
                    "failed to index_block at height {}: {:?}",
                    height,
                    e
                  );
                  return;
                }
                Reorg::prune_change_record(network, height);
                for subscriber in subscribers.iter() {
                  let _ = crate::notifier::notify_new_block(
                    *subscriber,
                    height,
                    block_hash.to_string(),
                    block_timestamp,
                    txids.clone(),
                  )
                  .await;
                  log!(
                    INFO,
                    "notified subscriber: {:?} with block_height: {:?} block_hash: {:?} block_timestamp: {:?}",
                    subscriber.to_text(),
                    height,
                    block_hash,
                    block_timestamp
                  );
                }
              }
              Err(e) => match e {
                reorg::Error::Recoverable { height, depth } => {
                  Reorg::handle_reorg(height, depth);
                }
                reorg::Error::Unrecoverable => {
                  log!(
                    CRITICAL,
                    "unrecoverable reorg detected at height {}",
                    height
                  );
                  return;
                }
                reorg::Error::Retry => {
                  log!(INFO, "retry reorg detected at height {}", height);
                }
              },
            }
          }
          Err(e) => {
            log!(
              CRITICAL,
              "failed to get_block: {:?} error: {:?}",
              block_hash,
              e
            );
          }
        },
        Ok(None) => {}
        Err(e) => {
          let message = format!("failed to get_block_hash at height {}: {:?}", height, e);
          let is_new_message = CRITICAL.with_borrow(|sink| {
            sink
              .iter()
              .last()
              .map_or(true, |entry| entry.message != message)
          });

          if is_new_message {
            log!(CRITICAL, "{}", message);
          }
        }
      }
      if is_shutting_down() {
        log!(
          INFO,
          "shutting down index thread, skipping update at height {}",
          height
        );
      } else {
        let _ = update_index(network, subscribers);
      }
    });
  });

  Ok(())
}

async fn index_block(height: u32, block: BlockData) -> Result<()> {
  log!(
    INFO,
    "Block {} at {} with {} transactions…",
    height,
    timestamp(block.header.time.into()),
    block.txdata.len()
  );

  let runes = crate::index::mem_statistic_runes();
  let reserved_runes = crate::index::mem_statistic_reserved_runes();

  if height % 10 == 0 {
    log!(
      INFO,
      "Index statistics at height {}: latest_block: {:?}, reserved_runes: {}, runes: {}, rune_to_rune_id: {}, rune_entry: {}, transaction_id_to_rune: {}, outpoint_to_rune_balances: {}, outpoint_to_height: {}",
      height,
      crate::index::mem_latest_block(),
      reserved_runes,
      runes,
      crate::index::mem_length_rune_to_rune_id(),
      crate::index::mem_length_rune_id_to_rune_entry(),
      crate::index::mem_length_transaction_id_to_rune(),
      crate::index::mem_length_outpoint_to_rune_balances(),
      crate::index::mem_length_outpoint_to_height(),
    );
  }

  // init statistic runes/reserved_runes for new height
  crate::index::mem_insert_statistic_runes(height, runes);
  crate::index::mem_insert_statistic_reserved_runes(height, reserved_runes);

  let mut rune_updater = RuneUpdater {
    block_time: block.header.time,
    burned: HashMap::new(),
    height,
    minimum: Rune::minimum_at_height(bitcoin::Network::Bitcoin, Height(height)),
    runes,
    change_record: ChangeRecord::new(),
  };

  for (i, (tx, txid)) in block.txdata.iter().enumerate() {
    rune_updater
      .index_runes(u32::try_from(i).unwrap(), tx, *txid)
      .await?;
  }

  rune_updater.update()?;

  crate::index::mem_insert_block_header(height, block.header.store());

  Ok(())
}
