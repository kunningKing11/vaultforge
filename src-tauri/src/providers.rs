use crate::address::bitcoin::validate_address as validate_bitcoin_address;
use crate::address::evm::validate_address as validate_evm_address;
use crate::address::filecoin::validate_address as validate_filecoin_address;
use crate::address::injective::validate_address as validate_injective_address;
use crate::address::solana::validate_address as validate_solana_address;
use crate::address::tron::validate_address as validate_tron_address;
use crate::address::zcash::validate_address as validate_zcash_address;
use crate::assets::cached_asset;
use crate::derivation::{
    bech32_account_address, bitcoin_bech32_address, ethereum_address_from_private_key,
    filecoin_address_from_private_key, tron_address_from_private_key, zcash_transparent_address,
};
use crate::dto::Asset;
use crate::providers::bitcoin::fetch_bitcoin_balance;
use crate::providers::evm::fetch_evm_assets;
use crate::providers::solana::fetch_solana_assets;
use crate::providers::tron::fetch_tron_assets;
use crate::registry::{NetworkConfig, evm_networks, registry};
use std::collections::HashMap;

pub(crate) mod bitcoin;
pub(crate) mod evm;
pub(crate) mod http;
pub(crate) mod prices;
pub(crate) mod solana;
pub(crate) mod tron;

pub(crate) async fn fetch_portfolio_assets(
    addresses: &HashMap<String, String>,
    cached_assets: &[Asset],
) -> Vec<Asset> {
    let mut assets = vec![];

    if let Some(evm_address) = addresses.get("evm") {
        for config in evm_networks() {
            assets.extend(fetch_evm_assets(config, evm_address, cached_assets).await);
        }
    }

    if let Some(solana_address) = addresses.get("solana") {
        assets.extend(fetch_solana_assets(solana_address, cached_assets).await);
    }

    if let Some(tron_address) = addresses.get("tron") {
        assets.extend(fetch_tron_assets(tron_address, cached_assets).await);
    }

    for config in &registry().networks {
        if matches!(config.kind.as_str(), "evm" | "svm" | "tron") {
            continue;
        }
        let Some(address) = addresses.get(&config.address_key) else {
            continue;
        };

        match fetch_non_evm_native_asset(config, address).await {
            Ok(asset) => assets.push(asset),
            Err(_) => {
                if let Some(cached) =
                    cached_asset(cached_assets, &config.id, &config.native_asset.symbol)
                {
                    assets.push(cached);
                }
            }
        }
    }

    assets
}

async fn fetch_non_evm_native_asset(
    config: &NetworkConfig,
    address: &str,
) -> Result<Asset, String> {
    let balance = match config.id.as_str() {
        "bitcoin" => fetch_bitcoin_balance(address).await?,
        _ => {
            return Err(format!(
                "{} provider is not implemented yet",
                config.native_asset.symbol
            ));
        }
    };

    Ok(Asset {
        symbol: config.native_asset.symbol.clone(),
        name: config.native_asset.name.clone(),
        balance,
        decimals: config.native_asset.decimals,
        price_usd: 0.0,
        change_24h: 0.0,
        network: config.id.clone(),
        token_address: None,
    })
}

#[allow(dead_code)]
pub(crate) trait ChainProvider: Send + Sync {
    fn chain_name(&self) -> &'static str;
    fn symbol(&self) -> &'static str;
    fn validate_address(&self, address: &str) -> Result<(), String>;
    fn derive_address(&self, private_key: &[u8; 32]) -> Result<String, String>;
}

struct BitcoinProvider;
struct EvmProvider;
struct FilecoinProvider;
struct InjectiveProvider;
struct SolanaProvider;
struct TronProvider;
struct ZcashProvider;

impl ChainProvider for EvmProvider {
    fn chain_name(&self) -> &'static str {
        "EVM"
    }
    fn symbol(&self) -> &'static str {
        "ETH"
    }
    fn validate_address(&self, address: &str) -> Result<(), String> {
        validate_evm_address(address)
    }
    fn derive_address(&self, private_key: &[u8; 32]) -> Result<String, String> {
        ethereum_address_from_private_key(private_key)
    }
}

impl ChainProvider for BitcoinProvider {
    fn chain_name(&self) -> &'static str {
        "Bitcoin"
    }
    fn symbol(&self) -> &'static str {
        "BTC"
    }
    fn validate_address(&self, address: &str) -> Result<(), String> {
        validate_bitcoin_address(address)
    }
    fn derive_address(&self, private_key: &[u8; 32]) -> Result<String, String> {
        bitcoin_bech32_address(private_key, false)
    }
}

impl ChainProvider for FilecoinProvider {
    fn chain_name(&self) -> &'static str {
        "Filecoin"
    }
    fn symbol(&self) -> &'static str {
        "FIL"
    }
    fn validate_address(&self, address: &str) -> Result<(), String> {
        validate_filecoin_address(address)
    }
    fn derive_address(&self, private_key: &[u8; 32]) -> Result<String, String> {
        filecoin_address_from_private_key(private_key)
    }
}

impl ChainProvider for InjectiveProvider {
    fn chain_name(&self) -> &'static str {
        "Injective"
    }
    fn symbol(&self) -> &'static str {
        "INJ"
    }
    fn validate_address(&self, address: &str) -> Result<(), String> {
        validate_injective_address(address)
    }
    fn derive_address(&self, private_key: &[u8; 32]) -> Result<String, String> {
        bech32_account_address(private_key, "inj")
    }
}

impl ChainProvider for SolanaProvider {
    fn chain_name(&self) -> &'static str {
        "Solana"
    }
    fn symbol(&self) -> &'static str {
        "SOL"
    }
    fn validate_address(&self, address: &str) -> Result<(), String> {
        validate_solana_address(address)
    }
    fn derive_address(&self, _private_key: &[u8; 32]) -> Result<String, String> {
        Err("Solana derivation requires seed bytes, not secp256k1 key".to_string())
    }
}

impl ChainProvider for TronProvider {
    fn chain_name(&self) -> &'static str {
        "Tron"
    }
    fn symbol(&self) -> &'static str {
        "TRX"
    }
    fn validate_address(&self, address: &str) -> Result<(), String> {
        validate_tron_address(address)
    }
    fn derive_address(&self, private_key: &[u8; 32]) -> Result<String, String> {
        tron_address_from_private_key(private_key)
    }
}

impl ChainProvider for ZcashProvider {
    fn chain_name(&self) -> &'static str {
        "Zcash"
    }
    fn symbol(&self) -> &'static str {
        "ZEC"
    }
    fn validate_address(&self, address: &str) -> Result<(), String> {
        validate_zcash_address(address)
    }
    fn derive_address(&self, private_key: &[u8; 32]) -> Result<String, String> {
        zcash_transparent_address(private_key, false)
    }
}

#[allow(dead_code)]
pub(crate) fn get_provider(symbol: &str) -> Option<Box<dyn ChainProvider>> {
    match symbol {
        "BTC" => Some(Box::new(BitcoinProvider)),
        "FIL" => Some(Box::new(FilecoinProvider)),
        "INJ" => Some(Box::new(InjectiveProvider)),
        "SOL" => Some(Box::new(SolanaProvider)),
        "TRX" => Some(Box::new(TronProvider)),
        "ZEC" => Some(Box::new(ZcashProvider)),
        _ => Some(Box::new(EvmProvider)),
    }
}
