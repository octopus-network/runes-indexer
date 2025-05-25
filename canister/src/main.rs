use bitcoin::{Amount, OutPoint};
use candid::{candid_method, Principal};
use common::logs::{CRITICAL, INFO, WARNING};
use etching::runes_etching::etching_state::{
  init_etching_account_info, mutate_state, update_bitcoin_fee_rate,
};
use etching::runes_etching::etching_state::{
  no_initial, read_state, replace_state, EtchingState, EtchingUpgradeArgs,
};
use etching::runes_etching::guard::RequestEtchingGuard;
use etching::runes_etching::transactions::{internal_etching, SendEtchingInfo};
use etching::runes_etching::types::{EtchingAccountInfo, SetTxFeePerVbyteArgs, UtxoArgs};
use etching::runes_etching::{EtchingArgs, Utxo};
use etching::DEFAULT_ETCHING_FEE_IN_ICP;
use ic_canister_log::log;
use ic_cdk::api::management_canister::http_request::{HttpResponse, TransformArgs};
use ic_cdk::caller;
use ic_cdk_macros::{init, post_upgrade, pre_upgrade, query, update};
use ic_cdk_timers::set_timer_interval;
use runes_indexer::config::RunesIndexerArgs;
use runes_indexer::etchin_tasks::process_etching_task;
use runes_indexer::index::entry::Entry;
use runes_indexer_interface::{Error, GetEtchingResult, RuneBalance, RuneEntry, Terms};
use std::str::FromStr;
use std::time::Duration;

pub const MAX_OUTPOINTS: usize = 256;

#[query]
#[candid_method(query)]
pub fn get_latest_block() -> (u32, String) {
  let (height, hash) = runes_indexer::index::mem_latest_block().expect("No block found");
  (height, hash.to_string())
}

#[query]
#[candid_method(query)]
pub fn get_etching(txid: String) -> Option<GetEtchingResult> {
  runes_indexer::index::get_etching(txid)
}

#[query]
#[candid_method(query)]
pub fn get_rune(str_spaced_rune: String) -> Option<RuneEntry> {
  let spaced_rune = ordinals::SpacedRune::from_str(&str_spaced_rune).ok()?;
  let rune_id_value = runes_indexer::index::mem_get_rune_to_rune_id(spaced_rune.rune.0)?;
  let rune_entry = runes_indexer::index::mem_get_rune_id_to_rune_entry(rune_id_value)?;
  let cur_height = runes_indexer::index::mem_latest_block_height().expect("No block height found");
  Some(RuneEntry {
    confirmations: cur_height - rune_entry.block as u32 + 1,
    rune_id: ordinals::RuneId::load(rune_id_value).to_string(),
    block: rune_entry.block,
    burned: rune_entry.burned,
    divisibility: rune_entry.divisibility,
    etching: rune_entry.etching.to_string(),
    mints: rune_entry.mints,
    number: rune_entry.number,
    premine: rune_entry.premine,
    spaced_rune: rune_entry.spaced_rune.to_string(),
    symbol: rune_entry.symbol.map(|c| c.to_string()),
    terms: rune_entry.terms.map(|t| Terms {
      amount: t.amount,
      cap: t.cap,
      height: t.height,
      offset: t.offset,
    }),
    timestamp: rune_entry.timestamp,
    turbo: rune_entry.turbo,
  })
}

#[query]
#[candid_method(query)]
pub fn get_rune_by_id(str_rune_id: String) -> Option<RuneEntry> {
  let rune_id = ordinals::RuneId::from_str(&str_rune_id).ok()?;
  let rune_entry = runes_indexer::index::mem_get_rune_id_to_rune_entry(rune_id.store())?;
  let cur_height = runes_indexer::index::mem_latest_block_height().expect("No block height found");
  Some(RuneEntry {
    confirmations: cur_height - rune_entry.block as u32 + 1,
    rune_id: str_rune_id,
    block: rune_entry.block,
    burned: rune_entry.burned,
    divisibility: rune_entry.divisibility,
    etching: rune_entry.etching.to_string(),
    mints: rune_entry.mints,
    number: rune_entry.number,
    premine: rune_entry.premine,
    spaced_rune: rune_entry.spaced_rune.to_string(),
    symbol: rune_entry.symbol.map(|c| c.to_string()),
    terms: rune_entry.terms.map(|t| Terms {
      amount: t.amount,
      cap: t.cap,
      height: t.height,
      offset: t.offset,
    }),
    timestamp: rune_entry.timestamp,
    turbo: rune_entry.turbo,
  })
}

#[query]
#[candid_method(query)]
pub fn get_rune_balances_for_outputs(
  outpoints: Vec<String>,
) -> Result<Vec<Option<Vec<RuneBalance>>>, Error> {
  if outpoints.len() > MAX_OUTPOINTS {
    return Err(Error::MaxOutpointsExceeded);
  }

  let cur_height = runes_indexer::index::mem_latest_block_height().expect("No block height found");
  let mut piles = Vec::new();

  for str_outpoint in outpoints {
    let outpoint = match OutPoint::from_str(&str_outpoint) {
      Ok(o) => o,
      Err(e) => {
        log!(WARNING, "Failed to parse outpoint {}: {}", str_outpoint, e);
        piles.push(None);
        continue;
      }
    };
    let k = OutPoint::store(outpoint);
    if let Some(rune_balances) = runes_indexer::index::mem_get_outpoint_to_rune_balances(k) {
      if let Some(height) = runes_indexer::index::mem_get_outpoint_to_height(k) {
        let confirmations = cur_height - height + 1;

        let mut outpoint_balances = Vec::new();
        for rune_balance in rune_balances.balances.iter() {
          let rune_entry =
            runes_indexer::index::mem_get_rune_id_to_rune_entry(rune_balance.rune_id.store());
          if let Some(rune_entry) = rune_entry {
            outpoint_balances.push(RuneBalance {
              confirmations,
              rune_id: rune_balance.rune_id.to_string(),
              amount: rune_balance.balance,
              divisibility: rune_entry.divisibility,
              symbol: rune_entry.symbol.map(|c| c.to_string()),
            });
          } else {
            log!(
              CRITICAL,
              "Rune not found for rune_id {}",
              rune_balance.rune_id.to_string()
            );
          }
        }
        piles.push(Some(outpoint_balances));
      } else {
        log!(WARNING, "Height not found for outpoint {}", str_outpoint);
        piles.push(None);
      }
    } else {
      log!(
        WARNING,
        "Rune balances not found for outpoint {}",
        str_outpoint
      );
      piles.push(None);
    }
  }

  Ok(piles)
}

#[query(hidden = true)]
pub fn rpc_transform(args: TransformArgs) -> HttpResponse {
  let headers = args
    .response
    .headers
    .into_iter()
    .filter(|h| runes_indexer::rpc::should_keep(h.name.as_str()))
    .collect::<Vec<_>>();
  HttpResponse {
    status: args.response.status.clone(),
    body: args.response.body.clone(),
    headers,
  }
}

#[update(hidden = true)]
pub fn start() -> Result<(), String> {
  let caller = ic_cdk::api::caller();
  if !ic_cdk::api::is_controller(&caller) {
    return Err("Not authorized".to_string());
  }

  runes_indexer::index::cancel_shutdown();
  let config = runes_indexer::index::mem_get_config();
  let _ = runes_indexer::index::updater::update_index(config.network, config.subscribers);

  Ok(())
}

#[update(hidden = true)]
pub fn stop() -> Result<(), String> {
  let caller = ic_cdk::api::caller();
  if !ic_cdk::api::is_controller(&caller) {
    return Err("Not authorized".to_string());
  }

  runes_indexer::index::shut_down();
  log!(INFO, "Waiting for index thread to finish...");

  Ok(())
}

#[update(hidden = true)]
pub fn set_bitcoin_rpc_url(url: String) -> Result<(), String> {
  let caller = ic_cdk::api::caller();
  if !ic_cdk::api::is_controller(&caller) {
    return Err("Not authorized".to_string());
  }
  let mut config = runes_indexer::index::mem_get_config();
  config.bitcoin_rpc_url = url;
  runes_indexer::index::mem_set_config(config).unwrap();

  Ok(())
}

#[query(hidden = true)]
pub fn get_subscribers() -> Vec<Principal> {
  runes_indexer::index::mem_get_config().subscribers
}

#[query(hidden = true)]
fn http_request(
  req: ic_canisters_http_types::HttpRequest,
) -> ic_canisters_http_types::HttpResponse {
  if ic_cdk::api::data_certificate().is_none() {
    ic_cdk::trap("update call rejected");
  }
  if req.path() == "/logs" {
    common::logs::do_reply(req)
  } else {
    ic_canisters_http_types::HttpResponseBuilder::not_found().build()
  }
}

#[init]
#[candid_method(init)]
fn init(runes_indexer_args: RunesIndexerArgs) {
  match runes_indexer_args {
    RunesIndexerArgs::Init(config) => {
      runes_indexer::index::mem_set_config(config).unwrap();
    }
    RunesIndexerArgs::Upgrade(_) => ic_cdk::trap(
      "Cannot initialize the canister with an Upgrade argument. Please provide an Init argument.",
    ),
  }
}

#[pre_upgrade]
fn pre_upgrade() {
  read_state(|s| s.pre_upgrade());
}

#[post_upgrade]
fn post_upgrade(runes_indexer_args: Option<RunesIndexerArgs>) {
  match runes_indexer_args {
    Some(RunesIndexerArgs::Upgrade(Some(upgrade_args))) => {
      let mut config = runes_indexer::index::mem_get_config();
      if let Some(bitcoin_rpc_url) = upgrade_args.bitcoin_rpc_url {
        config.bitcoin_rpc_url = bitcoin_rpc_url;
      }
      if let Some(subscribers) = upgrade_args.subscribers {
        config.subscribers = subscribers;
        log!(INFO, "subscribers updated: {:?}", config.subscribers);
      }
      runes_indexer::index::mem_set_config(config).unwrap();
    }
    None | Some(RunesIndexerArgs::Upgrade(None)) => {}
    _ => ic_cdk::trap(
      "Cannot upgrade the canister with an Init argument. Please provide an Upgrade argument.",
    ),
  }
  etching::runes_etching::etching_state::post_upgrade();
  set_timer_interval(Duration::from_secs(300), process_etching_task);
  log!(INFO, "post_upgrade successfully");
}

#[update]
pub async fn init_etching_sender_account() -> EtchingAccountInfo {
  init_etching_account_info().await
}

#[update]
pub async fn etching(args: EtchingArgs) -> Result<String, String> {
  let _guard = RequestEtchingGuard::new().ok_or("system busy, try later again")?;
  internal_etching(args).await
}

#[update(guard = "is_controller")]
pub fn etching_post_upgrade(args: EtchingUpgradeArgs) {
  match args {
    EtchingUpgradeArgs::Init(args) => {
      if no_initial() {
        let state = EtchingState::from(args);
        replace_state(state)
      }
    }
    EtchingUpgradeArgs::Upgrade(args) => {
      if let Some(a) = args {
        if let Some(fee) = a.etching_fee {
          mutate_state(|s| s.etching_fee = Some(fee));
        }
      }
    }
  }
}

#[update]
pub fn set_tx_fee_per_vbyte(args: SetTxFeePerVbyteArgs) -> Result<(), String> {
  if ic_cdk::api::is_controller(&caller()) {
    update_bitcoin_fee_rate(args.into());
    Ok(())
  } else {
    Err("Unauthorized".to_string())
  }
}

#[update(guard = "is_controller")]
pub fn set_etching_fee_utxos(us: Vec<UtxoArgs>) {
  for a in us {
    let utxo = etching::runes_etching::Utxo {
      id: bitcoin::hash_types::Txid::from_str(a.id.as_str()).unwrap(),
      index: a.index,
      amount: Amount::from_sat(a.amount),
    };
    mutate_state(|s| {
      if s.etching_fee_utxos.iter().find(|x| *x == utxo).is_none() {
        let _ = s.etching_fee_utxos.push(&utxo);
      }
    });
  }
}

pub fn is_controller() -> Result<(), String> {
  if ic_cdk::api::is_controller(&ic_cdk::caller()) {
    Ok(())
  } else {
    Err("caller is not controller".to_string())
  }
}

#[query]
pub fn etching_fee_utxos() -> Vec<UtxoArgs> {
  let r = read_state(|s| s.etching_fee_utxos.iter().collect::<Vec<Utxo>>());
  r.iter().map(|s| s.clone().into()).collect()
}

#[query]
pub fn get_etching_request(key: String) -> Option<SendEtchingInfo> {
  let r: Option<SendEtchingInfo> =
    read_state(|s| s.pending_etching_requests.get(&key)).map(|r| r.into());
  if r.is_some() {
    return r;
  }
  read_state(|s| s.finalized_etching_requests.get(&key)).map(|r| r.into())
}

#[query]
pub fn query_etching_fee() -> u64 {
  read_state(|s| s.etching_fee.unwrap_or(DEFAULT_ETCHING_FEE_IN_ICP))
}

ic_cdk::export_candid!();

fn main() {}
