use super::*;

fn order_request() -> OrderRequest {
    OrderRequest {
        taker: "GJRs4FwHtemZ5ZE9x3FNvJ8TMwitKTh21yxdRPqn7npE".to_string(),
        input_mint: crate::tx::swap_solana::NATIVE.to_string(),
        output_mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(),
        amount_base_units: "1000000".to_string(),
        slippage_bps: 50,
    }
}

fn order_response() -> Value {
    serde_json::json!({
        "inputMint": crate::tx::swap_solana::WSOL,
        "outputMint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
        "inAmount": "1000000",
        "outAmount": "104631",
        "otherAmountThreshold": "104107",
        "slippageBps": 50,
        "taker": "GJRs4FwHtemZ5ZE9x3FNvJ8TMwitKTh21yxdRPqn7npE",
        "transaction": "AQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "transactionVersion": 0,
        "requestId": "request-id",
        "router": "metis",
        "feeBps": 2
    })
}

#[test]
fn input_amount_mismatch_is_rejected() {
    let mut response = order_response();
    response["inAmount"] = serde_json::json!("999999");
    assert_eq!(
        parse_order(response, &order_request(), 1_000_000)
            .err()
            .unwrap(),
        "Jupiter returned a different input amount"
    );
}

#[test]
fn minimum_output_below_slippage_floor_is_rejected() {
    let mut response = order_response();
    response["otherAmountThreshold"] = serde_json::json!("104106");
    assert_eq!(
        parse_order(response, &order_request(), 1_000_000)
            .err()
            .unwrap(),
        "Jupiter minimum output exceeds the requested slippage"
    );
}

#[test]
fn native_sol_normalizes_to_wsol() {
    assert_eq!(
        normalize_mint(crate::tx::swap_solana::NATIVE),
        crate::tx::swap_solana::WSOL
    );
}

#[test]
fn order_response_is_parsed() {
    let order = parse_order(order_response(), &order_request(), 1_000_000).unwrap();
    assert_eq!(order.request_id, "request-id");
    assert_eq!(order.router, "metis");
    assert_eq!(
        order.transaction_base64,
        "AQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    );
    assert_eq!(order.quoted_output_base_units, "104631");
    assert_eq!(order.minimum_output_base_units, "104107");
    assert_eq!(order.raw_response["feeBps"], 2);
}

#[test]
fn relay_native_sol_normalizes_to_wsol() {
    assert_eq!(
        normalize_mint(crate::tx::relay::NATIVE),
        crate::tx::swap_solana::WSOL
    );
}

#[test]
fn spl_mint_is_unchanged() {
    let mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    assert_eq!(normalize_mint(mint), mint);
}

#[test]
fn transaction_missing_is_rejected() {
    let mut response = order_response();
    response["transaction"] = serde_json::json!("");
    assert_eq!(
        parse_order(response, &order_request(), 1_000_000)
            .err()
            .unwrap(),
        "Jupiter returned no executable transaction"
    );
}

#[test]
fn transaction_version_must_be_zero() {
    let mut response = order_response();
    response["transactionVersion"] = serde_json::json!(1);
    assert_eq!(
        parse_order(response, &order_request(), 1_000_000)
            .err()
            .unwrap(),
        "Jupiter returned an unsupported transaction version"
    );
}

#[test]
fn wsol_is_unchanged() {
    assert_eq!(
        normalize_mint(crate::tx::swap_solana::WSOL),
        crate::tx::swap_solana::WSOL
    );
}
