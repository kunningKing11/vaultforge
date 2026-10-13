use std::sync::Mutex;
use tauri::State;

use crate::cctp_attestation::{AttestationProgress, poll_attestation};
use crate::providers::cctp_attestation::CircleAttestationProvider;
use crate::providers::http::ProviderClients;
use crate::state::AppState;

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
