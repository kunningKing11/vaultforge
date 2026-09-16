use serde_json::json;

use super::{parse_xrpl_account_info, parse_xrpl_network_info};

#[test]
fn parses_funded_and_unfunded_accounts_without_float_conversion() {
    let funded = parse_xrpl_account_info(&json!({
        "result": {
            "account_data": { "Balance": "1234567", "OwnerCount": 2, "Sequence": 9 }
        }
    }))
    .unwrap()
    .unwrap();
    assert_eq!(funded.balance_drops, 1_234_567);
    assert_eq!(funded.owner_count, 2);
    assert_eq!(funded.sequence, 9);

    assert!(
        parse_xrpl_account_info(&json!({
            "result": { "error": "actNotFound", "error_message": "Account not found." }
        }))
        .unwrap()
        .is_none()
    );

    assert_eq!(
        parse_xrpl_account_info(&json!({
            "error": { "code": -32000, "message": "server unavailable" }
        }))
        .unwrap_err(),
        "XRP Ledger account_info RPC error: {\"code\":-32000,\"message\":\"server unavailable\"}"
    );
}

#[test]
fn parses_fee_and_reserve_values_as_drops() {
    let info = parse_xrpl_network_info(
        &json!({ "result": { "drops": { "open_ledger_fee": "12" } } }),
        &json!({
            "result": {
                "info": {
                    "validated_ledger": { "reserve_base_xrp": "1", "reserve_inc_xrp": 0.2, "seq": 95_000_000 }
                }
            }
        }),
    )
    .unwrap();
    assert_eq!(info.base_reserve_drops, 1_000_000);
    assert_eq!(info.owner_reserve_drops, 200_000);
    assert_eq!(info.open_ledger_fee_drops, 12);
    assert_eq!(info.validated_ledger_index, 95_000_000);
}
