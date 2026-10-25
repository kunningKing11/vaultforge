//! Tauri command dispatch for direct and routed swaps.
use std::sync::Mutex;
use tauri::State;

use crate::dto::WalletSession;
use crate::providers::http::ProviderClients;
use crate::providers::uniswap::{DEFAULT_SLIPPAGE_BPS, LiveSwapProvider};
use crate::state::AppState;
use crate::uniswap::{
    UniswapComparison, UniswapOperationResult, UniswapPreparedStep, UniswapState,
};

#[tauri::command]
pub(crate) fn get_uniswap_state(state: State<'_, Mutex<AppState>>) -> Result<UniswapState, String> {
    crate::uniswap::get_state(state.inner())
}

#[tauri::command(rename_all = "camelCase")]
pub(crate) async fn quote_uniswap_swap(
    state: State<'_, Mutex<AppState>>,
    clients: State<'_, ProviderClients>,
    network: String,
    destination_network: Option<String>,
    token_in: String,
    token_out: String,
    amount_base_units: String,
    slippage_bps: Option<u16>,
    max_amount: Option<bool>,
) -> Result<UniswapComparison, String> {
    crate::uniswap::compare_quotes(
        state.inner(),
        clients.http(),
        &network,
        destination_network.as_deref().unwrap_or(&network),
        &token_in,
        &token_out,
        &amount_base_units,
        slippage_bps.unwrap_or(DEFAULT_SLIPPAGE_BPS),
        max_amount.unwrap_or(false),
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub(crate) fn create_uniswap_swap(
    state: State<'_, Mutex<AppState>>,
    quote_id: String,
) -> Result<UniswapOperationResult, String> {
    crate::uniswap::create_swap_operation(state.inner(), &quote_id)
}

#[tauri::command(rename_all = "camelCase")]
pub(crate) async fn prepare_uniswap_swap(
    state: State<'_, Mutex<AppState>>,
    clients: State<'_, ProviderClients>,
    operation_id: String,
) -> Result<UniswapPreparedStep, String> {
    let (jupiter, native) = {
        let guard = state.lock().map_err(|_| "State lock failed")?;
        let operation = guard.wallet.as_ref().and_then(|wallet| {
            wallet
                .operations
                .iter()
                .find(|operation| operation.id == operation_id)
        });
        (
            operation.is_some_and(crate::jupiter::is_jupiter),
            operation.is_some_and(crate::swap_routes::is_solana),
        )
    };
    if jupiter {
        return crate::jupiter::prepare(state.inner(), clients.http(), &operation_id).await;
    }
    if native {
        return crate::swap_solana::prepare(state.inner(), clients.http(), &operation_id).await;
    }
    crate::uniswap::prepare_swap(
        state.inner(),
        &LiveSwapProvider {
            client: clients.http(),
        },
        &operation_id,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub(crate) async fn submit_uniswap_swap(
    state: State<'_, Mutex<AppState>>,
    clients: State<'_, ProviderClients>,
    operation_id: String,
    expected_hash: String,
) -> Result<WalletSession, String> {
    let (jupiter, native) = {
        let guard = state.lock().map_err(|_| "State lock failed")?;
        let operation = guard.wallet.as_ref().and_then(|wallet| {
            wallet
                .operations
                .iter()
                .find(|operation| operation.id == operation_id)
        });
        (
            operation.is_some_and(crate::jupiter::is_jupiter),
            operation.is_some_and(crate::swap_routes::is_solana),
        )
    };
    if jupiter {
        return crate::jupiter::submit(
            state.inner(),
            clients.http(),
            &operation_id,
            &expected_hash,
        )
        .await;
    }
    if native {
        return crate::swap_solana::submit(
            state.inner(),
            clients.http(),
            &operation_id,
            &expected_hash,
        )
        .await;
    }
    crate::uniswap::submit_swap(
        state.inner(),
        &LiveSwapProvider {
            client: clients.http(),
        },
        &operation_id,
        &expected_hash,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub(crate) async fn sync_uniswap_swap(
    state: State<'_, Mutex<AppState>>,
    clients: State<'_, ProviderClients>,
    operation_id: String,
) -> Result<WalletSession, String> {
    let (jupiter, native) = {
        let guard = state.lock().map_err(|_| "State lock failed")?;
        let operation = guard.wallet.as_ref().and_then(|wallet| {
            wallet
                .operations
                .iter()
                .find(|operation| operation.id == operation_id)
        });
        (
            operation.is_some_and(crate::jupiter::is_jupiter),
            operation.is_some_and(crate::swap_routes::is_solana),
        )
    };
    if jupiter {
        return crate::jupiter::sync(state.inner(), clients.http(), &operation_id).await;
    }
    if native {
        return crate::swap_solana::sync(state.inner(), clients.http(), &operation_id).await;
    }
    crate::uniswap::sync_swap(
        state.inner(),
        &LiveSwapProvider {
            client: clients.http(),
        },
        &operation_id,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
pub(crate) fn cancel_uniswap_swap(
    state: State<'_, Mutex<AppState>>,
    operation_id: String,
) -> Result<WalletSession, String> {
    crate::uniswap::cancel(state.inner(), &operation_id)
}
