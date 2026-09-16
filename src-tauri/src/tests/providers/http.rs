use super::json_rpc_result;

#[test]
fn returns_results_and_preserves_json_rpc_errors() {
    let success = serde_json::json!({ "result": { "value": 1 } });
    assert_eq!(json_rpc_result(&success, "Test").unwrap()["value"], 1);

    let error = serde_json::json!({ "error": { "code": -32000, "message": "rate limited" } });
    assert_eq!(
        json_rpc_result(&error, "Test").unwrap_err(),
        "Test RPC error: {\"code\":-32000,\"message\":\"rate limited\"}"
    );

    assert_eq!(
        json_rpc_result(&serde_json::json!({}), "Test").unwrap_err(),
        "Test RPC response is missing result"
    );
}
