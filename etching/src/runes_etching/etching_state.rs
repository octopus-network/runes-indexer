use serde_derive::{Deserialize, Serialize};

use crate::runes_etching::address::BitcoinAddress;
use crate::runes_etching::management::ecdsa_public_key;
use crate::runes_etching::transactions::SendEtchingRequest;
use crate::runes_etching::types::{BitcoinFeeRate, EtchingAccountInfo};
use candid::CandidType;
use ic_cdk::api::management_canister::bitcoin::BitcoinNetwork;
use ic_crypto_sha2::Sha256;
use ic_ic00_types::DerivationPath;
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{DefaultMemoryImpl, StableBTreeMap, StableVec};
use serde_bytes::ByteBuf;
use std::cell::RefCell;
use std::ops::Deref;

type VMem = VirtualMemory<DefaultMemoryImpl>;

const ETCHING_FEE_UTXOS_MEMORY_ID: MemoryId = MemoryId::new(101);
const PENDING_ETCHING_REQUESTS_MEMORY_ID: MemoryId = MemoryId::new(102);
const FINALIZED_ETCHING_REQUESTS_MEMORY_ID: MemoryId = MemoryId::new(103);
const UPGRADE_STASH_MEMORY_ID: MemoryId = MemoryId::new(110);

thread_local! {
    static __STATE: RefCell<Option<EtchingState>> = RefCell::default();

    static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> = RefCell::new(
        MemoryManager::init(DefaultMemoryImpl::default())
    );
}

#[derive(serde::Deserialize, Serialize)]
pub struct EtchingState {
  pub ecdsa_key_name: String,
  #[serde(default)]
  pub etching_acount_info: EtchingAccountInfo,
  pub btc_network: BitcoinNetwork,
  #[serde(
    skip,
    default = "crate::runes_etching::etching_state::init_etching_fee_utxos"
  )]
  pub etching_fee_utxos: StableVec<crate::runes_etching::Utxo, VMem>,
  #[serde(
    skip,
    default = "crate::runes_etching::etching_state::init_pending_etching_requests"
  )]
  pub pending_etching_requests: StableBTreeMap<String, SendEtchingRequest, VMem>,
  #[serde(
    skip,
    default = "crate::runes_etching::etching_state::init_finalized_etching_requests"
  )]
  pub finalized_etching_requests: StableBTreeMap<String, SendEtchingRequest, VMem>,
  #[serde(default)]
  pub etching_fee: Option<u64>,
  #[serde(default)]
  pub bitcoin_fee_rate: BitcoinFeeRate,
  #[serde(default)]
  pub is_process_etching_msg: bool,
  #[serde(default)]
  pub is_request_etching: bool,
}

impl From<EtchingStateArgs> for EtchingState {
  fn from(value: EtchingStateArgs) -> Self {
    EtchingState {
      ecdsa_key_name: value.ecdsa_key_name,
      etching_acount_info: Default::default(),
      btc_network: value.btc_network,
      etching_fee_utxos: init_etching_fee_utxos(),
      pending_etching_requests: init_pending_etching_requests(),
      finalized_etching_requests: init_finalized_etching_requests(),
      etching_fee: value.etching_fee,
      bitcoin_fee_rate: Default::default(),
      is_process_etching_msg: false,
      is_request_etching: false,
    }
  }
}
pub fn init_etching_fee_utxos() -> StableVec<crate::runes_etching::Utxo, VMem> {
  StableVec::init(with_memory_manager(|m| m.get(ETCHING_FEE_UTXOS_MEMORY_ID))).unwrap()
}

pub fn init_finalized_etching_requests() -> StableBTreeMap<String, SendEtchingRequest, VMem> {
  StableBTreeMap::init(with_memory_manager(|m| {
    m.get(FINALIZED_ETCHING_REQUESTS_MEMORY_ID)
  }))
}

fn with_memory_manager<R>(f: impl FnOnce(&MemoryManager<DefaultMemoryImpl>) -> R) -> R {
  MEMORY_MANAGER.with(|cell| f(cell.borrow().deref()))
}

pub fn init_pending_etching_requests() -> StableBTreeMap<String, SendEtchingRequest, VMem> {
  StableBTreeMap::init(with_memory_manager(|m| {
    m.get(PENDING_ETCHING_REQUESTS_MEMORY_ID)
  }))
}

/// Mutates (part of) the current state using `f`.
///
/// Panics if there is no state.
pub fn mutate_state<F, R>(f: F) -> R
where
  F: FnOnce(&mut EtchingState) -> R,
{
  __STATE.with(|s| f(s.borrow_mut().as_mut().expect("State not initialized!")))
}

/// Read (part of) the current state using `f`.
///
/// Panics if there is no state.
pub fn read_state<F, R>(f: F) -> R
where
  F: FnOnce(&EtchingState) -> R,
{
  __STATE.with(|s| f(s.borrow().as_ref().expect("State not initialized!")))
}

/// Replaces the current state.
pub fn replace_state(state: EtchingState) {
  __STATE.with(|s| {
    *s.borrow_mut() = Some(state);
  });
}
pub fn no_initial() -> bool {
  __STATE.with(|s| s.borrow().is_none())
}

pub async fn init_etching_account_info() -> EtchingAccountInfo {
  let account_info = read_state(|s| s.etching_acount_info.clone());
  if account_info.is_inited() {
    return account_info;
  }
  let btc_network = read_state(|s| s.btc_network);
  let key_name = read_state(|s| s.ecdsa_key_name.clone());
  let derive_path_str = "etching_address";
  let dp = DerivationPath::new(vec![ByteBuf::from(derive_path_str.as_bytes())]);
  let pub_key = ecdsa_public_key(key_name, dp.clone())
    .await
    .unwrap_or_else(|e| ic_cdk::trap(&format!("failed to retrieve ECDSA public key: {e}")));

  use ripemd::{Digest, Ripemd160};
  let address =
    BitcoinAddress::P2wpkhV0(Ripemd160::digest(Sha256::hash(&pub_key.public_key)).into());
  let deposit_addr = address.display(btc_network);
  let account_info = EtchingAccountInfo {
    pubkey: hex::encode(pub_key.public_key),
    address: deposit_addr,
    derive_path: derive_path_str.to_string(),
  };
  mutate_state(|s| {
    s.etching_acount_info = account_info.clone();
  });
  account_info
}

pub fn update_bitcoin_fee_rate(fee_rate: BitcoinFeeRate) {
  mutate_state(|s| s.bitcoin_fee_rate = fee_rate);
}

#[derive(CandidType, Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum EtchingUpgradeArgs {
  Init(EtchingStateArgs),
  Upgrade(Option<EtchingStateArgs>),
}

#[derive(CandidType, Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct EtchingStateArgs {
  pub btc_network: BitcoinNetwork,
  pub ecdsa_key_name: String,
  pub etching_fee: Option<u64>,
}

pub fn post_upgrade() {
    use ic_stable_structures::Memory;
    let memory = get_upgrade_stash_memory();
    // Read the length of the state bytes.
    let mut state_len_bytes = [0; 4];
    memory.read(0, &mut state_len_bytes);
    let state_len = u32::from_le_bytes(state_len_bytes) as usize;
    let mut state_bytes = vec![0; state_len];
    memory.read(4, &mut state_bytes);
    let state: EtchingState =
        ciborium::de::from_reader(&*state_bytes).expect("failed to decode state");
    replace_state(state);
}

pub fn get_upgrade_stash_memory() -> VMem {
    with_memory_manager(|m| m.get(UPGRADE_STASH_MEMORY_ID))
}

impl EtchingState {
    pub fn pre_upgrade(&self) {
        let mut state_bytes = vec![];
        let _ = ciborium::ser::into_writer(self, &mut state_bytes);
        let len = state_bytes.len() as u32;
        let mut memory = get_upgrade_stash_memory();
        let mut writer = Writer::new(&mut memory, 0);
        writer
            .write(&len.to_le_bytes())
            .expect("failed to save hub state len");
        writer
            .write(&state_bytes)
            .expect("failed to save hub state");
    }
}
