use k256::ecdsa::Signature;
use k256::ecdsa::signature::hazmat::PrehashSigner;
use serde_json::json;

use crate::derivation::signing_key_from_private_key;
use crate::providers::xrpl::{XrplAccountInfo, XrplNetworkInfo};

pub(crate) const MAX_XRPL_MEMO_BYTES: usize = 256;

pub(crate) struct SignedXrplPayment {
    pub(crate) raw_tx_hex: String,
    pub(crate) tx_hash: String,
    pub(crate) signature: String,
    pub(crate) fee_drops: u64,
    pub(crate) sequence: u32,
    #[allow(dead_code)]
    pub(crate) reserve_drops: u64,
    pub(crate) post_balance_drops: u64,
}

pub(crate) fn required_xrpl_reserve_drops(
    account: &XrplAccountInfo,
    network: &XrplNetworkInfo,
) -> Result<u64, String> {
    network
        .owner_reserve_drops
        .checked_mul(u64::from(account.owner_count))
        .and_then(|owner_reserve| network.base_reserve_drops.checked_add(owner_reserve))
        .ok_or_else(|| "XRP Ledger account reserve is too large".to_string())
}

pub(crate) fn sign_xrpl_payment(
    private_key: &[u8; 32],
    from: &str,
    to: &str,
    amount_drops: u64,
    destination_tag: Option<u32>,
    note: &str,
    account: &XrplAccountInfo,
    network: &XrplNetworkInfo,
) -> Result<SignedXrplPayment, String> {
    let note = note.trim();
    if note.as_bytes().len() > MAX_XRPL_MEMO_BYTES {
        return Err(format!(
            "XRP Ledger memo must be at most {MAX_XRPL_MEMO_BYTES} bytes"
        ));
    }

    let fee_drops = network.open_ledger_fee_drops;
    let reserve_drops = required_xrpl_reserve_drops(account, network)?;
    let total_debit = amount_drops
        .checked_add(fee_drops)
        .ok_or_else(|| "XRP amount plus fee is too large".to_string())?;
    let spendable_drops = account.balance_drops.saturating_sub(reserve_drops);
    if total_debit > spendable_drops {
        return Err(
            "Insufficient XRP balance after the required account reserve and fee".to_string(),
        );
    }
    let last_ledger_sequence = network
        .validated_ledger_index
        .checked_add(20)
        .ok_or_else(|| "XRP Ledger index is too large".to_string())?;

    let signing_key = signing_key_from_private_key(private_key)?;
    let public_key = signing_key.verifying_key().to_sec1_point(true);
    let signing_public_key = hex::encode_upper(public_key.as_bytes());

    let mut transaction = json!({
        "TransactionType": "Payment",
        "Account": from,
        "Destination": to,
        "Amount": amount_drops.to_string(),
        "Fee": fee_drops.to_string(),
        "Sequence": account.sequence,
        "LastLedgerSequence": last_ledger_sequence,
        "SigningPubKey": signing_public_key,
    });
    if let Some(destination_tag) = destination_tag {
        transaction["DestinationTag"] = json!(destination_tag);
    }
    if !note.is_empty() {
        transaction["Memos"] = json!([{
            "Memo": {
                "MemoData": hex::encode_upper(note.as_bytes()),
            }
        }]);
    }

    let signing_blob = xrpl::core::binarycodec::encode_for_signing(&transaction)
        .map_err(|error| format!("Failed to encode XRP Ledger payment for signing: {error}"))?;
    let signing_bytes = hex::decode(&signing_blob)
        .map_err(|_| "XRP Ledger signing payload is not valid hex".to_string())?;
    let signing_hash = xrpl::core::keypairs::utils::sha512_first_half(&signing_bytes);
    let signature: Signature = signing_key
        .sign_prehash(&signing_hash)
        .map_err(|_| "Failed to sign XRP Ledger payment".to_string())?;
    let signature = signature.normalize_s();
    let signature = hex::encode_upper(signature.to_der().as_bytes());
    transaction["TxnSignature"] = json!(signature);

    let raw_tx_hex = xrpl::core::binarycodec::encode(&transaction)
        .map_err(|error| format!("Failed to encode signed XRP Ledger payment: {error}"))?;
    let raw_tx = hex::decode(&raw_tx_hex)
        .map_err(|_| "Signed XRP Ledger transaction is not valid hex".to_string())?;
    let mut hash_input = Vec::with_capacity(4 + raw_tx.len());
    hash_input.extend_from_slice(b"TXN\0");
    hash_input.extend_from_slice(&raw_tx);
    let tx_hash = hex::encode_upper(xrpl::core::keypairs::utils::sha512_first_half(&hash_input));

    Ok(SignedXrplPayment {
        raw_tx_hex,
        tx_hash,
        signature,
        fee_drops,
        sequence: account.sequence,
        reserve_drops,
        post_balance_drops: account.balance_drops - total_debit,
    })
}

#[cfg(test)]
#[path = "../tests/tx/xrpl.rs"]
mod tests;
