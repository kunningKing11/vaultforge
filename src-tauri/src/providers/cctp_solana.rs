//! Native Solana CCTP evidence and on-chain attester-policy verification.
//! This module never signs, broadcasts, or treats Solana as an EVM chain.

use base64::Engine;
use sha2::{Digest as ShaDigest, Sha256};
use sha3::Keccak256;
use solana_pubkey::Pubkey;
use std::str::FromStr;

use crate::cctp::{CctpTransfer, SOLANA_MESSENGER, SOLANA_TRANSMITTER, address_word};
use crate::cctp_attestation::{BurnEvidence, validate_message_intent};
use crate::operations::Operation;
use crate::providers::http::{json_rpc_result, rpc_post};
use crate::providers::solana::fetch_solana_tx_observation;
use crate::tx::tx_status::{Execution, TxStatus};

fn discriminator(name: &str) -> [u8; 8] {
    Sha256::digest(name.as_bytes())[..8]
        .try_into()
        .expect("digest length")
}
fn pubkey(value: &str) -> Result<Pubkey, String> {
    Pubkey::from_str(value).map_err(|_| "Invalid Solana CCTP public key".into())
}
fn bytes(value: &serde_json::Value) -> Result<Vec<u8>, String> {
    if value["data"][1] != "base64" {
        return Err("Solana CCTP account must use base64 encoding".into());
    }
    let encoded = value["data"][0]
        .as_str()
        .ok_or("Missing Solana account data")?;
    if encoded.len() > 16384 {
        return Err("Solana CCTP account is too large".into());
    }
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| "Invalid Solana account data".into())
}
pub(crate) fn account_data(value: &serde_json::Value, owner: &str) -> Result<Vec<u8>, String> {
    if value["owner"] != owner || value["executable"] != false {
        return Err("Solana CCTP account owner mismatch".into());
    }
    bytes(value)
}
pub(crate) fn keys(transaction: &serde_json::Value) -> Result<Vec<String>, String> {
    let mut keys = transaction["transaction"]["message"]["accountKeys"]
        .as_array()
        .ok_or("Missing Solana transaction keys")?
        .iter()
        .map(|key| {
            key.as_str()
                .map(str::to_string)
                .ok_or("Invalid Solana transaction key".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    for field in ["writable", "readonly"] {
        if let Some(loaded) = transaction["meta"]["loadedAddresses"][field].as_array() {
            for key in loaded {
                keys.push(key.as_str().ok_or("Invalid loaded Solana key")?.into());
            }
        }
    }
    if keys.len() > 256 {
        return Err("Too many Solana transaction keys".into());
    }
    Ok(keys)
}
pub(crate) fn instruction_key<'a>(
    instruction: &serde_json::Value,
    position: usize,
    keys: &'a [String],
) -> Result<&'a str, String> {
    let index = instruction["accounts"][position]
        .as_u64()
        .ok_or("Missing Solana instruction account")? as usize;
    keys.get(index)
        .map(String::as_str)
        .ok_or_else(|| "Solana instruction account out of range".into())
}
pub(crate) fn instruction_data(instruction: &serde_json::Value) -> Result<Vec<u8>, String> {
    let encoded = instruction["data"]
        .as_str()
        .ok_or("Missing Solana instruction data")?;
    if encoded.len() > 16384 {
        return Err("Solana instruction is too large".into());
    }
    bs58::decode(encoded)
        .into_vec()
        .map_err(|_| "Invalid Solana instruction data".into())
}

/// Prove the event account was created by the matching executed deposit, with a
/// trusted MessageTransmitter CPI and a successful transaction in the same slot.
pub(crate) fn decode_source_evidence(
    operation: &Operation,
    proof: &serde_json::Value,
    status: &TxStatus,
    signature: &str,
) -> Result<BurnEvidence, String> {
    let transfer = operation.cctp_transfer()?;
    let transaction = &proof["transaction"];
    if transfer.route.source.network.id != "solana"
        || status.execution != Execution::Succeeded
        || status.slot.is_none()
        || status.slot != transaction["slot"].as_u64()
        || transaction["meta"].get("err") != Some(&serde_json::Value::Null)
        || transaction["meta"].is_null()
        || transaction["transaction"]["signatures"][0] != signature
        || bs58::decode(signature)
            .into_vec()
            .map_err(|_| "Invalid Solana signature")?
            .len()
            != 64
    {
        return Err("Invalid Solana burn transaction evidence".into());
    }
    let account = proof["message_account_address"]
        .as_str()
        .ok_or("Missing MessageSent account address")?;
    let data = account_data(&proof["message_account"], SOLANA_TRANSMITTER)?;
    if data.len() != 428
        || data[..8] != discriminator("account:MessageSent")
        || u32::from_le_bytes(
            data[48..52]
                .try_into()
                .map_err(|_| "Invalid MessageSent length")?,
        ) != 376
    {
        return Err("Invalid Solana MessageSent account".into());
    }
    if let Some(block_time) = transaction["blockTime"].as_i64() {
        if i64::from_le_bytes(
            data[40..48]
                .try_into()
                .map_err(|_| "Invalid MessageSent timestamp")?,
        ) != block_time
        {
            return Err("MessageSent creation time does not match burn".into());
        }
    }
    let emitted = data[52..].to_vec();
    validate_message_intent(&transfer, &emitted)?;
    if emitted[12..44] != [0; 32] || emitted[144..148] != [0; 4] || emitted[312..376] != [0; 64] {
        return Err("Invalid emitted Solana message fields".into());
    }
    let keys = keys(transaction)?;
    let required = transaction["transaction"]["message"]["header"]["numRequiredSignatures"]
        .as_u64()
        .ok_or("Missing signer count")? as usize;
    let signature_count = transaction["transaction"]["signatures"]
        .as_array()
        .ok_or("Missing Solana transaction signatures")?
        .len();
    if required == 0 || required > keys.len() || signature_count != required {
        return Err("Invalid Solana signer metadata".into());
    }
    let instructions = transaction["transaction"]["message"]["instructions"]
        .as_array()
        .ok_or("Missing Solana burn instructions")?;
    let mut matching = Vec::new();
    for (index, instruction) in instructions.iter().enumerate() {
        let program = instruction["programIdIndex"]
            .as_u64()
            .and_then(|index| keys.get(index as usize));
        if program.map(String::as_str) != Some(SOLANA_MESSENGER) {
            continue;
        }
        let params = instruction_data(instruction)?;
        if params.len() != 96 || params[..8] != discriminator("global:deposit_for_burn") {
            continue;
        }
        if params[8..16]
            != u64::try_from(transfer.amount())
                .map_err(|_| "Solana burn amount overflows")?
                .to_le_bytes()
            || params[16..20] != transfer.route.destination.domain.to_le_bytes()
            || params[20..52] != transfer.mint_recipient_word()?
            || params[52..84] != transfer.caller_word()?
            || params[84..92] != [0; 8]
            || params[92..96] != 2000u32.to_le_bytes()
        {
            continue;
        }
        if instruction_key(instruction, 0, &keys)? != transfer.sender()
            || instruction_key(instruction, 10, &keys)? != transfer.route.source.usdc
            || instruction_key(instruction, 11, &keys)? != account
            || instruction_key(instruction, 12, &keys)? != SOLANA_TRANSMITTER
            || instruction_key(instruction, 13, &keys)? != SOLANA_MESSENGER
            || data[8..40] != pubkey(instruction_key(instruction, 1, &keys)?)?.to_bytes()
            || instruction["accounts"][0]
                .as_u64()
                .ok_or("Missing burn signer")? as usize
                >= required
            || instruction["accounts"][11]
                .as_u64()
                .ok_or("Missing MessageSent signer")? as usize
                >= required
        {
            return Err("Solana burn instruction account mismatch".into());
        }
        let inner = transaction["meta"]["innerInstructions"]
            .as_array()
            .ok_or("Solana burn CPI evidence missing")?
            .iter()
            .find(|inner| inner["index"].as_u64() == Some(index as u64))
            .ok_or("Solana burn CPI missing")?;
        let mut sends = 0;
        for instruction in inner["instructions"]
            .as_array()
            .ok_or("Invalid Solana CPI instructions")?
        {
            let program = instruction["programIdIndex"]
                .as_u64()
                .and_then(|index| keys.get(index as usize));
            if program.map(String::as_str) != Some(SOLANA_TRANSMITTER) {
                continue;
            }
            let params = instruction_data(instruction)?;
            if params.len() != 312 || params[..8] != discriminator("global:send_message") {
                continue;
            }
            if instruction_key(instruction, 3, &keys)? != account
                || instruction_key(instruction, 4, &keys)? != SOLANA_MESSENGER
                || params[8..12] != transfer.route.destination.domain.to_le_bytes()
                || params[12..44]
                    != address_word(
                        &transfer.route.destination.network,
                        transfer.route.destination.token_messenger,
                    )?
                || params[44..76] != transfer.caller_word()?
                || params[76..80] != 2000u32.to_le_bytes()
                || params[80..84] != 228u32.to_le_bytes()
                || params[84..] != emitted[148..]
            {
                return Err("Solana MessageTransmitter CPI mismatch".into());
            }
            sends += 1;
        }
        if sends != 1 {
            return Err("Solana burn message CPI is ambiguous".into());
        }
        matching.push(index);
    }
    if matching.len() != 1 {
        return Err("Solana burn instruction is missing or ambiguous".into());
    }
    Ok(BurnEvidence {
        transaction_hash: signature.into(),
        block_hash: None,
        block_height: None,
        message_log_index: None,
        deposit_log_index: None,
        emitted_message: emitted,
        slot: status.slot,
        message_account: Some(account.into()),
    })
}

pub(crate) async fn verify_mainnet(
    client: &reqwest::Client,
    transfer: &CctpTransfer,
    destination: bool,
) -> Result<(), String> {
    let network = if destination {
        &transfer.route.destination.network
    } else {
        &transfer.route.source.network
    };
    let response = rpc_post(
        client,
        network.rpc_url()?,
        &serde_json::json!({"jsonrpc":"2.0","id":1,"method":"getGenesisHash","params":[]}),
    )
    .await?;
    if json_rpc_result(&response, "Solana genesis hash")?
        != "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d"
    {
        return Err("Solana RPC does not match the mainnet CCTP deployment".into());
    }
    Ok(())
}

pub(crate) async fn fetch_source_evidence(
    client: &reqwest::Client,
    transfer: &CctpTransfer,
    signature: &str,
) -> Result<Option<(serde_json::Value, TxStatus)>, String> {
    verify_mainnet(client, transfer, false).await?;
    let network = &transfer.route.source.network;
    let response=rpc_post(client,network.rpc_url()?,&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"getTransaction","params":[signature,{"encoding":"json","commitment":"confirmed","maxSupportedTransactionVersion":0}]})).await?;
    let transaction = json_rpc_result(&response, "Solana CCTP transaction")?;
    if transaction.is_null() {
        return Ok(None);
    }
    let Some(status) = fetch_solana_tx_observation(client, network, signature).await? else {
        return Ok(None);
    };
    let keys = keys(transaction)?;
    let mut candidates = Vec::new();
    for instruction in transaction["transaction"]["message"]["instructions"]
        .as_array()
        .ok_or("Solana burn instructions missing")?
    {
        if instruction["programIdIndex"]
            .as_u64()
            .and_then(|index| keys.get(index as usize))
            .map(String::as_str)
            != Some(SOLANA_MESSENGER)
        {
            continue;
        }
        let data = instruction_data(instruction)?;
        if data.len() == 96 && data[..8] == discriminator("global:deposit_for_burn") {
            candidates.push(instruction_key(instruction, 11, &keys)?.to_string());
        }
    }
    // Fetch candidates and let the strict decoder select the one bound deposit.
    if candidates.len() != 1 {
        return Err("Solana source burn selection is ambiguous".into());
    }
    let address = &candidates[0];
    let slot = transaction["slot"]
        .as_u64()
        .ok_or("Missing Solana burn slot")?;
    let response=rpc_post(client,network.rpc_url()?,&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"getAccountInfo","params":[address,{"encoding":"base64","commitment":"confirmed","minContextSlot":slot}]})).await?;
    let account = json_rpc_result(&response, "Solana MessageSent account")?;
    if account["context"]["slot"]
        .as_u64()
        .is_none_or(|context| context < slot)
        || account["value"].is_null()
    {
        return Err("Solana MessageSent account is unavailable or stale".into());
    }
    Ok(Some((
        serde_json::json!({"transaction":transaction,"message_account_address":address,"message_account":account["value"]}),
        status,
    )))
}

/// Verify exactly the enabled on-chain threshold, ordered distinct recovered
/// attesters, and low-s signatures, matching Circle's native verifier. This is
/// signature validation, not a claim that a destination mint was simulated.
pub(crate) fn verify_attesters(
    state: &[u8],
    message: &[u8],
    attestation: &[u8],
) -> Result<(), String> {
    if state.len() < 153
        || state[..8] != discriminator("account:MessageTransmitter")
        || state[136] != 0
        || u32::from_le_bytes(
            state[137..141]
                .try_into()
                .map_err(|_| "Invalid Solana domain")?,
        ) != 5
        || u32::from_le_bytes(
            state[141..145]
                .try_into()
                .map_err(|_| "Invalid Solana message version")?,
        ) != 1
    {
        return Err("Invalid Solana MessageTransmitter state".into());
    }
    let threshold = u32::from_le_bytes(
        state[145..149]
            .try_into()
            .map_err(|_| "Invalid signature threshold")?,
    ) as usize;
    let count = u32::from_le_bytes(
        state[149..153]
            .try_into()
            .map_err(|_| "Invalid attester count")?,
    ) as usize;
    if threshold == 0
        || threshold > count
        || count > 64
        || state.len() != 161 + count * 32
        || attestation.len() != threshold * 65
    {
        return Err("Invalid Solana attester threshold or signatures".into());
    }
    let hash = Keccak256::digest(message);
    let mut previous = [0; 32];
    for bytes in attestation.chunks_exact(65) {
        let signature = k256::ecdsa::Signature::from_slice(&bytes[..64])
            .map_err(|_| "Invalid attestation signature")?;
        if signature.normalize_s() != signature {
            return Err("High-s attestation signature".into());
        }
        let recovery = bytes[64]
            .checked_sub(27)
            .and_then(k256::ecdsa::RecoveryId::from_byte)
            .ok_or("Invalid attester recovery ID")?;
        let key = k256::ecdsa::VerifyingKey::recover_from_prehash(&hash, &signature, recovery)
            .map_err(|_| "Attester recovery failed")?
            .to_sec1_point(false);
        let digest = Keccak256::digest(&key.as_bytes()[1..]);
        let mut address = [0; 32];
        address[12..].copy_from_slice(&digest[12..]);
        if address <= previous
            || !state[153..153 + count * 32]
                .chunks_exact(32)
                .any(|enabled| enabled == address)
        {
            return Err("Disabled, duplicate, or unordered Solana attester".into());
        }
        previous = address;
    }
    Ok(())
}

pub(crate) async fn verify_destination_attesters(
    client: &reqwest::Client,
    transfer: &CctpTransfer,
    message: &[u8],
    attestation: &[u8],
) -> Result<(), String> {
    transfer.route.validate()?;
    verify_mainnet(client, transfer, true).await?;
    let network = &transfer.route.destination.network;
    let (state, _) =
        Pubkey::find_program_address(&[b"message_transmitter"], &pubkey(SOLANA_TRANSMITTER)?);
    let response=rpc_post(client,network.rpc_url()?,&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"getAccountInfo","params":[state.to_string(),{"encoding":"base64","commitment":"finalized"}]})).await?;
    let account = json_rpc_result(&response, "Solana CCTP attesters")?;
    let data = account_data(&account["value"], SOLANA_TRANSMITTER)?;
    verify_attesters(&data, message, attestation)
}

#[cfg(test)]
#[path = "../tests/providers/cctp_solana.rs"]
mod tests;
