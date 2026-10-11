use serde_json::json;

use super::{parse_xrpl_account_info, parse_xrpl_network_info};
use crate::tx::tx_status::{Execution, Finality, legacy_tx_status};

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
fn parses_validated_xrpl_observations_as_final_with_ledger_evidence() {
    for (code, execution, legacy) in [
        ("tesSUCCESS", Execution::Succeeded, "confirmed"),
        ("tecUNFUNDED_PAYMENT", Execution::Failed, "failed"),
    ] {
        let json = json!({"result":{"validated":true,"ledger_index":42,"ledger_hash":"AB".repeat(32),"meta":{"TransactionResult":code}}});
        let observation = super::parse_xrpl_tx_observation(&json).unwrap().unwrap();
        assert_eq!(observation.execution, execution);
        assert_eq!(observation.finality, Finality::Finalized);
        assert_eq!(observation.block_height, Some(42));
        assert_eq!(
            observation.block_hash.as_deref(),
            Some("AB".repeat(32).as_str())
        );
        assert_eq!(observation.confirmations, None);
        assert_eq!(legacy_tx_status(Some(observation)).as_deref(), Some(legacy));
    }
}

#[test]
fn rejects_invalid_xrpl_observation_metadata_and_lookup_errors() {
    for json in [
        json!({"result":{"error":"noPermission"}}),
        json!({"result":{"validated":"true"}}),
        json!({"result":{"validated":true,"meta":{}}}),
        json!({"result":{"validated":true,"meta":{"TransactionResult":"terQUEUED"}}}),
        json!({"result":{"validated":true,"ledger_index":-1,"meta":{"TransactionResult":"tesSUCCESS"}}}),
        json!({"result":{"validated":true,"ledger_hash":"bad","meta":{"TransactionResult":"tesSUCCESS"}}}),
    ] {
        assert!(super::parse_xrpl_tx_observation(&json).is_err());
    }
}

#[test]
fn returns_no_xrpl_observation_before_validation_or_when_not_found() {
    for json in [
        json!({"result":{"validated":false}}),
        json!({"result":{"error":"txnNotFound"}}),
        json!({"result":{}}),
    ] {
        assert!(super::parse_xrpl_tx_observation(&json).unwrap().is_none());
    }
    let observation = super::parse_xrpl_tx_observation(
        &json!({"result":{"validated":true,"meta":{"TransactionResult":"tesSUCCESS"}}}),
    )
    .unwrap()
    .unwrap();
    assert_eq!(observation.block_height, None);
    assert_eq!(observation.block_hash, None);
}
