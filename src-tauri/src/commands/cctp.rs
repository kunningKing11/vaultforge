use std::sync::Mutex;
use tauri::State;

use crate::cctp_attestation::{AttestationProgress, poll_attestation};
use crate::providers::cctp_attestation::CircleAttestationProvider;
use crate::providers::http::ProviderClients;
use crate::state::{AppState, session_from_state};

/// One bounded read-only polling tick; the returned deadline controls retries.
#[tauri::command(rename_all = "camelCase")]
pub(crate) async fn poll_cctp_attestation(
    state: State<'_, Mutex<AppState>>,
    clients: State<'_, ProviderClients>,
    operation_id: String,
) -> Result<AttestationProgress, String> {
    poll_attestation(
        state.inner(),
        &CircleAttestationProvider {
            client: clients.http(),
        },
        &operation_id,
    )
    .await
}

use crate::cctp::{CctpRoute, NATIVE_EXECUTION_ENABLED};
use crate::cctp_execution::{
    CctpOperationResult, CctpPreparedSource, begin_action, create_source_operation,
    prepare_source_step, save_operation, submit_source_step,
};
use crate::dto::{AssetIdentity, WalletSession};
use crate::operations::{OperationKind, OperationStep, cancel_operation, operation_activity};
use crate::providers::cctp_execution::LiveCctpProvider;
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct CctpRouteView {
    source: String,
    destination: String,
    source_usdc: String,
    destination_usdc: String,
    sender: String,
    destination_address: String,
    environment: String,
    executable: bool,
}
#[derive(Serialize)]
pub(crate) struct CctpOperationView {
    id: String,
    source: String,
    destination: String,
    recipient: String,
    amount_base_units: String,
    step: String,
    progress: crate::dto::Activity,
    can_mint: bool,
    last_error: Option<String>,
    needs_sync: bool,
    prepared: Option<CctpPreparedSource>,
    attempt_step: Option<String>,
    attempt_status: Option<String>,
    attempt_error: Option<String>,
    attempt_hash: Option<String>,
}
#[derive(Serialize)]
pub(crate) struct CctpStateView {
    routes: Vec<CctpRouteView>,
    operations: Vec<CctpOperationView>,
}

/// EVM routes execute through the proven mint path. Solana native execution
/// stays off until a funded integration proves the route and restart.
fn route_executable(route: &CctpRoute) -> bool {
    let native = route.source.network.kind == "svm" || route.destination.network.kind == "svm";
    !native || NATIVE_EXECUTION_ENABLED
}

fn require_route_executable(route: &CctpRoute) -> Result<(), String> {
    if route_executable(route) {
        Ok(())
    } else {
        Err("Native Solana CCTP execution is unavailable until a funded integration proves the route".into())
    }
}

#[tauri::command]
pub(crate) fn get_cctp_state(state: State<'_, Mutex<AppState>>) -> Result<CctpStateView, String> {
    let state = state.lock().map_err(|_| "State lock failed")?;
    if state.locked {
        return Err("Wallet is locked".into());
    }
    let wallet = state.wallet.as_ref().ok_or("No wallet exists")?;
    let networks = [
        "ethereum",
        "avalanche_c",
        "optimism",
        "arbitrum_one",
        "base",
        "polygon",
        "monad",
        "solana",
        "ethereum_sepolia",
        "arbitrum_sepolia",
    ];
    let mut routes = Vec::new();
    for source in networks {
        for destination in networks {
            let Ok(route) = CctpRoute::for_networks(source, destination) else {
                continue;
            };
            let Some(sender) = wallet.addresses.get(&route.source.network.address_key) else {
                continue;
            };
            let Some(address) = wallet.addresses.get(&route.destination.network.address_key) else {
                continue;
            };
            routes.push(CctpRouteView {
                source: source.into(),
                destination: destination.into(),
                source_usdc: route.source.usdc.into(),
                destination_usdc: route.destination.usdc.into(),
                sender: sender.clone(),
                destination_address: address.clone(),
                environment: format!("{:?}", route.environment),
                executable: route_executable(&route),
            });
        }
    }
    let operations = wallet
        .operations
        .iter()
        .filter(|o| o.kind == OperationKind::Cctp)
        .map(|o| {
            let attempt = o.attempts.last();
            Ok(CctpOperationView {
                id: o.id.clone(),
                source: o.source_asset.network.clone(),
                destination: o.destination_asset.network.clone(),
                recipient: o.recipient.clone(),
                amount_base_units: o.amount_base_units.clone(),
                step: format!("{:?}", o.step),
                progress: operation_activity(o)?,
                can_mint: crate::operations::attestation_available(o),
                last_error: o.last_error.clone(),
                needs_sync: o.attempts.iter().any(|attempt| {
                    matches!(
                        attempt.status,
                        crate::operations::TransactionStatus::Submitted
                            | crate::operations::TransactionStatus::SubmissionUnknown
                    ) && !attempt.tx_status.as_ref().is_some_and(|status| {
                        status.finality == crate::tx::tx_status::Finality::Finalized
                    })
                }),
                prepared: o
                    .attempts
                    .last()
                    .filter(|a| {
                        a.tx_status.is_none()
                            && matches!(
                                a.status,
                                crate::operations::TransactionStatus::Signed
                                    | crate::operations::TransactionStatus::Submitted
                                    | crate::operations::TransactionStatus::SubmissionUnknown
                            )
                    })
                    .and_then(|a| crate::cctp_execution::prepared_view(o, a).ok()),
                attempt_step: attempt.map(|a| format!("{:?}", a.step)),
                attempt_status: attempt.map(|a| format!("{:?}", a.status)),
                attempt_error: attempt.and_then(|a| a.error.clone()),
                attempt_hash: attempt.map(|a| a.tx_hash.clone()),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(CctpStateView { routes, operations })
}

#[tauri::command(rename_all = "camelCase")]
pub(crate) fn create_cctp_transfer(
    state: State<'_, Mutex<AppState>>,
    source_network: String,
    destination_network: String,
    recipient: String,
    amount_base_units: String,
) -> Result<CctpOperationResult, String> {
    let route = CctpRoute::for_networks(&source_network, &destination_network)?;
    require_route_executable(&route)?;
    create_source_operation(
        state.inner(),
        AssetIdentity::new(&source_network, Some(route.source.usdc)),
        AssetIdentity::new(&destination_network, Some(route.destination.usdc)),
        recipient,
        amount_base_units,
    )
}
#[tauri::command(rename_all = "camelCase")]
pub(crate) async fn prepare_cctp_step(
    state: State<'_, Mutex<AppState>>,
    clients: State<'_, ProviderClients>,
    operation_id: String,
) -> Result<CctpPreparedSource, String> {
    let operation = current_operation(state.inner(), &operation_id)?;
    let transfer = operation.cctp_transfer()?;
    require_route_executable(&transfer.route)?;
    let mint = operation.step == OperationStep::Mint
        || crate::operations::attestation_available(&operation);
    let native = crate::providers::cctp_native::LiveNative {
        client: clients.http(),
    };
    if transfer.route.destination.network.kind == "svm" {
        let setup_done = operation.attempts.iter().any(|attempt| {
            attempt.step == crate::operations::TransactionStep::DestinationSetup
                && attempt.tx_status.as_ref().is_some_and(|status| {
                    status.execution == crate::tx::tx_status::Execution::Succeeded
                        && status.finality == crate::tx::tx_status::Finality::Finalized
                })
        });
        if !setup_done {
            return crate::cctp_native::prepare_native(state.inner(), &native, &operation_id, true)
                .await;
        }
        if mint {
            return crate::cctp_native::prepare_native(
                state.inner(),
                &native,
                &operation_id,
                false,
            )
            .await;
        }
        native
            .lookup(
                &transfer,
                operation
                    .cctp
                    .as_ref()
                    .and_then(|data| data.native_lookup.as_ref())
                    .ok_or("Native lookup setup missing")?,
            )
            .await?;
    }
    if transfer.route.source.network.kind == "svm" && !mint {
        return crate::cctp_native::prepare_native(state.inner(), &native, &operation_id, false)
            .await;
    }
    let provider = LiveCctpProvider {
        client: clients.http(),
    };
    if mint {
        crate::cctp_mint::prepare_mint(state.inner(), &provider, &operation_id).await
    } else {
        prepare_source_step(state.inner(), &provider, &operation_id).await
    }
}

fn current_operation(
    state: &Mutex<AppState>,
    id: &str,
) -> Result<crate::operations::Operation, String> {
    let state = state.lock().map_err(|_| "State lock failed")?;
    if state.locked {
        return Err("Wallet is locked".into());
    }
    state
        .wallet
        .as_ref()
        .ok_or("No wallet exists")?
        .operations
        .iter()
        .find(|operation| operation.id == id)
        .cloned()
        .ok_or("CCTP operation missing".into())
}

#[tauri::command(rename_all = "camelCase")]
pub(crate) async fn submit_cctp_step(
    state: State<'_, Mutex<AppState>>,
    clients: State<'_, ProviderClients>,
    operation_id: String,
    expected_hash: String,
) -> Result<WalletSession, String> {
    let operation = current_operation(state.inner(), &operation_id)?;
    require_route_executable(&operation.cctp_transfer()?.route)?;
    let attempt = operation
        .attempts
        .last()
        .ok_or("Prepare a transaction first")?;
    if attempt.solana_context.is_some() {
        return crate::cctp_native::submit_native(
            state.inner(),
            &crate::providers::cctp_native::LiveNative {
                client: clients.http(),
            },
            &operation_id,
            &expected_hash,
        )
        .await;
    }
    let provider = LiveCctpProvider {
        client: clients.http(),
    };
    if operation.step == OperationStep::Mint {
        crate::cctp_mint::submit_mint(state.inner(), &provider, &operation_id, &expected_hash).await
    } else {
        submit_source_step(state.inner(), &provider, &operation_id, &expected_hash).await
    }
}

/// Abandons a transfer that has not reached the irreversible burn step.
/// Cancellation stays available for gated routes too so no saved transfer is
/// ever stuck. The action guard blocks racing an in-flight submission.
#[tauri::command(rename_all = "camelCase")]
pub(crate) fn cancel_cctp_transfer(
    state: State<'_, Mutex<AppState>>,
    operation_id: String,
) -> Result<WalletSession, String> {
    let (_action, generation, original) = begin_action(state.inner(), &operation_id)?;
    let mut cancelled = original.clone();
    cancel_operation(&mut cancelled)?;
    {
        let mut state = state.lock().map_err(|_| "State lock failed")?;
        save_operation(&mut state, generation, &original, &cancelled)?;
    }
    let state = state.lock().map_err(|_| "State lock failed")?;
    Ok(session_from_state(&state))
}

#[tauri::command(rename_all = "camelCase")]
pub(crate) async fn sync_cctp_transfer(
    state: State<'_, Mutex<AppState>>,
    clients: State<'_, ProviderClients>,
    operation_id: String,
) -> Result<WalletSession, String> {
    let operation = current_operation(state.inner(), &operation_id)?;
    require_route_executable(&operation.cctp_transfer()?.route)?;
    if operation.cctp_transfer()?.route.destination.network.kind == "svm" {
        crate::cctp_native::sync_native(
            state.inner(),
            &crate::providers::cctp_native::LiveNative {
                client: clients.http(),
            },
            &operation_id,
        )
        .await
    } else {
        crate::cctp_mint::sync_cctp(
            state.inner(),
            &LiveCctpProvider {
                client: clients.http(),
            },
            &operation_id,
        )
        .await
    }
}

#[tauri::command(rename_all = "camelCase")]
pub(crate) async fn refresh_cctp_attestation(
    state: State<'_, Mutex<AppState>>,
    clients: State<'_, ProviderClients>,
    operation_id: String,
) -> Result<AttestationProgress, String> {
    crate::cctp_attestation::refresh_attestation(
        state.inner(),
        &CircleAttestationProvider {
            client: clients.http(),
        },
        &operation_id,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{require_route_executable, route_executable};
    use crate::cctp::{CctpRoute, NATIVE_EXECUTION_ENABLED};

    #[test]
    fn native_routes_stay_gated_until_proven() {
        let evm = CctpRoute::for_networks("ethereum", "arbitrum_one").unwrap();
        assert!(route_executable(&evm));
        assert!(require_route_executable(&evm).is_ok());
        let native = CctpRoute::for_networks("ethereum", "solana").unwrap();
        assert_eq!(route_executable(&native), NATIVE_EXECUTION_ENABLED);
        assert!(require_route_executable(&native).is_err());
    }
}
