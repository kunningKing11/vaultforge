use super::{parse_token_metadata, validate_exchange_rate};

#[test]
fn parses_discovered_token_metadata_without_float_amounts() {
    let metadata = parse_token_metadata(&serde_json::json!({
        "data": {
            "attributes": {
                "name": "USD Coin",
                "symbol": "USDC",
                "decimals": 6,
                "price_usd": "0.9998"
            }
        }
    }))
    .unwrap();

    assert_eq!(metadata.name, "USD Coin");
    assert_eq!(metadata.symbol, "USDC");
    assert_eq!(metadata.decimals, Some(6));
    assert_eq!(metadata.price_usd, Some(0.9998));
}

#[test]
fn exchange_rates_must_be_finite_and_positive() {
    assert_eq!(validate_exchange_rate(0.92).unwrap(), 0.92);
    assert!(validate_exchange_rate(0.0).is_err());
    assert!(validate_exchange_rate(-0.92).is_err());
    assert!(validate_exchange_rate(f64::NAN).is_err());
    assert!(validate_exchange_rate(f64::INFINITY).is_err());
}
