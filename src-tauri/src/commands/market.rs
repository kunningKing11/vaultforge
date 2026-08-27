use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;
use tauri::State;

use crate::assets::{cached_asset, cached_asset_by_token_address};
use crate::dto::{Asset, RefreshWarning, RefreshWarningKind, WalletRefreshResult};
use crate::providers::bitcoin::BitcoinAccountSnapshot;
use crate::providers::fetch_portfolio_assets;
use crate::providers::prices::{
    CoinGeckoPriceResponse, TokenMetadata, fetch_market_prices, fetch_token_metadata,
    price_id_for_asset,
};
use crate::state::{AppState, refresh_result_from_state};
use crate::storage::persist_state_wallet;

pub(crate) struct PortfolioRefresh {
    pub(crate) assets: Vec<Asset>,
    pub(crate) warnings: Vec<RefreshWarning>,
    pub(crate) bitcoin_account: Option<BitcoinAccountSnapshot>,
}

#[tauri::command]
pub(crate) async fn refresh_portfolio(
    state: State<'_, Mutex<AppState>>,
) -> Result<WalletRefreshResult, String> {
    let (addresses, cached_assets, enabled_networks, bitcoin_account) = {
        let state = state.lock().map_err(|_| "State lock failed")?;
        if state.locked {
            return Err("Wallet is locked".to_string());
        }
        let wallet = state
            .wallet
            .as_ref()
            .ok_or_else(|| "No wallet exists yet".to_string())?;
        (
            wallet.addresses.clone(),
            wallet.assets.clone(),
            wallet.enabled_networks.clone(),
            state.bitcoin_account.clone(),
        )
    };

    let refreshed = refresh_wallet_portfolio(
        &addresses,
        &cached_assets,
        &enabled_networks,
        bitcoin_account.as_ref(),
    )
    .await;

    let mut state = state.lock().map_err(|_| "State lock failed")?;
    let wallet = state
        .wallet
        .as_mut()
        .ok_or_else(|| "No wallet exists yet".to_string())?;
    wallet.assets = refreshed.assets;
    state.bitcoin_account = refreshed.bitcoin_account;
    persist_state_wallet(&mut state)?;
    Ok(refresh_result_from_state(&state, refreshed.warnings))
}

pub(crate) async fn refresh_wallet_portfolio(
    addresses: &HashMap<String, String>,
    cached_assets: &[Asset],
    enabled_networks: &[String],
    bitcoin_account: Option<&BitcoinAccountSnapshot>,
) -> PortfolioRefresh {
    let refreshed =
        fetch_portfolio_assets(addresses, cached_assets, enabled_networks, bitcoin_account).await;
    let mut assets = refreshed.assets;
    preserve_cached_market_data(&mut assets, cached_assets);
    let mut warnings: Vec<RefreshWarning> = refreshed
        .failed_networks
        .into_iter()
        .map(balance_warning)
        .collect();
    warnings.extend(refresh_asset_prices(&mut assets).await);

    PortfolioRefresh {
        assets,
        warnings,
        bitcoin_account: refreshed.bitcoin_account,
    }
}

pub(crate) async fn refresh_asset_prices(assets: &mut [Asset]) -> Vec<RefreshWarning> {
    let price_ids = assets.iter().filter_map(price_id_for_asset).fold(
        Vec::<&'static str>::new(),
        |mut ids, id| {
            if !ids.contains(&id) {
                ids.push(id);
            }
            ids
        },
    );
    let mut warnings = vec![];
    let mut market_prices_failed = false;
    let prices = if price_ids.is_empty() {
        CoinGeckoPriceResponse::new()
    } else {
        match fetch_market_prices(&price_ids).await {
            Ok(prices) => prices,
            Err(_) => {
                warnings.push(value_warning("Market prices"));
                market_prices_failed = true;
                CoinGeckoPriceResponse::new()
            }
        }
    };

    let mut dynamic_tokens = HashMap::<(String, String), TokenMetadata>::new();
    let mut failed_token_networks = BTreeSet::new();
    for asset in assets.iter() {
        if price_id_for_asset(asset).is_some() {
            continue;
        }
        let Some(token_address) = asset.token_address.as_deref() else {
            continue;
        };
        match fetch_token_metadata(&asset.network, token_address).await {
            Ok(metadata) => {
                if metadata.price_usd.is_none() {
                    failed_token_networks.insert(asset.network.clone());
                }
                dynamic_tokens.insert(
                    (asset.network.clone(), token_address.to_ascii_lowercase()),
                    metadata,
                );
            }
            Err(_) => {
                failed_token_networks.insert(asset.network.clone());
            }
        }
    }

    let mut missing_simple_prices = false;
    for asset in assets {
        if let Some(price_id) = price_id_for_asset(asset) {
            missing_simple_prices |= !update_simple_price(asset, &prices, price_id);
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

    warnings.extend(
        failed_token_networks
            .into_iter()
            .map(|network| value_warning(&format!("{network} token prices"))),
    );
    if missing_simple_prices && !market_prices_failed {
        warnings.push(value_warning("Market prices"));
    }
    warnings
}

fn preserve_cached_market_data(assets: &mut [Asset], cached_assets: &[Asset]) {
    for asset in assets {
        let cached = asset.token_address.as_deref().and_then(|token_address| {
            cached_asset_by_token_address(cached_assets, &asset.network, token_address)
        });
        let cached = cached.or_else(|| cached_asset(cached_assets, &asset.network, &asset.symbol));
        if let Some(cached) = cached {
            asset.price_usd = cached.price_usd;
            asset.change_24h = cached.change_24h;
        }
    }
}

fn balance_warning(subject: String) -> RefreshWarning {
    RefreshWarning {
        kind: RefreshWarningKind::Balance,
        subject,
    }
}

fn value_warning(subject: impl Into<String>) -> RefreshWarning {
    RefreshWarning {
        kind: RefreshWarningKind::Value,
        subject: subject.into(),
    }
}

fn update_simple_price(
    asset: &mut crate::dto::Asset,
    prices: &CoinGeckoPriceResponse,
    price_id: &str,
) -> bool {
    let Some(price) = prices.get(price_id) else {
        return false;
    };
    let mut updated = false;
    if price.usd.is_finite() && price.usd > 0.0 {
        asset.price_usd = price.usd;
        updated = true;
    }
    if let Some(change) = price.usd_24h_change
        && change.is_finite()
    {
        asset.change_24h = change;
    }
    updated
}

#[cfg(test)]
mod tests {
    use super::{preserve_cached_market_data, update_simple_price};
    use crate::dto::Asset;
    use crate::providers::prices::CoinGeckoPriceResponse;

    fn asset(price_usd: f64, change_24h: f64, token_address: Option<&str>) -> Asset {
        Asset {
            symbol: "USDC".to_string(),
            name: "USD Coin".to_string(),
            balance: "1000000".to_string(),
            decimals: 6,
            price_usd,
            change_24h,
            network: "ethereum".to_string(),
            token_address: token_address.map(str::to_string),
        }
    }

    #[test]
    fn preserves_cached_market_data_when_a_price_refresh_fails() {
        let mut refreshed = vec![asset(0.0, 0.0, Some("0x1234"))];
        let cached = vec![asset(1.0, 0.25, Some("0x1234"))];

        preserve_cached_market_data(&mut refreshed, &cached);

        assert_eq!(refreshed[0].price_usd, 1.0);
        assert_eq!(refreshed[0].change_24h, 0.25);
    }

    #[test]
    fn missing_market_quote_keeps_the_cached_value() {
        let mut asset = asset(1.0, 0.25, None);
        asset.symbol = "ETH".to_string();
        let prices = CoinGeckoPriceResponse::new();

        assert!(!update_simple_price(&mut asset, &prices, "ethereum"));
        assert_eq!(asset.price_usd, 1.0);
        assert_eq!(asset.change_24h, 0.25);
    }
}
