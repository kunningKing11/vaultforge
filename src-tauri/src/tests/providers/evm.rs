use super::{evm_config_by_id, parse_evm_balance, parse_evm_fee_history, parse_evm_tx_receipt};
use crate::tx::tx_status::{Execution, Finality, legacy_tx_status};

#[test]
fn finalizes_evm_only_with_matching_canonical_block_and_finalized_head() {
    let receipt = serde_json::json!({"status":"0x1","blockNumber":"0x2a","blockHash":format!("0x{}","ab".repeat(32))});
    let observation = parse_evm_tx_receipt(&receipt).unwrap().unwrap();
    let canonical = serde_json::json!({"number":"0x2a","hash":receipt["blockHash"]});
    for head in [
        serde_json::Value::Null,
        serde_json::json!({"number":"0x29","hash":format!("0x{}","cd".repeat(32))}),
    ] {
        assert_eq!(
            super::finalize_evm_observation(observation.clone(), &canonical, &head)
                .unwrap()
                .unwrap()
                .finality,
            Finality::Included
        );
    }
    for height in ["0x2a", "0x2b"] {
        let finalized = serde_json::json!({"number":height,"hash":receipt["blockHash"]});
        assert_eq!(
            super::finalize_evm_observation(observation.clone(), &canonical, &finalized)
                .unwrap()
                .unwrap()
                .finality,
            Finality::Finalized
        );
    }
    let replaced = serde_json::json!({"number":"0x2a","hash":format!("0x{}","cd".repeat(32))});
    assert!(
        super::finalize_evm_observation(observation.clone(), &replaced, &canonical)
            .unwrap()
            .is_none()
    );
    assert!(
        super::finalize_evm_observation(observation.clone(), &serde_json::Value::Null, &canonical)
            .unwrap()
            .is_none()
    );
    assert!(super::finalize_evm_observation(observation.clone(), &canonical, &replaced).is_err());
    assert!(
        super::finalize_evm_observation(
            observation,
            &canonical,
            &serde_json::json!({"number":"bad"})
        )
        .is_err()
    );
}

#[test]
fn looks_up_evm_network_configs() {
    let ethereum = evm_config_by_id("ethereum").unwrap();
    assert_eq!(ethereum.name, "Ethereum");
    assert_eq!(ethereum.chain_id().unwrap(), 1);
    assert_eq!(ethereum.native_asset.symbol, "ETH");
    assert_eq!(
        ethereum.rpc_url().unwrap(),
        "https://ethereum-rpc.publicnode.com"
    );

    let avalanche = evm_config_by_id("avalanche_c").unwrap();
    assert_eq!(avalanche.chain_id().unwrap(), 43114);
    assert_eq!(avalanche.native_asset.symbol, "AVAX");
}

#[test]
fn parses_evm_fee_history() {
    let json = serde_json::json!({
        "jsonrpc": "2.0",
        "result": {
            "baseFeePerGas": ["0x3b9aca00", "0x4a817c800"],
            "reward": [["0x59682f00"], ["0x77359400"]]
        },
        "id": 1
    });

    let estimate = parse_evm_fee_history(&json).unwrap();
    assert_eq!(estimate.max_priority_fee_per_gas, 2_000_000_000);
    assert_eq!(estimate.max_fee_per_gas, 42_000_000_000);

    let error = serde_json::json!({
        "jsonrpc": "2.0",
        "error": { "code": -32000, "message": "rate limited" },
        "id": 1
    });
    assert_eq!(
        parse_evm_fee_history(&error).err().unwrap(),
        "EVM fee history RPC error: {\"code\":-32000,\"message\":\"rate limited\"}"
    );
}

#[test]
fn parses_evm_receipt_observations_and_preserves_legacy_labels() {
    for (status, execution, legacy) in [
        ("0x1", Execution::Succeeded, "confirmed"),
        ("0x0", Execution::Failed, "failed"),
    ] {
        let observation = parse_evm_tx_receipt(&serde_json::json!({"status":status,"blockNumber":"0x2a","blockHash":format!("0x{}","ab".repeat(32))})).unwrap().unwrap();
        assert_eq!(observation.execution, execution);
        assert_eq!(observation.finality, Finality::Included);
        assert_eq!(observation.block_height, Some(42));
        assert_eq!(observation.confirmations, None);
        assert_eq!(observation.slot, None);
        assert_eq!(legacy_tx_status(Some(observation)).as_deref(), Some(legacy));
    }
    assert!(
        parse_evm_tx_receipt(&serde_json::Value::Null)
            .unwrap()
            .is_none()
    );
}

#[test]
fn parses_live_evm_balance_rpc_responses_as_base_units() {
    let response = serde_json::json!({
        "jsonrpc": "2.0",
        "result": "0xde0b6b3a7640000",
        "id": 1
    });
    assert_eq!(
        parse_evm_balance(&response, "EVM balance").unwrap(),
        1_000_000_000_000_000_000
    );

    let error = serde_json::json!({
        "jsonrpc": "2.0",
        "error": { "code": -32000, "message": "unavailable" },
        "id": 1
    });
    assert!(parse_evm_balance(&error, "EVM token balance").is_err());

    let malformed = serde_json::json!({ "result": "0xnot-hex" });
    assert!(parse_evm_balance(&malformed, "EVM balance").is_err());
}

#[test]
fn rejects_malformed_evm_receipt_status_height_and_hash() {
    let valid = serde_json::json!({"status":"0x1","blockNumber":"0x2a","blockHash":format!("0x{}","ab".repeat(32))});
    for (field, value) in [
        ("status", serde_json::Value::Null),
        ("status", serde_json::json!("0x2")),
        ("blockNumber", serde_json::json!("0x00")),
        ("blockNumber", serde_json::json!("0x10000000000000000")),
        ("blockNumber", serde_json::json!("0x+1")),
        ("blockHash", serde_json::json!("bad")),
    ] {
        let mut receipt = valid.clone();
        receipt[field] = value;
        assert!(parse_evm_tx_receipt(&receipt).is_err());
    }
}
