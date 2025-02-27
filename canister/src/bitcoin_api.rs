use anyhow::anyhow;
use bitcoin::{block::Header, BlockHash};
use candid::{self, CandidType, Deserialize, Principal};
use ic_cdk::api::{call::RejectionCode, management_canister::bitcoin::BitcoinNetwork};

pub type Height = u32;
pub type BlockHeader = Vec<u8>;

/// A request for getting the block headers for a given height range.
#[derive(CandidType, Debug, Deserialize, PartialEq, Eq)]
pub struct GetBlockHeadersRequest {
  pub start_height: Height,
  pub end_height: Option<Height>,
  pub network: BitcoinNetwork,
}

/// The response returned for a request for getting the block headers from a given height.
#[derive(CandidType, Debug, Deserialize, PartialEq, Eq, Clone)]
pub struct GetBlockHeadersResponse {
  pub tip_height: Height,
  pub block_headers: Vec<BlockHeader>,
}

/// Returns the block headers in the given height range.
pub(crate) async fn get_block_headers(
  network: BitcoinNetwork,
  start_height: u32,
  end_height: Option<u32>,
) -> Result<(GetBlockHeadersResponse,), (RejectionCode, String)> {
  let cycles = match network {
    BitcoinNetwork::Mainnet => 10_000_000_000,
    BitcoinNetwork::Testnet => 10_000_000_000,
    BitcoinNetwork::Regtest => 0,
  };

  let request = GetBlockHeadersRequest {
    start_height,
    end_height,
    network,
  };

  let res = ic_cdk::api::call::call_with_payment128::<
    (GetBlockHeadersRequest,),
    (GetBlockHeadersResponse,),
  >(
    Principal::management_canister(),
    "bitcoin_get_block_headers",
    (request,),
    cycles,
  )
  .await;

  res
}

pub async fn get_block_hash(
  network: BitcoinNetwork,
  height: u32,
) -> crate::Result<Option<BlockHash>> {
  // We're using direct HTTPS outcalls to Bitcoin node to fetch block hashes for regtest
  if network == BitcoinNetwork::Regtest {
    return match crate::rpc::get_block_hash(height).await {
      Ok(hash) => Ok(Some(hash)),
      Err(_err) => Ok(None),
    };
  }

  match get_block_headers(network, height, Some(height)).await {
    Ok(response) => {
      let header_bytes = response
        .0
        .block_headers
        .first()
        .ok_or_else(|| anyhow!("failed to get header at height: {}", height))?;

      let header =
        <Header as bitcoin::consensus::Decodable>::consensus_decode(&mut header_bytes.as_slice())
          .map_err(|_| anyhow!("failed to decode block hash at height: {}", height))?;

      Ok(Some(header.block_hash()))
    }
    Err(err)
      if err.0 == RejectionCode::CanisterReject && err.1.contains("StartHeightDoesNotExist") =>
    {
      Ok(None)
    }
    Err(err) => Err(anyhow!("failed to bitcoin_get_block_headers: {:?}", err)),
  }
}
