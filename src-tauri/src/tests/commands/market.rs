use super::{exchange_rate_or_cached, preserve_cached_market_data, update_simple_price};
use crate::dto::{Asset, RefreshWarningKind};
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

#[test]
fn failed_exchange_rate_refresh_keeps_the_cached_rate() {
    let mut warnings = vec![];

    let rate =
        exchange_rate_or_cached(Err("provider unavailable".to_string()), 0.92, &mut warnings);

    assert_eq!(rate, 0.92);
    assert_eq!(warnings.len(), 1);
    assert!(matches!(warnings[0].kind, RefreshWarningKind::Value));
    assert_eq!(warnings[0].subject, "Exchange rate");
}

#[test]
fn successful_exchange_rate_refresh_replaces_the_cached_rate() {
    let mut warnings = vec![];

    let rate = exchange_rate_or_cached(Ok(0.92), 0.88, &mut warnings);

    assert_eq!(rate, 0.92);
    assert!(warnings.is_empty());
}
