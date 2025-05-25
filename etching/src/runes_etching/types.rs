use candid::CandidType;
use serde_derive::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Default, Eq, serde::Deserialize, Serialize, CandidType)]
pub struct EtchingAccountInfo {
  pub pubkey: String,
  pub address: String,
  pub derive_path: String,
}

impl EtchingAccountInfo {
  pub fn is_inited(&self) -> bool {
    !self.pubkey.is_empty() && !self.address.is_empty()
  }
}

#[derive(CandidType, Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct BitcoinFeeRate {
  pub low: u64,
  pub medium: u64,
  pub high: u64,
}

#[derive(CandidType, Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SetTxFeePerVbyteArgs {
  pub low: u64,
  pub medium: u64,
  pub high: u64,
}

/// Unspent transaction output to be used as input of a transaction
#[derive(CandidType, Debug, Clone, Serialize, Deserialize)]
pub struct UtxoArgs {
  pub id: String,
  pub index: u32,
  pub amount: u64,
}
impl From<SetTxFeePerVbyteArgs> for BitcoinFeeRate {
  fn from(value: SetTxFeePerVbyteArgs) -> Self {
    BitcoinFeeRate {
      low: value.low,
      medium: value.medium,
      high: value.high,
    }
  }
}
