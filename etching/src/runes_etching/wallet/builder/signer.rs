use super::taproot::TaprootPayload;
use super::Utxo;
use crate::runes_etching::etching_state::{init_etching_account_info, read_state};
use crate::runes_etching::management::sign_with_ecdsa;
use crate::runes_etching::{OrdError, OrdResult};
use bitcoin::hashes::Hash;
use bitcoin::key::Secp256k1;
use bitcoin::secp256k1::ecdsa::Signature;
use bitcoin::secp256k1::{self, All, Message};
use bitcoin::sighash::{Prevouts, SighashCache};
use bitcoin::taproot::{ControlBlock, LeafVersion};
use bitcoin::{Address, PublicKey, ScriptBuf, TapLeafHash, TapSighashType, Transaction, Witness};
use ic_ic00_types::DerivationPath;
use serde_bytes::ByteBuf;

/// An Ordinal-aware Bitcoin wallet.
#[derive(Clone)]
pub struct Wallet {
  pub signer: MixSigner,
  secp: Secp256k1<All>,
}

impl Wallet {
  pub fn new_with_signer(signer: MixSigner) -> Self {
    Self {
      signer,
      secp: Secp256k1::new(),
    }
  }

  pub async fn sign_commit_transaction(
    &mut self,
    own_pubkey: &PublicKey,
    inputs: &[Utxo],
    transaction: Transaction,
    txin_script: &ScriptBuf,
  ) -> OrdResult<Transaction> {
    self
      .sign_ecdsa(own_pubkey, inputs, transaction, txin_script)
      .await
  }

  pub fn sign_reveal_transaction_schnorr(
    &mut self,
    taproot: &TaprootPayload,
    redeem_script: &ScriptBuf,
    transaction: Transaction,
  ) -> OrdResult<Transaction> {
    let prevouts_array = vec![taproot.prevouts.clone()];
    let prevouts = Prevouts::All(&prevouts_array);

    let mut sighash_cache = SighashCache::new(transaction.clone());
    let sighash_sig = sighash_cache
      .taproot_script_spend_signature_hash(
        0,
        &prevouts,
        TapLeafHash::from_script(redeem_script, LeafVersion::TapScript),
        TapSighashType::Default,
      )
      .map_err(|e| OrdError::Custom(e.to_string()))?;

    let msg = secp256k1::Message::from_digest(sighash_sig.to_byte_array());
    let sig = self.secp.sign_schnorr_no_aux_rand(&msg, &taproot.keypair);

    // verify
    self
      .secp
      .verify_schnorr(&sig, &msg, &taproot.keypair.x_only_public_key().0)?;

    // append witness
    let signature = bitcoin::taproot::Signature {
      signature: sig,
      sighash_type: TapSighashType::Default,
    }
    .into();
    self.append_witness_to_input(
      &mut sighash_cache,
      signature,
      0,
      &taproot.keypair.public_key(),
      Some(redeem_script),
      Some(&taproot.control_block),
    )?;

    Ok(sighash_cache.into_transaction())
  }

  async fn sign_ecdsa(
    &mut self,
    own_pubkey: &PublicKey,
    utxos: &[Utxo],
    transaction: Transaction,
    script: &ScriptBuf,
  ) -> OrdResult<Transaction> {
    let mut hash = SighashCache::new(transaction.clone());
    for (index, input) in utxos.iter().enumerate() {
      let sighash = hash
        .p2wpkh_signature_hash(index, script, input.amount, bitcoin::EcdsaSighashType::All)
        .map_err(|e| OrdError::Custom(e.to_string()))?;
      let message = Message::from(sighash);
      let signature = self.signer.sign_with_ecdsa(message).await?;

      // append witness
      let signature = bitcoin::ecdsa::Signature::sighash_all(signature).into();
      self.append_witness_to_input(&mut hash, signature, index, &own_pubkey.inner, None, None)?;
    }

    Ok(hash.into_transaction())
  }

  fn append_witness_to_input(
    &self,
    sighasher: &mut SighashCache<Transaction>,
    signature: OrdSignature,
    index: usize,
    pubkey: &secp256k1::PublicKey,
    redeem_script: Option<&ScriptBuf>,
    control_block: Option<&ControlBlock>,
  ) -> OrdResult<()> {
    // push redeem script if necessary
    let witness = if let Some(redeem_script) = redeem_script {
      let mut witness = Witness::new();
      match signature {
        OrdSignature::Ecdsa(signature) => witness.push_ecdsa_signature(&signature),
        OrdSignature::Schnorr(signature) => witness.push(signature.to_vec()),
      }
      witness.push(redeem_script.as_bytes());
      if let Some(control_block) = control_block {
        witness.push(control_block.serialize());
      }
      witness
    } else {
      // otherwise, push pubkey
      match signature {
        OrdSignature::Ecdsa(signature) => Witness::p2wpkh(&signature, pubkey),
        OrdSignature::Schnorr(_) => return Err(OrdError::UnexpectedSignature),
      }
    };
    // append witness
    *sighasher
      .witness_mut(index)
      .ok_or(OrdError::InputNotFound(index))? = witness;

    Ok(())
  }
}

enum OrdSignature {
  Schnorr(bitcoin::taproot::Signature),
  Ecdsa(bitcoin::ecdsa::Signature),
}

impl From<bitcoin::taproot::Signature> for OrdSignature {
  fn from(sig: bitcoin::taproot::Signature) -> Self {
    Self::Schnorr(sig)
  }
}

impl From<bitcoin::ecdsa::Signature> for OrdSignature {
  fn from(sig: bitcoin::ecdsa::Signature) -> Self {
    Self::Ecdsa(sig)
  }
}

#[derive(Clone)]
pub struct MixSigner {
  pub pubkey: PublicKey,
  pub signer_addr: Address,
}

impl MixSigner {
  pub fn new(public_key: PublicKey, addr: Address) -> Self {
    Self {
      pubkey: public_key,
      signer_addr: addr,
    }
  }

  pub async fn sign_with_ecdsa(&self, message: Message) -> OrdResult<Signature> {
    let key_name = read_state(|s| s.ecdsa_key_name.clone());
    let etching_account = init_etching_account_info().await;
    let sighash = *message.as_ref();
    let sec1_signature = sign_with_ecdsa(
      key_name,
      DerivationPath::new(vec![ByteBuf::from(etching_account.derive_path.as_bytes())]),
      sighash,
    )
    .await
    .map_err(|_| OrdError::UnexpectedSignature)?;
    Signature::from_compact(sec1_signature.as_slice()).map_err(OrdError::Signature)
  }
}
