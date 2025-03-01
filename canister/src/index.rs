use self::entry::{Entry, RuneEntry};
use self::lot::Lot;
use super::Result;
use crate::config::Config;
use crate::index::entry::{
  ChangeRecord, HeaderValue, OutPointValue, RuneBalances, RuneIdValue, TxidValue,
};
use crate::logs::{CRITICAL, INFO};
use anyhow::anyhow;
use bitcoin::{
  block::Header,
  blockdata::constants::SUBSIDY_HALVING_INTERVAL,
  consensus::{self, Decodable, Encodable},
  hash_types::BlockHash,
  hashes::Hash,
  Block, OutPoint, Transaction, Txid,
};
use ic_canister_log::log;
use ic_cdk::api::management_canister::bitcoin::BitcoinNetwork;
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{DefaultMemoryImpl, StableBTreeMap, StableCell};
use ordinals::{
  Artifact, Edict, Etching, Height, Pile, Rune, RuneId, Runestone, SatPoint, SpacedRune, Terms,
};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{self, AtomicBool};

pub mod entry;
mod lot;
mod reorg;
pub mod updater;

type Memory = VirtualMemory<DefaultMemoryImpl>;

thread_local! {
  static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> =
      RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));

  static CONFIG: RefCell<StableCell<Config, Memory>> = RefCell::new(
      StableCell::init(
          MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(0))),
          Config::default()
      ).unwrap()
  );

  static HEIGHT_TO_BLOCK_HEADER: RefCell<StableBTreeMap<u32, HeaderValue, Memory>> = RefCell::new(
      StableBTreeMap::init(
          MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(1))),
      )
  );

  static HEIGHT_TO_STATISTIC_RESERVED_RUNES: RefCell<StableBTreeMap<u32, u64, Memory>> = RefCell::new(
      StableBTreeMap::init(
          MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(2))),
      )
  );

  static HEIGHT_TO_STATISTIC_RUNES: RefCell<StableBTreeMap<u32, u64, Memory>> = RefCell::new(
      StableBTreeMap::init(
          MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(3))),
      )
  );

  static OUTPOINT_TO_RUNE_BALANCES: RefCell<StableBTreeMap<OutPointValue, RuneBalances, Memory>> = RefCell::new(
      StableBTreeMap::init(
          MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(4))),
      )
  );

  static RUNE_ID_TO_RUNE_ENTRY: RefCell<StableBTreeMap<RuneIdValue, RuneEntry, Memory>> = RefCell::new(
      StableBTreeMap::init(
          MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(5))),
      )
  );

  static RUNE_TO_RUNE_ID: RefCell<StableBTreeMap<u128, RuneIdValue, Memory>> = RefCell::new(
      StableBTreeMap::init(
          MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(6))),
      )
  );

  static TRANSACTION_ID_TO_RUNE: RefCell<StableBTreeMap<TxidValue, u128, Memory>> = RefCell::new(
      StableBTreeMap::init(
          MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(7))),
      )
  );

  static OUTPOINT_TO_HEIGHT: RefCell<StableBTreeMap<OutPointValue, u32, Memory>> = RefCell::new(
      StableBTreeMap::init(
          MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(8))),
      )
  );

  static HEIGHT_TO_CHANGE_RECORD: RefCell<StableBTreeMap<u32, ChangeRecord, Memory>> = RefCell::new(
      StableBTreeMap::init(
          MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(9))),
      )
  );
}

static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

pub fn shut_down() {
  SHUTTING_DOWN.store(true, atomic::Ordering::Relaxed);
}

pub fn cancel_shutdown() {
  SHUTTING_DOWN.store(false, atomic::Ordering::Relaxed);
}

pub fn is_shutting_down() -> bool {
  SHUTTING_DOWN.load(atomic::Ordering::Relaxed)
}

pub fn mem_get_config() -> Config {
  CONFIG.with(|m| m.borrow().get().clone())
}

pub fn mem_set_config(config: Config) -> Result<Config> {
  CONFIG
    .with(|m| m.borrow_mut().set(config))
    .map_err(|e| anyhow::anyhow!("Failed to set config: {:?}", e))
}

pub fn mem_latest_block() -> Option<(u32, BlockHash)> {
  HEIGHT_TO_BLOCK_HEADER.with(|m| {
    m.borrow()
      .iter()
      .rev()
      .next()
      .map(|(height, header_value)| {
        let header = Header::load(header_value);
        (height, header.block_hash())
      })
  })
}

pub fn mem_latest_block_height() -> Option<u32> {
  HEIGHT_TO_BLOCK_HEADER.with(|m| m.borrow().iter().rev().next().map(|(height, _)| height))
}

pub fn mem_block_hash(height: u32) -> Option<BlockHash> {
  HEIGHT_TO_BLOCK_HEADER.with(|m| {
    m.borrow()
      .get(&height)
      .map(|header_value| Header::load(header_value).block_hash())
  })
}

pub fn mem_insert_block_header(height: u32, header_value: HeaderValue) {
  HEIGHT_TO_BLOCK_HEADER.with(|m| m.borrow_mut().insert(height, header_value));
}

pub fn mem_remove_block_header(height: u32) -> Option<HeaderValue> {
  HEIGHT_TO_BLOCK_HEADER.with(|m| m.borrow_mut().remove(&height))
}

pub fn mem_prune_block_header(height: u32) {
  HEIGHT_TO_BLOCK_HEADER.with(|m| {
    let mut map = m.borrow_mut();
    let keys_to_remove: Vec<u32> = map
      .iter()
      .take_while(|(h, _)| *h <= height)
      .map(|(h, _)| h)
      .collect();
    for key in keys_to_remove {
      map.remove(&key);
    }
  });
}

pub fn mem_statistic_reserved_runes() -> u64 {
  HEIGHT_TO_STATISTIC_RESERVED_RUNES.with(|m| {
    m.borrow()
      .iter()
      .rev()
      .next()
      .map(|(_, runes)| runes)
      .unwrap_or(0)
  })
}

pub fn mem_insert_statistic_reserved_runes(height: u32, runes: u64) {
  HEIGHT_TO_STATISTIC_RESERVED_RUNES.with(|m| m.borrow_mut().insert(height, runes));
}

pub fn mem_remove_statistic_reserved_runes(height: u32) -> Option<u64> {
  HEIGHT_TO_STATISTIC_RESERVED_RUNES.with(|m| m.borrow_mut().remove(&height))
}

pub fn mem_prune_statistic_reserved_runes(height: u32) {
  HEIGHT_TO_STATISTIC_RESERVED_RUNES.with(|m| {
    let mut map = m.borrow_mut();
    let keys_to_remove: Vec<u32> = map
      .iter()
      .take_while(|(h, _)| *h <= height)
      .map(|(h, _)| h)
      .collect();
    for key in keys_to_remove {
      map.remove(&key);
    }
  });
}

pub fn mem_statistic_runes() -> u64 {
  HEIGHT_TO_STATISTIC_RUNES.with(|m| {
    m.borrow()
      .iter()
      .rev()
      .next()
      .map(|(_, runes)| runes)
      .unwrap_or(0)
  })
}

pub fn mem_insert_statistic_runes(height: u32, runes: u64) {
  HEIGHT_TO_STATISTIC_RUNES.with(|m| m.borrow_mut().insert(height, runes));
}

pub fn mem_remove_statistic_runes(height: u32) -> Option<u64> {
  HEIGHT_TO_STATISTIC_RUNES.with(|m| m.borrow_mut().remove(&height))
}

pub fn mem_prune_statistic_runes(height: u32) {
  HEIGHT_TO_STATISTIC_RUNES.with(|m| {
    let mut map = m.borrow_mut();
    // Get all keys less or equal than height
    let keys_to_remove: Vec<u32> = map
      .iter()
      .take_while(|(h, _)| *h <= height)
      .map(|(h, _)| h)
      .collect();

    // Remove all entries with those keys
    for key in keys_to_remove {
      map.remove(&key);
    }

    map.remove(&height)
  });
}

pub fn mem_length_outpoint_to_rune_balances() -> u64 {
  OUTPOINT_TO_RUNE_BALANCES.with(|m| m.borrow().len())
}

pub fn mem_get_outpoint_to_rune_balances(outpoint_value: OutPointValue) -> Option<RuneBalances> {
  OUTPOINT_TO_RUNE_BALANCES.with(|m| m.borrow().get(&outpoint_value))
}

pub fn mem_insert_outpoint_to_rune_balances(
  outpoint_value: OutPointValue,
  rune_balances: RuneBalances,
) {
  OUTPOINT_TO_RUNE_BALANCES.with(|m| m.borrow_mut().insert(outpoint_value, rune_balances));
}

pub(crate) fn mem_remove_outpoint_to_rune_balances(
  outpoint_value: OutPointValue,
) -> Option<RuneBalances> {
  OUTPOINT_TO_RUNE_BALANCES.with(|m| m.borrow_mut().remove(&outpoint_value))
}

pub fn mem_length_rune_id_to_rune_entry() -> u64 {
  RUNE_ID_TO_RUNE_ENTRY.with(|m| m.borrow().len())
}

pub fn mem_get_rune_id_to_rune_entry(rune_id_value: RuneIdValue) -> Option<RuneEntry> {
  RUNE_ID_TO_RUNE_ENTRY.with(|m| m.borrow().get(&rune_id_value))
}

pub fn mem_insert_rune_id_to_rune_entry(rune_id_value: RuneIdValue, rune_entry: RuneEntry) {
  RUNE_ID_TO_RUNE_ENTRY.with(|m| m.borrow_mut().insert(rune_id_value, rune_entry));
}

pub(crate) fn mem_remove_rune_id_to_rune_entry(rune_id_value: RuneIdValue) -> Option<RuneEntry> {
  RUNE_ID_TO_RUNE_ENTRY.with(|m| m.borrow_mut().remove(&rune_id_value))
}

pub fn mem_length_rune_to_rune_id() -> u64 {
  RUNE_TO_RUNE_ID.with(|m| m.borrow().len())
}

pub fn mem_get_rune_to_rune_id(rune: u128) -> Option<RuneIdValue> {
  RUNE_TO_RUNE_ID.with(|m| m.borrow().get(&rune))
}

pub fn mem_insert_rune_to_rune_id(rune: u128, rune_id_value: RuneIdValue) {
  RUNE_TO_RUNE_ID.with(|m| m.borrow_mut().insert(rune, rune_id_value));
}

pub(crate) fn mem_remove_rune_to_rune_id(rune: u128) -> Option<RuneIdValue> {
  RUNE_TO_RUNE_ID.with(|m| m.borrow_mut().remove(&rune))
}

pub fn mem_length_transaction_id_to_rune() -> u64 {
  TRANSACTION_ID_TO_RUNE.with(|m| m.borrow().len())
}

pub fn mem_insert_transaction_id_to_rune(txid: TxidValue, rune: u128) {
  TRANSACTION_ID_TO_RUNE.with(|m| m.borrow_mut().insert(txid, rune));
}

pub(crate) fn mem_remove_transaction_id_to_rune(txid: TxidValue) -> Option<u128> {
  TRANSACTION_ID_TO_RUNE.with(|m| m.borrow_mut().remove(&txid))
}

pub fn mem_length_outpoint_to_height() -> u64 {
  OUTPOINT_TO_HEIGHT.with(|m| m.borrow().len())
}

pub fn mem_get_outpoint_to_height(outpoint: OutPointValue) -> Option<u32> {
  OUTPOINT_TO_HEIGHT.with(|m| m.borrow().get(&outpoint))
}

pub fn mem_insert_outpoint_to_height(outpoint: OutPointValue, height: u32) {
  OUTPOINT_TO_HEIGHT.with(|m| m.borrow_mut().insert(outpoint, height));
}

pub(crate) fn mem_remove_outpoint_to_height(outpoint_value: OutPointValue) -> Option<u32> {
  OUTPOINT_TO_HEIGHT.with(|m| m.borrow_mut().remove(&outpoint_value))
}

pub fn mem_length_change_record() -> u64 {
  HEIGHT_TO_CHANGE_RECORD.with(|m| m.borrow().len())
}

pub(crate) fn mem_insert_change_record(height: u32, change_record: ChangeRecord) {
  HEIGHT_TO_CHANGE_RECORD.with(|m| m.borrow_mut().insert(height, change_record));
}

pub(crate) fn mem_get_change_record(height: u32) -> Option<ChangeRecord> {
  HEIGHT_TO_CHANGE_RECORD.with(|m| m.borrow().get(&height))
}

pub(crate) fn mem_remove_change_record(height: u32) -> Option<ChangeRecord> {
  HEIGHT_TO_CHANGE_RECORD.with(|m| m.borrow_mut().remove(&height))
}

pub fn mem_prune_change_record(height: u32) {
  HEIGHT_TO_CHANGE_RECORD.with(|m| {
    let mut map = m.borrow_mut();
    let keys_to_remove: Vec<u32> = map
      .iter()
      .take_while(|(h, _)| *h <= height)
      .map(|(h, _)| h)
      .collect();
    for key in keys_to_remove {
      map.remove(&key);
    }
  });
}

pub fn mem_get_etching(txid: Txid) -> Option<(RuneId, RuneEntry)> {
  TRANSACTION_ID_TO_RUNE.with(|m| {
    m.borrow()
      .get(&Txid::store(txid))
      .and_then(|rune| RUNE_TO_RUNE_ID.with(|m| m.borrow().get(&rune)))
      .and_then(|id| {
        RUNE_ID_TO_RUNE_ENTRY.with(|m| m.borrow().get(&id).map(|e| (RuneId::load(id), e)))
      })
  })
}

pub fn init_mainnet() {
  let rune = Rune(2055900680524219742);

  let id = RuneId { block: 1, tx: 0 };
  let etching = Txid::all_zeros();

  mem_insert_rune_to_rune_id(rune.store(), id.store());
  mem_insert_statistic_runes(1, 1);

  mem_insert_rune_id_to_rune_entry(
    id.store(),
    RuneEntry {
      block: id.block,
      burned: 0,
      divisibility: 0,
      etching,
      terms: Some(Terms {
        amount: Some(1),
        cap: Some(u128::MAX),
        height: (
          Some((SUBSIDY_HALVING_INTERVAL * 4).into()),
          Some((SUBSIDY_HALVING_INTERVAL * 5).into()),
        ),
        offset: (None, None),
      }),
      mints: 0,
      number: 0,
      premine: 0,
      spaced_rune: SpacedRune { rune, spacers: 128 },
      symbol: Some('\u{29C9}'),
      timestamp: 0,
      turbo: true,
    },
  );

  mem_insert_transaction_id_to_rune(etching.store(), rune.store());
}

pub fn next_block(network: BitcoinNetwork) -> (u32, Option<BlockHash>) {
  mem_latest_block()
    .map(|(height, prev_blockhash)| (height + 1, Some(prev_blockhash)))
    .unwrap_or(match network {
      BitcoinNetwork::Mainnet => (Rune::first_rune_height(bitcoin::Network::Bitcoin), None),
      BitcoinNetwork::Testnet => (Rune::first_rune_height(bitcoin::Network::Testnet4), None),
      BitcoinNetwork::Regtest => (Rune::first_rune_height(bitcoin::Network::Regtest), None),
    })
}
