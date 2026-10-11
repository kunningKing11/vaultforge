use super::parse_tron_tx_observation;
use crate::tx::tx_status::{Execution, Finality, legacy_tx_status};
use serde_json::json;

#[test]
fn finalizes_tron_only_with_matching_solidified_execution() {
    let id = "ab".repeat(32);
    let json = json!({"id":id,"blockNumber":42,"blockTimeStamp":123,"receipt":{}});
    let observation = parse_tron_tx_observation(&json).unwrap().unwrap();
    assert_eq!(observation.execution, Execution::Succeeded);
    assert_eq!(
        super::finalize_tron_observation(observation.clone(), &json, &id)
            .unwrap()
            .unwrap()
            .finality,
        Finality::Finalized
    );
    assert_eq!(
        super::finalize_tron_observation(observation.clone(), &json!({}), &id)
            .unwrap()
            .unwrap()
            .finality,
        Finality::Included
    );
    assert!(
        super::finalize_tron_observation(
            observation.clone(),
            &json!({"id":"cd".repeat(32),"blockNumber":42,"result":"SUCESS"}),
            &id
        )
        .is_err()
    );
    assert!(
        super::finalize_tron_observation(
            observation.clone(),
            &json!({"id":id,"result":"SUCESS"}),
            &id
        )
        .is_err()
    );
    for changed in [
        json!({"id":id,"blockNumber":43,"result":"SUCESS"}),
        json!({"id":id,"blockNumber":42,"result":"FAILED"}),
    ] {
        let final_observation =
            super::finalize_tron_observation(observation.clone(), &changed, &id)
                .unwrap()
                .unwrap();
        assert_eq!(final_observation.finality, Finality::Finalized);
        assert_eq!(
            final_observation.block_height,
            changed["blockNumber"].as_u64()
        );
        assert_eq!(
            final_observation.execution,
            if changed["result"] == "FAILED" {
                Execution::Failed
            } else {
                Execution::Succeeded
            }
        );
    }
}

#[test]
fn parses_tron_execution_without_assuming_solidification() {
    for (result, execution) in [
        ("SUCCESS", Execution::Succeeded),
        ("REVERT", Execution::Failed),
        ("OUT_OF_ENERGY", Execution::Failed),
    ] {
        let observation =
            parse_tron_tx_observation(&json!({"receipt":{"result":result},"blockNumber":42}))
                .unwrap()
                .unwrap();
        assert_eq!(observation.execution, execution);
        assert_eq!(observation.finality, Finality::Included);
        assert_eq!(observation.block_height, Some(42));
        assert_eq!(observation.block_hash, None);
        assert_eq!(observation.confirmations, None);
        assert_eq!(
            legacy_tx_status(Some(observation)).as_deref(),
            Some(if result == "SUCCESS" {
                "confirmed"
            } else {
                "failed"
            })
        );
    }
}

#[test]
fn parses_tron_explicit_native_transaction_results_without_a_contract_receipt() {
    for (result, execution) in [
        ("SUCESS", Execution::Succeeded),
        ("FAILED", Execution::Failed),
    ] {
        let observation = parse_tron_tx_observation(&json!({"result":result,"blockNumber":42}))
            .unwrap()
            .unwrap();
        assert_eq!(observation.execution, execution);
        assert_eq!(observation.finality, Finality::Included);
    }
    let failed = parse_tron_tx_observation(
        &json!({"result":"FAILED","receipt":{"result":"DEFAULT"},"blockNumber":42}),
    )
    .unwrap()
    .unwrap();
    assert_eq!(failed.execution, Execution::Failed);
}

#[test]
fn rejects_malformed_tron_status_and_metadata() {
    for value in [
        json!(null),
        json!({"Error":"lookup failed"}),
        json!({"receipt":{"result":123}}),
        json!({"receipt":{"result":"invalid"}}),
        json!({"receipt":{"result":"SUCCESS"},"blockNumber":-1}),
    ] {
        assert!(parse_tron_tx_observation(&value).is_err());
    }
}

#[test]
fn returns_no_tron_observation_without_execution_evidence() {
    for value in [
        json!({}),
        json!({"receipt":{}}),
        json!({"receipt":{"result":"DEFAULT"}}),
    ] {
        assert!(parse_tron_tx_observation(&value).unwrap().is_none());
    }
}
