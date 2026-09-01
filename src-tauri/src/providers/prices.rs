use serde::Deserialize;
use std::collections::HashMap;

use crate::dto::{Asset, FiatCurrency};
use crate::registry::{configured_asset, network_by_id};

#[derive(serde::Deserialize)]
struct ExchangeRateResponse {
    rate: f64,
}

pub(crate) async fn fetch_usd_exchange_rate(
    client: &reqwest::Client,
    currency: FiatCurrency,
) -> Result<f64, String> {
    if currency == FiatCurrency::Usd {
        return Ok(1.0);
    }

    let url = format!(
        "https://api.frankfurter.dev/v2/rate/USD/{}",
        currency.as_str()
    );

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("failed to fetch exchange rate: {error}"))?
        .error_for_status()
        .map_err(|error| format!("exchange rate API returned an error: {error}"))?
        .json::<ExchangeRateResponse>()
        .await
        .map_err(|error| format!("invalid exchange rate response: {error}"))?;

    validate_exchange_rate(response.rate)
}

fn validate_exchange_rate(rate: f64) -> Result<f64, String> {
    if rate.is_finite() && rate > 0.0 {
        Ok(rate)
    } else {
        Err("exchange rate response contains an invalid rate".to_string())
    }
}

#[derive(Deserialize)]
pub(crate) struct CoinGeckoPrice {
    pub(crate) usd: f64,
    pub(crate) usd_24h_change: Option<f64>,
}

pub(crate) type CoinGeckoPriceResponse = HashMap<String, CoinGeckoPrice>;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TokenMetadata {
    pub(crate) symbol: String,
    pub(crate) name: String,
    pub(crate) decimals: Option<u32>,
    pub(crate) price_usd: Option<f64>,
}

pub(crate) fn price_id_for_asset(asset: &Asset) -> Option<&'static str> {
    let network = network_by_id(&asset.network)?;
    configured_asset(network, asset.token_address.as_deref())
        .map(|configured| configured.coin_gecko_id.as_str())
}

pub(crate) async fn fetch_market_prices(
    client: &reqwest::Client,
    ids: &[&str],
) -> Result<CoinGeckoPriceResponse, String> {
    let ids = ids.join(",");
    let url = format!(
        "https://api.coingecko.com/api/v3/simple/price?ids={ids}&vs_currencies=usd&include_24hr_change=true"
    );
    let response = client
        .get(url)
        .header("accept", "application/json")
        .header("user-agent", "VaultForge Wallet/0.1.0")
        .send()
        .await
        .map_err(|_| "Failed to reach price service")?;

    if !response.status().is_success() {
        return Err(format!("Price service returned HTTP {}", response.status()));
    }

    response
        .json::<CoinGeckoPriceResponse>()
        .await
        .map_err(|_| "Price service returned invalid data".to_string())
}

pub(crate) async fn fetch_token_metadata(
    client: &reqwest::Client,
    network_id: &str,
    token_address: &str,
) -> Result<TokenMetadata, String> {
    let network =
        network_by_id(network_id).ok_or_else(|| format!("Unknown token network {network_id}"))?;
    let gecko_network = network
        .coin_gecko_network_id
        .as_deref()
        .ok_or_else(|| format!("{} token metadata provider is not configured", network.name))?;
    let url = format!(
        "https://api.geckoterminal.com/api/v2/networks/{gecko_network}/tokens/{token_address}"
    );
    let response = client
        .get(url)
        .header("accept", "application/json")
        .header("user-agent", "VaultForge Wallet/0.1.0")
        .send()
        .await
        .map_err(|_| "Failed to reach token metadata service")?;
    if !response.status().is_success() {
        return Err(format!(
            "Token metadata service returned HTTP {}",
            response.status()
        ));
    }
    let json = response
        .json::<serde_json::Value>()
        .await
        .map_err(|_| "Token metadata service returned invalid data".to_string())?;
    parse_token_metadata(&json)
}

pub(crate) fn parse_token_metadata(json: &serde_json::Value) -> Result<TokenMetadata, String> {
    let attributes = &json["data"]["attributes"];
    let symbol = attributes["symbol"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Token metadata is missing symbol".to_string())?;
    let name = attributes["name"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Token metadata is missing name".to_string())?;
    let decimals = attributes["decimals"]
        .as_u64()
        .and_then(|value| u32::try_from(value).ok());
    let price_usd = attributes["price_usd"]
        .as_str()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0);

    Ok(TokenMetadata {
        symbol: symbol.to_string(),
        name: name.to_string(),
        decimals,
        price_usd,
    })
}

#[cfg(test)]
#[path = "../tests/providers/prices.rs"]
mod tests;
