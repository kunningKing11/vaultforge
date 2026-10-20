//! Durable source-leg execution. Each command authorizes one step, never a route.

use std::future::Future;
use std::sync::Mutex;

use chrono::Utc;
use serde::Serialize;
use sha3::{Digest, Keccak256};

use crate::cctp::CctpTransfer;
use crate::derivation::signing_key_from_mnemonic;
use crate::dto::{AssetIdentity, WalletSession};
use crate::operations::{
    EvmAttemptContext, Operation, OperationKind, OperationStep, TransactionAttempt,
    TransactionStatus, TransactionStep, advance_operation, create_operation,
    reconcile_operation_with,
};
use crate::providers::cctp::{CctpPreflight, CctpSourcePreparation};
use crate::state::{AppState, session_from_state};
use crate::storage::persist_state_wallet;
use crate::tx::cctp::{build_approval_call, build_burn_call, build_mint_call};
use crate::tx::evm::{Eip1559TxDraft, EvmCall, sign_eip1559_transfer};
use crate::tx::tx_status::{Execution, Finality, TxStatus};

pub(crate) trait CctpSourceProvider {
    fn prepare(
        &self,
        transfer: &CctpTransfer,
    ) -> impl Future<Output = Result<CctpPreflight, String>> + Send;
    fn nonce(&self, transfer: &CctpTransfer) -> impl Future<Output = Result<u64, String>> + Send;
    fn observe(
        &self,
        transfer: &CctpTransfer,
        hash: &str,
    ) -> impl Future<Output = Result<Option<TxStatus>, String>> + Send;
    fn preflight_signed(
        &self,
        transfer: &CctpTransfer,
        attempt: &TransactionAttempt,
    ) -> impl Future<Output = Result<(), String>> + Send;
    /// Exactly one request: transport or RPC errors do not prove non-acceptance.
    fn broadcast(
        &self,
        transfer: &CctpTransfer,
        raw: &str,
    ) -> impl Future<Output = Result<String, String>> + Send;
    fn balances(
        &self,
        transfer: &CctpTransfer,
    ) -> impl Future<Output = Result<(u128, u128), String>> + Send;
}

#[derive(Serialize)]
pub(crate) struct CctpOperationResult {
    pub(crate) operation_id: String,
    pub(crate) session: WalletSession,
}

#[derive(Serialize)]
pub(crate) struct CctpPreparedSource {
    pub(crate) operation_id: String,
    pub(crate) step: String,
    pub(crate) tx_hash: String,
    pub(crate) network: String,
    pub(crate) from: String,
    pub(crate) contract: String,
    pub(crate) recipient: String,
    pub(crate) amount_base_units: String,
    pub(crate) max_native_fee: String,
    pub(crate) context: Option<EvmAttemptContext>,
    pub(crate) solana_context: Option<crate::operations::SolanaAttemptContext>,
}

pub(crate) struct ActionGuard<'a> {
    pub(crate) state: &'a Mutex<AppState>,
    pub(crate) key: (u64, String),
}

impl Drop for ActionGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.cctp_actions.remove(&self.key);
        }
    }
}

pub(crate) fn begin_action<'a>(
    state: &'a Mutex<AppState>,
    id: &str,
) -> Result<(ActionGuard<'a>, u64, Operation), String> {
    let mut state_guard = state.lock().map_err(|_| "State lock failed")?;
    if state_guard.locked {
        return Err("Wallet is locked".into());
    }
    let wallet = state_guard.wallet.as_ref().ok_or("No wallet exists")?;
    let operation = wallet
        .operations
        .iter()
        .find(|operation| operation.id == id)
        .ok_or("CCTP operation not found")?
        .clone();
    let transfer = operation.cctp_transfer()?;
    if !wallet
        .addresses
        .get(&transfer.route.source.network.address_key)
        .is_some_and(|address| {
            if transfer.route.source.network.kind == "evm" {
                address.eq_ignore_ascii_case(&operation.sender)
            } else {
                address == &operation.sender
            }
        })
    {
        return Err("CCTP operation does not belong to this wallet".into());
    }
    if let Some(payer) = operation.destination_signer.as_deref() {
        if !wallet
            .addresses
            .get(&transfer.route.destination.network.address_key)
            .is_some_and(|address| {
                if transfer.route.destination.network.kind == "evm" {
                    address.eq_ignore_ascii_case(payer)
                } else {
                    address == payer
                }
            })
        {
            return Err("Destination signer does not belong to this wallet".into());
        }
    }
    let generation = state_guard.wallet_generation;
    let key = (
        generation,
        format!("{}:{}", transfer.route.source.network.id, operation.sender),
    );
    if !state_guard.cctp_actions.insert(key.clone()) {
        return Err("A CCTP action is already in progress for this account".into());
    }
    Ok((ActionGuard { state, key }, generation, operation))
}

pub(crate) fn check_snapshot(
    state: &AppState,
    generation: u64,
    snapshot: &Operation,
) -> Result<usize, String> {
    if !state.can_commit_refresh(generation) {
        return Err("Wallet session changed during operation action".into());
    }
    let wallet = state.wallet.as_ref().ok_or("Wallet closed")?;
    let index = wallet
        .operations
        .iter()
        .position(|operation| operation.id == snapshot.id)
        .ok_or("Operation disappeared")?;
    if &wallet.operations[index] != snapshot {
        return Err("Operation changed during action".into());
    }
    Ok(index)
}

pub(crate) fn save_operation(
    state: &mut AppState,
    generation: u64,
    before: &Operation,
    after: &Operation,
) -> Result<(), String> {
    let index = check_snapshot(state, generation, before)?;
    if before != after {
        state.wallet.as_mut().ok_or("Wallet closed")?.operations[index] = after.clone();
        if let Err(error) = persist_state_wallet(state) {
            state.wallet.as_mut().ok_or("Wallet closed")?.operations[index] = before.clone();
            return Err(error);
        }
    }
    state.evm_nonces.restore_operations(
        &state
            .wallet
            .as_ref()
            .ok_or("Wallet closed")?
            .operations
            .clone(),
    )?;
    Ok(())
}

pub(crate) fn create_source_operation(
    state: &Mutex<AppState>,
    source: AssetIdentity,
    destination: AssetIdentity,
    recipient: String,
    amount: String,
) -> Result<CctpOperationResult, String> {
    let mut state = state.lock().map_err(|_| "State lock failed")?;
    if state.locked {
        return Err("Wallet is locked".into());
    }
    let wallet = state.wallet.as_ref().ok_or("No wallet exists")?;
    let route = crate::cctp::CctpRoute::for_networks(&source.network, &destination.network)?;
    let sender = wallet
        .addresses
        .get(&route.source.network.address_key)
        .ok_or("Wallet source-chain address unavailable")?
        .clone();
    // One active source workflow per account prevents double-click creation from
    // becoming two separately authorized burns. Completed/cancelled records remain.
    if wallet.operations.iter().any(|operation| {
        operation.kind == OperationKind::Cctp
            && operation.sender.eq_ignore_ascii_case(&sender)
            && !matches!(
                operation.step,
                OperationStep::Completed | OperationStep::Cancelled
            )
    }) {
        return Err("Resume the existing CCTP operation before creating another".into());
    }
    let mut operation = create_operation(
        source,
        destination,
        sender,
        recipient,
        amount.parse().map_err(|_| "Invalid CCTP amount")?,
    )?;
    operation.destination_signer = wallet
        .addresses
        .get(&route.destination.network.address_key)
        .cloned();
    let id = operation.id.clone();
    state
        .wallet
        .as_mut()
        .ok_or("Wallet closed")?
        .operations
        .push(operation);
    if let Err(error) = persist_state_wallet(&mut state) {
        state
            .wallet
            .as_mut()
            .ok_or("Wallet closed")?
            .operations
            .pop();
        return Err(error);
    }
    Ok(CctpOperationResult {
        operation_id: id,
        session: session_from_state(&state),
    })
}

async fn reconcile_source<P: CctpSourceProvider>(
    state: &Mutex<AppState>,
    provider: &P,
    generation: u64,
    before: &Operation,
) -> Result<Operation, String> {
    let transfer = before.cctp_transfer()?;
    let mut after = before.clone();
    reconcile_operation_with(&mut after, |_, hash| {
        let transfer = &transfer;
        async move { provider.observe(transfer, &hash).await }
    })
    .await?;
    let mut state = state.lock().map_err(|_| "State lock failed")?;
    save_operation(&mut state, generation, before, &after)?;
    Ok(after)
}

pub(crate) fn prepared_view(
    operation: &Operation,
    attempt: &TransactionAttempt,
) -> Result<CctpPreparedSource, String> {
    if attempt.evm_context.is_none() && attempt.solana_context.is_none() {
        return Err("Legacy attempt has no execution context; reconcile only".into());
    }
    let max_native_fee = if let Some(context) = &attempt.evm_context {
        crate::providers::evm::evm_max_native_fee(
            context.gas_limit,
            context
                .max_fee_per_gas
                .parse()
                .map_err(|_| "Invalid saved fee cap")?,
        )?
        .to_string()
    } else {
        attempt
            .solana_context
            .as_ref()
            .ok_or("Native context missing")?
            .fee_lamports
            .to_string()
    };
    let destination = matches!(
        attempt.step,
        TransactionStep::Mint | TransactionStep::DestinationSetup
    );
    Ok(CctpPreparedSource {
        operation_id: operation.id.clone(),
        step: match attempt.step {
            TransactionStep::Approval => "approval",
            TransactionStep::Burn => "burn",
            TransactionStep::Mint => "mint",
            TransactionStep::DestinationSetup => "destination_setup",
            TransactionStep::Send => return Err("Not CCTP".into()),
        }
        .into(),
        tx_hash: attempt.tx_hash.clone(),
        network: if destination {
            operation.destination_asset.network.clone()
        } else {
            operation.source_asset.network.clone()
        },
        from: attempt
            .evm_context
            .as_ref()
            .map(|c| c.from.clone())
            .or_else(|| attempt.solana_context.as_ref().map(|c| c.payer.clone()))
            .ok_or("Signer missing")?,
        contract: attempt
            .evm_context
            .as_ref()
            .map(|c| c.to.clone())
            .unwrap_or_else(|| {
                if destination {
                    crate::cctp::SOLANA_TRANSMITTER.into()
                } else {
                    crate::cctp::SOLANA_MESSENGER.into()
                }
            }),
        recipient: operation.recipient.clone(),
        amount_base_units: operation.amount_base_units.clone(),
        max_native_fee,
        context: attempt.evm_context.clone(),
        solana_context: attempt.solana_context.clone(),
    })
}

pub(crate) async fn prepare_source_step<P: CctpSourceProvider + Sync>(
    state: &Mutex<AppState>,
    provider: &P,
    id: &str,
) -> Result<CctpPreparedSource, String> {
    let (_action, generation, original) = begin_action(state, id)?;
    let operation = reconcile_source(state, provider, generation, &original).await?;
    if matches!(
        operation.step,
        OperationStep::Completed | OperationStep::Cancelled | OperationStep::Mint
    ) {
        return Err("CCTP source is not actionable".into());
    }
    if let Some(attempt) = operation.attempts.last() {
        let successful = attempt.tx_status.as_ref().is_some_and(|status| {
            status.execution == Execution::Succeeded && status.finality == Finality::Finalized
        });
        if attempt.step == TransactionStep::Burn || !successful {
            // Never sign a new attempt for an uncertain, missing, or failed receipt.
            return prepared_view(&operation, attempt);
        }
    }
    let transfer = operation.cctp_transfer()?;
    let preflight = provider.prepare(&transfer).await?;
    if preflight.protocol_fee != 0
        || preflight.expected_receive_amount != transfer.amount()
        || preflight.source_native_budget == 0
    {
        return Err(
            "CCTP preflight changed the zero-fee transfer policy or omitted source funding".into(),
        );
    }
    let destination_fee = preflight.provisional_destination_fee.to_string();
    let (step, prepared, provisional_burn_fee) = match preflight.source {
        CctpSourcePreparation::Approval {
            approval,
            provisional_burn_fee,
        } => {
            if operation
                .attempts
                .iter()
                .any(|a| matches!(a.step, TransactionStep::Approval | TransactionStep::Burn))
            {
                return Err(
                    "Approval allowance is insufficient; automatic replacement is unavailable"
                        .into(),
                );
            }
            (
                TransactionStep::Approval,
                approval,
                Some(provisional_burn_fee.to_string()),
            )
        }
        CctpSourcePreparation::Burn(burn) => (TransactionStep::Burn, burn, None),
    };
    let pending_nonce = provider.nonce(&transfer).await?;
    let mut state = state.lock().map_err(|_| "State lock failed")?;
    check_snapshot(&state, generation, &operation)?;
    let existing = state
        .wallet
        .as_ref()
        .ok_or("Wallet closed")?
        .operations
        .clone();
    state.evm_nonces.restore_operations(&existing)?;
    let chain = transfer.route.source.network.chain_id()?;
    let nonce = state
        .evm_nonces
        .reserve(chain, transfer.sender(), pending_nonce)?;
    let result = (|| {
        let wallet = state.wallet.as_ref().ok_or("Wallet closed")?;
        let key = signing_key_from_mnemonic(&wallet.mnemonic, &wallet.bip39_passphrase)?;
        let public = key.verifying_key().to_sec1_point(false);
        let actual_sender = format!(
            "0x{}",
            hex::encode(&Keccak256::digest(&public.as_bytes()[1..])[12..])
        );
        if !actual_sender.eq_ignore_ascii_case(transfer.sender()) {
            return Err("Signing key does not match CCTP sender".into());
        }
        let context = EvmAttemptContext {
            chain_id: chain,
            from: actual_sender,
            to: prepared.call.to.clone(),
            value: prepared.call.value.to_string(),
            data: hex::encode(&prepared.call.data),
            nonce,
            gas_limit: prepared.estimate.gas_limit,
            max_fee_per_gas: prepared.estimate.fees.max_fee_per_gas.to_string(),
            max_priority_fee_per_gas: prepared.estimate.fees.max_priority_fee_per_gas.to_string(),
            provisional_burn_fee,
            provisional_destination_fee: destination_fee,
        };
        let (_, hash, raw, r, s) = sign_eip1559_transfer(&Eip1559TxDraft {
            signing_key: &key,
            chain_id: chain,
            nonce,
            max_priority_fee_per_gas: prepared.estimate.fees.max_priority_fee_per_gas,
            max_fee_per_gas: prepared.estimate.fees.max_fee_per_gas,
            gas_limit: prepared.estimate.gas_limit,
            to: &prepared.call.to,
            value: prepared.call.value,
            data: &prepared.call.data,
        })?;
        let attempt = TransactionAttempt {
            step,
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
        let mut after = operation.clone();
        if step == TransactionStep::Burn && after.step == OperationStep::Approval {
            advance_operation(&mut after, OperationStep::Burn)?;
        }
        after.attempts.push(attempt.clone());
        after.updated_at = Utc::now().to_rfc3339();
        save_operation(&mut state, generation, &operation, &after)?;
        prepared_view(&after, &attempt)
    })();
    // Durable protection now owns the nonce (or persistence failed and no network
    // request occurred). Release only this temporary signing reservation.
    state
        .evm_nonces
        .cancel_unbroadcast(chain, transfer.sender(), nonce);
    result
}

/// Validate authenticated storage against both operation intent and actual signed
/// bytes. Neither raw bytes nor calldata can be supplied by a frontend command.
pub(crate) fn validate_source_attempt(
    operation: &Operation,
    attempt: &TransactionAttempt,
) -> Result<EvmCall, String> {
    let transfer = operation.cctp_transfer()?;
    let call = match attempt.step {
        TransactionStep::Approval => build_approval_call(&transfer)?,
        TransactionStep::Burn => build_burn_call(&transfer)?,
        TransactionStep::DestinationSetup => {
            return Err("Native destination setup requires native validation".into());
        }
        TransactionStep::Mint => {
            if !crate::operations::attestation_available(operation) {
                return Err("Bound attestation and finalized source evidence are required".into());
            }
            let data = operation.cctp.as_ref().ok_or("CCTP data missing")?;
            build_mint_call(
                &transfer,
                &data.message,
                data.attestation.as_deref().ok_or("Attestation missing")?,
            )?
        }
        TransactionStep::Send => return Err("Not a CCTP execution attempt".into()),
    };
    let destination = attempt.step == TransactionStep::Mint;
    crate::tx::evm::validate_signed_call(
        attempt,
        &call,
        if destination {
            transfer.route.destination.network.chain_id()?
        } else {
            transfer.route.source.network.chain_id()?
        },
        if destination {
            transfer.destination_fee_payer()?
        } else {
            transfer.sender()
        },
    )?;
    Ok(call)
}

pub(crate) async fn submit_source_step<P: CctpSourceProvider + Sync>(
    state: &Mutex<AppState>,
    provider: &P,
    id: &str,
    expected_hash: &str,
) -> Result<WalletSession, String> {
    let (_action, generation, original) = begin_action(state, id)?;
    let operation = reconcile_source(state, provider, generation, &original).await?;
    let attempt = operation
        .attempts
        .last()
        .ok_or("Prepare a source transaction first")?;
    if attempt.tx_hash != expected_hash {
        return Err("Reviewed CCTP transaction changed".into());
    }
    if let Some(status) = &attempt.tx_status {
        if status.execution == Execution::Failed {
            return Err(
                "Source transaction failed on chain; automatic replacement is unavailable".into(),
            );
        }
        let state = state.lock().map_err(|_| "State lock failed")?;
        check_snapshot(&state, generation, &operation)?;
        return Ok(session_from_state(&state)); // included/finalized: never re-execute
    }
    if let Some(detail) = attempt
        .error
        .as_deref()
        .filter(|error| error.starts_with("Status check failed:"))
    {
        // Fail closed without hiding the cause: nothing was broadcast, and the
        // same step succeeds once the provider responds.
        return Err(format!(
            "Reconciliation failed; submission is blocked ({detail}). Nothing was broadcast; retry the same step"
        ));
    }
    if !matches!(
        attempt.status,
        TransactionStatus::Signed
            | TransactionStatus::Submitted
            | TransactionStatus::SubmissionUnknown
    ) {
        return Err("Source transaction cannot be submitted".into());
    }
    validate_source_attempt(&operation, attempt)?;
    let transfer = operation.cctp_transfer()?;
    provider.preflight_signed(&transfer, attempt).await?;
    let mut submitting = operation.clone();
    let attempt = submitting.attempts.last_mut().ok_or("Attempt missing")?;
    attempt.status = TransactionStatus::SubmissionUnknown;
    attempt.error = Some("Submission authorized; reconcile before retry".into());
    attempt.updated_at = Utc::now().to_rfc3339();
    submitting.updated_at = attempt.updated_at.clone();
    {
        let mut state = state.lock().map_err(|_| "State lock failed")?;
        save_operation(&mut state, generation, &operation, &submitting)?;
    }
    let raw = submitting
        .attempts
        .last()
        .and_then(|attempt| attempt.raw_tx.as_deref())
        .ok_or("Signed bytes missing")?;
    let result = provider.broadcast(&transfer, raw).await;
    let mut after = submitting.clone();
    let attempt = after.attempts.last_mut().ok_or("Attempt missing")?;
    let error = match result {
        Ok(hash) if hash.eq_ignore_ascii_case(expected_hash) => {
            attempt.status = TransactionStatus::Submitted;
            attempt.error = None;
            None
        }
        Ok(_) => Some(
            "Provider returned an unexpected transaction hash; acceptance is uncertain".to_string(),
        ),
        Err(error) => Some(error),
    };
    if let Some(error) = &error {
        attempt.error = Some(error.clone());
    }
    attempt.updated_at = Utc::now().to_rfc3339();
    after.updated_at = attempt.updated_at.clone();
    after.last_error = error.clone();
    if attempt.step == TransactionStep::Burn && after.step == OperationStep::Burn {
        advance_operation(&mut after, OperationStep::AwaitingAttestation)?;
        after.last_error = error.clone();
    }
    {
        let mut state = state.lock().map_err(|_| "State lock failed")?;
        save_operation(&mut state, generation, &submitting, &after)?;
    }
    // A lost response may still have moved funds. Refresh from RPC in either case.
    let balances = provider.balances(&transfer).await;
    let mut state = state.lock().map_err(|_| "State lock failed")?;
    check_snapshot(&state, generation, &after)?;
    if let Ok((native, usdc)) = balances {
        let wallet = state.wallet.as_mut().ok_or("Wallet closed")?;
        for asset in &mut wallet.assets {
            if asset.network == transfer.route.source.network.id {
                if asset.token_address.is_none() {
                    asset.balance = native.to_string();
                } else if asset
                    .token_address
                    .as_deref()
                    .is_some_and(|address| address.eq_ignore_ascii_case(transfer.route.source.usdc))
                {
                    asset.balance = usdc.to_string();
                }
            }
        }
        persist_state_wallet(&mut state)?;
    }
    if let Some(error) = error {
        return Err(format!(
            "Submission outcome is uncertain; saved hash {expected_hash}: {error}"
        ));
    }
    Ok(session_from_state(&state))
}

#[cfg(test)]
#[path = "tests/cctp_execution.rs"]
pub(crate) mod tests;
