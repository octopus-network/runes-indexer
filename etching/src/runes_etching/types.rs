use candid::CandidType;
use serde_derive::Serialize;

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
