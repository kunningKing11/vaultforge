//! Explicit destination authorization; sync is read-only and never re-burns.
use std::future::Future;
use std::sync::Mutex;

use chrono::Utc;

use crate::cctp_execution::{
    CctpPreparedSource, begin_action, check_snapshot, prepared_view, save_operation,
    validate_source_attempt,
};
use crate::derivation::signing_key_from_mnemonic;
use crate::dto::WalletSession;
use crate::operations::{
    EvmAttemptContext, MintEvidence, Operation, OperationStep, TransactionAttempt,
    TransactionStatus, TransactionStep, advance_operation, attestation_available,
    reconcile_operation_with,
};
use crate::providers::cctp::{PreparedCctpCall, prepare_cctp_mint};
use crate::providers::cctp_execution::{LiveCctpProvider, broadcast_once, validate_live_context};
use crate::providers::evm::{fetch_evm_nonce, fetch_evm_tx_observation};
use crate::state::{AppState, session_from_state};
use crate::tx::cctp::build_mint_call;
use crate::tx::evm::Eip1559TxDraft;
use crate::tx::tx_status::TxStatus;

pub(crate) trait CctpMintProvider {
    fn nonce_settled(
        &self,
        _operation: &Operation,
        _attempt: &TransactionAttempt,
    ) -> impl Future<Output = Result<bool, String>> + Send {
        async { Ok(false) }
    }
    fn observe(
        &self,
        operation: &Operation,
        destination: bool,
        hash: &str,
    ) -> impl Future<Output = Result<Option<TxStatus>, String>> + Send;
    fn replay(
        &self,
        operation: &Operation,
    ) -> impl Future<Output = Result<(bool, Option<MintEvidence>, Option<u64>), String>> + Send;
    fn receipt(
        &self,
        operation: &Operation,
        hash: &str,
    ) -> impl Future<Output = Result<Option<MintEvidence>, String>> + Send;
    fn prepare(
        &self,
        operation: &Operation,
    ) -> impl Future<Output = Result<PreparedCctpCall, String>> + Send;
    fn nonce(&self, operation: &Operation) -> impl Future<Output = Result<u64, String>> + Send;
    fn preflight(
        &self,
        operation: &Operation,
        attempt: &TransactionAttempt,
    ) -> impl Future<Output = Result<(), String>> + Send;
    fn broadcast(
        &self,
        operation: &Operation,
        raw: &str,
    ) -> impl Future<Output = Result<String, String>> + Send;
}

impl CctpMintProvider for LiveCctpProvider<'_> {
    async fn nonce_settled(
        &self,
        operation: &Operation,
        attempt: &TransactionAttempt,
    ) -> Result<bool, String> {
        let Some(context) = &attempt.evm_context else {
            return Ok(false);
        };
        let transfer = operation.cctp_transfer()?;
        let network = &transfer.route.destination.network;
        crate::providers::evm::verify_evm_chain(self.client, network).await?;
        let json=crate::providers::http::rpc_post(self.client,network.rpc_url()?,&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"eth_getTransactionCount","params":[context.from,"finalized"]})).await?;
        let value = crate::providers::http::json_rpc_result(&json, "Finalized nonce")?
            .as_str()
            .and_then(|v| u64::from_str_radix(v.strip_prefix("0x")?, 16).ok())
            .ok_or("Finalized nonce unavailable")?;
        Ok(value > context.nonce)
    }
    async fn observe(
        &self,
        operation: &Operation,
        destination: bool,
        hash: &str,
    ) -> Result<Option<TxStatus>, String> {
        let transfer = operation.cctp_transfer()?;
        let network = if destination {
            &transfer.route.destination.network
        } else {
            &transfer.route.source.network
        };
        if network.kind == "svm" {
            crate::providers::solana::fetch_solana_tx_observation(self.client, network, hash).await
        } else {
            fetch_evm_tx_observation(self.client, network, hash).await
        }
    }
    async fn replay(
        &self,
        operation: &Operation,
    ) -> Result<(bool, Option<MintEvidence>, Option<u64>), String> {
        self.replay_page(operation).await
    }
    async fn receipt(
        &self,
        operation: &Operation,
        hash: &str,
    ) -> Result<Option<MintEvidence>, String> {
        self.mint_receipt(operation, hash).await
    }
    async fn prepare(&self, operation: &Operation) -> Result<PreparedCctpCall, String> {
        let transfer = operation.cctp_transfer()?;
        let data = operation.cctp.as_ref().ok_or("Attestation missing")?;
        prepare_cctp_mint(
            self.client,
            &transfer,
            &data.message,
            data.attestation.as_deref().ok_or("Attestation missing")?,
        )
        .await
    }
    async fn nonce(&self, operation: &Operation) -> Result<u64, String> {
        let transfer = operation.cctp_transfer()?;
        fetch_evm_nonce(
            self.client,
            &transfer.route.destination.network,
            transfer.destination_fee_payer()?,
        )
        .await
    }
    async fn preflight(
        &self,
        operation: &Operation,
        attempt: &TransactionAttempt,
    ) -> Result<(), String> {
        let transfer = operation.cctp_transfer()?;
        let data = operation.cctp.as_ref().ok_or("Attestation missing")?;
        crate::providers::cctp_attestation::simulate_message_from(
            self.client,
            &transfer,
            &data.message,
            data.attestation.as_deref().ok_or("Attestation missing")?,
            transfer.destination_fee_payer()?,
        )
        .await?;
        let call = build_mint_call(
            &transfer,
            &data.message,
            data.attestation.as_deref().ok_or("Attestation missing")?,
        )?;
        validate_live_context(
            self.client,
            &transfer.route.destination.network,
            attempt,
            &call,
        )
        .await
    }
    async fn broadcast(&self, operation: &Operation, raw: &str) -> Result<String, String> {
        broadcast_once(
            self.client,
            &operation.cctp_transfer()?.route.destination.network,
            raw,
        )
        .await
    }
}

fn require_evm_route(operation: &Operation) -> Result<(), String> {
    let transfer = operation.cctp_transfer()?;
    if transfer.route.destination.network.kind != "evm" {
        return Err("Native Solana CCTP execution is unavailable".into());
    }
    Ok(())
}

async fn reconcile<P: CctpMintProvider + Sync>(
    state: &Mutex<AppState>,
    provider: &P,
    generation: u64,
    original: &Operation,
) -> Result<(Operation, bool), String> {
    require_evm_route(original)?;
    let mut after = original.clone();
    reconcile_operation_with(&mut after, |network, hash| {
        let destination = network == original.destination_asset.network;
        async move { provider.observe(original, destination, &hash).await }
    })
    .await?;
    let mut consumed = false;
    if attestation_available(&after)
        && !matches!(
            after.step,
            OperationStep::Completed | OperationStep::Cancelled
        )
    {
        let own_hash = after
            .attempts
            .iter()
            .rev()
            .find(|a| a.step == TransactionStep::Mint)
            .map(|a| a.tx_hash.clone());
        let own_proof = if let Some(proof) = after
            .cctp
            .as_ref()
            .and_then(|data| data.mint_evidence.clone())
        {
            Some(proof)
        } else if let Some(hash) = own_hash {
            provider.receipt(&after, &hash).await?
        } else {
            None
        };
        let (used, proof, cursor) = if own_proof.is_some() {
            (true, own_proof, None)
        } else {
            provider.replay(&after).await?
        };
        consumed = used;
        if let Some(data) = after.cctp.as_mut() {
            if cursor.is_some() {
                data.replay_cursor = cursor;
            }
        }
        if let Some(proof) = proof {
            // Validate provider output independently; production provider decoded
            // both events. Never upgrade a nonce flag into execution evidence.
            use sha3::{Digest, Keccak256};
            let data = after.cctp.as_ref().ok_or("Message missing")?;
            if proof.message_hash != hex::encode(Keccak256::digest(&data.message))
                || proof.observation.execution != crate::tx::tx_status::Execution::Succeeded
                || proof.observation.finality != crate::tx::tx_status::Finality::Finalized
            {
                return Err("Mint proof does not match finalized bound message".into());
            }
            if after.step == OperationStep::AwaitingAttestation {
                advance_operation(&mut after, OperationStep::Mint)?;
            }
            if let Some(attempt) = after.attempts.iter_mut().find(|a| {
                a.step == TransactionStep::Mint
                    && a.tx_hash.eq_ignore_ascii_case(&proof.transaction_hash)
            }) {
                attempt.tx_status = Some(proof.observation.clone());
                attempt.status = TransactionStatus::Confirmed;
                attempt.error = None;
            } else {
                after.attempts.push(TransactionAttempt {
                    step: TransactionStep::Mint,
                    status: TransactionStatus::Confirmed,
                    tx_hash: proof.transaction_hash.clone(),
                    raw_tx: None,
                    signature: None,
                    fee: None,
                    nonce_or_sequence: None,
                    error: None,
                    updated_at: Utc::now().to_rfc3339(),
                    tx_status: Some(proof.observation.clone()),
                    evm_context: None,
                    solana_context: None,
                });
            }
            for attempt in &mut after.attempts {
                if attempt.step == TransactionStep::Mint
                    && attempt.status == TransactionStatus::Signed
                {
                    attempt.status = TransactionStatus::Superseded;
                    attempt.error = Some(
                        "Unsubmitted mint superseded by verified destination execution".into(),
                    );
                }
            }
            after.cctp.as_mut().ok_or("Message missing")?.mint_evidence = Some(proof);
            advance_operation(&mut after, OperationStep::Completed)?;
        } else if used {
            after.last_error = Some(
                "Message consumed; locating finalized destination message and mint evidence".into(),
            );
        }
    }
    if after.step == OperationStep::Completed {
        for index in 0..after.attempts.len() {
            let attempt = &after.attempts[index];
            if attempt.step == TransactionStep::Mint
                && matches!(
                    attempt.status,
                    TransactionStatus::Submitted | TransactionStatus::SubmissionUnknown
                )
                && !attempt
                    .tx_status
                    .as_ref()
                    .is_some_and(|s| s.finality == crate::tx::tx_status::Finality::Finalized)
                && provider
                    .nonce_settled(&after, attempt)
                    .await
                    .unwrap_or(false)
            {
                after.attempts[index].status = TransactionStatus::Superseded;
                after.attempts[index].error = Some(
                    "Another verified mint completed; this saved nonce is finalized on chain"
                        .into(),
                );
            }
        }
    }
    save_operation(
        &mut *state.lock().map_err(|_| "State lock failed")?,
        generation,
        original,
        &after,
    )?;
    Ok((after, consumed))
}

pub(crate) async fn sync_cctp<P: CctpMintProvider + Sync>(
    state: &Mutex<AppState>,
    provider: &P,
    id: &str,
) -> Result<WalletSession, String> {
    let (_action, generation, original) = begin_action(state, id)?;
    let (operation, _) = reconcile(state, provider, generation, &original).await?;
    let state = state.lock().map_err(|_| "State lock failed")?;
    check_snapshot(&state, generation, &operation)?;
    Ok(session_from_state(&state))
}

pub(crate) async fn prepare_mint<P: CctpMintProvider + Sync>(
    state: &Mutex<AppState>,
    provider: &P,
    id: &str,
) -> Result<CctpPreparedSource, String> {
    let (_action, generation, original) = begin_action(state, id)?;
    let (operation, consumed) = reconcile(state, provider, generation, &original).await?;
    if consumed || operation.step == OperationStep::Completed {
        return Err("Message already consumed; synchronize its destination evidence".into());
    }
    if !attestation_available(&operation) {
        return Err("Verified attestation and finalized source burn are required".into());
    }
    if let Some(attempt) = operation
        .attempts
        .iter()
        .rev()
        .find(|a| a.step == TransactionStep::Mint && a.status != TransactionStatus::Superseded)
    {
        let failed_final = attempt.tx_status.as_ref().is_some_and(|status| {
            status.execution == crate::tx::tx_status::Execution::Failed
                && status.finality == crate::tx::tx_status::Finality::Finalized
        });
        if !failed_final {
            if attempt.tx_status.is_some() {
                return Err("Wait for destination finality before reviewing another mint".into());
            }
            return prepared_view(&operation, attempt);
        }
    }
    let transfer = operation.cctp_transfer()?;
    let prepared = provider.prepare(&operation).await?;
    let pending_nonce = provider.nonce(&operation).await?;
    let mut state = state.lock().map_err(|_| "State lock failed")?;
    check_snapshot(&state, generation, &operation)?;
    let chain = transfer.route.destination.network.chain_id()?;
    let nonce =
        state
            .evm_nonces
            .reserve(chain, transfer.destination_fee_payer()?, pending_nonce)?;
    let result = (|| {
        let wallet = state.wallet.as_ref().ok_or("Wallet closed")?;
        let key = signing_key_from_mnemonic(&wallet.mnemonic, &wallet.bip39_passphrase)?;
        let context = EvmAttemptContext {
            chain_id: chain,
            from: transfer.destination_fee_payer()?.into(),
            to: prepared.call.to.clone(),
            value: "0".into(),
            data: hex::encode(&prepared.call.data),
            nonce,
            gas_limit: prepared.estimate.gas_limit,
            max_fee_per_gas: prepared.estimate.fees.max_fee_per_gas.to_string(),
            max_priority_fee_per_gas: prepared.estimate.fees.max_priority_fee_per_gas.to_string(),
            provisional_burn_fee: None,
            provisional_destination_fee: prepared.estimate.max_native_fee.to_string(),
        };
        let (_, hash, raw, r, s) = crate::tx::evm::sign_eip1559_transfer(&Eip1559TxDraft {
            signing_key: &key,
            chain_id: chain,
            nonce,
            max_priority_fee_per_gas: prepared.estimate.fees.max_priority_fee_per_gas,
            max_fee_per_gas: prepared.estimate.fees.max_fee_per_gas,
            gas_limit: prepared.estimate.gas_limit,
            to: &prepared.call.to,
            value: 0,
            data: &prepared.call.data,
        })?;
        let attempt = TransactionAttempt {
            step: TransactionStep::Mint,
            status: TransactionStatus::Signed,
            tx_hash: hash,
            raw_tx: Some(raw),
            signature: Some(format!("0x{r}{s}")),
            fee: Some(prepared.estimate.max_native_fee.to_string()),
            nonce_or_sequence: Some(nonce.to_string()),
            error: None,
            updated_at: Utc::now().to_rfc3339(),
            tx_status: None,
            evm_context: Some(context),
            solana_context: None,
        };
        validate_source_attempt(&operation, &attempt)?;
        let mut after = operation.clone();
        if after.step == OperationStep::AwaitingAttestation {
            advance_operation(&mut after, OperationStep::Mint)?;
        }
        if after.step != OperationStep::Mint {
            return Err("Operation is not ready for mint".into());
        }
        after.attempts.push(attempt.clone());
        save_operation(&mut state, generation, &operation, &after)?;
        prepared_view(&after, &attempt)
    })();
    state
        .evm_nonces
        .cancel_unbroadcast(chain, transfer.destination_fee_payer()?, nonce);
    result
}

pub(crate) async fn submit_mint<P: CctpMintProvider + Sync>(
    state: &Mutex<AppState>,
    provider: &P,
    id: &str,
    expected_hash: &str,
) -> Result<WalletSession, String> {
    let (_action, generation, original) = begin_action(state, id)?;
    let (operation, consumed) = reconcile(state, provider, generation, &original).await?;
    if operation.step == OperationStep::Completed {
        return Ok(session_from_state(
            &*state.lock().map_err(|_| "State lock failed")?,
        ));
    }
    if consumed {
        return Err(
            "Message consumed; synchronize finalized mint evidence before continuing".into(),
        );
    }
    if operation.step != OperationStep::Mint {
        return Err("Prepare destination mint first".into());
    }
    let attempt = operation
        .attempts
        .iter()
        .rev()
        .find(|a| a.step == TransactionStep::Mint)
        .ok_or("Mint attempt missing")?;
    if attempt.tx_hash != expected_hash {
        return Err("Reviewed mint transaction changed".into());
    }
    if attempt.tx_status.is_some() {
        return Err("Mint is already included or failed; synchronize before continuing".into());
    }
    if let Some(detail) = attempt
        .error
        .as_deref()
        .filter(|e| e.starts_with("Status check failed:"))
    {
        return Err(format!(
            "Reconciliation failed; mint submission blocked ({detail}). Nothing was broadcast; retry the same step"
        ));
    }
    if !matches!(
        attempt.status,
        TransactionStatus::Signed
            | TransactionStatus::Submitted
            | TransactionStatus::SubmissionUnknown
    ) {
        return Err("Mint attempt is not actionable".into());
    }
    validate_source_attempt(&operation, attempt)?;
    provider.preflight(&operation, attempt).await?;
    let mut submitting = operation.clone();
    let attempt = submitting
        .attempts
        .iter_mut()
        .rev()
        .find(|a| a.step == TransactionStep::Mint)
        .ok_or("Mint missing")?;
    attempt.status = TransactionStatus::SubmissionUnknown;
    attempt.error = Some("Mint authorized; reconcile before retry".into());
    let raw = attempt.raw_tx.clone().ok_or("Signed bytes missing")?;
    save_operation(
        &mut *state.lock().map_err(|_| "State lock failed")?,
        generation,
        &operation,
        &submitting,
    )?;
    let outcome = provider.broadcast(&submitting, &raw).await;
    let mut after = submitting.clone();
    let attempt = after
        .attempts
        .iter_mut()
        .rev()
        .find(|a| a.step == TransactionStep::Mint)
        .ok_or("Mint missing")?;
    let error = match outcome {
        Ok(hash) if hash.eq_ignore_ascii_case(expected_hash) => {
            attempt.status = TransactionStatus::Submitted;
            None
        }
        Ok(_) => Some("Unexpected mint hash; acceptance uncertain".to_string()),
        Err(error) => Some(error),
    };
    attempt.error = error.clone();
    attempt.updated_at = Utc::now().to_rfc3339();
    after.last_error = error.clone();
    save_operation(
        &mut *state.lock().map_err(|_| "State lock failed")?,
        generation,
        &submitting,
        &after,
    )?;
    // A competing caller may have consumed the message between preflight and
    // broadcast. The next sync discovers proof; no replacement or re-burn.
    if let Some(error) = error {
        return Err(format!(
            "Mint outcome uncertain; saved hash {expected_hash}: {error}"
        ));
    }
    Ok(session_from_state(
        &*state.lock().map_err(|_| "State lock failed")?,
    ))
}

#[cfg(test)]
#[path = "tests/cctp_mint.rs"]
mod tests;
