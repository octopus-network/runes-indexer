use candid::{CandidType, Deserialize};
use serde::Serialize;

#[derive(Serialize, CandidType, Deserialize, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Destination {
    pub target_chain_id: String,
    pub receiver: String,
    pub token: Option<String>,
}

impl Destination {
    #[inline]
    pub fn effective_token(&self) -> String {
        self.token.clone().unwrap_or_default()
    }
}
