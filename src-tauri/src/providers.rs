use reqwest::Client;

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
use crate::providers::bitcoin::{BitcoinAccountSnapshot, scan_bitcoin_account};
use crate::providers::evm::fetch_evm_assets;
use crate::providers::solana::fetch_solana_assets;
use crate::providers::tron::fetch_tron_assets;
use crate::registry::{evm_networks, network_by_id};
use std::collections::HashMap;

pub(crate) mod bitcoin;
pub(crate) mod evm;
pub(crate) mod http;
pub(crate) mod prices;
pub(crate) mod solana;
pub(crate) mod tron;

pub(crate) struct NetworkAssetRefresh {
    pub(crate) assets: Vec<Asset>,
    pub(crate) balance_failed: bool,
}

pub(crate) struct PortfolioAssetRefresh {
    pub(crate) assets: Vec<Asset>,
    pub(crate) failed_networks: Vec<String>,
    pub(crate) bitcoin_account: Option<BitcoinAccountSnapshot>,
}

pub(crate) async fn fetch_portfolio_assets(
    client: &Client,
    addresses: &HashMap<String, String>,
    cached_assets: &[Asset],
    enabled_networks: &[String],
    cached_bitcoin_account: Option<&BitcoinAccountSnapshot>,
) -> PortfolioAssetRefresh {
    let mut assets = vec![];
    let mut failed_networks = vec![];
    let mut bitcoin_account = cached_bitcoin_account.cloned();

    if let Some(evm_address) = addresses.get("evm") {
        for config in evm_networks() {
            if !enabled_networks.contains(&config.id) {
                continue;
            }

            let refreshed = fetch_evm_assets(client, config, evm_address, cached_assets).await;
            if refreshed.balance_failed {
                failed_networks.push(config.name.clone());
            }
            assets.extend(refreshed.assets);
        }
    }

    if enabled_networks.iter().any(|id| id == "solana") {
        if let Some(solana_address) = addresses.get("solana") {
            let refreshed = fetch_solana_assets(client, solana_address, cached_assets).await;
            if refreshed.balance_failed {
                failed_networks.push("Solana".to_string());
            }
            assets.extend(refreshed.assets);
        }
    }

    if enabled_networks.iter().any(|id| id == "tron") {
        if let Some(tron_address) = addresses.get("tron") {
            let refreshed = fetch_tron_assets(client, tron_address, cached_assets).await;
            if refreshed.balance_failed {
                failed_networks.push("Tron".to_string());
            }
            assets.extend(refreshed.assets);
        }
    }

    if enabled_networks.iter().any(|id| id == "bitcoin") {
        if let Some(config) = network_by_id("bitcoin") {
            let refreshed =
                refresh_bitcoin_account_asset(client, addresses, bitcoin_account.as_ref()).await;
            match refreshed {
                Ok((asset, refreshed_account)) => {
                    assets.push(asset);
                    bitcoin_account = Some(refreshed_account);
                }
                Err(_) => {
                    failed_networks.push(config.name.clone());
                    if let Some(cached) =
                        cached_asset(cached_assets, &config.id, &config.native_asset.symbol)
                    {
                        assets.push(cached);
                    }
                }
            }
        } else {
            failed_networks.push("Bitcoin".to_string());
        }
    } else {
        bitcoin_account = None;
    }

    PortfolioAssetRefresh {
        assets,
        failed_networks,
        bitcoin_account,
    }
}

async fn refresh_bitcoin_account_asset(
    client: &reqwest::Client,
    addresses: &HashMap<String, String>,
    cached_account: Option<&BitcoinAccountSnapshot>,
) -> Result<(Asset, BitcoinAccountSnapshot), String> {
    let config = network_by_id("bitcoin")
        .ok_or_else(|| "Bitcoin is missing from the network registry".to_string())?;
    let primary_address = addresses
        .get(&config.address_key)
        .ok_or_else(|| "Wallet BTC address is not available".to_string())?;
    let cached_account =
        cached_account.ok_or_else(|| "Bitcoin account discovery is not initialized".to_string())?;
    if cached_account.account().primary_address()?.address != *primary_address {
        return Err("Bitcoin account does not match the wallet receive address".to_string());
    }

    let refreshed_account = scan_bitcoin_account(client, cached_account.account()).await?;
    let balance = refreshed_account.total_balance()?.to_string();
    let asset = Asset {
        symbol: config.native_asset.symbol.clone(),
        name: config.native_asset.name.clone(),
        balance,
        decimals: config.native_asset.decimals,
        price_usd: 0.0,
        change_24h: 0.0,
        network: config.id.clone(),
        token_address: None,
    };
    Ok((asset, refreshed_account))
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
