use ic_cdk::api::management_canister::bitcoin::BitcoinNetwork;
use ic_crypto_sha2::Sha256;
use serde::{Deserialize, Serialize};

// See https://en.bitcoin.it/wiki/List_of_address_prefixes.
const BTC_MAINNET_PREFIX: u8 = 0;
const BTC_MAINNET_P2SH_PREFIX: u8 = 5;
const BTC_TESTNET_PREFIX: u8 = 111;
const BTC_TESTNET_P2SH_PREFIX: u8 = 196;

#[derive(candid::CandidType, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BitcoinAddress {
    /// Pay to witness public key hash address.
    /// See BIP-173.
    #[serde(rename = "p2wpkh_v0")]
    P2wpkhV0([u8; 20]),
    /// Pay to witness script hash address.
    /// See BIP-141.
    #[serde(rename = "p2wsh_v0")]
    P2wshV0([u8; 32]),
    /// Pay to taproot address.
    /// See BIP-341.
    #[serde(rename = "p2tr_v1")]
    P2trV1([u8; 32]),
    /// Pay to public key hash address.
    #[serde(rename = "p2pkh")]
    P2pkh([u8; 20]),
    /// Pay to script hash address.
    #[serde(rename = "p2sh")]
    P2sh([u8; 20]),
    /// Pay to OP_RETURN
    OpReturn(Vec<u8>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WitnessVersion {
    V0 = 0,
    V1 = 1,
}

impl BitcoinAddress {
    /// Converts the address to the textual representation.
    pub fn display(&self, network: BitcoinNetwork) -> String {
        match self {
            Self::P2wpkhV0(pkhash) => encode_bech32(network, pkhash, WitnessVersion::V0),
            Self::P2wshV0(pkhash) => encode_bech32(network, pkhash, WitnessVersion::V0),
            Self::P2pkh(pkhash) => version_and_hash_to_address(
                match network {
                    BitcoinNetwork::Mainnet => BTC_MAINNET_PREFIX,
                    BitcoinNetwork::Testnet | BitcoinNetwork::Regtest => BTC_TESTNET_PREFIX,
                },
                pkhash,
            ),
            Self::P2sh(script_hash) => version_and_hash_to_address(
                match network {
                    BitcoinNetwork::Mainnet => BTC_MAINNET_P2SH_PREFIX,
                    BitcoinNetwork::Testnet | BitcoinNetwork::Regtest => BTC_TESTNET_P2SH_PREFIX,
                },
                script_hash,
            ),
            Self::P2trV1(pkhash) => encode_bech32(network, pkhash, WitnessVersion::V1),
            Self::OpReturn(_) => String::new(),
        }
    }
}

fn encode_bech32(network: BitcoinNetwork, hash: &[u8], version: WitnessVersion) -> String {
    use bech32::u5;

    let hrp = hrp(network);
    let witness_version: u5 =
        u5::try_from_u8(version as u8).expect("bug: witness version must be smaller than 32");
    let data: Vec<u5> = std::iter::once(witness_version)
        .chain(
            bech32::convert_bits(hash, 8, 5, true)
                .expect("bug: bech32 bit conversion failed on valid inputs")
                .into_iter()
                .map(|b| {
                    u5::try_from_u8(b).expect("bug: bech32 bit conversion produced invalid outputs")
                }),
        )
        .collect();
    match version {
        WitnessVersion::V0 => bech32::encode(hrp, data, bech32::Variant::Bech32)
            .expect("bug: bech32 encoding failed on valid inputs"),
        WitnessVersion::V1 => bech32::encode(hrp, data, bech32::Variant::Bech32m)
            .expect("bug: bech32m encoding failed on valid inputs"),
    }
}

pub fn version_and_hash_to_address(version: u8, hash: &[u8; 20]) -> String {
    let mut buf = Vec::with_capacity(25);
    buf.push(version);
    buf.extend_from_slice(hash);
    let sha256d = Sha256::hash(&Sha256::hash(&buf));
    buf.extend_from_slice(&sha256d[0..4]);
    bs58::encode(&buf).into_string()
}

/// Returns the human-readable part of a bech32 address
pub fn hrp(network: BitcoinNetwork) -> &'static str {
    match network {
        BitcoinNetwork::Mainnet => "bc",
        BitcoinNetwork::Testnet => "tb",
        BitcoinNetwork::Regtest => "bcrt",
    }
}