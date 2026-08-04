use std::collections::HashMap;
use std::sync::Mutex;
use tauri::State;

use crate::dto::{Asset, WalletSession};
use crate::providers::prices::{
    CoinGeckoPriceResponse, TokenMetadata, fetch_market_prices, fetch_token_metadata,
    price_id_for_asset,
};
use crate::state::{AppState, session_from_state};
use crate::storage::persist_state_wallet;

#[tauri::command]
pub(crate) async fn refresh_prices(
    state: State<'_, Mutex<AppState>>,
) -> Result<WalletSession, String> {
    let mut assets = {
        let state = state.lock().map_err(|_| "State lock failed")?;
        if state.locked {
            return Err("Wallet is locked".to_string());
        }
        state
            .wallet
            .as_ref()
            .ok_or_else(|| "No wallet exists yet".to_string())?
            .assets
            .clone()
    };

    refresh_asset_prices(&mut assets).await?;

    let mut state = state.lock().map_err(|_| "State lock failed")?;
    let wallet = state
        .wallet
        .as_mut()
        .ok_or_else(|| "No wallet exists yet".to_string())?;
    wallet.assets = assets;
    persist_state_wallet(&mut state)?;
    Ok(session_from_state(&state))
}

pub(crate) async fn refresh_asset_prices(assets: &mut [Asset]) -> Result<(), String> {
    let price_ids = assets.iter().filter_map(price_id_for_asset).fold(
        Vec::<&'static str>::new(),
        |mut ids, id| {
            if !ids.contains(&id) {
                ids.push(id);
            }
            ids
        },
    );
    let prices = if price_ids.is_empty() {
        CoinGeckoPriceResponse::new()
    } else {
        fetch_market_prices(&price_ids).await?
    };

    let mut dynamic_tokens = HashMap::<(String, String), TokenMetadata>::new();
    for asset in assets.iter() {
        if price_id_for_asset(asset).is_some() {
            continue;
        }
        let Some(token_address) = asset.token_address.as_deref() else {
            continue;
        };
        if let Ok(metadata) = fetch_token_metadata(&asset.network, token_address).await {
            dynamic_tokens.insert(
                (asset.network.clone(), token_address.to_ascii_lowercase()),
                metadata,
            );
        }
    }

    for asset in assets {
        if let Some(price_id) = price_id_for_asset(asset) {
            update_simple_price(asset, &prices, price_id);
            continue;
        }
        let Some(token_address) = asset.token_address.as_deref() else {
            continue;
        };
        if let Some(metadata) =
            dynamic_tokens.get(&(asset.network.clone(), token_address.to_ascii_lowercase()))
        {
            asset.symbol.clone_from(&metadata.symbol);
            asset.name.clone_from(&metadata.name);
            if let Some(price) = metadata.price_usd {
                asset.price_usd = price;
            }
        }
    }

    Ok(())
}

fn update_simple_price(
    asset: &mut crate::dto::Asset,
    prices: &CoinGeckoPriceResponse,
    price_id: &str,
) {
    let Some(price) = prices.get(price_id) else {
        return;
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
