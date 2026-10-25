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
    let validated = crate::tx::jupiter::validate_order(&terms.order, &request, lookups)?;
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

/// Combine chain status with delivery evidence during unlock and UI polling.
pub(crate) async fn observe(
    client: &(impl crate::swap_solana::SolanaRpc + Sync),
    operation: &Operation,
    hash: &str,
) -> Result<Option<crate::tx::tx_status::TxStatus>, String> {
    use crate::tx::tx_status::{Execution, Finality};
    use serde_json::json;
    let status = client
        .swap_rpc(
            "getSignatureStatuses",
            json!([[hash], {"searchTransactionHistory":true}]),
        )
        .await?;
    let Some(observation) =
        crate::providers::solana::parse_solana_tx_observation(&json!({"result":status}))?
    else {
        return Ok(None);
    };
    if observation.execution == Execution::Failed {
        return Ok(Some(observation));
    }
    let commitment = if observation.finality == Finality::Finalized {
        "finalized"
    } else {
        "confirmed"
    };
    let proof = client.swap_rpc("getTransaction", json!([hash, {"encoding":"base64", "commitment":commitment, "maxSupportedTransactionVersion":0}])).await?;
    verify_delivery(operation, hash, &proof, observation.slot)?;
    Ok(Some(observation))
}

fn verify_delivery(
    operation: &Operation,
    hash: &str,
    proof: &serde_json::Value,
    slot: Option<u64>,
) -> Result<(), String> {
    use crate::tx::swap_solana::{self as encoding, key};
    let terms = terms(operation).ok_or("Jupiter terms missing")?;
    let raw = proof["transaction"][0]
        .as_str()
        .ok_or("Jupiter transaction evidence unavailable")?;
    crate::tx::jupiter::verify_signed_order(&terms.order, raw, &operation.sender)?;
    let tx = encoding::decode(raw)?;
    if tx.signatures[0].to_string() != hash
        || proof["slot"].as_u64() != slot
        || proof["meta"].get("err") != Some(&serde_json::Value::Null)
    {
        return Err(
            "Jupiter chain evidence does not match the saved successful transaction".into(),
        );
    }
    let minimum: u128 = operation
        .swap
        .as_ref()
        .ok_or("Swap missing")?
        .minimum_received
        .parse()
        .map_err(|_| "Invalid minimum")?;
    let api = operation
        .swap
        .as_ref()
        .and_then(|s| s.api.as_ref())
        .ok_or("API terms missing")?;
    let RouteTransaction::Solana { lookups, .. } = &api.transaction else {
        return Err("Native transaction missing".into());
    };
    let keys = encoding::resolved_keys(&tx, lookups)?;
    if let Some(mint) = &operation.destination_asset.token_address {
        let destination = encoding::associated(&operation.sender, mint)?;
        let index = keys
            .iter()
            .position(|k| *k == destination)
            .ok_or("Jupiter receiving account missing")?;
        let read_balance = |field: &str, allow_missing: bool| -> Result<u128, String> {
            let balances = proof["meta"][field]
                .as_array()
                .ok_or("Jupiter token evidence missing")?;
            let mut matching = balances
                .iter()
                .filter(|v| v["accountIndex"].as_u64() == Some(index as u64));
            let Some(value) = matching.next() else {
                return if allow_missing {
                    Ok(0)
                } else {
                    Err("Jupiter output balance missing".into())
                };
            };
            if matching.next().is_some()
                || value["mint"].as_str() != Some(mint)
                || value["owner"].as_str() != Some(&operation.sender)
                || value["programId"]
                    .as_str()
                    .is_some_and(|v| v != "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
            {
                return Err("Jupiter output account identity changed".into());
            }
            value["uiTokenAmount"]["amount"]
                .as_str()
                .ok_or("Jupiter output amount missing")?
                .parse()
                .map_err(|_| "Invalid Jupiter output amount".into())
        };
        let before = read_balance("preTokenBalances", true)?;
        let after = read_balance("postTokenBalances", false)?;
        if after.checked_sub(before).is_none_or(|net| net < minimum) {
            return Err("Jupiter chain evidence does not prove minimum delivery".into());
        }
    } else {
        let recipient = key(&operation.sender)?;
        let index = keys
            .iter()
            .position(|k| *k == recipient)
            .ok_or("Jupiter recipient missing")?;
        let before = proof["meta"]["preBalances"][index]
            .as_u64()
            .ok_or("Native pre-balance missing")?;
        let after = proof["meta"]["postBalances"][index]
            .as_u64()
            .ok_or("Native post-balance missing")?;
        let fee = proof["meta"]["fee"].as_u64().ok_or("Native fee missing")?;
        if u128::from(after)
            .checked_add(u128::from(fee))
            .and_then(|v| v.checked_sub(u128::from(before)))
            .is_none_or(|v| v < minimum)
        {
            return Err("Jupiter native minimum delivery is not proved".into());
        }
    }
    Ok(())
}
