use super::{
    exchange_rate_or_cached, preserve_cached_market_data, update_simple_price,
    update_token_metadata,
};
use crate::dto::{Asset, RefreshWarningKind};
use crate::providers::prices::CoinGeckoPriceResponse;

fn asset(price_usd: f64, change_24h: f64, token_address: Option<&str>) -> Asset {
    Asset {
        symbol: "USDC".to_string(),
        unicode_symbol: None,
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
fn does_not_reuse_prices_from_another_same_symbol_asset() {
    let cached = vec![asset(99.0, 0.25, Some("0x1234"))];
    let mut refreshed = vec![asset(0.0, 0.0, Some("0x5678")), asset(0.0, 0.0, None)];
    preserve_cached_market_data(&mut refreshed, &cached);
    assert!(refreshed.iter().all(|asset| asset.price_usd == 0.0));

    let mut other_network = asset(0.0, 0.0, Some("0x1234"));
    other_network.network = "base".into();
    preserve_cached_market_data(std::slice::from_mut(&mut other_network), &cached);
    assert_eq!(other_network.price_usd, 0.0);
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
fn matches_cached_market_data_by_identity_not_symbol_or_case_on_evm() {
    let mut refreshed = vec![asset(0.0, 0.0, Some("0xABCD"))];
    refreshed[0].symbol = "renamed".into();
    preserve_cached_market_data(&mut refreshed, &[asset(1.0, 0.25, Some("0xabcd"))]);
    assert_eq!(refreshed[0].price_usd, 1.0);
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
fn never_case_folds_solana_price_cache_identifiers() {
    let mut refreshed = vec![asset(0.0, 0.0, Some("So111"))];
    refreshed[0].network = "solana".into();
    let mut cached = vec![asset(99.0, 0.25, Some("so111"))];
    cached[0].network = "solana".into();
    preserve_cached_market_data(&mut refreshed, &cached);
    assert_eq!(refreshed[0].price_usd, 0.0);
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
fn receipt_market_prices_preserve_curated_names_and_chain_decimals() {
    let mut receipt = asset(
        1.02,
        0.0,
        Some("7GxATsNMnaC88vdwd2t3mwrFuQwwGvmYPrUQ4D6FotXk"),
    );
    receipt.network = "solana".into();
    receipt.symbol = "JUICED".into();
    receipt.name = "Jupiter Lend JupUSD".into();
    let metadata = crate::providers::prices::TokenMetadata {
        symbol: "JupUSD".into(),
        name: "Unreviewed market label".into(),
        decimals: Some(9),
        price_usd: Some(1.04),
    };
    update_token_metadata(&mut receipt, &metadata);
    assert_eq!(receipt.symbol, "JUICED");
    assert_eq!(receipt.name, "Jupiter Lend JupUSD");
    assert_eq!(receipt.decimals, 6);
    assert_eq!(receipt.price_usd, 1.04);

    let mut unknown = asset(0.0, 0.0, Some("unknown"));
    update_token_metadata(&mut unknown, &metadata);
    assert_eq!(unknown.symbol, metadata.symbol);
    assert_eq!(unknown.name, metadata.name);
    assert_eq!(unknown.decimals, 6);
    assert_eq!(unknown.price_usd, 1.04);
}

#[test]
fn successful_exchange_rate_refresh_replaces_the_cached_rate() {
    let mut warnings = vec![];

    let rate = exchange_rate_or_cached(Ok(0.92), 0.88, &mut warnings);

    assert_eq!(rate, 0.92);
    assert!(warnings.is_empty());
}
