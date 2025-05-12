use crate::runes_etching::wallet::{ScriptType, RUNE_POSTAGE};
use bitcoin::absolute::LockTime;
use bitcoin::transaction::Version;
use bitcoin::{
    Address, Amount, FeeRate, Network, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut,
    Witness,
};
use serde::{Deserialize, Serialize};

/// Single ECDSA signature + SIGHASH type size in bytes.
const ECDSA_SIGHASH_SIZE: usize = 72 + 1;
/// Single Schnorr signature + SIGHASH type size for Taproot in bytes.
const SCHNORR_SIGHASH_SIZE: usize = 64 + 1;

/// Represents multisig configuration (m of n) for a transaction, if applicable.
/// Encapsulates the number of required signatures and the total number of signatories.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MultisigConfig {
    /// Number of required signatures (m)
    pub required: usize,
    /// Total number of signatories (n)
    pub total: usize,
}

/// Estimates the transaction fees for a transaction.
pub fn estimate_transaction_fees(
    script_type: ScriptType,
    number_of_inputs: usize,
    current_fee_rate: FeeRate,
    multisig_config: &Option<MultisigConfig>,
    outputs: Vec<TxOut>,
) -> Amount {
    let vbytes = estimate_vbytes(number_of_inputs, script_type, multisig_config, outputs);

    current_fee_rate.fee_vb(vbytes as u64).unwrap()
}

pub struct EstimateEdictTxFeesArgs {
    pub script_type: ScriptType,
    pub number_of_inputs: usize,
    pub current_fee_rate: FeeRate,
    pub multisig_config: Option<MultisigConfig>,
    pub rune_change_address: Address,
    pub destination_address: Address,
    pub change_address: Address,
    pub rune: ordinals::RuneId,
    pub rune_amount: u128,
}

/// Estimates the transaction fees for an edict transaction.
pub fn estimate_edict_transaction_fees(args: EstimateEdictTxFeesArgs) -> Amount {
    let runestone = ordinals::Runestone {
        edicts: vec![ordinals::Edict {
            id: args.rune,
            amount: args.rune_amount,
            output: 2,
        }],
        etching: None,
        mint: None,
        pointer: None,
    };

    let runestone_out = TxOut {
        value: Amount::ZERO,
        script_pubkey: ScriptBuf::from_bytes(runestone.encipher().into_bytes()),
    };
    let rune_change_out = TxOut {
        value: RUNE_POSTAGE,
        script_pubkey: args.rune_change_address.script_pubkey(),
    };
    let rune_destination_out = TxOut {
        value: RUNE_POSTAGE,
        script_pubkey: args.destination_address.script_pubkey(),
    };
    let funding_change_out = TxOut {
        value: Amount::ZERO,
        script_pubkey: args.change_address.script_pubkey(),
    };

    let outputs = vec![
        runestone_out,
        rune_change_out,
        rune_destination_out,
        funding_change_out,
    ];

    estimate_transaction_fees(
        args.script_type,
        args.number_of_inputs,
        args.current_fee_rate,
        &args.multisig_config,
        outputs,
    )
}

fn estimate_vbytes(
    inputs: usize,
    script_type: ScriptType,
    multisig_config: &Option<MultisigConfig>,
    outputs: Vec<TxOut>,
) -> usize {
    let sighash_size = match script_type {
        // For P2WSH, calculate based on the multisig configuration if provided.
        ScriptType::P2WSH => match multisig_config {
            Some(config) => ECDSA_SIGHASH_SIZE * config.required,
            None => ECDSA_SIGHASH_SIZE, // Default to single signature size if no multisig config is provided.
        },
        // For P2TR, use the fixed Schnorr signature size.
        ScriptType::P2TR => SCHNORR_SIGHASH_SIZE,
    };

    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: (0..inputs)
            .map(|_| TxIn {
                previous_output: OutPoint::null(),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                witness: Witness::from_slice(&[&vec![0; sighash_size]]),
            })
            .collect(),
        output: outputs,
    }
    .vsize()
}

pub fn calc_fees(network: Network) -> Fees {
    match network {
        Network::Bitcoin => Fees {
            commit_fee: Amount::from_sat(15_000),
            reveal_fee: Amount::from_sat(7_000),
        },
        Network::Testnet | Network::Regtest | Network::Signet => Fees {
            commit_fee: Amount::from_sat(2_500),
            reveal_fee: Amount::from_sat(4_700),
        },
        _ => panic!("unknown network"),
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Fees {
    pub commit_fee: Amount,
    pub reveal_fee: Amount,
}

impl Fees {
    pub fn sum(&self) -> u64 {
        self.commit_fee.to_sat() + self.reveal_fee.to_sat()
    }
}
