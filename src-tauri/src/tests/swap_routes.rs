use super::*;
use crate::dto::Asset;
use serde_json::json;

fn selected(network: &str, token: &str) -> Asset {
    Asset {
        network: network.into(),
        token_address: Some(token.into()),
        symbol: "TOKEN".into(),
        name: "Token".into(),
        unicode_symbol: None,
        decimals: 6,
        balance: "1000000".into(),
        price_usd: 0.,
        change_24h: 0.,
    }
}

#[test]
fn lifi_response_binds_network_wallet_assets_amount_decimals_and_slippage() {
    let network = crate::registry::network_by_id("base").unwrap();
    let sender = "0xabcdefabcdefabcdefabcdefabcdefabcdefabcd";
    let from = selected("base", "0x1111111111111111111111111111111111111111");
    let to = selected("base", "0x2222222222222222222222222222222222222222");
    let body = json!({"action":{
        "fromChainId":8453,"toChainId":8453,
        "fromAddress":sender.to_ascii_uppercase(),"toAddress":sender,
        "fromToken":{"address":from.token_address,"decimals":6},
        "toToken":{"address":to.token_address,"decimals":6},"fromAmount":"1000000"
    },"estimate":{"fromAmount":"1000000","toAmount":"2000000","toAmountMin":"1990000"}});
    assert_eq!(
        validate_lifi_response(&body, network, sender, &from, &to, 1000000, 50).unwrap(),
        (2000000, 1990000)
    );
    for (pointer, value) in [
        ("/action/fromChainId", json!(1)),
        ("/action/toChainId", json!(1)),
        ("/action/fromAddress", json!(from.token_address)),
        ("/action/toAddress", json!(from.token_address)),
        ("/action/fromToken/address", json!(to.token_address)),
        ("/action/toToken/address", json!(from.token_address)),
        ("/action/fromToken/decimals", json!(18)),
        ("/action/toToken/decimals", json!(18)),
        ("/action/fromAmount", json!("999999")),
        ("/estimate/fromAmount", json!("999999")),
        ("/estimate/toAmount", json!("not an amount")),
        ("/estimate/toAmountMin", json!("1989999")),
        ("/estimate/toAmountMin", json!("2000001")),
        ("/estimate/toAmountMin", json!("0")),
    ] {
        let mut changed = body.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert!(
            validate_lifi_response(&changed, network, sender, &from, &to, 1000000, 50).is_err(),
            "{pointer}"
        );
    }
}

#[test]
fn solana_response_requires_exact_wallet_and_mint_case() {
    let network = crate::registry::network_by_id("solana").unwrap();
    let from = selected("solana", "CaseSensitiveMint");
    let to = selected("solana", "OtherMint");
    let body = json!({"action":{
        "fromChainId":1151111081099710u64,"toChainId":1151111081099710u64,
        "fromAddress":"CaseSensitiveWallet","toAddress":"CaseSensitiveWallet",
        "fromToken":{"address":from.token_address,"decimals":6},
        "toToken":{"address":to.token_address,"decimals":6},"fromAmount":"1000000"
    },"estimate":{"fromAmount":"1000000","toAmount":"2000000","toAmountMin":"1990000"}});
    assert!(
        validate_lifi_response(
            &body,
            network,
            "CaseSensitiveWallet",
            &from,
            &to,
            1000000,
            50
        )
        .is_ok()
    );
    for pointer in [
        "/action/fromAddress",
        "/action/toAddress",
        "/action/fromToken/address",
        "/action/toToken/address",
    ] {
        let mut changed = body.clone();
        let field = changed.pointer_mut(pointer).unwrap();
        *field = json!(field.as_str().unwrap().to_ascii_lowercase());
        assert!(
            validate_lifi_response(
                &changed,
                network,
                "CaseSensitiveWallet",
                &from,
                &to,
                1000000,
                50
            )
            .is_err()
        );
    }
}
