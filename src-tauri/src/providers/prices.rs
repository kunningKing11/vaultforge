use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
pub(crate) struct CoinGeckoPrice {
    pub(crate) usd: f64,
    pub(crate) usd_24h_change: Option<f64>,
}

pub(crate) type CoinGeckoPriceResponse = HashMap<String, CoinGeckoPrice>;

pub(crate) fn price_id_for_symbol(symbol: &str) -> Option<&'static str> {
    match symbol {
        "ETH" => Some("ethereum"),
        "BTC" => Some("bitcoin"),
        "SOL" => Some("solana"),
        "USDC" => Some("usd-coin"),
        _ => None,
    }
}

pub(crate) async fn fetch_market_prices(ids: &[&str]) -> Result<CoinGeckoPriceResponse, String> {
    let ids = ids.join(",");
    let url = format!(
        "https://api.coingecko.com/api/v3/simple/price?ids={ids}&vs_currencies=usd&include_24hr_change=true"
    );
    let response = reqwest::Client::new()
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
