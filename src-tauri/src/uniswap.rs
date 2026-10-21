//! Local and cross-chain swaps: agreed terms, durable signing, submission, and recovery.

use std::future::Future;
use std::sync::Mutex;

use chrono::Utc;
use serde::Serialize;
use sha3::{Digest, Keccak256};

use crate::activity::random_hex;
use crate::assets::cached_asset;
use crate::cctp_execution::{ActionGuard, check_snapshot, save_operation};
use crate::derivation::signing_key_from_mnemonic;
use crate::dto::{Activity, AssetIdentity, WalletSession};
use crate::operations::{
    EvmAttemptContext, Operation, OperationKind, OperationStep, SwapData, TransactionAttempt,
    TransactionStatus, TransactionStep, create_send_operation, reconcile_operation_with,
};
use crate::providers::evm::{EvmCallEstimate, evm_max_native_fee};
use crate::providers::uniswap::{UniswapQuoteRequest, minimum_output, verify_encoded_swap};
use crate::registry::{NetworkConfig, network_by_id};
use crate::state::{AppState, session_from_state};
use crate::storage::persist_state_wallet;
use crate::tx::evm::{
    Eip1559TxDraft, EvmCall, evm_address_word, sign_eip1559_transfer, validate_signed_call,
};
use crate::tx::tx_status::{Execution, Finality, TxStatus};
use crate::tx::uniswap::{build_approval_call, build_exact_input_single, wrap_swap_with_deadline};
use crate::uniswap_config::deployment_for_network;

pub(crate) const POOL_FEES: [u32; 4] = [100, 500, 3000, 10_000];

pub(crate) struct SwapIntent {
    pub(crate) api: Option<crate::swap_routes::ApiSwap>,
    pub(crate) relay: Option<crate::providers::relay::RelayData>,
    pub(crate) network: NetworkConfig,
    pub(crate) token_in: String,
    pub(crate) token_out: String,
    pub(crate) sender: String,
    pub(crate) amount: u128,
}

pub(crate) struct PoolQuote {
    pub(crate) pool_fee: u32,
    pub(crate) amount_out: u128,
}

/// Provider reads stay separate from signing, encrypted persistence, and UI types.
pub(crate) trait SwapProvider {
    fn route_progress(
        &self,
        _intent: &SwapIntent,
        _hash: &str,
    ) -> impl Future<Output = Result<crate::providers::relay::RelayData, String>> + Send {
        async { Err("Cross-chain progress provider unavailable".into()) }
    }

    fn quote(
        &self,
        intent: &SwapIntent,
        fee: Option<u32>,
    ) -> impl Future<Output = Result<PoolQuote, String>> + Send;
    fn allowance(&self, intent: &SwapIntent) -> impl Future<Output = Result<bool, String>> + Send;
    fn balances(
        &self,
        intent: &SwapIntent,
    ) -> impl Future<Output = Result<(u128, u128, u128), String>> + Send;
    fn estimate(
        &self,
        intent: &SwapIntent,
        call: &EvmCall,
    ) -> impl Future<Output = Result<EvmCallEstimate, String>> + Send;
    fn nonce(&self, intent: &SwapIntent) -> impl Future<Output = Result<u64, String>> + Send;
    fn observe(
        &self,
        intent: &SwapIntent,
        hash: &str,
    ) -> impl Future<Output = Result<Option<TxStatus>, String>> + Send;
    fn observe_inclusion(
        &self,
        intent: &SwapIntent,
        hash: &str,
    ) -> impl Future<Output = Result<Option<TxStatus>, String>> + Send {
        self.observe(intent, hash)
    }
    fn broadcast(
        &self,
        intent: &SwapIntent,
        raw: &str,
    ) -> impl Future<Output = Result<String, String>> + Send;
}

#[derive(Clone, Serialize)]
pub(crate) struct UniswapQuote {
    pub(crate) api: Option<crate::swap_routes::ApiSwap>,
    pub(crate) destination_network: String,
    pub(crate) relay: Option<crate::providers::relay::RelayData>,
    pub(crate) route: Vec<crate::providers::relay::RouteNode>,
    pub(crate) id: String,
    pub(crate) network: String,
    pub(crate) token_in: String,
    pub(crate) token_out: String,
    pub(crate) sender: String,
    pub(crate) amount_base_units: String,
    pub(crate) quoted_output: String,
    pub(crate) minimum_received: String,
    pub(crate) slippage_bps: u16,
    pub(crate) pool_fee: Option<u32>,
    pub(crate) deadline: u64,
    pub(crate) approval_required: bool,
    pub(crate) approval_max_fee: Option<String>,
    pub(crate) swap_max_fee: Option<String>,
    pub(crate) input_symbol: String,
    pub(crate) output_symbol: String,
    pub(crate) input_decimals: u32,
    pub(crate) output_decimals: u32,
}

#[derive(Clone, Serialize)]
pub(crate) struct UniswapPreparedStep {
    pub(crate) operation_id: String,
    pub(crate) step: String,
    pub(crate) tx_hash: String,
    pub(crate) network: String,
    pub(crate) max_native_fee: String,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SwapStage {
    Approve,
    Swap,
    Receive,
    Done,
}

/// The stepper follows persisted transaction evidence, never display labels.
fn swap_stage(operation: &Operation) -> SwapStage {
    if operation.step == OperationStep::Completed || local_swap_confirmed(operation) {
        return SwapStage::Done;
    }
    match operation.attempts.last() {
        Some(attempt) if attempt.step == TransactionStep::Approval => {
            if approval_confirmed(attempt) {
                SwapStage::Swap
            } else {
                SwapStage::Approve
            }
        }
        Some(attempt) if attempt.step == TransactionStep::Send => {
            if succeeded(attempt) && operation.swap.as_ref().is_some_and(|s| s.relay.is_some()) {
                SwapStage::Receive
            } else {
                SwapStage::Swap
            }
        }
        _ => SwapStage::Approve,
    }
}

#[derive(Serialize)]
pub(crate) struct UniswapOperationView {
    pub(crate) id: String,
    pub(crate) network: String,
    pub(crate) token_in: String,
    pub(crate) token_out: String,
    pub(crate) amount_base_units: String,
    pub(crate) terms: SwapData,
    pub(crate) progress: Activity,
    pub(crate) stage: SwapStage,
    pub(crate) terminal: bool,
    pub(crate) needs_sync: bool,
    pub(crate) can_cancel: bool,
    pub(crate) can_prepare: bool,
    pub(crate) prepared: Option<UniswapPreparedStep>,
    pub(crate) attempt_status: Option<String>,
    pub(crate) last_error: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct UniswapState {
    pub(crate) networks: Vec<String>,
    pub(crate) operations: Vec<UniswapOperationView>,
}

#[derive(Serialize)]
pub(crate) struct UniswapOperationResult {
    pub(crate) operation_id: String,
    pub(crate) session: WalletSession,
}

pub(crate) fn now() -> u64 {
    Utc::now().timestamp().try_into().unwrap_or(0)
}

fn amount(value: &str) -> Result<u128, String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("Swap amounts must be decimal base-unit integers".into());
    }
    value
        .parse()
        .map_err(|_| "Swap amount exceeds supported range".into())
}

fn request(intent: &SwapIntent) -> UniswapQuoteRequest<'_> {
    UniswapQuoteRequest {
        network_id: &intent.network.id,
        token_in: &intent.token_in,
        token_out: &intent.token_out,
        sender: &intent.sender,
        amount: intent.amount,
    }
}

fn swap_call(intent: &SwapIntent, data: &SwapData) -> Result<EvmCall, String> {
    if let Some(api) = &intent.api {
        let call = api.evm_call()?;
        crate::tx::lifi::verify_call(
            &call,
            &intent.sender,
            &intent.token_in,
            &intent.token_out,
            intent.amount,
            amount(&data.minimum_received)?,
        )?;
        return Ok(call);
    }
    if let Some(relay) = &data.relay {
        return crate::providers::relay::deposit_call(
            relay,
            &intent.network.id,
            &intent.token_in,
            &intent.token_out,
            &intent.sender,
            intent.amount,
            amount(&data.minimum_received)?,
            amount(&data.quoted_output)?,
        );
    }
    let call = wrap_swap_with_deadline(
        build_exact_input_single(
            &intent.token_in,
            &intent.token_out,
            &data.router,
            &intent.sender,
            data.pool_fee.ok_or("Local pool fee missing")?,
            intent.amount,
            amount(&data.minimum_received)?,
        )?,
        data.deadline,
    )?;
    // Enforce the same decoder and checks used for externally supplied calldata.
    verify_encoded_swap(
        &request(intent),
        amount(&data.quoted_output)?,
        EvmCall {
            to: call.to.clone(),
            value: call.value,
            data: call.data.clone(),
        },
        Some(data.slippage_bps),
        now(),
    )?;
    Ok(call)
}

/// Validates persisted identity against the current pinned deployment, not just a ticker.
pub(crate) fn validate_operation(operation: &Operation) -> Result<(SwapIntent, &SwapData), String> {
    if operation.kind != OperationKind::Swap {
        return Err("Operation is not a swap".into());
    }
    let data = operation.swap.as_ref().ok_or("Saved swap terms missing")?;
    if data.api.is_some() {
        return crate::swap_routes::validate_evm(operation);
    }
    if data.relay.is_some() {
        return validate_relay_operation(operation, data);
    }
    let deployment = deployment_for_network(&operation.source_asset.network)?;
    let token_in = operation
        .source_asset
        .token_address
        .as_deref()
        .ok_or("Swap input must be ERC-20")?;
    let token_out = operation
        .destination_asset
        .token_address
        .as_deref()
        .ok_or("Swap output must be ERC-20")?;
    let input = evm_address_word(token_in)?;
    let output = evm_address_word(token_out)?;
    let sender = evm_address_word(&operation.sender)?;
    let input_amount = amount(&operation.amount_base_units)?;
    if operation.destination_asset.network != deployment.network_id
        || data.chain_id != Some(deployment.chain_id)
        || !data.router.eq_ignore_ascii_case(deployment.router)
        || !data.quoter.eq_ignore_ascii_case(deployment.quoter_v2)
        || data.input_decimals > 255
        || data.output_decimals > 255
        || data.pool_fee.is_none_or(|fee| !POOL_FEES.contains(&fee))
        || input == output
        || input == [0; 32]
        || output == [0; 32]
        || sender == [0; 32]
        || !operation.sender.eq_ignore_ascii_case(&operation.recipient)
        || input_amount == 0
        || amount(&data.quoted_output)? == 0
        || minimum_output(amount(&data.quoted_output)?, data.slippage_bps)?
            != amount(&data.minimum_received)?
    {
        return Err("Saved swap terms or deployment do not match the supported route".into());
    }
    Ok((
        SwapIntent {
            api: None,
            relay: None,
            network: network_by_id(deployment.network_id)
                .ok_or("Swap network unavailable")?
                .clone(),
            token_in: token_in.into(),
            token_out: token_out.into(),
            sender: operation.sender.clone(),
            amount: input_amount,
        },
        data,
    ))
}

fn is_final(attempt: &TransactionAttempt) -> bool {
    attempt
        .tx_status
        .as_ref()
        .is_some_and(|status| status.finality == Finality::Finalized)
}

fn succeeded(attempt: &TransactionAttempt) -> bool {
    is_final(attempt)
        && attempt
            .tx_status
            .as_ref()
            .is_some_and(|status| status.execution == Execution::Succeeded)
}

/// A canonical successful approval receipt allows the swap to proceed. Full
/// finality remains tracked independently; preparation rechecks live allowance.
pub(crate) fn approval_confirmed(attempt: &TransactionAttempt) -> bool {
    attempt.step == TransactionStep::Approval
        && attempt.tx_status.as_ref().is_some_and(|status| {
            status.execution == Execution::Succeeded
                && status.finality.satisfies(&Finality::Included)
        })
}

/// Local execution is complete at successful canonical inclusion. Keep the
/// durable step and finality observation separate so recovery can detect a reorg.
/// A solver deposit still needs destination settlement proof.
pub(crate) fn local_swap_confirmed(operation: &Operation) -> bool {
    operation.kind == OperationKind::Swap
        && operation.source_asset.network == operation.destination_asset.network
        && operation
            .swap
            .as_ref()
            .is_some_and(|terms| terms.relay.is_none())
        && operation.attempts.last().is_some_and(|attempt| {
            attempt.step == TransactionStep::Send
                && attempt.tx_status.as_ref().is_some_and(|status| {
                    status.execution == Execution::Succeeded
                        && status.finality.satisfies(&Finality::Included)
                })
        })
}

pub(crate) fn terminal(operation: &Operation) -> bool {
    local_swap_confirmed(operation)
        || crate::swap_solana::expired(operation)
        || operation
            .swap
            .as_ref()
            .and_then(|s| s.relay.as_ref())
            .is_some_and(|r| matches!(r.status.as_deref(), Some("failure" | "refund")))
        || matches!(
            operation.step,
            OperationStep::Completed | OperationStep::Cancelled
        )
        || operation.attempts.last().is_some_and(|attempt| {
            is_final(attempt)
                && attempt
                    .tx_status
                    .as_ref()
                    .is_some_and(|status| status.execution == Execution::Failed)
        })
}

/// Receipt reads alone advance progress; an approval never completes the swap.
pub(crate) fn update_progress(operation: &mut Operation) {
    if operation.kind != OperationKind::Swap || operation.step == OperationStep::Cancelled {
        return;
    }
    if let Some(attempt) = operation.attempts.last() {
        if succeeded(attempt) || approval_confirmed(attempt) {
            operation.step = if attempt.step == TransactionStep::Send
                && operation
                    .swap
                    .as_ref()
                    .and_then(|s| s.relay.as_ref())
                    .is_none_or(|r| {
                        r.destination_status.as_ref().is_some_and(|s| {
                            s.finality == Finality::Finalized && s.execution == Execution::Succeeded
                        })
                    }) {
                OperationStep::Completed
            } else {
                OperationStep::Send
            };
            if operation.swap.as_ref().is_none_or(|s| s.relay.is_none())
                || operation.step == OperationStep::Completed
            {
                operation.last_error = None;
            }
        }
    }
}

pub(crate) fn cancel_swap(operation: &mut Operation) -> Result<(), String> {
    if operation.swap.as_ref().is_some_and(|s| s.relay.is_some())
        && operation
            .attempts
            .iter()
            .any(|a| a.step == TransactionStep::Send && succeeded(a))
    {
        return Err("Cross-chain deposit has been sent; track the fill or refund instead".into());
    }
    if matches!(
        operation.step,
        OperationStep::Completed | OperationStep::Cancelled
    ) {
        return Err("Swap is already complete or cancelled".into());
    }
    if operation.attempts.iter().any(|attempt| {
        !is_final(attempt)
            && !approval_confirmed(attempt)
            && matches!(
                attempt.status,
                TransactionStatus::Submitted
                    | TransactionStatus::SubmissionUnknown
                    | TransactionStatus::Confirmed
                    | TransactionStatus::Failed
            )
    }) {
        return Err("A transaction may have been sent; check its status before cancelling".into());
    }
    for attempt in &mut operation.attempts {
        if attempt.status == TransactionStatus::Signed && !is_final(attempt) {
            attempt.status = TransactionStatus::Superseded;
        }
    }
    operation.step = OperationStep::Cancelled;
    operation.last_error = None;
    operation.updated_at = Utc::now().to_rfc3339();
    Ok(())
}

pub(crate) fn swap_activity(operation: &Operation) -> Result<Activity, String> {
    let data = operation.swap.as_ref().ok_or("Saved swap terms missing")?;
    let attempt = operation.attempts.last();
    let status = if operation.step == OperationStep::Cancelled {
        "cancelled"
    } else if operation.step == OperationStep::Completed || local_swap_confirmed(operation) {
        "confirmed"
    } else if terminal(operation) {
        "failed"
    } else {
        "pending"
    };
    let title = match status {
        "confirmed" => "Swap complete",
        "cancelled" => "Swap cancelled",
        "failed" => "Swap failed",
        _ if operation.swap.as_ref().is_some_and(|s| s.relay.is_some())
            && attempt.is_some_and(|a| a.step == TransactionStep::Send && succeeded(a)) =>
        {
            "Waiting for cross-chain settlement"
        }
        _ if attempt.is_some_and(|a| a.status == TransactionStatus::Signed) => {
            "Swap ready to submit"
        }
        _ if attempt.is_some_and(|a| approval_confirmed(a)) => "Approval complete; prepare swap",
        _ if attempt.is_some_and(|a| a.step == TransactionStep::Approval) => {
            "Swap approval pending"
        }
        _ if attempt.is_some_and(|a| {
            a.step == TransactionStep::Send
                && a.tx_status
                    .as_ref()
                    .is_some_and(|s| s.execution == Execution::Succeeded)
        }) =>
        {
            if data.relay.is_some() {
                "Swap deposit confirmed"
            } else {
                "Swap confirmed"
            }
        }
        _ if operation.step == OperationStep::Send
            && attempt.is_some_and(|a| a.step == TransactionStep::Send) =>
        {
            "Swap pending"
        }
        _ => "Swap ready",
    };
    Ok(Activity {
        asset: Some(operation.source_asset.clone()),
        counter_asset: Some(operation.destination_asset.clone()),
        id: operation.id.clone(),
        kind: "swap".into(),
        title: title.into(),
        subtitle: match title {
            "Approval complete; prepare swap" => "Approval is confirmed on-chain. Choose Prepare next transaction to review its network fee, then confirm the swap. The approval alone does not exchange tokens.".into(),
            "Swap approval pending" => "Waiting for the approval transaction receipt. The swap has not been sent.".into(),
            "Swap confirmed" => "The swap transaction succeeded on-chain. Waiting for network finality before marking this operation complete.".into(),
            "Swap deposit confirmed" => "The deposit transaction succeeded on-chain. Waiting for source finality before checking destination settlement; a successful deposit does not yet prove the output tokens arrived.".into(),
            "Waiting for cross-chain settlement" => "The source deposit is finalized. Checking the destination receipt and received amount; the route completes after destination finality.".into(),
            "Swap pending" => "Waiting for the swap transaction receipt.".into(),
            _ => format!("{} → {}", data.input_symbol, data.output_symbol),
        },
        amount: display_amount(
            &operation.amount_base_units,
            data.input_decimals,
            &data.input_symbol,
        ),
        status: status.into(),
        timestamp: operation.updated_at.clone(),
        hash: attempt.map(|a| a.tx_hash.clone()).unwrap_or_default(),
        from: Some(operation.sender.clone()),
        to: Some(operation.recipient.clone()),
        network: Some(operation.source_asset.network.clone()),
        payload_hash: attempt.map(|a| a.tx_hash.clone()),
        signature: attempt.and_then(|a| a.signature.clone()),
        fee: attempt.and_then(|a| a.fee.clone()),
        tx_status: attempt.and_then(|a| a.tx_status.clone()),
    })
}

fn display_amount(base_units: &str, decimals: u32, symbol: &str) -> String {
    let decimals = usize::try_from(decimals.min(255)).unwrap_or(255);
    let padded = format!("{base_units:0>width$}", width = decimals + 1);
    let split = padded.len() - decimals;
    let fraction = padded[split..].trim_end_matches('0');
    let units = if fraction.is_empty() {
        padded[..split].to_string()
    } else {
        format!("{}.{}", &padded[..split], fraction)
    };
    format!("{units} {symbol}")
}

fn prepared_view(
    operation: &Operation,
    attempt: &TransactionAttempt,
) -> Result<UniswapPreparedStep, String> {
    if operation.source_asset.network == "solana" {
        return Ok(UniswapPreparedStep {
            operation_id: operation.id.clone(),
            step: "swap".into(),
            tx_hash: attempt.tx_hash.clone(),
            network: "solana".into(),
            max_native_fee: attempt.fee.clone().ok_or("Native swap fee missing")?,
        });
    }
    let context = attempt
        .evm_context
        .as_ref()
        .ok_or("Saved swap execution context missing")?;
    Ok(UniswapPreparedStep {
        operation_id: operation.id.clone(),
        step: if attempt.step == TransactionStep::Approval {
            "approval"
        } else {
            "swap"
        }
        .into(),
        tx_hash: attempt.tx_hash.clone(),
        network: operation.source_asset.network.clone(),
        max_native_fee: evm_max_native_fee(context.gas_limit, amount(&context.max_fee_per_gas)?)?
            .to_string(),
    })
}

pub(crate) fn get_state(state: &Mutex<AppState>) -> Result<UniswapState, String> {
    let state = state.lock().map_err(|_| "State lock failed")?;
    if state.locked {
        return Err("Wallet is locked".into());
    }
    let wallet = state.wallet.as_ref().ok_or("No wallet exists")?;
    let operations = wallet
        .operations
        .iter()
        .filter(|o| o.kind == OperationKind::Swap)
        .map(|operation| {
            if crate::swap_routes::is_solana(operation) {
                crate::swap_solana::validate_operation(operation)?;
            } else {
                validate_operation(operation)?;
            }
            let mut candidate = operation.clone();
            let attempt = operation.attempts.last();
            Ok(UniswapOperationView {
                id: operation.id.clone(),
                network: operation.source_asset.network.clone(),
                token_in: operation
                    .source_asset
                    .token_address
                    .clone()
                    .unwrap_or_else(|| {
                        crate::swap_routes::native(&operation.source_asset.network).into()
                    }),
                token_out: operation
                    .destination_asset
                    .token_address
                    .clone()
                    .unwrap_or_else(|| {
                        crate::swap_routes::native(&operation.source_asset.network).into()
                    }),
                amount_base_units: operation.amount_base_units.clone(),
                terms: operation.swap.clone().ok_or("Swap terms missing")?,
                progress: swap_activity(operation)?,
                stage: swap_stage(operation),
                terminal: terminal(operation),
                needs_sync: (!terminal(operation)
                    && operation.swap.as_ref().is_some_and(|s| s.relay.is_some())
                    && operation.attempts.iter().any(|a| {
                        a.step == TransactionStep::Send && a.status != TransactionStatus::Signed
                    }))
                    || operation.attempts.iter().any(|a| {
                        !is_final(a)
                            && matches!(
                                a.status,
                                TransactionStatus::Submitted
                                    | TransactionStatus::SubmissionUnknown
                                    | TransactionStatus::Confirmed
                                    | TransactionStatus::Failed
                            )
                    }),
                can_cancel: cancel_swap(&mut candidate).is_ok(),
                can_prepare: !terminal(operation) && attempt.is_none_or(approval_confirmed),
                prepared: attempt
                    .filter(|a| a.status == TransactionStatus::Signed && a.tx_status.is_none())
                    .map(|a| prepared_view(operation, a))
                    .transpose()?,
                attempt_status: attempt.map(|a| format!("{:?}", a.status)),
                last_error: operation
                    .last_error
                    .clone()
                    .or_else(|| attempt.and_then(|a| a.error.clone())),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(UniswapState {
        networks: crate::registry::evm_networks()
            .filter(|n| crate::providers::relay::protocol_chain(&n.id).is_ok())
            .map(|n| n.id.clone())
            .chain(std::iter::once("solana".to_string()))
            .collect(),
        operations,
    })
}

/// Read a quote without signing or writing wallet storage. Cache it by an opaque ID
/// so creation cannot silently accept altered frontend terms or duplicate a retry.
pub(crate) async fn quote_swap<P: SwapProvider + Sync>(
    state: &Mutex<AppState>,
    provider: &P,
    network: &str,
    token_in: &str,
    token_out: &str,
    input_amount: &str,
    slippage_bps: u16,
) -> Result<UniswapQuote, String> {
    let (generation, intent, input, output) = {
        let state = state.lock().map_err(|_| "State lock failed")?;
        if state.locked {
            return Err("Wallet is locked".into());
        }
        let wallet = state.wallet.as_ref().ok_or("No wallet exists")?;
        let deployment = deployment_for_network(network)?;
        if !wallet.enabled_networks.iter().any(|id| id == network) {
            return Err("Enable the swap network first".into());
        }
        let input = cached_asset(&wallet.assets, network, Some(token_in))
            .ok_or("Select an input token from the wallet assets")?
            .clone();
        let output = cached_asset(&wallet.assets, network, Some(token_out))
            .ok_or("Select an output token from the wallet assets")?
            .clone();
        if evm_address_word(token_in)? == evm_address_word(token_out)? || amount(input_amount)? == 0
        {
            return Err("Choose different tokens and a positive amount".into());
        }
        let intent = SwapIntent {
            api: None,
            relay: None,
            network: network_by_id(deployment.network_id)
                .ok_or("Swap network missing")?
                .clone(),
            token_in: token_in.into(),
            token_out: token_out.into(),
            sender: wallet
                .addresses
                .get("evm")
                .ok_or("Wallet EVM address missing")?
                .clone(),
            amount: amount(input_amount)?,
        };
        (state.wallet_generation, intent, input, output)
    };
    minimum_output(1, slippage_bps)?;
    let pool = provider.quote(&intent, None).await?;
    if pool.amount_out == 0 || !POOL_FEES.contains(&pool.pool_fee) {
        return Err("Unsupported or empty pool quote".into());
    }
    let minimum = minimum_output(pool.amount_out, slippage_bps)?;
    let allowance = provider.allowance(&intent).await?;
    let deployment = deployment_for_network(network)?;
    let deadline = now().checked_add(1200).ok_or("Swap expiry overflow")?;
    let terms = SwapData {
        api: None,
        relay: None,
        quote_id: random_hex(16),
        chain_id: Some(deployment.chain_id),
        router: deployment.router.into(),
        quoter: deployment.quoter_v2.into(),
        pool_fee: Some(pool.pool_fee),
        quoted_output: pool.amount_out.to_string(),
        minimum_received: minimum.to_string(),
        slippage_bps,
        deadline,
        approval_fee_budget: None,
        swap_fee_budget: None,
        input_symbol: input.symbol.clone(),
        output_symbol: output.symbol.clone(),
        input_decimals: input.decimals,
        output_decimals: output.decimals,
    };
    let call = if allowance {
        swap_call(&intent, &terms)?
    } else {
        build_approval_call(token_in, deployment.router, intent.amount)?
    };
    let estimate = provider.estimate(&intent, &call).await?;
    let balances = provider.balances(&intent).await?;
    ensure_balances(&intent, balances, estimate.max_native_fee)?;
    let quote = UniswapQuote {
        api: None,
        destination_network: network.into(),
        relay: None,
        route: vec![
            crate::providers::relay::RouteNode {
                network: network.into(),
                token_address: Some(token_in.into()),
                symbol: input.symbol.clone(),
                pool_fee: Some(pool.pool_fee),
                via: Some("Uniswap V3".into()),
            },
            crate::providers::relay::RouteNode {
                network: network.into(),
                token_address: Some(token_out.into()),
                symbol: output.symbol.clone(),
                pool_fee: None,
                via: None,
            },
        ],
        id: terms.quote_id,
        network: network.into(),
        token_in: token_in.into(),
        token_out: token_out.into(),
        sender: intent.sender,
        amount_base_units: input_amount.into(),
        quoted_output: terms.quoted_output,
        minimum_received: terms.minimum_received,
        slippage_bps,
        pool_fee: terms.pool_fee,
        deadline,
        approval_required: !allowance,
        approval_max_fee: (!allowance).then(|| estimate.max_native_fee.to_string()),
        swap_max_fee: allowance.then(|| estimate.max_native_fee.to_string()),
        input_symbol: input.symbol,
        output_symbol: output.symbol,
        input_decimals: input.decimals,
        output_decimals: output.decimals,
    };
    let mut state = state.lock().map_err(|_| "State lock failed")?;
    if !state.can_commit_refresh(generation) {
        return Err("Wallet session changed while quoting".into());
    }
    state
        .uniswap_quotes
        .retain(|_, quote| quote.deadline > now());
    if state.uniswap_quotes.len() >= 16 {
        if let Some(oldest) = state
            .uniswap_quotes
            .values()
            .min_by_key(|q| q.deadline)
            .map(|q| q.id.clone())
        {
            state.uniswap_quotes.remove(&oldest);
        }
    }
    state.uniswap_quotes.insert(quote.id.clone(), quote.clone());
    Ok(quote)
}

pub(crate) fn create_swap_operation(
    state: &Mutex<AppState>,
    quote_id: &str,
) -> Result<UniswapOperationResult, String> {
    let mut state = state.lock().map_err(|_| "State lock failed")?;
    if state.locked {
        return Err("Wallet is locked".into());
    }
    let wallet = state.wallet.as_ref().ok_or("No wallet exists")?;
    if let Some(operation) = wallet.operations.iter().find(|o| {
        o.kind == OperationKind::Swap && o.swap.as_ref().is_some_and(|s| s.quote_id == quote_id)
    }) {
        return Ok(UniswapOperationResult {
            operation_id: operation.id.clone(),
            session: session_from_state(&state),
        });
    }
    let quote = state
        .uniswap_quotes
        .get(quote_id)
        .ok_or("Quote expired or unavailable; request a fresh quote")?
        .clone();
    if quote.deadline <= now() {
        return Err("Quote expired; request a fresh quote".into());
    }
    let source = network_by_id(&quote.network).ok_or("Swap network missing")?;
    if !wallet.addresses.get(&source.address_key).is_some_and(|a| {
        if source.kind == "svm" {
            a == &quote.sender
        } else {
            a.eq_ignore_ascii_case(&quote.sender)
        }
    }) {
        return Err("Quote belongs to another wallet".into());
    }
    if wallet.operations.iter().any(|o| {
        o.kind == OperationKind::Swap
            && !terminal(o)
            && o.source_asset.network == quote.network
            && o.sender.eq_ignore_ascii_case(&quote.sender)
    }) {
        return Err("Resume or cancel the existing swap before creating another".into());
    }
    if !wallet.enabled_networks.contains(&quote.network)
        || !wallet.enabled_networks.contains(&quote.destination_network)
    {
        return Err("Enable both quoted swap networks before continuing".into());
    }
    let (chain_id, router, quoter) = if let Some(api) = &quote.api {
        match &api.transaction {
            crate::swap_routes::RouteTransaction::Solana { .. } => (
                None,
                crate::tx::swap_solana::JUPITER.to_string(),
                String::new(),
            ),
            crate::swap_routes::RouteTransaction::Evm { to, .. } => {
                (Some(source.chain_id()?), to.clone(), String::new())
            }
        }
    } else if quote.relay.is_some() {
        (
            Some(source.chain_id()?),
            crate::providers::relay::DEPOSITORY.to_string(),
            String::new(),
        )
    } else {
        let deployment = deployment_for_network(&quote.network)?;
        (
            Some(deployment.chain_id),
            deployment.router.to_string(),
            deployment.quoter_v2.to_string(),
        )
    };
    let mut operation = create_send_operation(
        AssetIdentity::new(
            &quote.network,
            crate::swap_routes::token_identity(&quote.network, &quote.token_in),
        ),
        quote.sender.clone(),
        quote.sender,
        amount(&quote.amount_base_units)?,
        quote.input_symbol.clone(),
        String::new(),
    );
    operation.kind = OperationKind::Swap;
    operation.destination_asset = AssetIdentity::new(
        &quote.destination_network,
        crate::swap_routes::token_identity(&quote.destination_network, &quote.token_out),
    );
    operation.step = OperationStep::Approval;
    operation.swap = Some(SwapData {
        api: quote.api.clone(),
        relay: quote.relay.clone(),
        quote_id: quote.id,
        chain_id,
        router: router.into(),
        quoter: quoter.into(),
        pool_fee: quote.pool_fee,
        quoted_output: quote.quoted_output,
        minimum_received: quote.minimum_received,
        slippage_bps: quote.slippage_bps,
        deadline: quote.deadline,
        approval_fee_budget: quote.approval_max_fee,
        swap_fee_budget: quote.swap_max_fee,
        input_symbol: quote.input_symbol,
        output_symbol: quote.output_symbol,
        input_decimals: quote.input_decimals,
        output_decimals: quote.output_decimals,
    });
    if crate::swap_routes::is_solana(&operation) {
        crate::swap_solana::validate_operation(&operation)?;
    } else {
        validate_operation(&operation)?;
    }
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
    Ok(UniswapOperationResult {
        operation_id: id,
        session: session_from_state(&state),
    })
}

pub(crate) fn begin_action<'a>(
    state: &'a Mutex<AppState>,
    id: &str,
) -> Result<(ActionGuard<'a>, u64, Operation), String> {
    let mut guard = state.lock().map_err(|_| "State lock failed")?;
    if guard.locked {
        return Err("Wallet is locked".into());
    }
    let wallet = guard.wallet.as_ref().ok_or("No wallet exists")?;
    let operation = wallet
        .operations
        .iter()
        .find(|o| o.id == id && o.kind == OperationKind::Swap)
        .ok_or("Swap operation missing")?
        .clone();
    let source = network_by_id(&operation.source_asset.network).ok_or("Swap source missing")?;
    if crate::swap_routes::is_solana(&operation) {
        crate::swap_solana::validate_operation(&operation)?;
    } else {
        validate_operation(&operation)?;
    }
    if !wallet.addresses.get(&source.address_key).is_some_and(|a| {
        if source.kind == "svm" {
            a == &operation.sender
        } else {
            a.eq_ignore_ascii_case(&operation.sender)
        }
    }) {
        return Err("Swap does not belong to this wallet".into());
    }
    let generation = guard.wallet_generation;
    let key = (
        generation,
        format!(
            "{}:{}",
            operation.source_asset.network,
            if source.kind == "svm" {
                operation.sender.clone()
            } else {
                operation.sender.to_ascii_lowercase()
            }
        ),
    );
    if !guard.cctp_actions.insert(key.clone()) {
        return Err("An operation is already in progress for this account".into());
    }
    Ok((ActionGuard { state, key }, generation, operation))
}

async fn reconcile<P: SwapProvider + Sync>(
    state: &Mutex<AppState>,
    provider: &P,
    generation: u64,
    original: &Operation,
) -> Result<Operation, String> {
    let (intent, _) = validate_operation(original)?;
    let mut operation = original.clone();
    reconcile_operation_with(&mut operation, |_, hash| {
        let intent = &intent;
        let first_transaction_receipt = original.attempts.iter().any(|attempt| {
            matches!(
                attempt.step,
                TransactionStep::Approval | TransactionStep::Send
            ) && attempt.tx_hash == hash
                && attempt.tx_status.is_none()
        });
        async move {
            if first_transaction_receipt {
                provider.observe_inclusion(intent, &hash).await
            } else {
                provider.observe(intent, &hash).await
            }
        }
    })
    .await?;
    if operation.swap.as_ref().is_some_and(|s| s.relay.is_some()) {
        if let Some(attempt) = operation
            .attempts
            .last()
            .filter(|a| a.step == TransactionStep::Send && succeeded(a))
        {
            match provider.route_progress(&intent, &attempt.tx_hash).await {
                Ok(relay) => {
                    let status = relay.status.clone();
                    operation.swap.as_mut().ok_or("Route terms missing")?.relay = Some(relay);
                    operation.last_error = match status.as_deref() {
                        Some("failure") => Some("Cross-chain route failed. The deposit was sent; check Relay for refund or recovery status.".into()),
                        Some("refund") => Some("Relay reports a refund. Refresh your portfolio to check returned funds.".into()),
                        _ => None,
                    };
                    update_progress(&mut operation);
                }
                Err(error) => operation.last_error = Some(error),
            }
        }
    }
    save_operation(
        &mut *state.lock().map_err(|_| "State lock failed")?,
        generation,
        original,
        &operation,
    )?;
    Ok(operation)
}

fn ensure_balances(
    intent: &SwapIntent,
    balances: (u128, u128, u128),
    fee: u128,
) -> Result<(), String> {
    let required_native = if intent
        .token_in
        .eq_ignore_ascii_case(crate::providers::relay::NATIVE)
    {
        fee.checked_add(intent.amount)
            .ok_or("Native amount and fee overflow")?
    } else {
        fee
    };
    if balances.0 < required_native {
        return Err("Insufficient native coin for the network fee".into());
    }
    if balances.1 < intent.amount {
        return Err("Insufficient input token balance".into());
    }
    Ok(())
}

/// Relay's verified order already commits to the minimum; only the local fallback
/// has a specific V3 pool whose current output must be rechecked.
async fn check_live_pool<P: SwapProvider + Sync>(
    provider: &P,
    intent: &SwapIntent,
    data: &SwapData,
) -> Result<(), String> {
    if data.relay.is_some() {
        return Ok(());
    }
    if intent.api.is_some() {
        swap_call(intent, data)?;
        return Ok(());
    }
    let fee = data.pool_fee.ok_or("Local pool fee missing")?;
    let live = provider.quote(intent, Some(fee)).await?;
    if live.pool_fee != fee || live.amount_out < amount(&data.minimum_received)? {
        return Err("Live quote is below the agreed minimum; nothing was broadcast".into());
    }
    Ok(())
}

pub(crate) async fn prepare_swap<P: SwapProvider + Sync>(
    state: &Mutex<AppState>,
    provider: &P,
    id: &str,
) -> Result<UniswapPreparedStep, String> {
    let (_action, generation, original) = begin_action(state, id)?;
    let operation = reconcile(state, provider, generation, &original).await?;
    if terminal(&operation) {
        return Err("Swap is complete, failed, or cancelled".into());
    }
    if let Some(attempt) = operation.attempts.last() {
        if !succeeded(attempt) && !approval_confirmed(attempt) {
            if attempt.status == TransactionStatus::Signed && attempt.tx_status.is_none() {
                return prepared_view(&operation, attempt);
            }
            return Err(
                "Wait for the saved approval to confirm or the swap to finalize before preparing another".into(),
            );
        }
    }
    if operation.swap.as_ref().is_some_and(|s| s.relay.is_some())
        && operation
            .attempts
            .last()
            .is_some_and(|a| a.step == TransactionStep::Send)
    {
        return Err(
            "The cross-chain deposit is already prepared or sent; check its progress".into(),
        );
    }
    let (intent, data) = validate_operation(&operation)?;
    if data.deadline <= now()
        || data.relay.as_ref().is_some_and(|r| {
            r.order["output"]["deadline"]
                .as_u64()
                .is_none_or(|d| d <= now())
        })
    {
        return Err("Swap quote expired; cancel and request a fresh quote".into());
    }
    let allowed = provider.allowance(&intent).await?;
    let (step, call, budget) = if allowed {
        check_live_pool(provider, &intent, data).await?;
        (
            TransactionStep::Send,
            swap_call(&intent, data)?,
            data.swap_fee_budget.as_deref(),
        )
    } else {
        if !operation.attempts.is_empty() {
            return Err("Approval completed but spending permission is still insufficient".into());
        }
        (
            TransactionStep::Approval,
            build_approval_call(&intent.token_in, &data.router, intent.amount)?,
            data.approval_fee_budget.as_deref(),
        )
    };
    let estimate = provider.estimate(&intent, &call).await?;
    if let Some(budget) = budget {
        if estimate.max_native_fee > amount(budget)? {
            return Err(
                "Network fee exceeds the reviewed quote; cancel and request a fresh quote".into(),
            );
        }
    }
    ensure_balances(
        &intent,
        provider.balances(&intent).await?,
        estimate.max_native_fee,
    )?;
    let pending_nonce = provider.nonce(&intent).await?;
    let mut state = state.lock().map_err(|_| "State lock failed")?;
    check_snapshot(&state, generation, &operation)?;
    let existing = state
        .wallet
        .as_ref()
        .ok_or("Wallet closed")?
        .operations
        .clone();
    state.evm_nonces.restore_operations(&existing)?;
    let nonce = state.evm_nonces.reserve(
        data.chain_id.ok_or("EVM swap chain ID missing")?,
        &intent.sender,
        pending_nonce,
    )?;
    let result = (|| {
        let wallet = state.wallet.as_ref().ok_or("Wallet closed")?;
        let key = signing_key_from_mnemonic(&wallet.mnemonic, &wallet.bip39_passphrase)?;
        let public = key.verifying_key().to_sec1_point(false);
        let sender = format!(
            "0x{}",
            hex::encode(&Keccak256::digest(&public.as_bytes()[1..])[12..])
        );
        if !sender.eq_ignore_ascii_case(&intent.sender) {
            return Err("Signing key does not match swap sender".into());
        }
        let (_, hash, raw, r, s) = sign_eip1559_transfer(&Eip1559TxDraft {
            signing_key: &key,
            chain_id: data.chain_id.ok_or("EVM swap chain ID missing")?,
            nonce,
            gas_limit: estimate.gas_limit,
            max_fee_per_gas: estimate.fees.max_fee_per_gas,
            max_priority_fee_per_gas: estimate.fees.max_priority_fee_per_gas,
            to: &call.to,
            value: call.value,
            data: &call.data,
        })?;
        let attempt = TransactionAttempt {
            step,
            status: TransactionStatus::Signed,
            tx_hash: hash,
            raw_tx: Some(raw),
            signature: Some(format!("0x{r}{s}")),
            fee: Some(estimate.max_native_fee.to_string()),
            nonce_or_sequence: Some(nonce.to_string()),
            error: None,
            updated_at: Utc::now().to_rfc3339(),
            tx_status: None,
            solana_context: None,
            evm_context: Some(EvmAttemptContext {
                chain_id: data.chain_id.ok_or("EVM swap chain ID missing")?,
                from: sender,
                to: call.to,
                value: call.value.to_string(),
                data: hex::encode(call.data),
                nonce,
                gas_limit: estimate.gas_limit,
                max_fee_per_gas: estimate.fees.max_fee_per_gas.to_string(),
                max_priority_fee_per_gas: estimate.fees.max_priority_fee_per_gas.to_string(),
                provisional_burn_fee: None,
                provisional_destination_fee: "0".into(),
            }),
        };
        let mut after = operation.clone();
        after.step = if step == TransactionStep::Approval {
            OperationStep::Approval
        } else {
            OperationStep::Send
        };
        after.attempts.push(attempt.clone());
        after.updated_at = attempt.updated_at.clone();
        save_operation(&mut state, generation, &operation, &after)?;
        prepared_view(&after, &attempt)
    })();
    state.evm_nonces.cancel_unbroadcast(
        data.chain_id.ok_or("EVM swap chain ID missing")?,
        &intent.sender,
        nonce,
    );
    result
}

fn validate_attempt(
    operation: &Operation,
    attempt: &TransactionAttempt,
) -> Result<EvmCall, String> {
    let (intent, data) = validate_operation(operation)?;
    let call = match attempt.step {
        TransactionStep::Approval => {
            build_approval_call(&intent.token_in, &data.router, intent.amount)?
        }
        TransactionStep::Send => swap_call(&intent, data)?,
        _ => return Err("Unsupported swap transaction step".into()),
    };
    validate_signed_call(
        attempt,
        &call,
        data.chain_id.ok_or("EVM swap chain ID missing")?,
        &intent.sender,
    )?;
    Ok(call)
}

pub(crate) async fn submit_swap<P: SwapProvider + Sync>(
    state: &Mutex<AppState>,
    provider: &P,
    id: &str,
    expected_hash: &str,
) -> Result<WalletSession, String> {
    let (_action, generation, original) = begin_action(state, id)?;
    let operation = reconcile(state, provider, generation, &original).await?;
    let attempt = operation
        .attempts
        .last()
        .ok_or("Prepare a swap transaction first")?;
    if attempt.tx_hash != expected_hash {
        return Err("Reviewed swap transaction changed".into());
    }
    if operation.step == OperationStep::Cancelled {
        return Err("Swap was cancelled".into());
    }
    if attempt.tx_status.is_some() {
        return Ok(session_from_state(
            &*state.lock().map_err(|_| "State lock failed")?,
        ));
    }
    if attempt
        .error
        .as_deref()
        .is_some_and(|e| e.starts_with("Status check failed:"))
    {
        return Err("Reconciliation failed; submission is blocked".into());
    }
    if !matches!(
        attempt.status,
        TransactionStatus::Signed
            | TransactionStatus::Submitted
            | TransactionStatus::SubmissionUnknown
    ) {
        return Err("Saved transaction cannot be submitted".into());
    }
    let (intent, data) = validate_operation(&operation)?;
    if data.deadline <= now()
        || data.relay.as_ref().is_some_and(|r| {
            r.order["output"]["deadline"]
                .as_u64()
                .is_none_or(|d| d <= now())
        })
    {
        return Err("Swap quote expired; nothing was broadcast".into());
    }
    let call = validate_attempt(&operation, attempt)?;
    if attempt.step == TransactionStep::Send {
        check_live_pool(provider, &intent, data).await?;
        if !provider.allowance(&intent).await? {
            return Err("Swap spending permission is insufficient".into());
        }
    }
    let context = attempt
        .evm_context
        .as_ref()
        .ok_or("Saved transaction context missing")?;
    if provider.nonce(&intent).await? != context.nonce {
        return Err(
            "Account nonce changed; check the saved transaction status before retrying".into(),
        );
    }
    let live_fee = provider.estimate(&intent, &call).await?;
    if live_fee.gas_limit > context.gas_limit
        || live_fee.fees.max_fee_per_gas > amount(&context.max_fee_per_gas)?
    {
        return Err("Saved transaction fee is too low; nothing was broadcast".into());
    }
    let max_fee = evm_max_native_fee(context.gas_limit, amount(&context.max_fee_per_gas)?)?;
    ensure_balances(&intent, provider.balances(&intent).await?, max_fee)?;
    if data.deadline <= now()
        || data.relay.as_ref().is_some_and(|r| {
            r.order["output"]["deadline"]
                .as_u64()
                .is_none_or(|d| d <= now())
        })
    {
        return Err("Swap quote expired during preflight; nothing was broadcast".into());
    }
    let mut submitting = operation.clone();
    let next = submitting.attempts.last_mut().ok_or("Attempt missing")?;
    next.status = TransactionStatus::SubmissionUnknown;
    next.error = Some("Submission authorized; reconcile before retry".into());
    next.updated_at = Utc::now().to_rfc3339();
    submitting.updated_at = next.updated_at.clone();
    save_operation(
        &mut *state.lock().map_err(|_| "State lock failed")?,
        generation,
        &operation,
        &submitting,
    )?;
    let raw = submitting
        .attempts
        .last()
        .and_then(|a| a.raw_tx.as_deref())
        .ok_or("Signed bytes missing")?;
    let result = provider.broadcast(&intent, raw).await;
    let mut after = submitting.clone();
    let next = after.attempts.last_mut().ok_or("Attempt missing")?;
    match result {
        Ok(hash) if hash.eq_ignore_ascii_case(expected_hash) => {
            next.status = TransactionStatus::Submitted;
            next.error = None;
        }
        Ok(_) => {
            next.error = Some("Provider returned a different hash; submission is uncertain".into())
        }
        Err(error) => next.error = Some(format!("Submission uncertain: {error}")),
    }
    after.last_error = next.error.clone();
    next.updated_at = Utc::now().to_rfc3339();
    after.updated_at = next.updated_at.clone();
    save_operation(
        &mut *state.lock().map_err(|_| "State lock failed")?,
        generation,
        &submitting,
        &after,
    )?;
    refresh_balances(state, provider, generation, &after).await?;
    Ok(session_from_state(
        &*state.lock().map_err(|_| "State lock failed")?,
    ))
}

async fn refresh_balances<P: SwapProvider + Sync>(
    state: &Mutex<AppState>,
    provider: &P,
    generation: u64,
    operation: &Operation,
) -> Result<(), String> {
    let (intent, _) = validate_operation(operation)?;
    let result = provider.balances(&intent).await;
    let mut state = state.lock().map_err(|_| "State lock failed")?;
    check_snapshot(&state, generation, operation)?;
    if let Ok((native, input, output)) = result {
        let wallet = state.wallet.as_mut().ok_or("Wallet closed")?;
        for asset in &mut wallet.assets {
            if let Some(relay) = &intent.relay {
                if asset.network == relay.destination_network
                    && asset
                        .token_address
                        .as_deref()
                        .unwrap_or(crate::providers::relay::NATIVE)
                        .eq_ignore_ascii_case(&intent.token_out)
                {
                    asset.balance = output.to_string();
                }
            }
            if asset.network != intent.network.id {
                continue;
            }
            let balance = match asset.token_address.as_deref() {
                None => Some(native),
                Some(token) if token.eq_ignore_ascii_case(&intent.token_in) => Some(input),
                Some(token)
                    if intent.relay.is_none() && token.eq_ignore_ascii_case(&intent.token_out) =>
                {
                    Some(output)
                }
                _ => None,
            };
            if let Some(balance) = balance {
                asset.balance = balance.to_string();
            }
        }
    }
    Ok(())
}

pub(crate) async fn sync_swap<P: SwapProvider + Sync>(
    state: &Mutex<AppState>,
    provider: &P,
    id: &str,
) -> Result<WalletSession, String> {
    let (_action, generation, original) = begin_action(state, id)?;
    let operation = reconcile(state, provider, generation, &original).await?;
    // Publish the first receipt before portfolio RPCs can delay its UI update.
    // Approval changes permission, not token balances. Subsequent send polls
    // refresh real balances while finality and destination settlement continue.
    if operation
        .attempts
        .last()
        .is_some_and(|attempt| attempt.step == TransactionStep::Send)
        && original
            .attempts
            .last()
            .is_some_and(|attempt| attempt.tx_status.is_some())
    {
        refresh_balances(state, provider, generation, &operation).await?;
    }
    Ok(session_from_state(
        &*state.lock().map_err(|_| "State lock failed")?,
    ))
}

pub(crate) fn cancel(state: &Mutex<AppState>, id: &str) -> Result<WalletSession, String> {
    let (_action, generation, original) = begin_action(state, id)?;
    let mut operation = original.clone();
    cancel_swap(&mut operation)?;
    let mut state = state.lock().map_err(|_| "State lock failed")?;
    save_operation(&mut state, generation, &original, &operation)?;
    Ok(session_from_state(&state))
}

#[cfg(test)]
#[path = "tests/uniswap.rs"]
mod tests;

fn token_identity(token: &str) -> Option<&str> {
    (!token.eq_ignore_ascii_case(crate::providers::relay::NATIVE)).then_some(token)
}

fn validate_relay_operation<'a>(
    operation: &Operation,
    data: &'a SwapData,
) -> Result<(SwapIntent, &'a SwapData), String> {
    let relay = data.relay.as_ref().ok_or("Route missing")?;
    let network = network_by_id(&operation.source_asset.network).ok_or("Source network missing")?;
    let destination =
        network_by_id(&operation.destination_asset.network).ok_or("Destination network missing")?;
    crate::providers::relay::protocol_chain(&network.id)?;
    crate::providers::relay::protocol_chain(&destination.id)?;
    let token_in = operation
        .source_asset
        .token_address
        .as_deref()
        .unwrap_or(crate::providers::relay::NATIVE);
    let token_out = operation
        .destination_asset
        .token_address
        .as_deref()
        .ok_or("No working route: native destination settlement proof is unavailable")?;
    if relay.destination_network != destination.id
        || data.chain_id != Some(network.chain_id()?)
        || !data
            .router
            .eq_ignore_ascii_case(crate::providers::relay::DEPOSITORY)
        || !operation.sender.eq_ignore_ascii_case(&operation.recipient)
        || amount(&operation.amount_base_units)? == 0
        || data.input_decimals > 255
        || data.output_decimals > 255
        || amount(&data.minimum_received)? == 0
        || amount(&data.minimum_received)?
            < minimum_output(amount(&data.quoted_output)?, data.slippage_bps)?
    {
        return Err("Saved cross-chain route no longer matches its terms".into());
    }
    let intent = SwapIntent {
        api: None,
        network: network.clone(),
        token_in: token_in.into(),
        token_out: token_out.into(),
        sender: operation.sender.clone(),
        amount: amount(&operation.amount_base_units)?,
        relay: Some(relay.clone()),
    };
    swap_call(&intent, data)?;
    Ok((intent, data))
}

/// Quotes a real aggregator route, then estimates only a locally reconstructed deposit
/// or exact approval. Unknown chains and unprovable settlement paths fail before signing.
pub(crate) async fn quote_routed_swap(
    state: &Mutex<AppState>,
    client: &reqwest::Client,
    source: &str,
    destination: &str,
    token_in: &str,
    token_out: &str,
    input_amount: &str,
    slippage: u16,
) -> Result<UniswapQuote, String> {
    build_routed_quote(
        state,
        client,
        source,
        destination,
        token_in,
        token_out,
        input_amount,
        slippage,
        true,
    )
    .await
}

async fn build_routed_quote(
    state: &Mutex<AppState>,
    client: &reqwest::Client,
    source: &str,
    destination: &str,
    token_in: &str,
    token_out: &str,
    input_amount: &str,
    slippage: u16,
    check_funds: bool,
) -> Result<UniswapQuote, String> {
    let (generation, sender, input, output) = {
        let guard = state.lock().map_err(|_| "State lock failed")?;
        if guard.locked {
            return Err("Wallet is locked".into());
        }
        let wallet = guard.wallet.as_ref().ok_or("No wallet exists")?;
        if !wallet.enabled_networks.iter().any(|n| n == source)
            || !wallet.enabled_networks.iter().any(|n| n == destination)
        {
            return Err("Enable both swap networks first".into());
        }
        let input = cached_asset(&wallet.assets, source, token_identity(token_in))
            .ok_or("Select an input asset from your wallet")?
            .clone();
        let output = cached_asset(&wallet.assets, destination, token_identity(token_out))
            .ok_or("Select an output asset from your wallet")?
            .clone();
        (
            guard.wallet_generation,
            wallet
                .addresses
                .get("evm")
                .ok_or("Wallet EVM address missing")?
                .clone(),
            input,
            output,
        )
    };
    let network = network_by_id(source).ok_or("Unknown source network")?;
    let dest = network_by_id(destination).ok_or("Unknown destination network")?;
    crate::providers::relay::protocol_chain(source)?;
    crate::providers::relay::protocol_chain(destination)?;
    if token_identity(token_out).is_none() {
        return Err("No working route: this wallet cannot yet verify native-coin cross-chain settlement. Select a destination token.".into());
    }
    if amount(input_amount)? == 0
        || (source == destination && token_in.eq_ignore_ascii_case(token_out))
    {
        return Err("Choose different assets and a positive amount".into());
    }
    if slippage >= 10000 {
        return Err("Slippage must be below 100%".into());
    }
    let (relay, expected, minimum) = crate::providers::relay::quote(
        client,
        network,
        dest,
        token_in,
        token_out,
        &sender,
        amount(input_amount)?,
        slippage,
    )
    .await?;
    let intent = SwapIntent {
        api: None,
        network: network.clone(),
        token_in: token_in.into(),
        token_out: token_out.into(),
        sender: sender.clone(),
        amount: amount(input_amount)?,
        relay: Some(relay.clone()),
    };
    let provider = crate::providers::uniswap::LiveSwapProvider { client };
    let allowed = provider.allowance(&intent).await?;
    let call = if allowed {
        crate::providers::relay::deposit_call(
            &relay,
            source,
            token_in,
            token_out,
            &sender,
            intent.amount,
            minimum,
            expected,
        )?
    } else {
        build_approval_call(token_in, crate::providers::relay::DEPOSITORY, intent.amount)?
    };
    let estimate = provider.estimate(&intent, &call).await?;
    if check_funds {
        ensure_balances(
            &intent,
            provider.balances(&intent).await?,
            estimate.max_native_fee,
        )?;
    }

    let quote = UniswapQuote {
        api: None,
        id: random_hex(16),
        network: source.into(),
        destination_network: destination.into(),
        route: relay.route.clone(),
        relay: Some(relay),
        token_in: token_in.into(),
        token_out: token_out.into(),
        sender,
        amount_base_units: input_amount.into(),
        quoted_output: expected.to_string(),
        minimum_received: minimum.to_string(),
        slippage_bps: slippage,
        pool_fee: None,
        deadline: (now() + 1200).min(
            intent
                .relay
                .as_ref()
                .and_then(|r| r.order["output"]["deadline"].as_u64())
                .ok_or("Route expiry missing")?,
        ),
        approval_required: !allowed,
        approval_max_fee: (!allowed).then(|| estimate.max_native_fee.to_string()),
        swap_max_fee: allowed.then(|| estimate.max_native_fee.to_string()),
        input_symbol: input.symbol,
        output_symbol: output.symbol,
        input_decimals: input.decimals,
        output_decimals: output.decimals,
    };
    let mut guard = state.lock().map_err(|_| "State lock failed")?;
    if !guard.can_commit_refresh(generation) {
        return Err("Wallet session changed while quoting".into());
    }
    guard.uniswap_quotes.retain(|_, q| q.deadline > now());
    if guard.uniswap_quotes.len() >= 16 {
        if let Some(oldest) = guard
            .uniswap_quotes
            .iter()
            .min_by_key(|(_, q)| q.deadline)
            .map(|(id, _)| id.clone())
        {
            guard.uniswap_quotes.remove(&oldest);
        }
    }
    guard.uniswap_quotes.insert(quote.id.clone(), quote.clone());
    Ok(quote)
}

/// Native Max uses real route fees and balances. Each revised amount gets a new
/// verified quote; no transaction is signed and no cached balance is spent locally.
pub(crate) async fn quote_native_max(
    state: &Mutex<AppState>,
    client: &reqwest::Client,
    source: &str,
    destination: &str,
    token_out: &str,
    slippage: u16,
) -> Result<UniswapQuote, String> {
    crate::providers::relay::protocol_chain(source)?;
    let network = network_by_id(source).ok_or("Unknown source network")?;
    let (generation, sender) = {
        let guard = state.lock().map_err(|_| "State lock failed")?;
        if guard.locked {
            return Err("Wallet is locked".into());
        }
        let wallet = guard.wallet.as_ref().ok_or("No wallet exists")?;
        if !wallet.enabled_networks.iter().any(|id| id == source)
            || !wallet.enabled_networks.iter().any(|id| id == destination)
        {
            return Err("Enable both swap networks first".into());
        }
        (
            guard.wallet_generation,
            wallet
                .addresses
                .get("evm")
                .ok_or("Wallet EVM address missing")?
                .clone(),
        )
    };
    crate::providers::evm::verify_evm_chain(client, network).await?;
    let balance = crate::providers::evm::fetch_evm_native_balance(client, network, &sender).await?;
    if balance < 2 {
        return Err("No native balance is available to swap".into());
    }
    // A smaller probe leaves gas for eth_estimateGas. Its real fee gives the
    // starting spendable amount; subsequent quotes may only lower that maximum.
    let mut spend = balance / 2;
    for pass in 0..4 {
        let quote = build_routed_quote(
            state,
            client,
            source,
            destination,
            crate::providers::relay::NATIVE,
            token_out,
            &spend.to_string(),
            slippage,
            false,
        )
        .await?;
        let live_balance =
            crate::providers::evm::fetch_evm_native_balance(client, network, &sender).await?;
        if !state
            .lock()
            .map_err(|_| "State lock failed")?
            .can_commit_refresh(generation)
        {
            return Err("Wallet session changed while calculating Max".into());
        }
        let fee = amount(
            quote
                .swap_max_fee
                .as_deref()
                .ok_or("Native swap fee unavailable")?,
        )?;
        let maximum = native_spendable(live_balance, fee)?;
        if pass > 0 && spend <= maximum {
            return Ok(quote);
        }
        spend = maximum;
    }
    Err("Network fees changed while calculating Max. Try Max again.".into())
}

fn native_spendable(balance: u128, maximum_fee: u128) -> Result<u128, String> {
    if maximum_fee == 0 {
        return Err("Network fee is unavailable; refresh before using Max".into());
    }
    balance
        .checked_sub(maximum_fee)
        .filter(|amount| *amount > 0)
        .ok_or_else(|| "Insufficient native balance after reserving the network fee".into())
}

#[derive(Clone, Serialize)]
pub(crate) struct UniswapQuoteError {
    pub(crate) provider: String,
    pub(crate) error: String,
}

#[derive(Serialize)]
pub(crate) struct UniswapComparison {
    /// All quotes have the same input/output identities and exact input amount.
    /// Higher output comes first. Source-chain gas costs are displayed separately.
    pub(crate) quotes: Vec<UniswapQuote>,
    pub(crate) unavailable: Vec<UniswapQuoteError>,
}

fn direct_quote_eligible(source: &str, destination: &str, token_in: &str, token_out: &str) -> bool {
    source == destination
        && token_identity(token_in).is_some()
        && token_identity(token_out).is_some()
        && deployment_for_network(source).is_ok()
}

/// Quote both providers independently. A failed provider must not discard a
/// working alternative, and direct Uniswap is never called across chains.
pub(crate) async fn compare_quotes(
    state: &Mutex<AppState>,
    client: &reqwest::Client,
    source: &str,
    destination: &str,
    token_in: &str,
    token_out: &str,
    input: &str,
    slippage: u16,
    native_max: bool,
) -> Result<UniswapComparison, String> {
    let generation = {
        let guard = state.lock().map_err(|_| "State lock failed")?;
        if guard.locked || guard.wallet.is_none() {
            return Err("Unlock the wallet before requesting swap routes".into());
        }
        guard.wallet_generation
    };
    if native_max && source == "solana" && destination == "solana" {
        if crate::swap_routes::token_identity(source, token_in).is_some() {
            return Err("Native Max requires SOL input".into());
        }
        return crate::swap_routes::solana_max(state, client, token_out, slippage).await;
    }
    if native_max {
        if token_identity(token_in).is_some() {
            return Err("Native Max requires a native input asset".into());
        }
        let quote =
            quote_native_max(state, client, source, destination, token_out, slippage).await?;
        return Ok(UniswapComparison {
            quotes: vec![quote],
            unavailable: vec![],
        });
    }
    let direct = async {
        if !direct_quote_eligible(source, destination, token_in, token_out) {
            return None;
        }
        Some(
            quote_swap(
                state,
                &crate::providers::uniswap::LiveSwapProvider { client },
                source,
                token_in,
                token_out,
                input,
                slippage,
            )
            .await,
        )
    };
    let relay = async {
        if source == "solana" && destination == "solana" {
            crate::swap_routes::quote(
                state,
                client,
                crate::swap_routes::RouteProvider::Relay,
                source,
                token_in,
                token_out,
                input,
                slippage,
            )
            .await
        } else {
            quote_routed_swap(
                state,
                client,
                source,
                destination,
                token_in,
                token_out,
                input,
                slippage,
            )
            .await
        }
    };
    let lifi = async {
        if source != destination {
            return Err("LI.FI cross-chain execution is unavailable until its bridge-specific instructions are validated".into());
        }
        crate::swap_routes::quote(
            state,
            client,
            crate::swap_routes::RouteProvider::Lifi,
            source,
            token_in,
            token_out,
            input,
            slippage,
        )
        .await
    };
    let timeout = std::time::Duration::from_secs(30);
    let (relay, direct, lifi) = futures_util::future::join3(
        tokio::time::timeout(timeout, relay),
        tokio::time::timeout(timeout, direct),
        tokio::time::timeout(timeout, lifi),
    )
    .await;
    let relay = relay.unwrap_or_else(|_| Err("Relay quote timed out; refresh to retry".into()));
    let direct = direct.unwrap_or_else(|_| {
        Some(Err(
            "Direct Uniswap quote timed out; refresh to retry".into()
        ))
    });
    let lifi = lifi.unwrap_or_else(|_| Err("LI.FI quote timed out; refresh to retry".into()));
    let comparison = collect_provider_quotes(vec![
        ("Relay", Some(relay)),
        ("Uniswap V3", direct),
        ("LI.FI", Some(lifi)),
    ])?;
    let mut guard = state.lock().map_err(|_| "State lock failed")?;
    if !guard.can_commit_refresh(generation) {
        return Err("Wallet session changed while comparing routes".into());
    }
    // Provider reads populate the cache independently. Preserve every offered quote
    // together so eviction during the other provider's read cannot invalidate a choice.
    for quote in &comparison.quotes {
        guard.uniswap_quotes.insert(quote.id.clone(), quote.clone());
    }
    while guard.uniswap_quotes.len() > 16 {
        let oldest = guard
            .uniswap_quotes
            .iter()
            .filter(|(id, _)| !comparison.quotes.iter().any(|q| &q.id == *id))
            .min_by_key(|(_, q)| q.deadline)
            .map(|(id, _)| id.clone());
        if let Some(id) = oldest {
            guard.uniswap_quotes.remove(&id);
        } else {
            break;
        }
    }
    Ok(comparison)
}

#[cfg(test)]
fn collect_quotes(
    relay: Result<UniswapQuote, String>,
    direct: Option<Result<UniswapQuote, String>>,
) -> Result<UniswapComparison, String> {
    collect_provider_quotes(vec![("Relay", Some(relay)), ("Uniswap V3", direct)])
}

fn collect_provider_quotes(
    candidates: Vec<(&str, Option<Result<UniswapQuote, String>>)>,
) -> Result<UniswapComparison, String> {
    let mut quotes = Vec::new();
    let mut unavailable = Vec::new();
    for (provider, result) in candidates {
        match result {
            Some(Ok(quote)) => {
                amount(&quote.quoted_output)?;
                quotes.push(quote);
            }
            Some(Err(error)) => unavailable.push(UniswapQuoteError {
                provider: provider.into(),
                error,
            }),
            None => {}
        }
    }
    if quotes.is_empty() {
        return Err(unavailable
            .iter()
            .map(|e| format!("{}: {}", e.provider, e.error))
            .collect::<Vec<_>>()
            .join("; "));
    }
    let first = &quotes[0];
    for quote in &quotes[1..] {
        if quote.network != first.network
            || quote.destination_network != first.destination_network
            || AssetIdentity::new(
                &quote.network,
                crate::swap_routes::token_identity(&quote.network, &quote.token_in),
            ) != AssetIdentity::new(
                &first.network,
                crate::swap_routes::token_identity(&first.network, &first.token_in),
            )
            || AssetIdentity::new(
                &quote.destination_network,
                crate::swap_routes::token_identity(&quote.destination_network, &quote.token_out),
            ) != AssetIdentity::new(
                &first.destination_network,
                crate::swap_routes::token_identity(&first.destination_network, &first.token_out),
            )
            || amount(&quote.amount_base_units)? != amount(&first.amount_base_units)?
            || quote.output_decimals != first.output_decimals
            || quote.input_decimals != first.input_decimals
            || quote.slippage_bps != first.slippage_bps
            || (if quote.network == "solana" {
                quote.sender != first.sender
            } else {
                !quote.sender.eq_ignore_ascii_case(&first.sender)
            })
        {
            return Err("Provider quotes describe different trades and cannot be compared".into());
        }
    }
    quotes.sort_by(|a, b| {
        amount(&b.quoted_output)
            .unwrap_or(0)
            .cmp(&amount(&a.quoted_output).unwrap_or(0))
    });
    Ok(UniswapComparison {
        quotes,
        unavailable,
    })
}
