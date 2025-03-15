use bitcoin::{OutPoint, Txid};
use candid::{candid_method, Principal};
use ic_canister_log::log;
use ic_cdk::api::management_canister::http_request::{HttpResponse, TransformArgs};
use ic_cdk_macros::{init, post_upgrade, query, update};
use runes_indexer::config::RunesIndexerArgs;
use runes_indexer::index::entry::Entry;
use runes_indexer::logs::{CRITICAL, INFO, WARNING};
use runes_indexer_interface::{Error, GetEtchingResult, RuneBalance, RuneEntry, Terms};
use std::str::FromStr;

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
  let txid = Txid::from_str(&txid).ok()?;
  let cur_height = runes_indexer::index::mem_latest_block_height().expect("No block height found");

  runes_indexer::index::mem_get_etching(txid).map(|(id, entry)| GetEtchingResult {
    confirmations: cur_height - entry.block as u32 + 1,
    rune_id: id.to_string(),
  })
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
    runes_indexer::logs::do_reply(req)
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
}

ic_cdk::export_candid!();

fn main() {}
