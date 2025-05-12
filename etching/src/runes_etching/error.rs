use thiserror::Error;

/// Ordinal transaction handling error types
#[derive(Error, Debug)]
pub enum OrdError {
    #[error("when using P2TR, the taproot keypair option must be provided")]
    TaprootKeypairNotProvided,
    #[error("Hex codec error: {0}")]
    HexCodec(#[from] hex::FromHexError),
    #[error("Ord codec error: {0}")]
    Codec(#[from] serde_json::Error),
    #[error("Bitcoin script error: {0}")]
    PushBytes(#[from] bitcoin::script::PushBytesError),
    #[error("Bad transaction input: {0}")]
    InputNotFound(usize),
    #[error("Insufficient balance")]
    InsufficientBalance { required: u64, available: u64 },
    #[error("Invalid signature: {0}")]
    Signature(#[from] bitcoin::secp256k1::Error),
    #[error("Invalid signature")]
    UnexpectedSignature,
    #[error("Taproot builder error: {0}")]
    TaprootBuilder(#[from] bitcoin::taproot::TaprootBuilderError),
    #[error("Taproot compute error")]
    TaprootCompute,
    #[error("Scripterror: {0}")]
    Script(#[from] bitcoin::blockdata::script::Error),
    #[error("No transaction inputs")]
    NoInputs,
    #[error("Invalid UTF-8 in: {0}")]
    Utf8Encoding(#[from] std::str::Utf8Error),
    #[error("Inscription parser error: {0}")]
    InscriptionParser(#[from] InscriptionParseError),
    #[error("Invalid inputs")]
    InvalidInputs,
    #[error("Invalid script type")]
    InvalidScriptType,
    #[error("custom error: {0}")]
    Custom(String),
}

/// Inscription parsing errors.
#[derive(Error, Debug)]
pub enum InscriptionParseError {
    #[error("invalid transaction id: {0}")]
    Txid(#[from] bitcoin::hashes::hex::HexToArrayError),
    #[error("invalid character: {0}")]
    Character(char),
    #[error("invalid MIME type format")]
    ContentType,
    #[error("invalid length: {0}")]
    InscriptionIdLength(usize),
    #[error("unexpected opcode token")]
    UnexpectedOpcode,
    #[error("unexpected push bytes token")]
    UnexpectedPushBytes,
    #[error("bad data syntax")]
    BadDataSyntax,
    #[error("invalid separator: {0}")]
    CharacterSeparator(char),
    #[error("invalid index: {0}")]
    Index(#[from] std::num::ParseIntError),
    #[error("content of envelope: {0}")]
    ParsedEnvelope(String),
    #[error("cannot convert non-Ordinal inscription to Nft")]
    NotOrdinal,
    #[error("cannot convert non-Brc20 inscription to Brc20")]
    NotBrc20,
}

use ic_cdk::api::call::RejectionCode;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Represents an error from a management canister call, such as
/// `sign_with_ecdsa` or `bitcoin_send_transaction`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallError {
    pub method: String,
    pub reason: Reason,
}

impl CallError {
    /// Returns the name of the method that resulted in this error.
    pub fn method(&self) -> &str {
        &self.method
    }

    /// Returns the failure reason.
    pub fn reason(&self) -> &Reason {
        &self.reason
    }
}

impl fmt::Display for CallError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            fmt,
            "management call '{}' failed: {}",
            self.method, self.reason
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// The reason for the management call failure.
pub enum Reason {
    /// Failed to send a signature request because the local output queue is
    /// full.
    QueueIsFull,
    /// The canister does not have enough cycles to submit the request.
    OutOfCycles,
    /// The call failed with an error.
    CanisterError(String),
    /// The management canister rejected the signature request (not enough
    /// cycles, the ECDSA subnet is overloaded, etc.).
    Rejected(String),
}

impl fmt::Display for Reason {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QueueIsFull => write!(fmt, "the canister queue is full"),
            Self::OutOfCycles => write!(fmt, "the canister is out of cycles"),
            Self::CanisterError(msg) => write!(fmt, "canister error: {}", msg),
            Self::Rejected(msg) => {
                write!(fmt, "the management canister rejected the call: {}", msg)
            }
        }
    }
}

impl Reason {
    pub fn from_reject(reject_code: RejectionCode, reject_message: String) -> Self {
        match reject_code {
            RejectionCode::CanisterReject => Self::Rejected(reject_message),
            _ => Self::CanisterError(reject_message),
        }
    }
}
