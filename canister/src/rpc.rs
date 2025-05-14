use super::Result;
use common::logs::{DEBUG, ERROR};
use anyhow::anyhow;
use bitcoin::{consensus::encode, Block};
use bitcoin::{BlockHash, Txid};
use bitcoincore_rpc_json::{GetBlockHeaderResult, GetRawTransactionResult};
use ic_canister_log::log;
use ic_cdk::api::management_canister::http_request::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

lazy_static::lazy_static! {
  static ref ESSENTIAL_HEADERS: std::collections::HashSet<String> = {
    let mut set = std::collections::HashSet::new();
    set.insert("content-length".to_string());
    set.insert("content-range".to_string());
    set
  };
}

pub fn should_keep(header: &str) -> bool {
  let h = header.to_ascii_lowercase();
  ESSENTIAL_HEADERS.contains(&h)
}

#[derive(Serialize, Debug)]
struct Payload {
  pub jsonrpc: &'static str,
  pub id: &'static str,
  pub method: &'static str,
  pub params: serde_json::Value,
}

#[derive(Deserialize, Serialize, Debug)]
struct Reply<R> {
  #[allow(dead_code)]
  pub id: String,
  pub error: Option<ErrorMsg>,
  pub result: Option<R>,
}

#[derive(Deserialize, Serialize, Debug)]
struct ErrorMsg {
  #[allow(dead_code)]
  code: i64,
  message: String,
}

/// [   0..1023] + [1024..2047] + [2048..3071] = 3072
/// [start, end] + [start, end] + [start, end] = total
fn split(end: u64, total: u64, limit: u64) -> (u64, u64) {
  let start = end + 1;
  let end = if start + limit >= total {
    total - 1
  } else {
    start + limit - 1
  };
  (start, end)
}

fn partial_request(
  url: impl ToString,
  endpoint: &'static str,
  params: impl Into<serde_json::Value>,
  range: (u64, u64),
  subnet_nodes: u64,
) -> (CanisterHttpRequestArgument, u128) {
  let payload = Payload {
    jsonrpc: "1.0",
    id: "btc0",
    method: endpoint.as_ref(),
    params: params.into(),
  };
  let body = serde_json::to_vec(&payload).unwrap();
  let mut hasher = Sha256::new();
  hasher.update(&body);
  hasher.update(&range.0.to_le_bytes());
  hasher.update(&range.1.to_le_bytes());
  let uniq: [u8; 32] = hasher.finalize().into();
  let uniq = hex::encode(uniq[0..16].to_vec());
  let cycles = estimate_cycles(
    body.len() as u64 + 512,
    range.1 - range.0 + 1 + 999,
    subnet_nodes,
  );
  (
    CanisterHttpRequestArgument {
      url: url.to_string(),
      method: HttpMethod::POST,
      body: Some(body),
      max_response_bytes: Some(range.1 - range.0 + 1 + 999),
      transform: Some(TransformContext {
        function: TransformFunc(candid::Func {
          principal: ic_cdk::api::id(),
          method: "rpc_transform".to_string(),
        }),
        context: vec![],
      }),
      headers: vec![
        HttpHeader {
          name: "Content-Type".to_string(),
          value: "application/json".to_string(),
        },
        HttpHeader {
          name: "Idempotency-Key".to_string(),
          value: uniq.clone(),
        },
        HttpHeader {
          name: "X-Cloud-Trace-Context".to_string(),
          value: uniq.clone(),
        },
        HttpHeader {
          name: "Range".to_string(),
          value: format!("bytes={}-{}", range.0, range.1),
        },
      ],
    },
    cycles,
  )
}

const MAX_RESPONSE_BYTES: u64 = 1_999_000;

pub(crate) fn estimate_cycles(req_len: u64, rsp_len: u64, n: u64) -> u128 {
  (3_000_000 + 60_000 * n as u128 + 400 * req_len as u128 + 800 * rsp_len as u128) * n as u128
}

async fn make_single_request(
  args: CanisterHttpRequestArgument,
  estimate_cycle: u128,
) -> Result<HttpResponse> {
  let mut retry = 0;
  let mut cycles = estimate_cycle;
  loop {
    let response = http_request(args.clone(), cycles).await;
    match response {
      Ok((response,)) => return Ok(response),
      Err((code, e)) => {
        retry += 1;
        cycles += cycles / 10;
        if retry > 3 {
          log!(
            ERROR,
            "rpc error: {:?} => {}; won't retry(exceeds retry limit)",
            code,
            e
          );
          break Err(anyhow!("make_single_request: retry limit exceeded {}", e));
        }
        log!(
          ERROR,
          "rpc error: {:?} => {}; will retry with extra {} cycles",
          code,
          e,
          cycles
        );
      }
    }
  }
}

pub(crate) async fn make_rpc<R>(
  url: impl ToString,
  endpoint: &'static str,
  params: impl Into<serde_json::Value> + Clone,
  max_response_bytes: u64,
  subnet_nodes: u64,
) -> Result<R>
where
  R: for<'a> Deserialize<'a> + std::fmt::Debug,
{
  let mut range = (0, max_response_bytes - 1);
  let mut buf = Vec::<u8>::with_capacity(max_response_bytes as usize);
  let mut total_cycles = 0;
  loop {
    let (args, cycles) = partial_request(
      url.to_string(),
      endpoint,
      params.clone(),
      range,
      subnet_nodes,
    );
    total_cycles += cycles;
    let response = make_single_request(args, cycles).await?;
    if response.status == candid::Nat::from(200u32) {
      buf.extend_from_slice(response.body.as_slice());
      break;
    }
    if let Some(new_range) = response
      .headers
      .iter()
      .find(|h| h.name.eq_ignore_ascii_case("Content-Range"))
      .map(|r| -> Option<(u64, u64)> {
        let r = r.value.trim_start_matches("bytes ");
        let range_and_total = r.split('/').collect::<Vec<&str>>();
        let total = range_and_total[1].parse::<u64>().ok()?;
        let range = range_and_total[0].split('-').collect::<Vec<&str>>();
        let end = range[1].parse::<u64>().ok()?;
        Some(split(end, total, MAX_RESPONSE_BYTES))
      })
      .flatten()
    {
      log!(DEBUG, "bytes range: {:?} => {:?}", range, new_range);
      range = new_range;
      buf.extend_from_slice(response.body.as_slice());
      if range.0 >= range.1 {
        break;
      }
    } else {
      // some unexpected behaviour since we are not going to compatible with all servers
      buf.extend_from_slice(response.body.as_slice());
      break;
    }
  }
  log!(
    DEBUG,
    "reading all {} bytes from rpc {}, consumed {} cycles",
    buf.len(),
    endpoint,
    total_cycles
  );
  let reply: Reply<R> = serde_json::from_slice(&buf)?;
  if reply.error.is_some() {
    return Err(anyhow!(
      "rpc error: {:?} => {}",
      endpoint,
      reply.error.map(|e| e.message).unwrap()
    ));
  }
  reply
    .result
    .ok_or(anyhow!("rpc error: {:?} => {}", endpoint, "No result"))
}

async fn inner_get_block(
  url: &str,
  max_response_bytes: u64,
  subnet_nodes: u64,
  hash: BlockHash,
) -> Result<Block> {
  let args = [into_json(hash)?, 0.into()];
  let hex: String = make_rpc(
    url,
    "getblock",
    args.to_vec(),
    max_response_bytes,
    subnet_nodes,
  )
  .await?;
  Ok(encode::deserialize_hex(&hex)?)
}

pub(crate) async fn get_block(hash: BlockHash) -> Result<crate::index::updater::BlockData> {
  let config = crate::index::mem_get_config();
  let block = inner_get_block(
    &config.bitcoin_rpc_url,
    MAX_RESPONSE_BYTES,
    config.get_subnet_nodes(),
    hash,
  )
  .await?;

  if block.block_hash() != hash {
    return Err(anyhow!("wrong block hash: {}", hash.to_string()));
  }

  block
    .check_merkle_root()
    .then(|| crate::index::updater::BlockData::from(block))
    .ok_or(anyhow!("wrong block merkle root: {}", hash.to_string()))
}

async fn inner_get_raw_transaction_info(
  url: &str,
  max_response_bytes: u64,
  subnet_nodes: u64,
  txid: &Txid,
  block_hash: Option<&BlockHash>,
) -> Result<GetRawTransactionResult> {
  let args = [
    into_json(txid)?,
    into_json(true)?,
    opt_into_json(block_hash)?,
  ];
  let res: GetRawTransactionResult = make_rpc(
    url,
    "getrawtransaction",
    args.to_vec(),
    max_response_bytes,
    subnet_nodes,
  )
  .await?;
  Ok(res)
}

// 1885 ~ 3522 bytes
pub(crate) async fn get_raw_transaction_info(
  txid: &Txid,
  block_hash: Option<&BlockHash>,
) -> Result<GetRawTransactionResult> {
  let config = crate::index::mem_get_config();
  inner_get_raw_transaction_info(
    &config.bitcoin_rpc_url,
    4_096,
    config.get_subnet_nodes(),
    txid,
    block_hash,
  )
  .await
}

async fn inner_get_block_header_info(
  url: &str,
  max_response_bytes: u64,
  subnet_nodes: u64,
  hash: &bitcoin::BlockHash,
) -> Result<GetBlockHeaderResult> {
  let args = [into_json(hash)?, true.into()];
  let res: GetBlockHeaderResult = make_rpc(
    url,
    "getblockheader",
    args.to_vec(),
    max_response_bytes,
    subnet_nodes,
  )
  .await?;
  Ok(res)
}

// 640 ~ 644 bytes
pub(crate) async fn get_block_header_info(
  hash: &bitcoin::BlockHash,
) -> Result<GetBlockHeaderResult> {
  let config = crate::index::mem_get_config();
  inner_get_block_header_info(
    &config.bitcoin_rpc_url,
    1_024,
    config.get_subnet_nodes(),
    hash,
  )
  .await
}

async fn inner_get_block_hash(
  url: &str,
  max_response_bytes: u64,
  subnet_nodes: u64,
  height: u32,
) -> Result<BlockHash> {
  let args: [serde_json::Value; 1] = [(height as u64).into()];
  let res: BlockHash = make_rpc(
    url,
    "getblockhash",
    args.to_vec(),
    max_response_bytes,
    subnet_nodes,
  )
  .await?;
  Ok(res)
}

pub(crate) async fn get_block_hash(height: u32) -> Result<BlockHash> {
  let config = crate::index::mem_get_config();
  inner_get_block_hash(
    &config.bitcoin_rpc_url,
    256,
    config.get_subnet_nodes(),
    height,
  )
  .await
}

/// Shorthand for converting a variable into a serde_json::Value.
fn into_json<T>(val: T) -> Result<serde_json::Value>
where
  T: serde::ser::Serialize,
{
  Ok(serde_json::to_value(val)?)
}

/// Shorthand for converting an Option into an Option<serde_json::Value>.
fn opt_into_json<T>(opt: Option<T>) -> Result<serde_json::Value>
where
  T: serde::ser::Serialize,
{
  match opt {
    Some(val) => Ok(into_json(val)?),
    None => Ok(serde_json::Value::Null),
  }
}
