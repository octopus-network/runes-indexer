use bitcoin::absolute::LockTime;
use bitcoin::transaction::Version;
use bitcoin::{Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness};
use ordinals::Runestone as OrdRunestone;

use crate::runes_etching::constants::POSTAGE;
use crate::runes_etching::wallet::EtchingTransactionArgs;
use crate::runes_etching::{OrdResult, OrdTransactionBuilder};

pub const RUNE_POSTAGE: Amount = Amount::from_sat(10_000);

impl OrdTransactionBuilder {
    /// Create the reveal transaction
    pub async fn build_etching_transaction(
        &mut self,
        args: EtchingTransactionArgs,
    ) -> OrdResult<Transaction> {
        let previous_output = OutPoint {
            txid: args.input.id,
            vout: args.input.index,
        };
        let runestone = OrdRunestone::from(args.runestone);
        let btc_030_script = runestone.encipher();
        let btc_031_script = ScriptBuf::from_bytes(btc_030_script.to_bytes());

        // tx out
        let tx_out = vec![
            TxOut {
                value: Amount::from_sat(POSTAGE),
                script_pubkey: args.recipient_address.script_pubkey(),
            },
            TxOut {
                value: Amount::from_sat(POSTAGE),
                script_pubkey: args.recipient_address.script_pubkey(),
            },
            TxOut {
                value: Amount::from_sat(0),
                script_pubkey: btc_031_script,
            },
        ];
        // txin
        let tx_in = vec![TxIn {
            previous_output,
            script_sig: ScriptBuf::new(),
            sequence: Sequence::from_consensus(0xfffffffd),
            witness: Witness::new(),
        }];

        // make transaction and sign it
        let unsigned_tx = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: tx_in,
            output: tx_out,
        };
        let tx = match self.taproot_payload.as_ref() {
            Some(taproot_payload) => self.signer.sign_reveal_transaction_schnorr(
                taproot_payload,
                &args.redeem_script,
                unsigned_tx,
            ),
            None => {
                panic!("taproot error");
            }
        }?;

        Ok(tx)
    }
}
