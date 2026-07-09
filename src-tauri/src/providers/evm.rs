use crate::assets::cached_asset;
use crate::dto::Asset;
use crate::providers::http::rpc_post;

#[derive(Clone, Copy)]
pub(crate) struct EvmNetworkConfig {
    pub(crate) id: &'static str,
    pub(crate) display_name: &'static str,
    pub(crate) chain_id: u64,
    pub(crate) native_symbol: &'static str,
    pub(crate) rpc_url: &'static str,
}

pub(crate) const DEFAULT_EVM_CONFIG: &EvmNetworkConfig = &EVM_NETWORKS[0];

pub(crate) const EVM_NETWORKS: &[EvmNetworkConfig] = &[
    EvmNetworkConfig {
        id: "ethereum",
        display_name: "Ethereum",
        chain_id: 1,
        native_symbol: "ETH",
        rpc_url: "https://ethereum-rpc.publicnode.com",
    },
    EvmNetworkConfig {
        id: "monad",
        display_name: "Monad",
        chain_id: 167004,
        native_symbol: "MON",
        rpc_url: "https://rpc.monad.xyz",
    },
    EvmNetworkConfig {
        id: "polygon",
        display_name: "Polygon",
        chain_id: 137,
        native_symbol: "MATIC",
        rpc_url: "https://polygon-bor-rpc.publicnode.com",
    },
    EvmNetworkConfig {
        id: "arbitrum_one",
        display_name: "Arbitrum One",
        chain_id: 42161,
        native_symbol: "ETH",
        rpc_url: "https://arbitrum-one-rpc.publicnode.com",
    },
    EvmNetworkConfig {
        id: "base",
        display_name: "Base",
        chain_id: 8453,
        native_symbol: "ETH",
        rpc_url: "https://base-rpc.publicnode.com",
    },
    EvmNetworkConfig {
        id: "optimism",
        display_name: "Optimism",
        chain_id: 10,
        native_symbol: "ETH",
        rpc_url: "https://optimism-rpc.publicnode.com",
    },
    EvmNetworkConfig {
        id: "avalanche_c",
        display_name: "Avalanche C-Chain",
        chain_id: 43114,
        native_symbol: "AVAX",
        rpc_url: "https://avalanche-c-chain-rpc.publicnode.com",
    },
];

#[derive(Clone, Copy)]
pub(crate) struct EvmTokenConfig {
    pub(crate) symbol: &'static str,
    pub(crate) name: &'static str,
    pub(crate) contract: &'static str,
    pub(crate) decimals: u32,
}

const EVM_TOKENS: &[(&str, &[EvmTokenConfig])] = &[
    (
        "ethereum",
        &[
            EvmTokenConfig {
                symbol: "USDC",
                name: "USD Coin",
                contract: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
                decimals: 6,
            },
            EvmTokenConfig {
                symbol: "USDT",
                name: "Tether USD",
                contract: "0xdAC17F958D2ee523a2206206994597C13D831ec7",
                decimals: 6,
            },
        ],
    ),
    (
        "polygon",
        &[
            EvmTokenConfig {
                symbol: "USDC",
                name: "USD Coin",
                contract: "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174",
                decimals: 6,
            },
            EvmTokenConfig {
                symbol: "USDT",
                name: "Tether USD",
                contract: "0xc2132D05D31c914a87C6611C10748AEb04B58e8F",
                decimals: 6,
            },
        ],
    ),
    (
        "arbitrum_one",
        &[EvmTokenConfig {
            symbol: "USDC",
            name: "USD Coin",
            contract: "0xaf88d065e77c8cC2239327C5EDb3A432268e5831",
            decimals: 6,
        }],
    ),
    (
        "base",
        &[EvmTokenConfig {
            symbol: "USDC",
            name: "USD Coin",
            contract: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
            decimals: 6,
        }],
    ),
    (
        "optimism",
        &[EvmTokenConfig {
            symbol: "USDC",
            name: "USD Coin",
            contract: "0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85",
            decimals: 6,
        }],
    ),
    (
        "avalanche_c",
        &[EvmTokenConfig {
            symbol: "USDC",
            name: "USD Coin",
            contract: "0xB97EF9Ef8734C71904D8002F8b6Bc66Dd9c48a6E",
            decimals: 6,
        }],
    ),
];

pub(crate) fn evm_tokens_for_network(network_id: &str) -> &[EvmTokenConfig] {
    EVM_TOKENS
        .iter()
        .find(|(id, _)| *id == network_id)
        .map(|(_, tokens)| *tokens)
        .unwrap_or(&[])
}

pub(crate) fn evm_config_by_id(network_id: &str) -> Option<&'static EvmNetworkConfig> {
    EVM_NETWORKS.iter().find(|c| c.id == network_id)
}

#[allow(dead_code)]
pub(crate) fn evm_network_id_for_token(symbol: &str) -> Option<&'static str> {
    EVM_TOKENS
        .iter()
        .find(|(_, tokens)| tokens.iter().any(|t| t.symbol == symbol))
        .map(|(id, _)| *id)
}

pub(crate) async fn fetch_evm_native_balance(
    config: &EvmNetworkConfig,
    address: &str,
) -> Result<u128, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getBalance",
        "params": [address, "latest"],
        "id": 1,
    });

    let json = rpc_post(config.rpc_url, &body).await?;
    let balance_hex = json["result"]
        .as_str()
        .ok_or_else(|| "RPC response missing result field".to_string())?;

    u128::from_str_radix(balance_hex.trim_start_matches("0x"), 16)
        .map_err(|e| format!("Invalid balance hex: {e}"))
}

pub(crate) async fn fetch_evm_token_balance(
    config: &EvmNetworkConfig,
    token: &EvmTokenConfig,
    address: &str,
) -> Result<u128, String> {
    let addr_hex = address.trim_start_matches("0x");
    let addr_bytes = hex::decode(addr_hex).map_err(|_| "Invalid address".to_string())?;
    let mut padded = vec![0u8; 32];
    padded[32 - addr_bytes.len()..].copy_from_slice(&addr_bytes);

    let mut data = vec![0x70, 0xa0, 0x82, 0x31]; // keccak256("balanceOf(address)")[..4]
    data.extend_from_slice(&padded);

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_call",
        "params": [{
            "to": token.contract,
            "data": format!("0x{}", hex::encode(&data))
        }, "latest"],
        "id": 1,
    });

    let json = rpc_post(config.rpc_url, &body).await?;
    let hex_str = json["result"]
        .as_str()
        .ok_or_else(|| "Token balance RPC missing result".to_string())?;

    u128::from_str_radix(hex_str.trim_start_matches("0x"), 16)
        .map_err(|e| format!("Invalid token balance hex: {e}"))
}

pub(crate) async fn fetch_evm_assets(
    config: &EvmNetworkConfig,
    address: &str,
    cached_assets: &[Asset],
) -> Vec<Asset> {
    let native = match fetch_evm_native_balance(config, address).await {
        Ok(wei) => Asset {
            symbol: config.native_symbol.to_string(),
            name: config.display_name.to_string(),
            balance: wei.to_string(),
            decimals: 18,
            price_usd: 0.0,
            change_24h: 0.0,
            network: config.id.to_string(),
            token_address: None,
        },
        Err(_) => {
            cached_asset(cached_assets, config.id, config.native_symbol).unwrap_or_else(|| Asset {
                symbol: config.native_symbol.to_string(),
                name: config.display_name.to_string(),
                balance: "0".to_string(),
                decimals: 18,
                price_usd: 0.0,
                change_24h: 0.0,
                network: config.id.to_string(),
                token_address: None,
            })
        }
    };

    let mut assets = vec![native];

    for token in evm_tokens_for_network(config.id) {
        match fetch_evm_token_balance(config, token, address).await {
            Ok(balance) => {
                assets.push(Asset {
                    symbol: token.symbol.to_string(),
                    name: token.name.to_string(),
                    balance: balance.to_string(),
                    decimals: token.decimals,
                    price_usd: 0.0,
                    change_24h: 0.0,
                    network: config.id.to_string(),
                    token_address: Some(token.contract.to_string()),
                });
            }
            Err(_) => {
                if let Some(cached) = cached_asset(cached_assets, config.id, token.symbol) {
                    assets.push(cached);
                }
            }
        }
    }

    assets
}

pub(crate) async fn fetch_evm_nonce(
    config: &EvmNetworkConfig,
    address: &str,
) -> Result<u64, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionCount",
        "params": [address, "latest"],
        "id": 1,
    });
    let json = rpc_post(config.rpc_url, &body).await?;
    let hex_str = json["result"]
        .as_str()
        .ok_or_else(|| "Nonce RPC missing result".to_string())?;
    u64::from_str_radix(hex_str.trim_start_matches("0x"), 16)
        .map_err(|e| format!("Invalid nonce hex: {e}"))
}

pub(crate) async fn fetch_evm_gas_price(config: &EvmNetworkConfig) -> Result<u128, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_gasPrice",
        "params": [],
        "id": 1,
    });
    let json = rpc_post(config.rpc_url, &body).await?;
    let hex_str = json["result"]
        .as_str()
        .ok_or_else(|| "Gas price RPC missing result".to_string())?;
    u128::from_str_radix(hex_str.trim_start_matches("0x"), 16)
        .map_err(|e| format!("Invalid gas price hex: {e}"))
}

pub(crate) async fn fetch_evm_estimated_gas(
    config: &EvmNetworkConfig,
    from: &str,
    to: &str,
    value: u128,
    data: &[u8],
) -> Result<u64, String> {
    let mut params = serde_json::json!({
        "from": from,
        "to": to,
        "value": format!("0x{:x}", value),
    });
    if !data.is_empty() {
        params["data"] = serde_json::Value::String(format!("0x{}", hex::encode(data)));
    }
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_estimateGas",
        "params": [params],
        "id": 1,
    });
    let json = rpc_post(config.rpc_url, &body).await?;
    let hex_str = json["result"]
        .as_str()
        .ok_or_else(|| "Estimate gas RPC missing result".to_string())?;
    u64::from_str_radix(hex_str.trim_start_matches("0x"), 16)
        .map_err(|e| format!("Invalid gas estimate hex: {e}"))
}

pub(crate) async fn broadcast_evm_tx(
    config: &EvmNetworkConfig,
    raw_tx_hex: &str,
) -> Result<String, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_sendRawTransaction",
        "params": [raw_tx_hex],
        "id": 1,
    });
    let json = rpc_post(config.rpc_url, &body).await?;
    json["result"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| {
            json["error"]["message"]
                .as_str()
                .unwrap_or("Unknown broadcast error")
                .to_string()
        })
}

pub(crate) async fn fetch_evm_tx_status(
    config: &EvmNetworkConfig,
    tx_hash: &str,
) -> Result<Option<String>, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionReceipt",
        "params": [tx_hash],
        "id": 1,
    });
    let json = rpc_post(config.rpc_url, &body).await?;

    if json["result"].is_null() {
        return Ok(None);
    }

    let status_hex = json["result"]["status"].as_str().unwrap_or("0x0");
    if status_hex == "0x1" {
        Ok(Some("confirmed".to_string()))
    } else {
        Ok(Some("failed".to_string()))
    }
}
