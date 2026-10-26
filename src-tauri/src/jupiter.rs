//! Direct Jupiter orchestration using shared Solana persistence and recovery.
//! Orders retain their original message and blockhash and submit through `/execute`.
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::dto::WalletSession;
use crate::operations::Operation;
use crate::providers::jupiter::{JupiterOrder, OrderRequest};
use crate::state::AppState;
use crate::swap_routes::{RouteProvider, RouteTransaction};
use crate::uniswap::{UniswapPreparedStep, UniswapQuote};

/// Encrypted order terms stored in `SwapData.api.jupiter` since version 16.
/// With one signer, the transaction ID is known before execution.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct JupiterTerms {
    pub(crate) order: JupiterOrder,
    pub(crate) message_hash: String,
    pub(crate) last_valid_height: u64,
    pub(crate) deadline: u64,
}
pub(crate) fn terms(operation: &Operation) -> Option<&JupiterTerms> {
    operation.swap.as_ref()?.api.as_ref()?.jupiter.as_ref()
}
pub(crate) fn is_jupiter(operation: &Operation) -> bool {
    operation
        .swap
        .as_ref()
        .and_then(|s| s.api.as_ref())
        .is_some_and(|api| api.provider == RouteProvider::Jupiter)
}
pub(crate) fn last_valid_height(order: &JupiterOrder) -> Result<u64, String> {
    order.raw_response["lastValidBlockHeight"]
        .as_str()
        .and_then(|v| v.parse().ok())
        .or_else(|| order.raw_response["lastValidBlockHeight"].as_u64())
        .filter(|height| *height > 0)
        .ok_or("Jupiter order expiry height missing".into())
}
pub(crate) fn deadline(order: &JupiterOrder) -> Result<u64, String> {
    let local = crate::uniswap::now() + 60;
    let deadline = match order.raw_response.get("expireAt").filter(|v| !v.is_null()) {
        Some(value) => {
            chrono::DateTime::parse_from_rfc3339(value.as_str().ok_or("Invalid Jupiter expiry")?)
                .map_err(|_| "Invalid Jupiter expiry")?
                .timestamp()
                .try_into()
                .map_err(|_| "Invalid Jupiter expiry")?
        }
        None => local,
    };
    if deadline <= crate::uniswap::now() {
        return Err("Jupiter order expired".into());
    }
    Ok(deadline.min(local))
}
pub(crate) fn validate_operation(operation: &Operation) -> Result<(), String> {
    let swap = operation.swap.as_ref().ok_or("Swap terms missing")?;
    let api = swap.api.as_ref().ok_or("Jupiter provider missing")?;
    let terms = terms(operation).ok_or("Saved Jupiter terms missing")?;
    let RouteTransaction::Solana {
        transaction,
        lookups,
    } = &api.transaction
    else {
        return Err("Jupiter transaction family changed".into());
    };
    let request = OrderRequest {
        taker: operation.sender.clone(),
        input_mint: operation
            .source_asset
            .token_address
            .clone()
            .unwrap_or(crate::tx::swap_solana::NATIVE.into()),
        output_mint: operation
            .destination_asset
            .token_address
            .clone()
            .unwrap_or(crate::tx::swap_solana::NATIVE.into()),
        amount_base_units: operation.amount_base_units.clone(),
        slippage_bps: swap.slippage_bps,
    };
    let validated = crate::tx::jupiter::validate_order_with_tokens(
        &terms.order,
        &request,
        lookups,
        api.solana_tokens.as_ref(),
    )?;
    if validated.transaction.message.static_account_keys().first()
        != Some(&crate::tx::swap_solana::key(&operation.sender)?)
        || !is_jupiter(operation)
        || validated.wallet_signer_index != 0
        || transaction != &terms.order.transaction_base64
        || validated.message_hash != terms.message_hash
        || api.request_id != terms.order.request_id
        || swap.quoted_output != terms.order.quoted_output_base_units
        || swap.minimum_received != terms.order.minimum_output_base_units
        || swap.deadline != terms.deadline
        || last_valid_height(&terms.order)? != terms.last_valid_height
    {
        return Err("Saved Jupiter order differs from reviewed terms".into());
    }
    Ok(())
}
pub(crate) async fn quote(
    state: &Mutex<AppState>,
    client: &reqwest::Client,
    input: &str,
    output: &str,
    amount: &str,
    slippage: u16,
) -> Result<UniswapQuote, String> {
    crate::swap_routes::quote(
        state,
        client,
        RouteProvider::Jupiter,
        "solana",
        input,
        output,
        amount,
        slippage,
    )
    .await
}
pub(crate) async fn prepare(
    state: &Mutex<AppState>,
    client: &(impl crate::swap_solana::SolanaRpc + Sync),
    id: &str,
) -> Result<UniswapPreparedStep, String> {
    crate::swap_solana::prepare(state, client, id).await
}
pub(crate) async fn submit(
    state: &Mutex<AppState>,
    client: &(impl crate::swap_solana::SolanaRpc + Sync),
    id: &str,
    expected: &str,
) -> Result<WalletSession, String> {
    crate::swap_solana::submit(state, client, id, expected).await
}
pub(crate) async fn sync(
    state: &Mutex<AppState>,
    client: &(impl crate::swap_solana::SolanaRpc + Sync),
    id: &str,
) -> Result<WalletSession, String> {
    crate::swap_solana::sync(state, client, id).await
}
