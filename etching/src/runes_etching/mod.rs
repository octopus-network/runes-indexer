use anyhow::anyhow;
use base64::Engine;
pub use bitcoin;
use candid::{CandidType, Deserialize};
use ic_stable_structures::storable::Bound;
use ic_stable_structures::Storable;
use ordinals::{Etching, SpacedRune};
use serde::Serialize;
use std::borrow::Cow;
use std::str::FromStr;

use crate::runes_etching::fee_calculator::MAX_LOGO_CONTENT_SIZE;
use crate::runes_etching::transactions::EtchingStatus::Initial;
use crate::runes_etching::transactions::SendEtchingInfo;
pub use error::{InscriptionParseError, OrdError};
pub use inscription::iid::InscriptionId;
pub use inscription::nft::Nft;
pub use inscription::Inscription;
pub use result::OrdResult;
pub use utils::fees::{self, MultisigConfig};
pub use utils::{constants, push_bytes};
pub use wallet::{
  CreateCommitTransaction, OrdTransactionBuilder, RevealTransactionArgs, SignCommitTransactionArgs,
  Utxo, Wallet,
};

mod address;
mod destination;
pub mod error;
pub mod etching_state;
pub mod fee_calculator;
pub mod guard;
pub mod inscription;
pub mod management;
pub mod result;
pub mod sync;
mod task;
pub mod transactions;
pub mod types;
pub mod utils;
pub mod wallet;

#[derive(CandidType, Clone, Default, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct EtchingArgs {
  pub rune_name: String,
  pub divisibility: Option<u8>,
  pub premine: Option<u128>,
  pub logo: Option<LogoParams>,
  pub symbol: Option<String>,
  pub terms: Option<OrdinalsTerms>,
  pub premine_receiver: String,
  pub turbo: bool,
}

#[derive(Default, CandidType, Serialize, Deserialize, Debug, PartialEq, Copy, Clone, Eq)]
pub struct OrdinalsTerms {
  pub amount: u128,
  pub cap: u128,
  pub height: (Option<u64>, Option<u64>),
  pub offset: (Option<u64>, Option<u64>),
}

impl OrdinalsTerms {
  pub fn check(&self) -> anyhow::Result<()> {
    if self.amount == 0 || self.cap == 0 {
      return Err(anyhow!("cap or amt is none".to_string()));
    }
    Ok(())
  }
}

impl EtchingArgs {
  pub fn check(&self) -> anyhow::Result<()> {
    if let Some(t) = self.terms {
      t.check()?;
    }
    if let Some(d) = self.divisibility {
      if d > Etching::MAX_DIVISIBILITY {
        return Err(anyhow!("the max divisibility is 38"));
      }
    }
    if let Some(l) = self.logo.clone() {
      let logo_content = base64::engine::general_purpose::STANDARD
        .decode(l.content_base64)
        .map_err(|e| anyhow!(e.to_string()))?;
      if logo_content.len() > MAX_LOGO_CONTENT_SIZE {
        return Err(anyhow!("the max size of logo content is 128k".to_string()));
      }
    }
    let space_rune =
      SpacedRune::from_str(self.rune_name.as_str()).map_err(|e| anyhow!(e.to_string()))?;
    let name = space_rune.rune.to_string();
    if name.len() < 10 || name.len() > 26 {
      return Err(anyhow!("rune name's length must be >= 10 and <=26"));
    }
    Ok(())
  }
}
pub type InternalEtchingArgs = EtchingArgs;

impl Into<SendEtchingInfo> for InternalEtchingArgs {
  fn into(self) -> SendEtchingInfo {
    SendEtchingInfo {
      etching_args: self.clone().into(),
      commit_txid: "".to_string(),
      reveal_txid: "".to_string(),
      err_info: "".to_string(),
      time_at: ic_cdk::api::time(),
      script_out_address: "".to_string(),
      status: Initial,
      receiver: self.premine_receiver.clone(),
    }
  }
}

impl Storable for InternalEtchingArgs {
  fn to_bytes(&self) -> std::borrow::Cow<[u8]> {
    let mut bytes = vec![];
    let _ = ciborium::ser::into_writer(self, &mut bytes);
    Cow::Owned(bytes)
  }

  fn from_bytes(bytes: std::borrow::Cow<[u8]>) -> Self {
    let args = ciborium::de::from_reader(bytes.as_ref()).expect("failed to decode etching args");
    args
  }

  const BOUND: Bound = Bound::Unbounded;
}

#[derive(CandidType, Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LogoParams {
  pub content_type: String,
  pub content_base64: String,
}
