use super::{evm_config_by_id, parse_evm_fee_history};

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
