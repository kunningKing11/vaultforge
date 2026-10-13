//! Live EVM execution and bounded destination replay recovery.
use sha3::{Digest, Keccak256};

use crate::cctp::CctpTransfer;
use crate::cctp_execution::CctpSourceProvider;
use crate::operations::{MintEvidence, Operation, TransactionAttempt, TransactionStep};
use crate::providers::cctp::{CctpPreflight, prepare_cctp_source};
use crate::providers::evm::{
    estimate_evm_call, fetch_evm_native_balance, fetch_evm_nonce,
    fetch_evm_token_balance_for_contract, fetch_evm_tx_observation, verify_evm_chain,
};
use crate::providers::http::{json_rpc_result, rpc_post};
use crate::tx::evm::{evm_address_word, evm_uint_word};
use crate::tx::tx_status::{Execution, Finality, TxStatus};

pub(crate) struct LiveCctpProvider<'a> {
    pub(crate) client: &'a reqwest::Client,
}

impl CctpSourceProvider for LiveCctpProvider<'_> {
    async fn prepare(&self, transfer: &CctpTransfer) -> Result<CctpPreflight, String> {
        prepare_cctp_source(self.client, transfer).await
    }
    async fn nonce(&self, transfer: &CctpTransfer) -> Result<u64, String> {
        fetch_evm_nonce(
            self.client,
            &transfer.route.source.network,
            transfer.sender(),
        )
        .await
    }
    async fn observe(
        &self,
        transfer: &CctpTransfer,
        hash: &str,
    ) -> Result<Option<TxStatus>, String> {
        verify_evm_chain(self.client, &transfer.route.source.network).await?;
        fetch_evm_tx_observation(self.client, &transfer.route.source.network, hash).await
    }
    async fn preflight_signed(
        &self,
        transfer: &CctpTransfer,
        attempt: &TransactionAttempt,
    ) -> Result<(), String> {
        let preflight = prepare_cctp_source(self.client, transfer).await?;
        let call = match (attempt.step, preflight.source) {
            (
                TransactionStep::Approval,
                crate::providers::cctp::CctpSourcePreparation::Approval { approval, .. },
            ) => approval,
            (TransactionStep::Burn, crate::providers::cctp::CctpSourcePreparation::Burn(burn)) => {
                burn
            }
            _ => {
                return Err(
                    "Source allowance changed; reviewed transaction is no longer actionable".into(),
                );
            }
        };
        validate_live_context(
            self.client,
            &transfer.route.source.network,
            attempt,
            &call.call,
        )
        .await
    }
    async fn broadcast(&self, transfer: &CctpTransfer, raw: &str) -> Result<String, String> {
        broadcast_once(self.client, &transfer.route.source.network, raw).await
    }
    async fn balances(&self, transfer: &CctpTransfer) -> Result<(u128, u128), String> {
        let source = &transfer.route.source;
        Ok((
            fetch_evm_native_balance(self.client, &source.network, transfer.sender()).await?,
            fetch_evm_token_balance_for_contract(
                self.client,
                &source.network,
                source.usdc,
                transfer.sender(),
            )
            .await?,
        ))
    }
}

/// Broadcasts identical signed bytes, failing over across same-network
/// endpoints. Resubmission is idempotent (same hash), so a lost response may
/// be retried on the next endpoint. Never invents acceptance: only a returned
/// hash counts, and reconciliation still resolves uncertain outcomes.
pub(crate) async fn broadcast_once_at(
    client: &reqwest::Client,
    urls: &[String],
    raw: &str,
) -> Result<String, String> {
    let body = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"eth_sendRawTransaction","params":[raw]});
    let mut last_err = "No broadcast endpoint available".to_string();
    for url in urls {
        match crate::providers::http::rpc_post(client, url, &body).await {
            Ok(json) => {
                match crate::providers::http::json_rpc_result(&json, "CCTP broadcast")?.as_str() {
                    Some(hash) => return Ok(hash.to_string()),
                    None => last_err = "Broadcast hash missing".to_string(),
                }
            }
            Err(error) => last_err = error,
        }
    }
    Err(last_err)
}

pub(crate) async fn broadcast_once(
    client: &reqwest::Client,
    network: &crate::registry::NetworkConfig,
    raw: &str,
) -> Result<String, String> {
    let mut urls = vec![network.rpc_url()?.to_string()];
    urls.extend(
        crate::providers::http::fallback_rpc_urls(&network.id)
            .iter()
            .map(|url| url.to_string()),
    );
    broadcast_once_at(client, &urls, raw).await
}

pub(crate) async fn validate_live_context(
    client: &reqwest::Client,
    network: &crate::registry::NetworkConfig,
    attempt: &TransactionAttempt,
    call: &crate::tx::evm::EvmCall,
) -> Result<(), String> {
    let context = attempt
        .evm_context
        .as_ref()
        .ok_or("Execution context missing")?;
    verify_evm_chain(client, network).await?;
    let nonce = fetch_evm_nonce(client, network, &context.from).await?;
    if nonce != context.nonce {
        return Err(
            "Pending nonce differs from saved transaction; reconcile instead of replacing it"
                .into(),
        );
    }
    let estimate = estimate_evm_call(client, network, &context.from, call).await?;
    let cap: u128 = context
        .max_fee_per_gas
        .parse()
        .map_err(|_| "Invalid fee cap")?;
    if estimate.gas_limit > context.gas_limit || estimate.fees.max_fee_per_gas > cap {
        return Err(
            "Saved gas limit or fee cap is insufficient; automatic replacement is unavailable"
                .into(),
        );
    }
    let budget = crate::providers::evm::evm_max_native_fee(context.gas_limit, cap)?;
    if fetch_evm_native_balance(client, network, &context.from).await? < budget {
        return Err("Insufficient native gas funds".into());
    }
    Ok(())
}

fn topic(signature: &str) -> String {
    format!("0x{}", hex::encode(Keccak256::digest(signature.as_bytes())))
}
fn bytes(value: &serde_json::Value) -> Result<Vec<u8>, String> {
    hex::decode(
        value
            .as_str()
            .and_then(|v| v.strip_prefix("0x"))
            .ok_or("Missing log hex")?,
    )
    .map_err(|_| "Invalid log hex".into())
}

/// Decodes both trusted events and canonical receipt coordinates. An RPC nonce
/// flag alone is deliberately insufficient, including third-party submissions.
pub(crate) fn decode_mint_receipt(
    operation: &Operation,
    hash: &str,
    receipt: &serde_json::Value,
    observation: &TxStatus,
) -> Result<MintEvidence, String> {
    let transfer = operation.cctp_transfer()?;
    let message = &operation.cctp.as_ref().ok_or("Message missing")?.message;
    if message.len() != 376
        || observation.execution != Execution::Succeeded
        || observation.finality != Finality::Finalized
    {
        return Err("Finalized successful mint evidence is required".into());
    }
    if receipt["transactionHash"]
        .as_str()
        .is_none_or(|v| !v.eq_ignore_ascii_case(hash))
        || receipt["blockHash"]
            .as_str()
            .zip(observation.block_hash.as_deref())
            .is_none_or(|(a, b)| !a.eq_ignore_ascii_case(b))
        || receipt["status"] != "0x1"
        || receipt["blockNumber"]
            .as_str()
            .and_then(|v| u64::from_str_radix(v.strip_prefix("0x")?, 16).ok())
            != observation.block_height
    {
        return Err("Mint receipt and canonical observation differ".into());
    }
    let mut received = 0;
    let mut minted = 0;
    let logs = receipt["logs"].as_array().ok_or("Mint logs missing")?;
    if logs.len() > 1024 {
        return Err("Too many mint logs".into());
    }
    for log in logs {
        if log["removed"] == true {
            return Err("Removed mint log".into());
        }
        let emitter = log["address"].as_str().ok_or("Log emitter missing")?;
        let topics = log["topics"].as_array().ok_or("Log topics missing")?;
        if topics
            .first()
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| {
                value.eq_ignore_ascii_case(&topic(
                    "MessageReceived(address,uint32,bytes32,bytes32,uint32,bytes)",
                ))
            })
            && emitter.eq_ignore_ascii_case(transfer.route.destination.message_transmitter)
        {
            if topics.len() != 4
                || bytes(&topics[2])? != message[12..44]
                || bytes(&topics[3])?
                    != evm_uint_word(u32::from_be_bytes(
                        message[144..148]
                            .try_into()
                            .map_err(|_| "Invalid finality")?,
                    ) as u128)
            {
                return Err("Received message nonce/finality mismatch".into());
            }
            let caller = bytes(&topics[1])?;
            if caller.len() != 32
                || caller[..12] != [0; 12]
                || (message[108..140] != [0; 32] && caller != message[108..140])
            {
                return Err("Received caller mismatch".into());
            }
            let mut expected = Vec::new();
            expected.extend_from_slice(&evm_uint_word(transfer.route.source.domain as u128));
            expected.extend_from_slice(&message[44..76]);
            expected.extend_from_slice(&evm_uint_word(96));
            expected.extend_from_slice(&evm_uint_word(228));
            expected.extend_from_slice(&message[148..]);
            expected.resize(4 * 32 + 256, 0);
            if bytes(&log["data"])? != expected {
                return Err("Received body does not match bound message".into());
            }
            received += 1;
        }
        if topics
            .first()
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| {
                value.eq_ignore_ascii_case(&topic(
                    "MintAndWithdraw(address,uint256,address,uint256)",
                ))
            })
            && emitter.eq_ignore_ascii_case(transfer.route.destination.token_messenger)
        {
            if topics.len() != 3
                || bytes(&topics[1])? != evm_address_word(transfer.recipient())?
                || bytes(&topics[2])? != evm_address_word(transfer.route.destination.usdc)?
            {
                return Err("Mint recipient/token mismatch".into());
            }
            let mut expected = evm_uint_word(transfer.amount()).to_vec();
            expected.extend_from_slice(&evm_uint_word(0));
            if bytes(&log["data"])? != expected {
                return Err("Mint amount/fee mismatch".into());
            }
            minted += 1;
        }
    }
    if received != 1 || minted != 1 {
        return Err("Unique matching message and mint evidence required".into());
    }
    Ok(MintEvidence {
        transaction_hash: hash.into(),
        message_hash: hex::encode(Keccak256::digest(message)),
        observation: observation.clone(),
    })
}

impl LiveCctpProvider<'_> {
    pub(crate) async fn mint_receipt(
        &self,
        operation: &Operation,
        hash: &str,
    ) -> Result<Option<MintEvidence>, String> {
        let transfer = operation.cctp_transfer()?;
        let network = &transfer.route.destination.network;
        verify_evm_chain(self.client, network).await?;
        let Some(observation) = fetch_evm_tx_observation(self.client, network, hash).await? else {
            return Ok(None);
        };
        if observation.execution != Execution::Succeeded
            || observation.finality != Finality::Finalized
        {
            return Ok(None);
        }
        let json=rpc_post(self.client,network.rpc_url()?,&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"eth_getTransactionReceipt","params":[hash]})).await?;
        decode_mint_receipt(
            operation,
            hash,
            json_rpc_result(&json, "Mint receipt")?,
            &observation,
        )
        .map(Some)
    }
    /// One 2,000-block page per explicit sync. Cursor advances only after a
    /// successful finalized-range read, so provider limits never skip evidence.
    pub(crate) async fn replay_page(
        &self,
        operation: &Operation,
    ) -> Result<(bool, Option<MintEvidence>, Option<u64>), String> {
        let transfer = operation.cctp_transfer()?;
        let network = &transfer.route.destination.network;
        verify_evm_chain(self.client, network).await?;
        let data = operation.cctp.as_ref().ok_or("Message missing")?;
        let nonce = data.message.get(12..44).ok_or("Message nonce missing")?;
        let mut query = Keccak256::digest(b"usedNonces(bytes32)")[..4].to_vec();
        query.extend_from_slice(nonce);
        let json=rpc_post(self.client,network.rpc_url()?,&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{"to":transfer.route.destination.message_transmitter,"data":format!("0x{}",hex::encode(query))},"latest"]})).await?;
        let flag = bytes(json_rpc_result(&json, "Replay state")?)?;
        if flag == evm_uint_word(0) {
            return Ok((false, None, None));
        }
        if flag != evm_uint_word(1) {
            return Err("Invalid replay state".into());
        }
        let json=rpc_post(self.client,network.rpc_url()?,&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"eth_getBlockByNumber","params":["finalized",false]})).await?;
        let head = json_rpc_result(&json, "Replay finalized head")?["number"]
            .as_str()
            .and_then(|v| u64::from_str_radix(v.strip_prefix("0x")?, 16).ok())
            .ok_or("Finalized head unavailable")?;
        let start = if let Some(cursor) = data.replay_cursor {
            cursor
        } else {
            // A mint cannot precede its source operation. Locate a conservative
            // destination time floor instead of scanning the chain from genesis.
            let evidence = data
                .evidence
                .as_ref()
                .ok_or("Source burn evidence missing")?;
            // Use the chain's burn time, not the desktop clock, for recovery.
            let burn_time = if transfer.route.source.network.kind == "svm" {
                let slot = evidence.slot.ok_or("Source slot missing")?;
                let json=rpc_post(self.client,transfer.route.source.network.rpc_url()?,&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"getBlockTime","params":[slot]})).await?;
                json_rpc_result(&json, "Source burn time")?
                    .as_u64()
                    .ok_or("Source burn time unavailable")?
            } else {
                let height = evidence.block_height.ok_or("Source height missing")?;
                let json=rpc_post(self.client,transfer.route.source.network.rpc_url()?,&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"eth_getBlockByNumber","params":[format!("0x{height:x}"),false]})).await?;
                json_rpc_result(&json, "Source burn time")?["timestamp"]
                    .as_str()
                    .and_then(|value| u64::from_str_radix(value.strip_prefix("0x")?, 16).ok())
                    .ok_or("Source burn time unavailable")?
            };
            let timestamp = burn_time.saturating_sub(3600);
            let mut low = 0u64;
            let mut high = head;
            while low < high {
                let mid = low + (high - low) / 2;
                let json=rpc_post(self.client,network.rpc_url()?,&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"eth_getBlockByNumber","params":[format!("0x{mid:x}"),false]})).await?;
                let time = json_rpc_result(&json, "Replay time floor")?["timestamp"]
                    .as_str()
                    .and_then(|v| u64::from_str_radix(v.strip_prefix("0x")?, 16).ok())
                    .ok_or("Block timestamp missing")?;
                if time < timestamp {
                    low = mid + 1;
                } else {
                    high = mid;
                }
            }
            low.saturating_sub(1)
        };
        if start > head {
            return Ok((true, None, Some(start)));
        }
        let end = start.saturating_add(1999).min(head);
        let json=rpc_post(self.client,network.rpc_url()?,&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"eth_getLogs","params":[{"address":transfer.route.destination.message_transmitter,"fromBlock":format!("0x{start:x}"),"toBlock":format!("0x{end:x}"),"topics":[topic("MessageReceived(address,uint32,bytes32,bytes32,uint32,bytes)"),null,format!("0x{}",hex::encode(nonce))]}]})).await?;
        let logs = json_rpc_result(&json, "Replay logs")?
            .as_array()
            .ok_or("Invalid replay logs")?;
        for log in logs {
            let hash = log["transactionHash"]
                .as_str()
                .ok_or("Replay hash missing")?;
            if let Some(proof) = self.mint_receipt(operation, hash).await? {
                return Ok((true, Some(proof), Some(end.saturating_add(1))));
            }
        }
        // A matching finalized-range log with temporarily missing receipt/finality
        // evidence must be retried, not skipped by advancing the scan cursor.
        Ok((
            true,
            None,
            Some(if logs.is_empty() {
                end.saturating_add(1)
            } else {
                start
            }),
        ))
    }
}

#[cfg(test)]
#[path = "../tests/providers/cctp_execution.rs"]
mod tests;
