use super::{parse_token_metadata, validate_exchange_rate};

#[test]
fn exchange_rates_must_be_finite_and_positive() {
    assert_eq!(validate_exchange_rate(0.92).unwrap(), 0.92);
    assert!(validate_exchange_rate(0.0).is_err());
    assert!(validate_exchange_rate(-0.92).is_err());
    assert!(validate_exchange_rate(f64::NAN).is_err());
    assert!(validate_exchange_rate(f64::INFINITY).is_err());
}

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
fn receipt_tokens_do_not_inherit_the_underlying_price_id() {
    let mut asset = crate::dto::Asset {
        network: "solana".into(),
        token_address: Some("JuprjznTrTSp2UFa3ZBUFgwdAmtZCq4MQCwysN55USD".into()),
        symbol: "JupUSD".into(),
        unicode_symbol: None,
        name: "Jupiter USD".into(),
        balance: "1000000".into(),
        decimals: 6,
        price_usd: 0.0,
        change_24h: 0.0,
    };
    assert_eq!(super::price_id_for_asset(&asset), Some("jupusd"));
    asset.token_address = Some("7GxATsNMnaC88vdwd2t3mwrFuQwwGvmYPrUQ4D6FotXk".into());
    assert_eq!(super::price_id_for_asset(&asset), None);
    asset.token_address = Some("juprjznTrTSp2UFa3ZBUFgwdAmtZCq4MQCwysN55USD".into());
    assert_eq!(super::price_id_for_asset(&asset), None);
}
