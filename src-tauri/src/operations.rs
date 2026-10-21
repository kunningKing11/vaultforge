//! Cross-chain operation records, transition checks, and provider status reconciliation.

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::activity::{random_hex, short_address};
use crate::cctp::{CctpDeploymentIdentity, CctpTransfer};
use crate::dto::{Activity, AssetIdentity};
use crate::providers::http::ProviderClients;
use crate::providers::tx_observation_provider;
use crate::tx::tx_status::{Execution, Finality, TxStatus};

/// Distinguishes the CCTP route workflow from a single-transaction chain send.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OperationKind {
    #[default]
    Cctp,
    Send,
    Swap,
}

/// A transfer workflow retained in encrypted storage for restart recovery.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct Operation {
    pub(crate) id: String,
    pub(crate) source_asset: AssetIdentity,
    pub(crate) destination_asset: AssetIdentity,
    pub(crate) sender: String,
    pub(crate) recipient: String,
    #[serde(default)]
    pub(crate) destination_signer: Option<String>,
    pub(crate) amount_base_units: String,
    pub(crate) step: OperationStep,
    #[serde(default)]
    pub(crate) kind: OperationKind,
    #[serde(default)]
    pub(crate) symbol: Option<String>,
    #[serde(default)]
    pub(crate) note: Option<String>,
    #[serde(default)]
    pub(crate) required_source_finality: Finality,
    #[serde(default)]
    pub(crate) attempts: Vec<TransactionAttempt>,
    #[serde(default)]
    pub(crate) cctp: Option<CctpData>,
    #[serde(default)]
    pub(crate) deployment: Option<CctpDeploymentIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) swap: Option<SwapData>,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    #[serde(default)]
    pub(crate) last_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct RouteNode {
    pub(crate) network: String,
    pub(crate) token_address: Option<String>,
    pub(crate) symbol: String,
    /// Hundredths of a basis point, as in Uniswap V3's fee tier. None means undisclosed.
    pub(crate) pool_fee: Option<u32>,
    pub(crate) via: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct RelayData {
    pub(crate) destination_network: String,
    pub(crate) request_id: String,
    pub(crate) order_id: String,
    pub(crate) order: serde_json::Value,
    pub(crate) route: Vec<RouteNode>,
    #[serde(default)]
    pub(crate) destination_hash: Option<String>,
    #[serde(default)]
    pub(crate) destination_status: Option<crate::tx::tx_status::TxStatus>,
    #[serde(default)]
    pub(crate) status: Option<String>,
}

/// Encrypted, agreed swap terms. Signed attempts keep the exact execution bytes.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct SwapData {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) api: Option<crate::swap_routes::ApiSwap>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) relay: Option<RelayData>,
    pub(crate) quote_id: String,
    pub(crate) chain_id: Option<u64>,
    pub(crate) router: String,
    pub(crate) quoter: String,
    pub(crate) pool_fee: Option<u32>,
    pub(crate) quoted_output: String,
    pub(crate) minimum_received: String,
    pub(crate) slippage_bps: u16,
    pub(crate) deadline: u64,
    pub(crate) approval_fee_budget: Option<String>,
    pub(crate) swap_fee_budget: Option<String>,
    pub(crate) input_symbol: String,
    pub(crate) output_symbol: String,
    pub(crate) input_decimals: u32,
    pub(crate) output_decimals: u32,
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
    #[serde(default)]
    pub(crate) evm_context: Option<EvmAttemptContext>,
    #[serde(default)]
    pub(crate) solana_context: Option<SolanaAttemptContext>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct SolanaLookup {
    pub(crate) address: String,
    pub(crate) addresses: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct SolanaAttemptContext {
    pub(crate) payer: String,
    pub(crate) blockhash: String,
    pub(crate) last_valid_height: u64,
    #[serde(with = "u64_units")]
    pub(crate) fee_lamports: u64,
    #[serde(with = "u64_units")]
    pub(crate) rent_lamports: u64,
    pub(crate) event_account: Option<String>,
    pub(crate) fee_recipient: Option<String>,
    pub(crate) lookup: Option<SolanaLookup>,
    pub(crate) setup_slot: Option<u64>,
    pub(crate) create_ata: bool,
    pub(crate) provisional_destination_fee: Option<String>,
}

mod u64_units {
    use serde::{Deserialize, Deserializer, Serializer};
    pub(super) fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }
    pub(super) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// Exact unsigned EVM context corresponding to the encrypted signed bytes.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct EvmAttemptContext {
    pub(crate) chain_id: u64,
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) value: String,
    pub(crate) data: String,
    pub(crate) nonce: u64,
    pub(crate) gas_limit: u64,
    pub(crate) max_fee_per_gas: String,
    pub(crate) max_priority_fee_per_gas: String,
    pub(crate) provisional_burn_fee: Option<String>,
    pub(crate) provisional_destination_fee: String,
}

/// CCTP recovery payload, separate from transaction attempts and workflow progress.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct CctpData {
    #[serde(default)]
    pub(crate) mint_evidence: Option<MintEvidence>,
    #[serde(default)]
    pub(crate) replay_cursor: Option<u64>,
    #[serde(default)]
    pub(crate) native_lookup: Option<SolanaLookup>,
    #[serde(default)]
    pub(crate) replay_signature: Option<String>,
    #[serde(default)]
    pub(crate) evidence: Option<crate::cctp_attestation::BurnEvidence>,
    #[serde(default)]
    pub(crate) nonce: Option<String>,
    #[serde(default)]
    pub(crate) simulated: bool,
    #[serde(default)]
    pub(crate) signature_verified: bool,
    #[serde(default)]
    pub(crate) poll: Option<crate::cctp_attestation::AttestationPoll>,
    pub(crate) message: Vec<u8>,
    #[serde(default)]
    pub(crate) attestation: Option<Vec<u8>>,
}

/// Finalized destination execution with decoded message and mint logs.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct MintEvidence {
    pub(crate) transaction_hash: String,
    pub(crate) message_hash: String,
    pub(crate) observation: TxStatus,
}

pub(crate) fn mint_evidence_matches(operation: &Operation, hash: &str) -> bool {
    use sha3::{Digest, Keccak256};
    operation.cctp.as_ref().is_some_and(|data| {
        data.mint_evidence.as_ref().is_some_and(|proof| {
            (if operation.destination_asset.network == "solana" {
                proof.transaction_hash == hash
            } else {
                proof.transaction_hash.eq_ignore_ascii_case(hash)
            }) && proof.message_hash == hex::encode(Keccak256::digest(&data.message))
                && proof.observation.execution == Execution::Succeeded
                && proof.observation.finality == Finality::Finalized
        })
    })
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
    Send,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) enum TransactionStep {
    DestinationSetup,
    Approval,
    Burn,
    Mint,
    Send,
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
    /// Unable to execute after verified completion and nonce/expiry reconciliation.
    Superseded,
}

/// Creates an approval-stage record for the supported CCTP route and native USDC.
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

    let transfer = CctpTransfer::new(
        &source,
        &destination,
        &sender,
        &recipient,
        amount_base_units,
    )?;

    let now = Utc::now().to_rfc3339();
    let operation = Operation {
        id: random_hex(16),
        source_asset: source,
        destination_asset: destination,
        sender,
        recipient,
        destination_signer: None,
        amount_base_units: amount_base_units.to_string(),
        step: OperationStep::Approval,
        kind: OperationKind::Cctp,
        symbol: None,
        note: None,
        required_source_finality: transfer.route.required_source_finality(),
        attempts: Vec::new(),
        cctp: None,
        deployment: Some(transfer.route.identity()?),
        swap: None,
        created_at: now.clone(),
        updated_at: now,
        last_error: None,
    };
    Ok(operation)
}

/// Creates a durable record for a single-transaction chain send. The signed
/// attempt is persisted before broadcast so a lost response stays recoverable.
pub(crate) fn create_send_operation(
    asset: AssetIdentity,
    sender: String,
    recipient: String,
    amount_base_units: u128,
    symbol: String,
    note: String,
) -> Operation {
    let asset = AssetIdentity::new(asset.network.as_str(), asset.token_address.as_deref());
    let now = Utc::now().to_rfc3339();
    Operation {
        id: random_hex(16),
        source_asset: asset.clone(),
        destination_asset: asset,
        sender,
        recipient,
        destination_signer: None,
        amount_base_units: amount_base_units.to_string(),
        step: OperationStep::Send,
        kind: OperationKind::Send,
        symbol: Some(symbol),
        note: Some(note),
        required_source_finality: Finality::Finalized,
        attempts: Vec::new(),
        cctp: None,
        deployment: None,
        swap: None,
        created_at: now.clone(),
        updated_at: now,
        last_error: None,
    }
}

impl Operation {
    /// Legacy records remain readable/reconcilable, but cannot authorize execution.
    pub(crate) fn cctp_transfer(&self) -> Result<CctpTransfer, String> {
        if self.kind != OperationKind::Cctp {
            return Err("Operation is not a CCTP transfer".into());
        }
        let transfer = CctpTransfer::new(
            &self.source_asset,
            &self.destination_asset,
            &self.sender,
            &self.recipient,
            self.amount_base_units
                .parse()
                .map_err(|_| "Invalid CCTP operation amount")?,
        )?;
        if self.deployment.as_ref() != Some(&transfer.route.identity()?)
            || self.required_source_finality != Finality::Finalized
        {
            return Err("CCTP operation deployment is missing or changed".into());
        }
        transfer.with_destination_fee_payer(self.destination_signer.as_deref())
    }
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
            if !source_finality_reached(operation) || !attestation_available(operation) {
                return Err(
                    "Bound attestation and required source finality are missing".to_string()
                );
            }
            let minted = operation.attempts.iter().any(|attempt| {
                attempt.step == TransactionStep::Mint
                    && mint_evidence_matches(operation, &attempt.tx_hash)
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

pub(crate) fn attestation_available(operation: &Operation) -> bool {
    let Ok(transfer) = operation.cctp_transfer() else {
        return false;
    };
    operation.cctp.as_ref().is_some_and(|data| {
        (if transfer.route.destination.network.kind == "evm" {
            data.simulated
        } else {
            data.signature_verified
        }) && data
            .attestation
            .as_ref()
            .is_some_and(|bytes| !bytes.is_empty() && bytes.len() % 65 == 0)
            && data.evidence.as_ref().is_some_and(|evidence| {
                crate::cctp_attestation::validate_bound_message(operation, evidence, &data.message)
                    .is_ok()
                    && operation.attempts.iter().any(|attempt| {
                        attempt.step == TransactionStep::Burn
                            && transfer
                                .source_hash_matches(&attempt.tx_hash, &evidence.transaction_hash)
                            && attempt.tx_status.as_ref().is_some_and(|status| {
                                status.execution == Execution::Succeeded
                                    && status
                                        .finality
                                        .satisfies(&operation.required_source_finality)
                                    && status.block_height == evidence.block_height
                                    && status.block_hash == evidence.block_hash
                                    && status.slot == evidence.slot
                            })
                    })
            })
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

/// Stores bound V2 payloads without advancing the operation. Destination simulation
/// is a separate mandatory gate; local signature recovery is not performed.
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
    let evidence = operation
        .cctp
        .as_ref()
        .and_then(|data| data.evidence.as_ref())
        .ok_or("CCTP burn evidence must be bound before attestation")?;
    crate::cctp_attestation::validate_bound_message(operation, evidence, &message)?;
    if attestation.len() % 65 != 0 {
        return Err("Invalid CCTP attestation signature length".into());
    }
    let nonce = format!("0x{}", hex::encode(&message[12..44]));
    let data = operation.cctp.as_mut().ok_or("CCTP evidence missing")?;
    data.message = message;
    data.attestation = Some(attestation);
    data.nonce = Some(nonce);
    data.simulated = false;
    data.signature_verified = false;
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
    if operation.kind == OperationKind::Swap {
        return crate::uniswap::cancel_swap(operation);
    }
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
    // Unbroadcast attempts must not keep protecting the account nonce after
    // cancellation; without this the account could never transact again.
    for attempt in &mut operation.attempts {
        let resolved = attempt
            .tx_status
            .as_ref()
            .is_some_and(|status| status.finality == Finality::Finalized);
        if !resolved
            && matches!(
                attempt.status,
                TransactionStatus::Signed
                    | TransactionStatus::Submitted
                    | TransactionStatus::SubmissionUnknown
            )
        {
            attempt.status = TransactionStatus::Superseded;
        }
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
    let route = operation
        .deployment
        .as_ref()
        .map(|_| operation.cctp_transfer())
        .transpose()?;
    reconcile_operation_with(operation, |network, hash| {
        let config = route.as_ref().map(|transfer| {
            if network == transfer.route.source.network.id {
                transfer.route.source.network.clone()
            } else {
                transfer.route.destination.network.clone()
            }
        });
        async move {
            if let Some(config) = config {
                if config.kind == "svm" {
                    crate::providers::solana::fetch_solana_tx_observation(
                        clients.http(),
                        &config,
                        &hash,
                    )
                    .await
                } else {
                    crate::providers::evm::fetch_evm_tx_observation(clients.http(), &config, &hash)
                        .await
                }
            } else {
                crate::providers::fetch_tx_observation(clients.http(), &network, &hash).await
            }
        }
    })
    .await
}

/// The network whose provider confirms an attempt, based on its leg.
pub(crate) fn operation_step_network(operation: &Operation, step: TransactionStep) -> String {
    match step {
        TransactionStep::Mint | TransactionStep::DestinationSetup => {
            operation.destination_asset.network.clone()
        }
        _ => operation.source_asset.network.clone(),
    }
}

pub(crate) async fn reconcile_operation_with<F, Fut>(
    operation: &mut Operation,
    mut lookup: F,
) -> Result<(), String>
where
    F: FnMut(String, String) -> Fut,
    Fut: std::future::Future<Output = Result<Option<TxStatus>, String>>,
{
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
        let network = operation_step_network(operation, leg);
        let provider = if operation.deployment.is_some() {
            operation.cctp_transfer().map(|_| ())
        } else {
            tx_observation_provider(&network).map(|_| ())
        };
        if let Err(message) = provider {
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
                if attempt
                    .error
                    .as_deref()
                    .is_some_and(|error| error.starts_with("Status check failed:"))
                {
                    attempt.error = None;
                    attempt.updated_at = Utc::now().to_rfc3339();
                    changed = true;
                }
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
    if operation.kind == OperationKind::Swap {
        crate::uniswap::update_progress(operation);
    }
    if changed {
        operation.updated_at = Utc::now().to_rfc3339();
    }
    Ok(())
}

pub(crate) fn operation_activity(operation: &Operation) -> Result<Activity, String> {
    if operation.kind == OperationKind::Send {
        return Ok(send_operation_activity(operation));
    }
    if operation.kind == OperationKind::Swap {
        return crate::uniswap::swap_activity(operation);
    }
    let title = match operation.step {
        OperationStep::Approval
            if operation
                .attempts
                .last()
                .is_some_and(|attempt| attempt.step == TransactionStep::DestinationSetup) =>
        {
            "Destination account setup"
        }
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
        OperationStep::Send => "Transfer sent",
    }
    .to_string();
    let current_leg = match operation.step {
        OperationStep::Approval
            if operation
                .attempts
                .last()
                .is_some_and(|attempt| attempt.step == TransactionStep::DestinationSetup) =>
        {
            TransactionStep::DestinationSetup
        }
        OperationStep::Approval => TransactionStep::Approval,
        OperationStep::Burn | OperationStep::AwaitingAttestation => TransactionStep::Burn,
        OperationStep::Mint | OperationStep::Completed | OperationStep::Cancelled => {
            TransactionStep::Mint
        }
        OperationStep::Send => TransactionStep::Send,
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
        })
        .or_else(|| {
            operation
                .attempts
                .iter()
                .rfind(|attempt| attempt.step == TransactionStep::DestinationSetup)
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
            Some(TransactionStep::Mint | TransactionStep::DestinationSetup) => {
                operation.destination_asset.network.clone()
            }
            _ => operation.source_asset.network.clone(),
        }),
        payload_hash: None,
        signature: None,
        fee: None,
        tx_status: displayed_attempt.and_then(|attempt| attempt.tx_status.clone()),
    })
}

/// Renders a durable chain send as the frontend `send` activity it replaces.
fn send_operation_activity(operation: &Operation) -> Activity {
    let attempt = operation
        .attempts
        .iter()
        .rfind(|attempt| attempt.step == TransactionStep::Send);
    let tx_status = attempt.and_then(|attempt| attempt.tx_status.clone());
    let status = match &tx_status {
        Some(observation) => observation.legacy_label().to_string(),
        None if attempt.is_some_and(|attempt| attempt.status == TransactionStatus::Failed) => {
            "failed".to_string()
        }
        None => "pending".to_string(),
    };
    let subtitle = match operation.note.as_deref() {
        Some(note) if !note.trim().is_empty() => note.to_string(),
        _ => format!("Sent to {}", short_address(&operation.recipient)),
    };
    Activity {
        asset: Some(operation.source_asset.clone()),
        counter_asset: None,
        id: operation.id.clone(),
        kind: "send".to_string(),
        title: "Transfer sent".to_string(),
        subtitle,
        amount: format!(
            "-{} {}",
            operation.amount_base_units,
            operation.symbol.as_deref().unwrap_or("")
        ),
        status,
        timestamp: operation.updated_at.clone(),
        hash: attempt
            .map(|attempt| attempt.tx_hash.clone())
            .unwrap_or_default(),
        from: Some(operation.sender.clone()),
        to: Some(operation.recipient.clone()),
        network: Some(operation.source_asset.network.clone()),
        payload_hash: attempt.map(|attempt| attempt.tx_hash.clone()),
        signature: attempt.and_then(|attempt| attempt.signature.clone()),
        fee: attempt.and_then(|attempt| attempt.fee.clone()),
        tx_status,
    }
}

#[cfg(test)]
#[path = "tests/operations.rs"]
mod tests;
