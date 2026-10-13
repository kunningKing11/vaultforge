//! CCTP V2 message binding and resumable, read-only attestation polling.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use std::{future::Future, sync::Mutex};

use crate::cctp::{CctpTransfer, STANDARD_FINALITY_THRESHOLD, address_word};
use crate::cctp_execution::{begin_action, check_snapshot, save_operation};
use crate::dto::WalletSession;
use crate::operations::{CctpData, Operation, OperationStep, TransactionStep};
use crate::state::{AppState, session_from_state};
use crate::tx::evm::{evm_address_word, evm_uint_word};
use crate::tx::tx_status::{Execution, Finality, TxStatus};

const MESSAGE_SIZE: usize = 376; // 148-byte V2 header + 228-byte burn body; no hooks
pub(crate) const MAX_API_BYTES: usize = 512 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct BurnEvidence {
    pub(crate) transaction_hash: String,
    pub(crate) block_hash: Option<String>,
    pub(crate) block_height: Option<u64>,
    pub(crate) message_log_index: Option<u64>,
    pub(crate) deposit_log_index: Option<u64>,
    pub(crate) emitted_message: Vec<u8>,
    #[serde(default)]
    pub(crate) slot: Option<u64>,
    #[serde(default)]
    pub(crate) message_account: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct AttestationPoll {
    pub(crate) failures: u32,
    pub(crate) next_poll_at: i64,
}

#[derive(Serialize)]
pub(crate) struct AttestationProgress {
    pub(crate) session: WalletSession,
    pub(crate) state: String,
    pub(crate) retry_after_secs: u32,
    pub(crate) error: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) enum MessageResponse {
    Pending,
    Messages(serde_json::Value),
    RateLimited(u32),
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum AttestationCheck {
    DestinationSimulation,
    SolanaAttesters,
}

pub(crate) trait AttestationProvider {
    fn receipt(
        &self,
        transfer: &CctpTransfer,
        hash: &str,
    ) -> impl Future<Output = Result<Option<(serde_json::Value, TxStatus)>, String>> + Send;
    fn messages(
        &self,
        transfer: &CctpTransfer,
        hash: &str,
    ) -> impl Future<Output = Result<MessageResponse, String>> + Send;
    fn verify(
        &self,
        transfer: &CctpTransfer,
        message: &[u8],
        attestation: &[u8],
    ) -> impl Future<Output = Result<AttestationCheck, String>> + Send;
}

fn hex_bytes(value: &str, max: usize) -> Result<Vec<u8>, String> {
    let value = value
        .strip_prefix("0x")
        .ok_or("CCTP bytes must be 0x-prefixed")?;
    if value.len() > max * 2 || value.len() % 2 != 0 {
        return Err("CCTP byte length is invalid".into());
    }
    hex::decode(value).map_err(|_| "Invalid CCTP hex bytes".into())
}

fn hash(value: &serde_json::Value, field: &str) -> Result<String, String> {
    let value = value[field]
        .as_str()
        .ok_or_else(|| format!("Missing CCTP {field}"))?;
    if hex_bytes(value, 32)?.len() != 32 {
        return Err(format!("Invalid CCTP {field}"));
    }
    Ok(value.to_ascii_lowercase())
}

fn quantity(value: &serde_json::Value, field: &str) -> Result<u64, String> {
    let text = value[field]
        .as_str()
        .and_then(|text| text.strip_prefix("0x"))
        .ok_or_else(|| format!("Missing CCTP {field}"))?;
    if text.is_empty()
        || !text.bytes().all(|byte| byte.is_ascii_hexdigit())
        || (text.len() > 1 && text.starts_with('0'))
    {
        return Err(format!("Invalid CCTP {field}"));
    }
    u64::from_str_radix(text, 16).map_err(|_| format!("Invalid CCTP {field}"))
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated message length"),
    )
}

/// Pin both message versions (1), immutable route/account fields, zero fee cap,
/// and hook absence. Protocol V2 and its wire message version are distinct.
pub(crate) fn validate_message_intent(
    transfer: &CctpTransfer,
    message: &[u8],
) -> Result<(), String> {
    transfer.route.validate()?;
    if message.len() != MESSAGE_SIZE {
        return Err("CCTP message length or hooks are unsupported".into());
    }
    if u32_at(message, 0) != 1
        || u32_at(message, 148) != 1
        || u32_at(message, 4) != transfer.route.source.domain
        || u32_at(message, 8) != transfer.route.destination.domain
        || u32_at(message, 140) != STANDARD_FINALITY_THRESHOLD
    {
        return Err("CCTP message version, domain, or finality policy mismatch".into());
    }
    for (offset, word) in [
        (
            44,
            address_word(
                &transfer.route.source.network,
                transfer.route.source.token_messenger,
            )?,
        ),
        (
            76,
            address_word(
                &transfer.route.destination.network,
                transfer.route.destination.token_messenger,
            )?,
        ),
        (108, transfer.caller_word()?),
        (
            152,
            address_word(&transfer.route.source.network, transfer.route.source.usdc)?,
        ),
        (184, transfer.mint_recipient_word()?),
        (
            248,
            address_word(&transfer.route.source.network, transfer.sender())?,
        ),
    ] {
        if message[offset..offset + 32] != word {
            return Err("CCTP message account or token mismatch".into());
        }
    }
    if message[216..248] != evm_uint_word(transfer.amount())
        || message[280..312] != evm_uint_word(0)
    {
        return Err("CCTP message amount or maximum fee mismatch".into());
    }
    Ok(())
}

pub(crate) fn validate_bound_message(
    operation: &Operation,
    evidence: &BurnEvidence,
    message: &[u8],
) -> Result<(), String> {
    let transfer = operation.cctp_transfer()?;
    validate_message_intent(&transfer, &evidence.emitted_message)?;
    validate_message_intent(&transfer, message)?;
    if transfer.route.source.network.kind == "evm" {
        if evidence.block_hash.is_none()
            || evidence.block_height.is_none()
            || evidence.slot.is_some()
        {
            return Err("Missing EVM burn evidence".into());
        }
    } else if evidence.slot.is_none()
        || evidence.message_account.is_none()
        || evidence.block_height.is_some()
        || evidence.block_hash.is_some()
    {
        return Err("Invalid Solana burn evidence".into());
    }
    let emitted = &evidence.emitted_message;
    if emitted[12..44] != [0; 32] || emitted[144..148] != [0; 4] || emitted[312..376] != [0; 64] {
        return Err("CCTP emitted message has nonzero attestation-time fields".into());
    }
    if message[..12] != emitted[..12]
        || message[44..144] != emitted[44..144]
        || message[148..312] != emitted[148..312]
    {
        return Err("CCTP attested message is not bound to the burn".into());
    }
    if message[12..44] == [0; 32]
        || u32_at(message, 144) < STANDARD_FINALITY_THRESHOLD
        || message[312..344] != [0; 32]
    {
        return Err("CCTP attested nonce, executed finality, or fee is invalid".into());
    }
    // expirationBlock is retained verbatim. The finalized Standard handler does
    // not enforce Fast Transfer expiry; do not compare an Arbitrum L2 height to it.
    if !operation.attempts.iter().any(|attempt| {
        attempt.step == TransactionStep::Burn
            && transfer.source_hash_matches(&attempt.tx_hash, &evidence.transaction_hash)
    }) {
        return Err("CCTP evidence belongs to another burn transaction".into());
    }
    Ok(())
}

fn event_topic(signature: &str) -> Vec<u8> {
    Keccak256::digest(signature.as_bytes()).to_vec()
}

/// Select exactly one matching pair, ignoring unrelated messages in the receipt.
/// RPC canonicality is checked by the provider before this decoder is called.
pub(crate) fn decode_burn_receipt(
    operation: &Operation,
    receipt: &serde_json::Value,
    observation: &TxStatus,
    tx_hash: &str,
) -> Result<BurnEvidence, String> {
    let transfer = operation.cctp_transfer()?;
    if transfer.route.source.network.kind == "svm" {
        return crate::providers::cctp_solana::decode_source_evidence(
            operation,
            receipt,
            observation,
            tx_hash,
        );
    }
    let block_hash = hash(receipt, "blockHash")?;
    let block_height = quantity(receipt, "blockNumber")?;
    if receipt["status"] != "0x1"
        || hash(receipt, "transactionHash")? != tx_hash.to_ascii_lowercase()
        || observation.execution != Execution::Succeeded
        || observation.block_height != Some(block_height)
        || !observation
            .block_hash
            .as_deref()
            .is_some_and(|hash| hash.eq_ignore_ascii_case(&block_hash))
    {
        return Err("CCTP burn receipt is unsuccessful or changed".into());
    }
    let logs = receipt["logs"]
        .as_array()
        .ok_or("CCTP burn receipt has no logs")?;
    if logs.len() > 1024 {
        return Err("CCTP receipt has too many logs".into());
    }
    let mut messages = Vec::new();
    let mut deposits = Vec::new();
    for log in logs {
        let Some(address) = log["address"].as_str() else {
            return Err("Missing receipt log address".into());
        };
        let transmitter = address.eq_ignore_ascii_case(transfer.route.source.message_transmitter);
        let messenger = address.eq_ignore_ascii_case(transfer.route.source.token_messenger);
        if !transmitter && !messenger {
            continue;
        }
        let topics = log["topics"].as_array().ok_or("Missing CCTP log topics")?;
        let Some(topic) = topics.first().and_then(|value| value.as_str()) else {
            continue;
        };
        let topic = hex_bytes(topic, 32)?;
        let message_event = transmitter && topic == event_topic("MessageSent(bytes)");
        let deposit_event = messenger
            && topic
                == event_topic(
                    "DepositForBurn(address,uint256,address,bytes32,uint32,bytes32,bytes32,uint256,uint32,bytes)",
                );
        if !message_event && !deposit_event {
            continue;
        }
        if log["removed"] == true {
            return Err("CCTP burn log was removed".into());
        }
        for field in ["transactionHash", "blockHash"] {
            if !log[field].is_null() {
                let expected = if field == "transactionHash" {
                    tx_hash
                } else {
                    &block_hash
                };
                if !hash(log, field)?.eq_ignore_ascii_case(expected) {
                    return Err("CCTP log receipt identity mismatch".into());
                }
            }
        }
        if !log["blockNumber"].is_null() && quantity(log, "blockNumber")? != block_height {
            return Err("CCTP log block height mismatch".into());
        }
        let index = quantity(log, "logIndex")?;
        let data = hex_bytes(log["data"].as_str().ok_or("Missing CCTP log data")?, 16384)?;
        if message_event {
            if topics.len() != 1 || data.len() < 64 || data[..32] != evm_uint_word(32) {
                return Err("Malformed MessageSent ABI".into());
            }
            let length = if data[32..48] == [0; 16] {
                u128::from_be_bytes(
                    data[48..64]
                        .try_into()
                        .map_err(|_| "Invalid message length")?,
                )
            } else {
                return Err("MessageSent length overflows".into());
            };
            let length: usize = length
                .try_into()
                .map_err(|_| "MessageSent length overflows")?;
            if length > 16384
                || data.len() != 64 + length.div_ceil(32) * 32
                || data[64 + length..].iter().any(|byte| *byte != 0)
            {
                return Err("Malformed MessageSent padding".into());
            }
            let message = data[64..64 + length].to_vec();
            if validate_message_intent(&transfer, &message).is_ok()
                && message[12..44] == [0; 32]
                && message[144..148] == [0; 4]
                && message[312..376] == [0; 64]
            {
                messages.push((index, message));
            }
        } else {
            if topics.len() != 4
                || data.len() != 256
                || data[192..224] != evm_uint_word(224)
                || data[224..256] != [0; 32]
            {
                return Err("Malformed DepositForBurn ABI or hooks".into());
            }
            let token = hex_bytes(topics[1].as_str().ok_or("Missing burn token")?, 32)?;
            let sender = hex_bytes(topics[2].as_str().ok_or("Missing depositor")?, 32)?;
            let finality = hex_bytes(topics[3].as_str().ok_or("Missing burn finality")?, 32)?;
            if token == evm_address_word(transfer.route.source.usdc)?
                && sender == evm_address_word(transfer.sender())?
                && finality == evm_uint_word(STANDARD_FINALITY_THRESHOLD as u128)
                && data[..32] == evm_uint_word(transfer.amount())
                && data[32..64] == evm_address_word(transfer.recipient())?
                && data[64..96] == evm_uint_word(transfer.route.destination.domain as u128)
                && data[96..128] == evm_address_word(transfer.route.destination.token_messenger)?
                && data[128..160] == evm_address_word(transfer.destination_caller())?
                && data[160..192] == [0; 32]
            {
                deposits.push(index);
            }
        }
    }
    if messages.len() != 1 || deposits.len() != 1 || messages[0].0 >= deposits[0] {
        return Err("CCTP burn logs are missing, mismatched, or ambiguous".into());
    }
    Ok(BurnEvidence {
        transaction_hash: tx_hash.to_ascii_lowercase(),
        block_hash: Some(block_hash),
        block_height: Some(block_height),
        message_log_index: Some(messages[0].0),
        deposit_log_index: Some(deposits[0]),
        slot: None,
        message_account: None,
        emitted_message: messages.remove(0).1,
    })
}

pub(crate) fn select_attestation(
    operation: &Operation,
    evidence: &BurnEvidence,
    json: &serde_json::Value,
) -> Result<Option<(Vec<u8>, Vec<u8>)>, String> {
    let entries = json["messages"]
        .as_array()
        .ok_or("Invalid CCTP messages response")?;
    if entries.len() > 64 {
        return Err("Too many CCTP API messages".into());
    }
    if entries.is_empty() {
        return Ok(None);
    }
    let source_hash = json["sourceTxHash"]
        .as_str()
        .ok_or("Missing API source transaction hash")?;
    let source_hash = if operation.cctp_transfer()?.route.source.network.kind == "svm" {
        if bs58::decode(source_hash)
            .into_vec()
            .map_err(|_| "Invalid Solana API signature")?
            .len()
            != 64
        {
            return Err("Invalid Solana API signature length".into());
        }
        source_hash.to_string()
    } else {
        hash(json, "sourceTxHash")?
    };
    if source_hash != evidence.transaction_hash {
        return Err("CCTP API source transaction mismatch".into());
    }
    let mut selected = None;
    let mut matching_messages = 0;
    let mut matching_pending = false;
    for entry in entries {
        let status = entry["status"]
            .as_str()
            .ok_or("CCTP message status missing")?;
        if !matches!(status, "complete" | "pending_confirmations" | "pending") {
            return Err("Unknown CCTP attestation status".into());
        }
        let Some(raw) = entry["message"].as_str().filter(|raw| *raw != "0x") else {
            if status == "complete" {
                return Err("Complete attestation has no message".into());
            }
            matching_pending = true;
            continue;
        };
        let message = hex_bytes(raw, 16384)?;
        if validate_message_intent(&operation.cctp_transfer()?, &message).is_err() {
            continue;
        }
        matching_messages += 1;
        if matching_messages > 1 {
            return Err("CCTP API message selection is ambiguous".into());
        }
        if status != "complete" {
            matching_pending = true;
            continue;
        }
        validate_bound_message(operation, evidence, &message)?;
        let attestation = hex_bytes(
            entry["attestation"]
                .as_str()
                .ok_or("Complete attestation missing")?,
            16384,
        )?;
        if attestation.is_empty() || attestation.len() % 65 != 0 {
            return Err("Invalid CCTP attestation signature length".into());
        }
        if selected.replace((message, attestation)).is_some() {
            return Err("CCTP API message selection is ambiguous".into());
        }
    }
    if selected.is_none() && !matching_pending {
        return Err("CCTP API message does not match burn intent".into());
    }
    Ok(selected)
}

fn progress(state: &AppState, operation: &Operation, now: i64) -> AttestationProgress {
    let poll = operation.cctp.as_ref().and_then(|data| data.poll.as_ref());
    let retry = poll
        .map(|poll| poll.next_poll_at.saturating_sub(now).clamp(0, 300) as u32)
        .unwrap_or(0);
    let available = operation.cctp.as_ref().is_some_and(|data| {
        (data.simulated || data.signature_verified)
            && data
                .attestation
                .as_ref()
                .is_some_and(|bytes| !bytes.is_empty() && bytes.len() % 65 == 0)
            && data.evidence.as_ref().is_some_and(|evidence| {
                validate_bound_message(operation, evidence, &data.message).is_ok()
            })
    });
    let finality = crate::operations::attestation_available(operation);
    AttestationProgress {
        session: session_from_state(state),
        state: if operation.last_error.is_some() {
            "error"
        } else if available && finality {
            "available"
        } else if available {
            "waiting_source_finality"
        } else {
            "pending"
        }
        .into(),
        retry_after_secs: retry,
        error: operation.last_error.clone(),
    }
}

pub(crate) async fn poll_attestation<P: AttestationProvider + Sync>(
    state: &Mutex<AppState>,
    provider: &P,
    id: &str,
) -> Result<AttestationProgress, String> {
    poll_attestation_at(state, provider, id, Utc::now().timestamp()).await
}

pub(crate) async fn poll_attestation_at<P: AttestationProvider + Sync>(
    state: &Mutex<AppState>,
    provider: &P,
    id: &str,
    now: i64,
) -> Result<AttestationProgress, String> {
    let (_guard, generation, before) = begin_action(state, id)?;
    if !matches!(
        before.step,
        OperationStep::Burn | OperationStep::AwaitingAttestation
    ) {
        return Err("CCTP operation is not awaiting source evidence or attestation".into());
    }
    if crate::operations::attestation_available(&before) && before.last_error.is_none() {
        let state = state.lock().map_err(|_| "State lock failed")?;
        check_snapshot(&state, generation, &before)?;
        let mut result = progress(&state, &before, now);
        result.retry_after_secs = 0;
        return Ok(result);
    }
    if before
        .cctp
        .as_ref()
        .and_then(|data| data.poll.as_ref())
        .is_some_and(|poll| poll.next_poll_at > now)
    {
        let state = state.lock().map_err(|_| "State lock failed")?;
        check_snapshot(&state, generation, &before)?;
        return Ok(progress(&state, &before, now));
    }
    let mut after = before.clone();
    let result = poll_once(provider, &mut after).await;
    let failures = before
        .cctp
        .as_ref()
        .and_then(|data| data.poll.as_ref())
        .map(|poll| poll.failures)
        .unwrap_or(0);
    let (delay, failures) = match result {
        Ok(delay) => {
            after.last_error = None;
            (delay, 0)
        }
        Err(error) => {
            after = before.clone();
            after.last_error = Some(error);
            let failures = failures.saturating_add(1);
            (
                5u32.saturating_mul(1u32 << failures.min(6)).min(300),
                failures,
            )
        }
    };
    after.cctp.get_or_insert_with(CctpData::default).poll = Some(AttestationPoll {
        failures,
        next_poll_at: now.saturating_add(delay as i64),
    });
    after.updated_at = Utc::now().to_rfc3339();
    let mut state = state.lock().map_err(|_| "State lock failed")?;
    save_operation(&mut state, generation, &before, &after)?;
    Ok(progress(&state, &after, now))
}

async fn poll_once<P: AttestationProvider + Sync>(
    provider: &P,
    operation: &mut Operation,
) -> Result<u32, String> {
    let transfer = operation.cctp_transfer()?;
    let index = operation
        .attempts
        .iter()
        .rposition(|attempt| attempt.step == TransactionStep::Burn)
        .ok_or("CCTP burn attempt missing")?;
    let tx_hash = operation.attempts[index].tx_hash.clone();
    let Some((receipt, observation)) = provider.receipt(&transfer, &tx_hash).await? else {
        if !operation.attempts[index]
            .tx_status
            .as_ref()
            .is_some_and(|status| status.finality == Finality::Finalized)
        {
            operation.attempts[index].tx_status = None;
            operation.cctp = None;
        }
        return Ok(10);
    };
    let evidence = decode_burn_receipt(operation, &receipt, &observation, &tx_hash)?;
    if operation.attempts[index]
        .tx_status
        .as_ref()
        .is_some_and(|old| {
            old.finality == Finality::Finalized && old.block_hash != observation.block_hash
        })
    {
        return Err("CCTP finalized burn evidence changed".into());
    }
    if !operation.attempts[index]
        .tx_status
        .as_ref()
        .is_some_and(|old| old.finality == Finality::Finalized)
    {
        operation.attempts[index].tx_status = Some(observation);
    }
    let data = operation.cctp.get_or_insert_with(CctpData::default);
    if data.evidence.as_ref().is_some_and(|old| old != &evidence) {
        data.message.clear();
        data.attestation = None;
        data.nonce = None;
        data.simulated = false;
        data.signature_verified = false;
    }
    data.evidence = Some(evidence.clone());
    if operation.step == OperationStep::Burn {
        operation.step = OperationStep::AwaitingAttestation;
    }
    match provider.messages(&transfer, &tx_hash).await? {
        MessageResponse::Pending => Ok(10),
        MessageResponse::RateLimited(seconds) => Ok(seconds.clamp(10, 300)),
        MessageResponse::Messages(json) => {
            if let Some((message, attestation)) = select_attestation(operation, &evidence, &json)? {
                let check = provider.verify(&transfer, &message, &attestation).await?;
                crate::operations::record_attestation(operation, message, attestation)?;
                let data = operation.cctp.as_mut().ok_or("CCTP evidence missing")?;
                match check {
                    AttestationCheck::DestinationSimulation
                        if transfer.route.destination.network.kind == "evm" =>
                    {
                        data.simulated = true
                    }
                    AttestationCheck::SolanaAttesters
                        if transfer.route.destination.network.kind == "svm" =>
                    {
                        data.signature_verified = true
                    }
                    _ => {
                        return Err(
                            "Attestation verification method does not match destination family"
                                .into(),
                        );
                    }
                }
            }
            Ok(10)
        }
    }
}

#[cfg(test)]
#[path = "tests/cctp_attestation.rs"]
pub(crate) mod tests;
