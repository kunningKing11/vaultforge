use k256::ecdsa::signature::hazmat::PrehashSigner;
use sha2::{Digest, Sha256};

use crate::derivation::{tron_address_from_private_key, tron_private_key_from_mnemonic};
use crate::providers::tron::{TRON_NATIVE_FEE_SUN, create_tron_transfer};

pub(crate) struct SignedTronTransfer {
    pub(crate) txid: String,
    pub(crate) raw_tx: serde_json::Value,
    pub(crate) signature: String,
    pub(crate) fee_sun: u64,
}

pub(crate) fn sign_tron_unsigned_transfer(
    mnemonic: &str,
    from: &str,
    mut unsigned_tx: serde_json::Value,
) -> Result<SignedTronTransfer, String> {
    let private_key = tron_private_key_from_mnemonic(mnemonic)?;
    let derived = tron_address_from_private_key(&private_key)?;
    if derived != from {
        return Err("Tron signing key does not match from address".to_string());
    }

    let raw_data_hex = unsigned_tx["raw_data_hex"]
        .as_str()
        .ok_or_else(|| "Tron unsigned transaction missing raw_data_hex".to_string())?;
    let raw_data_bytes = hex::decode(raw_data_hex)
        .map_err(|_| "Invalid Tron raw_data_hex".to_string())?;
    let txid_bytes = Sha256::digest(&raw_data_bytes);
    let computed_txid = hex::encode(txid_bytes);
    let txid = unsigned_tx["txID"]
        .as_str()
        .map(|value| value.to_string())
        .unwrap_or_else(|| computed_txid.clone());
    if !txid.eq_ignore_ascii_case(&computed_txid) {
        return Err("Tron txID does not match raw_data_hex".to_string());
    }

    let signing_key = k256::ecdsa::SigningKey::from_bytes((&private_key).into())
        .map_err(|_| "Invalid Tron private key".to_string())?;
    let signature: k256::ecdsa::Signature = signing_key
        .sign_prehash(&txid_bytes)
        .map_err(|_| "Tron transaction signing failed".to_string())?;
    let recovery_id = recovery_id_for_signature(signing_key.verifying_key(), &txid_bytes, &signature)?;
    let mut signature_bytes = signature.to_bytes().to_vec();
    signature_bytes.push(recovery_id.to_byte());
    let signature_hex = hex::encode(signature_bytes);

    unsigned_tx["txID"] = serde_json::Value::String(txid.clone());
    unsigned_tx["signature"] = serde_json::json!([signature_hex.clone()]);

    Ok(SignedTronTransfer {
        txid,
        raw_tx: unsigned_tx,
        signature: signature_hex,
        fee_sun: TRON_NATIVE_FEE_SUN,
    })
}

fn recovery_id_for_signature(
    verifying_key: &k256::ecdsa::VerifyingKey,
    txid_bytes: &[u8],
    signature: &k256::ecdsa::Signature,
) -> Result<k256::ecdsa::RecoveryId, String> {
    for is_odd in [false, true] {
        let recovery_id = k256::ecdsa::RecoveryId::new(is_odd, false);
        if let Ok(recovered) =
            k256::ecdsa::VerifyingKey::recover_from_prehash(txid_bytes, signature, recovery_id)
            && &recovered == verifying_key
        {
            return Ok(recovery_id);
        }
    }
    Err("Failed to recover Tron signature id".to_string())
}

pub(crate) async fn sign_tron_transfer(
    mnemonic: &str,
    from: &str,
    to: &str,
    amount_sun: u64,
) -> Result<SignedTronTransfer, String> {
    let unsigned_tx = create_tron_transfer(from, to, amount_sun).await?;
    sign_tron_unsigned_transfer(mnemonic, from, unsigned_tx)
}
