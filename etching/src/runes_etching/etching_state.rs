use serde_derive::Serialize;


use std::cell::RefCell;
use std::ops::Deref;
use ic_cdk::api::management_canister::bitcoin::BitcoinNetwork;
use ic_ic00_types::{ DerivationPath};
use ic_stable_structures::{DefaultMemoryImpl, StableBTreeMap, StableVec};
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use serde_bytes::ByteBuf;
use crate::runes_etching::management::ecdsa_public_key;
use ic_crypto_sha2::Sha256;
use crate::runes_etching::address::BitcoinAddress;
use crate::runes_etching::transactions::SendEtchingRequest;
use crate::runes_etching::types::EtchingAccountInfo;

type VMem = VirtualMemory<DefaultMemoryImpl>;

const ETCHING_FEE_UTXOS_MEMORY_ID: MemoryId = MemoryId::new(101);
const PENDING_ETCHING_REQUESTS_MEMORY_ID: MemoryId = MemoryId::new(102);
const FINALIZED_ETCHING_REQUESTS_MEMORY_ID: MemoryId = MemoryId::new(103);
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
    #[serde(skip, default = "crate::runes_etching::etching_state::init_etching_fee_utxos")]
    pub etching_fee_utxos: StableVec<crate::runes_etching::Utxo, VMem>,
    #[serde(skip, default = "crate::runes_etching::etching_state::init_pending_etching_requests")]
    pub pending_etching_requests: StableBTreeMap<String, SendEtchingRequest, VMem>,
    #[serde(skip, default = "crate::runes_etching::etching_state::init_finalized_etching_requests")]
    pub finalized_etching_requests: StableBTreeMap<String, SendEtchingRequest, VMem>,
    #[serde(default)]
    pub etching_fee: Option<u64>,
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
