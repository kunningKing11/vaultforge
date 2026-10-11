//! Cross-chain operation records, transition checks, and provider status reconciliation.

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::activity::{random_hex, short_address};
use crate::assets::token_addresses_match;
use crate::dto::{Activity, AssetIdentity};
use crate::providers::http::ProviderClients;
use crate::providers::tx_observation_provider;
use crate::registry::network_by_id;
use crate::tx::tx_status::{Execution, Finality, TxStatus};
use crate::validation::validate_address_for_network;

/// A transfer workflow retained in encrypted storage for restart recovery.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct Operation {
    pub(crate) id: String,
    pub(crate) source_asset: AssetIdentity,
    pub(crate) destination_asset: AssetIdentity,
    pub(crate) sender: String,
    pub(crate) recipient: String,
    pub(crate) amount_base_units: String,
    pub(crate) step: OperationStep,
    #[serde(default)]
    pub(crate) required_source_finality: Finality,
    #[serde(default)]
    pub(crate) attempts: Vec<TransactionAttempt>,
    #[serde(default)]
    pub(crate) cctp: Option<CctpData>,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    #[serde(default)]
    pub(crate) last_error: Option<String>,
}

/// A transaction snapshot; the same hash may appear again as submission progresses.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct TransactionAttempt {
    pub(crate) step: TransactionStep,
    pub(crate) status: TransactionStatus,
    pub(crate) tx_hash: String,
    #[serde(default)]
    pub(crate) raw_tx: Option<String>,
    #[serde(default)]
    pub(crate) signature: Option<String>,
    #[serde(default)]
    pub(crate) fee: Option<String>,
    #[serde(default)]
    pub(crate) nonce_or_sequence: Option<String>,
    #[serde(default)]
    pub(crate) error: Option<String>,
    pub(crate) updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) tx_status: Option<TxStatus>,
}

/// CCTP recovery payload, separate from transaction attempts and workflow progress.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct CctpData {
    pub(crate) message: Vec<u8>,
    #[serde(default)]
    pub(crate) attestation: Option<Vec<u8>>,
}

/// Recovery position; errors are stored separately so failed attempts do not erase it.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) enum OperationStep {
    Approval,
    Burn,
    AwaitingAttestation,
    Mint,
    Completed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) enum TransactionStep {
    Approval,
    Burn,
    Mint,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) enum TransactionStatus {
    Built,
    Signed,
    Submitted,
    /// Submission failed to return a definitive result; the transaction may be on chain.
    SubmissionUnknown,
    Confirmed,
    Failed,
}

/// Creates an approval-stage record after validating assets, accounts, and amount.
/// This does not check allowances or execute a transfer.
pub(crate) fn create_operation(
    source_asset: AssetIdentity,
    destination_asset: AssetIdentity,
    sender: String,
    recipient: String,
    amount_base_units: u128,
) -> Result<Operation, String> {
    let source = AssetIdentity::new(
        source_asset.network.as_str(),
        source_asset.token_address.as_deref(),
    );
    let destination = AssetIdentity::new(
        destination_asset.network.as_str(),
        destination_asset.token_address.as_deref(),
    );

    network_by_id(source.network.as_str())
        .ok_or_else(|| format!("Unsupported network {}", source.network))?;
    network_by_id(destination.network.as_str())
        .ok_or_else(|| format!("Unsupported network {}", destination.network))?;

    let same_asset = source.network == destination.network
        && match (
            source.token_address.as_deref(),
            destination.token_address.as_deref(),
        ) {
            (None, None) => true,
            (Some(left), Some(right)) => token_addresses_match(&source.network, left, right),
            _ => false,
        };
    if same_asset {
        return Err("Source and destination assets must differ".to_string());
    }

    validate_address_for_network(sender.as_str(), source.network.as_str())?;
    validate_address_for_network(recipient.as_str(), destination.network.as_str())?;

    if amount_base_units == 0 {
        return Err("Amount must be greater than zero".to_string());
    }
    for network in [&source.network, &destination.network] {
        if matches!(network.as_str(), "bitcoin" | "solana" | "tron" | "xrpl")
            && amount_base_units > u64::MAX as u128
        {
            return Err(format!("Amount is too large for {network}"));
        }
    }

    let now = Utc::now().to_rfc3339();
    let operation = Operation {
        id: random_hex(16),
        source_asset: source,
        destination_asset: destination,
        sender,
        recipient,
        amount_base_units: amount_base_units.to_string(),
        step: OperationStep::Approval,
        required_source_finality: Finality::Finalized,
        attempts: Vec::new(),
        cctp: None,
        created_at: now.clone(),
        updated_at: now,
        last_error: None,
    };
    Ok(operation)
}

/// Checks forward-transition evidence before updating progress; errors leave the record unchanged.
pub(crate) fn advance_operation(
    operation: &mut Operation,
    next_step: OperationStep,
) -> Result<(), String> {
    if matches!(
        operation.step,
        OperationStep::Completed | OperationStep::Cancelled
    ) {
        return Err(format!("Operation is already {:?}", operation.step));
    }
    if operation.step == next_step {
        return Err(format!("Operation is already in {next_step:?}"));
    }
    match (operation.step, next_step) {
        (OperationStep::Approval, OperationStep::Burn) => {
            let approved = operation.attempts.iter().any(|attempt| {
                attempt.step == TransactionStep::Approval
                    && matches!(attempt.status, TransactionStatus::Confirmed)
            });
            let no_approval_attempts = !operation
                .attempts
                .iter()
                .any(|attempt| attempt.step == TransactionStep::Approval);
            if !(approved || no_approval_attempts) {
                return Err("Approval is not confirmed".to_string());
            }
        }
        (OperationStep::Burn, OperationStep::AwaitingAttestation) => {
            let burned = operation.attempts.iter().any(|attempt| {
                attempt.step == TransactionStep::Burn
                    && !attempt.tx_hash.trim().is_empty()
                    && matches!(
                        attempt.status,
                        TransactionStatus::Submitted
                            | TransactionStatus::SubmissionUnknown
                            | TransactionStatus::Confirmed
                    )
            });
            if !burned {
                return Err("Burn transaction hash is missing".to_string());
            }
        }
        (OperationStep::AwaitingAttestation, OperationStep::Mint) => {
            if !source_finality_reached(operation) {
                return Err("Source burn has not reached the required finality".to_string());
            }
            if !attestation_available(operation) {
                return Err("Attestation is missing".to_string());
            }
        }
        (OperationStep::Mint, OperationStep::Completed) => {
            if !source_finality_reached(operation) {
                return Err("Source burn has not reached the required finality".to_string());
            }
            let minted = operation.attempts.iter().any(|attempt| {
                attempt.step == TransactionStep::Mint
                    && matches!(attempt.status, TransactionStatus::Confirmed)
                    && attempt.tx_status.as_ref().is_some_and(|status| {
                        status.execution == Execution::Succeeded
                            && status.finality == Finality::Finalized
                    })
            });
            if !minted {
                return Err("Mint transaction is not confirmed".to_string());
            }
        }
        (current, next) => {
            return Err(format!("Cannot advance from {current:?} to {next:?}"));
        }
    }
    operation.step = next_step;
    operation.updated_at = Utc::now().to_rfc3339();
    operation.last_error = None;
    Ok(())
}

fn source_finality_reached(operation: &Operation) -> bool {
    operation.attempts.iter().any(|attempt| {
        attempt.step == TransactionStep::Burn
            && !attempt.tx_hash.trim().is_empty()
            && attempt.tx_status.as_ref().is_some_and(|status| {
                status.execution == Execution::Succeeded
                    && status
                        .finality
                        .satisfies(&operation.required_source_finality)
            })
    })
}

fn attestation_available(operation: &Operation) -> bool {
    operation.cctp.as_ref().is_some_and(|cctp| {
        !cctp.message.is_empty()
            && cctp
                .attestation
                .as_ref()
                .is_some_and(|attestation| !attestation.is_empty())
    })
}

pub(crate) fn record_transaction_attempt(
    operation: &mut Operation,
    step: TransactionStep,
    mut attempt: TransactionAttempt,
) -> Result<(), String> {
    if matches!(
        operation.step,
        OperationStep::Completed | OperationStep::Cancelled
    ) {
        let current = operation.step;
        return Err(format!("Operation is already {current:?}"));
    }
    if attempt.step != step {
        return Err("Attempt step mismatch".to_string());
    }
    if attempt.tx_hash.trim().is_empty() || attempt.tx_hash.chars().any(char::is_whitespace) {
        return Err("Transaction hash is missing".to_string());
    }
    if operation.attempts.iter().any(|existing| {
        existing.step == step && matches!(existing.status, TransactionStatus::Confirmed)
    }) {
        return Err(format!("{step:?} transaction is already confirmed"));
    }
    let now = Utc::now().to_rfc3339();
    attempt.updated_at = now.clone();
    let failed = matches!(attempt.status, TransactionStatus::Failed);
    let error = attempt.error.clone();
    operation.attempts.push(attempt);

    operation.updated_at = now;
    if failed || error.is_some() {
        operation.last_error =
            Some(error.unwrap_or_else(|| format!("{step:?} transaction failed")));
    }
    Ok(())
}

/// Stores nonempty, size-bounded payloads without advancing the operation.
/// Message binding and attestation authenticity are not yet verified.
pub(crate) fn record_attestation(
    operation: &mut Operation,
    message: Vec<u8>,
    attestation: Vec<u8>,
) -> Result<(), String> {
    if operation.step != OperationStep::AwaitingAttestation {
        return Err("Operation is not awaiting attestation".to_string());
    }
    if message.is_empty() {
        return Err("Attestation message is empty".to_string());
    }
    if attestation.is_empty() {
        return Err("Attestation is empty".to_string());
    }
    if message.len() > 16_384 {
        return Err("Attestation message is too large".to_string());
    }
    if attestation.len() > 16_384 {
        return Err("Attestation is too large".to_string());
    }
    // TODO: Verify the message's source domain, nonce, and amount against this operation.
    operation.cctp = Some(CctpData {
        message,
        attestation: Some(attestation),
    });
    operation.updated_at = Utc::now().to_rfc3339();
    if operation
        .last_error
        .as_deref()
        .is_some_and(|err| err.contains("attestation"))
    {
        operation.last_error = None;
    }
    Ok(())
}

pub(crate) fn record_operation_error(operation: &mut Operation, error: String) {
    if !error.trim().is_empty() {
        operation.last_error = Some(error);
        operation.updated_at = Utc::now().to_rfc3339();
    }
}

pub(crate) fn cancel_operation(operation: &mut Operation) -> Result<(), String> {
    if matches!(
        operation.step,
        OperationStep::Completed | OperationStep::Cancelled
    ) {
        let current = operation.step;
        return Err(format!("Operation is already {current:?}"));
    }
    // A signed burn may have been broadcast before its submission status was saved.
    if let Some(burn) = operation.attempts.iter().find(|attempt| {
        attempt.step == TransactionStep::Burn
            && attempt.status != TransactionStatus::Built
            && !attempt.tx_status.as_ref().is_some_and(|status| {
                status.execution == Execution::Failed && status.finality == Finality::Finalized
            })
    }) {
        return Err(format!(
            "Burn transaction {} may have been broadcast",
            burn.tx_hash
        ));
    }
    if operation.step != OperationStep::Approval {
        return Err("Cannot cancel after burn step".to_string());
    }
    operation.step = OperationStep::Cancelled;
    operation.updated_at = Utc::now().to_rfc3339();
    operation.last_error = None;
    Ok(())
}

/// Refreshes signed, submitted, and uncertain attempts without signing, rebroadcasting, or advancing steps.
/// RPC errors are retained on attempts without changing their submission status.
pub(crate) async fn reconcile_operation(
    operation: &mut Operation,
    clients: &ProviderClients,
) -> Result<(), String> {
    reconcile_operation_with(operation, |network, hash| async move {
        crate::providers::fetch_tx_observation(clients.http(), &network, &hash).await
    })
    .await
}

async fn reconcile_operation_with<F, Fut>(
    operation: &mut Operation,
    mut lookup: F,
) -> Result<(), String>
where
    F: FnMut(String, String) -> Fut,
    Fut: std::future::Future<Output = Result<Option<TxStatus>, String>>,
{
    if matches!(
        operation.step,
        OperationStep::Completed | OperationStep::Cancelled
    ) {
        return Ok(());
    }

    let mut changed = false;
    for index in 0..operation.attempts.len() {
        if operation.attempts[index]
            .tx_status
            .as_ref()
            .is_some_and(|status| status.finality == Finality::Finalized)
        {
            continue;
        }
        if !matches!(
            operation.attempts[index].status,
            TransactionStatus::Signed
                | TransactionStatus::Submitted
                | TransactionStatus::SubmissionUnknown
                | TransactionStatus::Confirmed
                | TransactionStatus::Failed
        ) {
            continue;
        }
        let leg = operation.attempts[index].step;
        let hash = operation.attempts[index].tx_hash.clone();
        let network = match leg {
            TransactionStep::Approval | TransactionStep::Burn => {
                operation.source_asset.network.clone()
            }
            TransactionStep::Mint => operation.destination_asset.network.clone(),
        };
        if let Err(message) = tx_observation_provider(&network) {
            operation.attempts[index].error = Some(message.clone());
            operation.attempts[index].updated_at = Utc::now().to_rfc3339();
            operation.last_error = Some(message);
            changed = true;
            continue;
        }
        let result = lookup(network, hash).await;
        match result {
            Ok(None) => {
                let attempt = &mut operation.attempts[index];
                if attempt.tx_status.take().is_some() {
                    attempt.status = TransactionStatus::SubmissionUnknown;
                    attempt.updated_at = Utc::now().to_rfc3339();
                    changed = true;
                }
            }
            Ok(Some(observation)) => {
                let status = match (&observation.execution, &observation.finality) {
                    (Execution::Failed, _) => TransactionStatus::Failed,
                    (Execution::Succeeded, Finality::Finalized) => TransactionStatus::Confirmed,
                    (Execution::Succeeded, Finality::Included) => TransactionStatus::Submitted,
                };
                let error = (observation.execution == Execution::Failed)
                    .then(|| "Transaction failed on chain".to_string());
                let attempt = &mut operation.attempts[index];
                if attempt.tx_status.as_ref() != Some(&observation)
                    || attempt.status != status
                    || attempt.error != error
                {
                    attempt.tx_status = Some(observation);
                    attempt.status = status;
                    attempt.error = error.clone();
                    attempt.updated_at = Utc::now().to_rfc3339();
                    if let Some(error) = error {
                        operation.last_error = Some(error);
                    }
                    changed = true;
                }
            }
            Err(rpc_err) => {
                operation.attempts[index].error = Some(format!("Status check failed: {rpc_err}"));
            }
        }
    }
    if changed {
        operation.updated_at = Utc::now().to_rfc3339();
    }
    Ok(())
}

pub(crate) fn operation_activity(operation: &Operation) -> Result<Activity, String> {
    let title = match operation.step {
        OperationStep::Approval => "Bridging approval pending",
        OperationStep::Burn => "Bridging burn pending",
        OperationStep::AwaitingAttestation if !source_finality_reached(operation) => {
            "Waiting for source finality"
        }
        OperationStep::AwaitingAttestation if attestation_available(operation) => {
            "Attestation available"
        }
        OperationStep::AwaitingAttestation => "Waiting for attestation",
        OperationStep::Mint => "Bridging mint pending",
        OperationStep::Completed => "Bridged",
        OperationStep::Cancelled => "Bridging cancelled",
    }
    .to_string();
    let current_leg = match operation.step {
        OperationStep::Approval => TransactionStep::Approval,
        OperationStep::Burn | OperationStep::AwaitingAttestation => TransactionStep::Burn,
        OperationStep::Mint | OperationStep::Completed | OperationStep::Cancelled => {
            TransactionStep::Mint
        }
    };
    let current_attempt = operation
        .attempts
        .iter()
        .rfind(|attempt| attempt.step == current_leg);
    let status = if operation.step == OperationStep::Cancelled {
        "cancelled"
    } else if operation.step == OperationStep::Completed {
        "confirmed"
    } else if current_attempt.is_some_and(|attempt| attempt.status == TransactionStatus::Failed) {
        "failed"
    } else {
        "pending"
    }
    .to_string();
    let displayed_attempt = operation
        .attempts
        .iter()
        .rfind(|attempt| attempt.step == TransactionStep::Mint)
        .or_else(|| {
            operation
                .attempts
                .iter()
                .rfind(|attempt| attempt.step == TransactionStep::Burn)
        })
        .or_else(|| {
            operation
                .attempts
                .iter()
                .rfind(|attempt| attempt.step == TransactionStep::Approval)
        });
    let hash = displayed_attempt
        .map(|attempt| attempt.tx_hash.clone())
        .unwrap_or_default();
    let subtitle = displayed_attempt
        .map(|attempt| short_address(&attempt.tx_hash))
        .unwrap_or_else(|| operation.destination_asset.network.clone());
    Ok(Activity {
        asset: Some(operation.source_asset.clone()),
        counter_asset: Some(operation.destination_asset.clone()),
        id: operation.id.clone(),
        kind: "cctp".to_string(),
        title,
        subtitle,
        amount: operation.amount_base_units.clone(),
        status,
        timestamp: operation.updated_at.clone(),
        hash,
        from: Some(operation.sender.clone()),
        to: Some(operation.recipient.clone()),
        network: Some(match displayed_attempt.map(|attempt| attempt.step) {
            Some(TransactionStep::Mint) => operation.destination_asset.network.clone(),
            _ => operation.source_asset.network.clone(),
        }),
        payload_hash: None,
        signature: None,
        fee: None,
        tx_status: displayed_attempt.and_then(|attempt| attempt.tx_status.clone()),
    })
}

#[cfg(test)]
#[path = "tests/operations.rs"]
mod tests;
