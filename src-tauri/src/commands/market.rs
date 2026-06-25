use std::sync::Mutex;
use tauri::State;

use crate::dto::WalletSession;
use crate::providers::prices::{fetch_market_prices, price_id_for_symbol};
use crate::state::{AppState, session_from_state};
use crate::storage::persist_state_wallet;

#[tauri::command]
pub(crate) async fn refresh_prices(
    state: State<'_, Mutex<AppState>>,
) -> Result<WalletSession, String> {
    let price_ids = {
        let state = state.lock().map_err(|_| "State lock failed")?;
        if state.locked {
            return Err("Wallet is locked".to_string());
        }
        let wallet = state
            .wallet
            .as_ref()
            .ok_or_else(|| "No wallet exists yet".to_string())?;
        wallet
            .assets
            .iter()
            .filter_map(|asset| price_id_for_symbol(&asset.symbol))
            .fold(Vec::<&'static str>::new(), |mut ids, id| {
                if !ids.contains(&id) {
                    ids.push(id);
                }
                ids
            })
    };

    if price_ids.is_empty() {
        let state = state.lock().map_err(|_| "State lock failed")?;
        return Ok(session_from_state(&state));
    }

    let prices = fetch_market_prices(&price_ids).await?;

    let mut state = state.lock().map_err(|_| "State lock failed")?;
    {
        let wallet = state
            .wallet
            .as_mut()
            .ok_or_else(|| "No wallet exists yet".to_string())?;
        for asset in &mut wallet.assets {
            let Some(price_id) = price_id_for_symbol(&asset.symbol) else {
                continue;
            };
            let Some(price) = prices.get(price_id) else {
                continue;
            };
            if price.usd.is_finite() && price.usd > 0.0 {
                asset.price_usd = price.usd;
            }
            if let Some(change) = price.usd_24h_change
                && change.is_finite()
            {
                asset.change_24h = change;
            }
        }
    }
    persist_state_wallet(&mut state)?;
    Ok(session_from_state(&state))
}
