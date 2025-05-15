//! NFT
//!
//! Closely follows <https://github.com/ordinals/ord/blob/master/src/inscriptions/inscription.rs>

use std::mem;
use std::str::FromStr;

use base64::Engine;
use bitcoin::constants::MAX_SCRIPT_ELEMENT_SIZE;
use bitcoin::opcodes;
use bitcoin::opcodes::all::OP_CHECKSIG;
use bitcoin::script::{Builder as ScriptBuilder, PushBytes, PushBytesBuf, ScriptBuf};
use serde::{Deserialize, Serialize};

use crate::runes_etching::push_bytes::bytes_to_push_bytes;
use crate::runes_etching::wallet::RedeemScriptPubkey;
use crate::runes_etching::{
  constants, Inscription, InscriptionParseError, LogoParams, OrdError, OrdResult,
};

/// Represents an arbitrary Ordinal inscription.
///
/// We're "unofficially" referring to this as an NFT (e.g., like an ERC721 token).
///
/// Ordinal inscriptions allow for the embedding of data directly
/// into individual satoshis on the Bitcoin blockchain, enabling a unique form of digital
/// artifact creation and ownership tracking.
///
/// NFTs may include fields before an optional body. Each field consists of two data pushes,
/// a tag and a value.
///
/// [Reference](https://docs.ordinals.com/inscriptions.html#fields)
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Nft {
  /// The main body of the NFT. This is the core data or content of the NFT,
  /// which might represent an image, text, or other types of digital assets.
  pub body: Option<Vec<u8>>,
  /// Has a tag of 1, representing the MIME type of the body. This describes
  /// the format of the body content, such as "image/png" or "text/plain".
  pub content_type: Option<Vec<u8>>,
  /// Has a tag of 2, representing the position of the inscribed satoshi in the outputs.
  /// It is used to locate the specific satoshi that carries this inscription.
  pub pointer: Option<Vec<u8>>,
  /// Has a tag of 3, representing the parent NFTs, i.e., the owner of an NFT
  /// can create child NFTs, establishing a hierarchy or a collection of related NFTs.
  pub parents: Vec<Vec<u8>>,
  /// Has a tag of 5, representing CBOR (Concise Binary Object Representation) metadata,
  /// stored as data pushes. This is used for storing structured metadata in a compact format.
  pub metadata: Option<Vec<u8>>,
  /// Has a tag of 7, representing the metaprotocol identifier. This field specifies
  /// which metaprotocol, if any, this NFT follows, allowing for interoperability and
  /// standardized behavior across different systems.
  pub metaprotocol: Option<Vec<u8>>,
  /// Indicates if any field is incomplete. This is used
  /// to signal that the data is partially constructed or awaiting further information.
  pub incomplete_field: bool,
  /// Indicates if there are any duplicate fields. Duplicate fields
  /// could arise from errors in data entry or processing and may need resolution for the
  /// this structure to be considered valid.
  pub duplicate_field: bool,
  /// Has a tag of 9, representing the encoding of the body. This specifies how the body content
  /// is encoded, such as "base64" or "utf-8", providing guidance on how to interpret or display the content.
  pub content_encoding: Option<Vec<u8>>,
  /// Indicates if there are any unrecognized even fields. Even tags are reserved for future use
  /// and should not be used by current implementations.
  pub unrecognized_even_field: bool,
  /// Has a tag of 11, representing a nominated NFT. Used to delegate certain rights or
  /// attributes from one NFT to another, effectively linking them in a specified relationship.
  pub delegate: Option<Vec<u8>>,

  pub logo: Option<LogoParams>,
  /// Has a tag of 13, denoting whether or not this inscription caries any rune.
  pub rune: Option<Vec<u8>>,
}

impl Nft {
  /// Creates a new `Nft` with optional data.
  pub fn new(
    content_type: Option<Vec<u8>>,
    body: Option<Vec<u8>>,
    logo: Option<LogoParams>,
  ) -> Self {
    Self {
      content_type,
      body,
      logo,
      ..Default::default()
    }
  }

  pub fn append_reveal_script_to_builder(
    &self,
    builder: ScriptBuilder,
  ) -> OrdResult<ScriptBuilder> {
    let mut pbb = PushBytesBuf::new();
    pbb
      .extend_from_slice(self.rune.clone().unwrap().as_slice())
      .unwrap();
    let mut builder = builder
      .push_opcode(opcodes::OP_FALSE)
      .push_opcode(opcodes::all::OP_IF)
      .push_slice(constants::PROTOCOL_ID)
      .push_opcode(opcodes::all::OP_PUSHNUM_13)
      .push_slice::<&PushBytes>(pbb.as_push_bytes());
    if let Some(l) = self.logo.clone() {
      Self::append(
        constants::CONTENT_TYPE_TAG,
        &mut builder,
        &Some(l.content_type.as_bytes().to_vec()),
      );
      let hex = base64::engine::general_purpose::STANDARD
        .decode(l.content_base64)
        .unwrap();
      builder = builder.push_slice(constants::BODY_TAG);
      for ch in hex.chunks(MAX_SCRIPT_ELEMENT_SIZE) {
        builder = builder.push_slice::<&PushBytes>(ch.try_into().unwrap());
      }
    } else {
      if self.body.is_some() && self.content_type.is_some() {
        Self::append(
          constants::CONTENT_TYPE_TAG,
          &mut builder,
          &self.content_type,
        );
        if let Some(body) = &self.body {
          builder = builder.push_slice(constants::BODY_TAG);
          for chunk in body.chunks(MAX_SCRIPT_ELEMENT_SIZE) {
            builder = builder.push_slice::<&PushBytes>(chunk.try_into().unwrap());
          }
        }
      }
    }

    builder = builder.push_opcode(opcodes::all::OP_ENDIF);
    println!("{}", builder.as_script().to_string());
    Ok(builder)
  }

  fn append(tag: [u8; 1], builder: &mut ScriptBuilder, value: &Option<Vec<u8>>) {
    if let Some(value) = value {
      let mut tmp = ScriptBuilder::new();
      mem::swap(&mut tmp, builder);

      if is_chunked(tag) {
        for chunk in value.chunks(MAX_SCRIPT_ELEMENT_SIZE) {
          tmp = tmp
            .push_slice::<&PushBytes>(tag.as_slice().try_into().unwrap())
            .push_slice::<&PushBytes>(chunk.try_into().unwrap());
        }
      } else {
        tmp = tmp
          .push_slice::<&PushBytes>(tag.as_slice().try_into().unwrap())
          .push_slice::<&PushBytes>(value.as_slice().try_into().unwrap());
      }

      mem::swap(&mut tmp, builder);
    }
  }

  /// Validates the NFT's content type.
  fn validate_content_type(&self) -> OrdResult<Self> {
    if let Some(content_type) = &self.content_type {
      let content_type_str = std::str::from_utf8(content_type).map_err(OrdError::Utf8Encoding)?;

      if !content_type_str.contains('/') {
        return Err(OrdError::InscriptionParser(
          InscriptionParseError::ContentType,
        ));
      }
    }

    Ok(self.clone())
  }

  /// Creates a new `Nft` from JSON-encoded string.
  pub fn from_json_str(data: &str) -> OrdResult<Self> {
    Self::from_str(data)?.validate_content_type()
  }

  /// Returns `Self` as a JSON-encoded data to be pushed to the redeem script.
  pub fn as_push_bytes(&self) -> OrdResult<PushBytesBuf> {
    bytes_to_push_bytes(self.encode()?.as_bytes())
  }

  pub fn body(&self) -> Option<&str> {
    std::str::from_utf8(self.body.as_ref()?).ok()
  }

  pub fn content_type(&self) -> Option<&str> {
    std::str::from_utf8(self.content_type.as_ref()?).ok()
  }

  pub fn reveal_script_as_scriptbuf(&self, builder: ScriptBuilder) -> OrdResult<ScriptBuf> {
    Ok(self.append_reveal_script_to_builder(builder)?.into_script())
  }
}

impl FromStr for Nft {
  type Err = OrdError;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    serde_json::from_str(s).map_err(OrdError::from)
  }
}

impl Inscription for Nft {
  fn generate_redeem_script(
    &self,
    builder: ScriptBuilder,
    pubkey: RedeemScriptPubkey,
  ) -> OrdResult<ScriptBuilder> {
    let encoded_pubkey = pubkey.encode()?;

    let builder = builder
      .push_slice(encoded_pubkey.as_push_bytes())
      .push_opcode(OP_CHECKSIG);

    self.append_reveal_script_to_builder(builder)
  }

  fn content_type(&self) -> String {
    match self.content_type() {
      Some(t) => t.to_string(),
      None => "".to_string(),
    }
  }

  fn data(&self) -> OrdResult<PushBytesBuf> {
    bytes_to_push_bytes(self.encode()?.as_bytes())
  }
}

fn is_chunked(tag: [u8; 1]) -> bool {
  matches!(tag, constants::METADATA_TAG)
}
