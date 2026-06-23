use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use zeroize::Zeroize;
use chrono::Utc;
use std::{fs, sync::Mutex};
use tauri::{Manager, State};

mod activity;
mod derivation;
mod dto;
mod providers;
mod state;
mod storage;
mod tx;
mod validation;

use activity::*;
use derivation::*;
use dto::*;
use providers::bitcoin::*;
use providers::evm::*;
use providers::prices::*;
use providers::*;
use state::*;
use storage::*;
use tx::evm::*;
use validation::*;

#[tauri::command]
fn get_wallet(state: State<'_, Mutex<AppState>>) -> Result<WalletSession, String> {
    let state = state.lock().map_err(|_| "State lock failed")?;
    Ok(session_from_state(&state))
}

#[tauri::command]
async fn refresh_prices(state: State<'_, Mutex<AppState>>) -> Result<WalletSession, String> {
    let price_ids = {
        let state = state.lock().map_err(|_| "State lock failed")?;
        if state.locked {
            return Err("Wallet is locked".to_string());
        }
        let wallet = state
            .wallet
            .as_ref()
            .ok_or_else(|| "No wallet exists yet".to_string())?;
        wallet
            .assets
            .iter()
            .filter_map(|asset| price_id_for_symbol(&asset.symbol))
            .fold(Vec::<&'static str>::new(), |mut ids, id| {
                if !ids.contains(&id) {
                    ids.push(id);
                }
                ids
            })
    };

    if price_ids.is_empty() {
        let state = state.lock().map_err(|_| "State lock failed")?;
        return Ok(session_from_state(&state));
    }

    let prices = fetch_market_prices(&price_ids).await?;

    let mut state = state.lock().map_err(|_| "State lock failed")?;
    {
        let wallet = state
            .wallet
            .as_mut()
            .ok_or_else(|| "No wallet exists yet".to_string())?;
        for asset in &mut wallet.assets {
            let Some(price_id) = price_id_for_symbol(&asset.symbol) else {
                continue;
            };
            let Some(price) = prices.get(price_id) else {
                continue;
            };
            if price.usd.is_finite() && price.usd > 0.0 {
                asset.price_usd = price.usd;
            }
            if let Some(change) = price.usd_24h_change {
                if change.is_finite() {
                    asset.change_24h = change;
                }
            }
        }
    }
    persist_state_wallet(&mut state)?;
    Ok(session_from_state(&state))
}

#[tauri::command]
async fn create_wallet(
    state: State<'_, Mutex<AppState>>,
    name: String,
    passphrase: String,
) -> Result<WalletSession, String> {
    validate_passphrase(&passphrase)?;
    let mnemonic = generate_mnemonic()?;
    let addresses = derive_addresses_from_mnemonic(&mnemonic)?;
    let primary_address = addresses
        .get("evm")
        .cloned()
        .unwrap_or_else(|| address_from_seed(&mnemonic));

    let assets = fetch_portfolio_assets(&addresses, &[]).await;

    let wallet = Wallet {
        name: clean_name(name),
        mnemonic,
        created_at: Utc::now().to_rfc3339(),
        address: primary_address,
        addresses,
        passphrase_hash: hash_secret(&passphrase),
        assets,
        activity: vec![activity(
            "system",
            "Wallet created",
            "Recovery phrase generated locally",
            "12 words",
        )],
    };

    let mut state = state.lock().map_err(|_| "State lock failed")?;
    let (key, salt) = derive_storage_key(&passphrase, None)?;
    state.encryption_key = Some(key);
    state.storage_salt = Some(salt);
    state.wallet = Some(wallet);
    state.locked = false;
    persist_state_wallet(&mut state)?;
    Ok(session_from_state(&state))
}

#[tauri::command]
async fn import_wallet(
    state: State<'_, Mutex<AppState>>,
    mnemonic: String,
    passphrase: String,
) -> Result<WalletSession, String> {
    let words = mnemonic.split_whitespace().count();
    if words != 12 && words != 24 {
        return Err("Recovery phrase must contain 12 or 24 words".to_string());
    }
    validate_passphrase(&passphrase)?;

    let mnemonic = mnemonic.trim().to_string();
    let addresses = derive_addresses_from_mnemonic(&mnemonic)?;
    let primary_address = addresses
        .get("evm")
        .cloned()
        .unwrap_or_else(|| address_from_seed(&mnemonic));

    let assets = fetch_portfolio_assets(&addresses, &[]).await;

    let wallet = Wallet {
        name: "Imported Wallet".to_string(),
        mnemonic,
        created_at: Utc::now().to_rfc3339(),
        address: primary_address,
        addresses,
        passphrase_hash: hash_secret(&passphrase),
        assets,
        activity: vec![activity(
            "import",
            "Wallet imported",
            "Recovery phrase verified locally",
            "Imported",
        )],
    };

    let mut state = state.lock().map_err(|_| "State lock failed")?;
    let (key, salt) = derive_storage_key(&passphrase, None)?;
    state.encryption_key = Some(key);
    state.storage_salt = Some(salt);
    state.wallet = Some(wallet);
    state.locked = false;
    persist_state_wallet(&mut state)?;
    Ok(session_from_state(&state))
}

#[tauri::command]
async fn unlock_wallet(
    state: State<'_, Mutex<AppState>>,
    passphrase: String,
) -> Result<WalletSession, String> {
    let passphrase_hash = hash_secret(&passphrase);

    let (address, addresses, cached_assets) = {
        let mut state = state.lock().map_err(|_| "State lock failed")?;

        let in_memory = state.wallet.as_ref().map(|w| {
            (w.passphrase_hash.clone(), w.address.clone(), w.addresses.clone(), w.assets.clone())
        });

        if let Some((stored_hash, addr, addresses, assets)) = in_memory {
            if stored_hash != passphrase_hash {
                return Err("Invalid passphrase".to_string());
            }
            state.locked = false;
            (addr, addresses, assets)
        } else {
            let stored = read_stored_wallet(&state.storage_path)?
                .ok_or_else(|| "No wallet exists yet".to_string())?;
            let wallet = decrypt_wallet(&stored, &passphrase)?;
            if wallet.passphrase_hash != passphrase_hash {
                return Err("Invalid passphrase".to_string());
            }
            state.stored_wallet = Some(StoredWalletMetadata {
                wallet_name: stored.wallet_name,
            });
            let salt = BASE64
                .decode(stored.salt)
                .map_err(|_| "Stored wallet salt is invalid")?;
            let (key, salt) = derive_storage_key(&passphrase, Some(&salt))?;
            state.encryption_key = Some(key);
            state.storage_salt = Some(salt);
            let address = wallet.address.clone();
            let addresses = wallet.addresses.clone();
            let assets = wallet.assets.clone();
            state.wallet = Some(wallet);
            state.locked = false;
            (address, addresses, assets)
        }
    };

    let mut refresh_addresses = addresses;
    refresh_addresses.entry("evm".to_string()).or_insert(address);
    let fresh_assets = fetch_portfolio_assets(&refresh_addresses, &cached_assets).await;

    let mut state = state.lock().map_err(|_| "State lock failed")?;
    if let Some(wallet) = state.wallet.as_mut() {
        wallet.assets = fresh_assets;
    }
    persist_state_wallet(&mut state)?;
    Ok(session_from_state(&state))
}

#[tauri::command]
fn lock_wallet(state: State<'_, Mutex<AppState>>) -> Result<(), String> {
    let mut state = state.lock().map_err(|_| "State lock failed")?;
    if let Some(ref mut wallet) = state.wallet {
        clear_secret_string(&mut wallet.mnemonic);
    }
    state.wallet = None;
    if let Some(ref mut key) = state.encryption_key {
        key.zeroize();
    }
    state.encryption_key = None;
    if let Some(ref mut salt) = state.storage_salt {
        salt.fill(0);
    }
    state.storage_salt = None;
    state.locked = true;
    Ok(())
}

#[tauri::command]
fn clear_wallet(state: State<'_, Mutex<AppState>>) -> Result<WalletSession, String> {
    let mut state = state.lock().map_err(|_| "State lock failed")?;
    if state.storage_path.exists() {
        fs::remove_file(&state.storage_path).map_err(|_| "Failed to remove stored wallet")?;
    }
    if let Some(ref mut wallet) = state.wallet {
        clear_secret_string(&mut wallet.mnemonic);
    }
    state.wallet = None;
    state.stored_wallet = None;
    if let Some(ref mut key) = state.encryption_key {
        key.zeroize();
    }
    state.encryption_key = None;
    if let Some(ref mut salt) = state.storage_salt {
        salt.fill(0);
    }
    state.storage_salt = None;
    state.locked = false;
    Ok(session_from_state(&state))
}

#[tauri::command]
async fn sign_transaction(
    state: State<'_, Mutex<AppState>>,
    to: String,
    symbol: String,
    amount: String,
    note: String,
) -> Result<SignedTransaction, String> {
    validate_unlocked(&state)?;

    let (mnemonic, address, addresses, assets) = {
        let state = state.lock().map_err(|_| "State lock failed")?;
        let wallet = state
            .wallet
            .as_ref()
            .ok_or_else(|| "No wallet exists yet".to_string())?;
        validate_transfer(wallet, &to, &symbol, &amount)?;
        (
            wallet.mnemonic.clone(),
            wallet.address.clone(),
            wallet.addresses.clone(),
            wallet.assets.clone(),
        )
    };

    let to = to.trim().to_string();
    let value_wei: u128 = amount.parse().map_err(|_| "Invalid amount".to_string())?;

    let asset = assets.iter()
        .find(|a| a.symbol == symbol)
        .ok_or_else(|| format!("Asset {symbol} not found in wallet"))?;
    let network_id = &asset.network;
    let decimals = asset.decimals;

    if network_id == "bitcoin" && symbol == "BTC" {
        let from = addresses
            .get("bitcoin")
            .ok_or_else(|| "Wallet BTC address is not available".to_string())?
            .clone();
        let amount_sats: u64 = value_wei
            .try_into()
            .map_err(|_| "BTC amount is too large".to_string())?;
        let signed_btc = sign_bitcoin_transfer(&mnemonic, &from, &to, amount_sats).await?;
        return Ok(SignedTransaction {
            from,
            to,
            symbol: symbol.clone(),
            amount: value_wei.to_string(),
            note: note.trim().to_string(),
            network: "bitcoin".to_string(),
            nonce: "utxo".to_string(),
            signed_at: Utc::now().to_rfc3339(),
            payload_hash: signed_btc.txid.clone(),
            signature: signed_btc.first_signature_hex,
            fee_amount: signed_btc.fee_sats.to_string(),
            fee_symbol: "BTC".to_string(),
            total_debit: (amount_sats + signed_btc.fee_sats).to_string(),
            post_balance: signed_btc.post_balance.to_string(),
            decimals,
            fiat_value: 0.0,
            raw_tx: Some(signed_btc.raw_tx_hex),
            tx_hash: Some(signed_btc.txid),
        });
    }

    if evm_config_by_id(network_id).is_none() {
        return Err(format!(
            "{} transfers on {} are not implemented yet",
            symbol, network_id
        ));
    }

    let config = evm_config_by_id(network_id)
        .ok_or_else(|| format!("No EVM chain configured for network {network_id}"))?;

    let is_native = evm_config_for_symbol(&symbol).is_some();

    let (tx_to, tx_data, display_to) = if is_native {
        (to.clone(), Vec::new(), to.clone())
    } else {
        let token = evm_tokens_for_network(config.id).iter()
            .find(|t| t.symbol == symbol)
            .ok_or_else(|| format!("Token {symbol} not found on {network_id}"))?;
        (token.contract.to_string(), encode_erc20_transfer(&to, value_wei)?, to.clone())
    };

    let nonce = fetch_evm_nonce(config, &address).await?;
    let gas_price = fetch_evm_gas_price(config).await?;
    let gas_limit = if tx_data.is_empty() {
        fetch_evm_estimate_gas(config, &address, &tx_to, value_wei, &[]).await?
    } else {
        fetch_evm_estimate_gas(config, &address, &tx_to, 0, &tx_data).await?
    };

    let max_priority_fee_per_gas = gas_price;
    let max_fee_per_gas = gas_price;
    let total_fee_wei = gas_limit as u128 * max_fee_per_gas;

    let signing_key = signing_key_from_mnemonic(&mnemonic)?;
    let (_, tx_hash, raw_tx_hex, r_hex, s_hex) = sign_eip1559_transfer(
        &signing_key,
        config.chain_id,
        nonce,
        max_priority_fee_per_gas,
        max_fee_per_gas,
        gas_limit,
        &tx_to,
        if tx_data.is_empty() { value_wei } else { 0 },
        &tx_data,
    )?;

    let amount_str = value_wei.to_string();
    let fee_str = total_fee_wei.to_string();
    let total_debit_str = if is_native {
        (value_wei + total_fee_wei).to_string()
    } else {
        total_fee_wei.to_string()
    };
    let signature_str = format!("0x{}{}", r_hex, s_hex);

    let post_balance_wei: u128 = asset.balance.parse().unwrap_or(0);
    let post_balance = if post_balance_wei >= value_wei {
        (post_balance_wei - value_wei).to_string()
    } else {
        "0".to_string()
    };

    let signed = SignedTransaction {
        from: address,
        to: display_to,
        symbol: symbol.clone(),
        amount: amount_str,
        note: note.trim().to_string(),
        network: config.id.to_string(),
        nonce: nonce.to_string(),
        signed_at: Utc::now().to_rfc3339(),
        payload_hash: tx_hash.clone(),
        signature: signature_str,
        fee_amount: fee_str,
        fee_symbol: symbol.clone(),
        total_debit: total_debit_str,
        post_balance,
        decimals,
        fiat_value: 0.0,
        raw_tx: Some(raw_tx_hex),
        tx_hash: Some(tx_hash),
    };

    Ok(signed)
}

#[tauri::command]
async fn send_transaction(
    state: State<'_, Mutex<AppState>>,
    signed: SignedTransaction,
) -> Result<WalletSession, String> {
    validate_unlocked(&state)?;

    {
        let state = state.lock().map_err(|_| "State lock failed")?;
        let wallet = state
            .wallet
            .as_ref()
            .ok_or_else(|| "No wallet exists yet".to_string())?;
        if signed.from != wallet.address && !wallet.addresses.values().any(|address| address == &signed.from) {
            return Err("Signed transaction does not match this wallet".to_string());
        }
    }

    let raw_tx = signed
        .raw_tx
        .as_ref()
        .ok_or_else(|| "No raw transaction data".to_string())?;

    let tx_hash = if signed.network == "bitcoin" && signed.symbol == "BTC" {
        broadcast_bitcoin_transaction(raw_tx).await?
    } else {
        let config = evm_config_for_symbol(&signed.symbol)
            .or_else(|| evm_config_by_id(&signed.network))
            .ok_or_else(|| format!("No EVM chain configured for {}", signed.symbol))?;
        broadcast_evm_transaction(config, raw_tx).await?
    };

    let memo = if signed.note.is_empty() {
        format!("Sent to {}", short_address(&signed.to))
    } else {
        signed.note.clone()
    };

    let mut state = state.lock().map_err(|_| "State lock failed")?;
    if let Some(wallet) = state.wallet.as_mut() {
        wallet.activity.insert(
            0,
            Activity {
                id: random_hex(8),
                kind: "send".to_string(),
                title: "Transfer sent".to_string(),
                subtitle: memo,
                amount: format!("-{} {}", signed.amount, signed.symbol),
                status: "pending".to_string(),
                timestamp: Utc::now().to_rfc3339(),
                hash: tx_hash.clone(),
                from: Some(signed.from.clone()),
                to: Some(signed.to.clone()),
                network: Some(signed.network.clone()),
                payload_hash: signed.tx_hash.clone(),
                signature: Some(signed.signature.clone()),
                fee: Some(format!("{} {}", signed.fee_amount, signed.fee_symbol)),
            },
        );
    }
    persist_state_wallet(&mut state)?;
    let session = session_from_state(&state);
    Ok(session)
}

#[tauri::command(rename_all = "camelCase")]
fn swap_tokens(
    state: State<'_, Mutex<AppState>>,
    from_symbol: String,
    to_symbol: String,
    amount: String,
) -> Result<WalletSession, String> {
    validate_unlocked(&state)?;
    if from_symbol == to_symbol {
        return Err("Choose two different assets".to_string());
    }

    let amount_wei: u128 = amount.parse().map_err(|_| "Invalid amount".to_string())?;
    if amount_wei == 0 {
        return Err("Amount must be greater than zero".to_string());
    }

    let mut state = state.lock().map_err(|_| "State lock failed")?;
    let wallet = state
        .wallet
        .as_mut()
        .ok_or_else(|| "No wallet exists yet".to_string())?;

    let from_index = wallet
        .assets
        .iter()
        .position(|asset| asset.symbol == from_symbol)
        .ok_or_else(|| "Source asset not found".to_string())?;
    let to_index = wallet
        .assets
        .iter()
        .position(|asset| asset.symbol == to_symbol)
        .ok_or_else(|| "Destination asset not found".to_string())?;

    let from_balance: u128 = wallet.assets[from_index].balance.parse().unwrap_or(0);
    if from_balance < amount_wei {
        return Err(format!("Insufficient {} balance", from_symbol));
    }

    let source_value = amount_wei as f64 / 1e18 * wallet.assets[from_index].price_usd;
    let received_wei = if wallet.assets[to_index].price_usd > 0.0 {
        let received_f64 = (source_value / wallet.assets[to_index].price_usd) * 0.995;
        (received_f64 * 1e18) as u128
    } else {
        0
    };

    wallet.assets[from_index].balance = (from_balance - amount_wei).to_string();
    let to_balance: u128 = wallet.assets[to_index].balance.parse().unwrap_or(0);
    wallet.assets[to_index].balance = (to_balance + received_wei).to_string();
    wallet.activity.insert(
        0,
        activity(
            "swap",
            "Swap executed",
            &format!("{} to {} with 0.5% route fee", from_symbol, to_symbol),
            &format!("{amount_wei} {from_symbol} -> {received_wei} {to_symbol}"),
        ),
    );
    persist_state_wallet(&mut state)?;
    Ok(session_from_state(&state))
}

#[tauri::command]
async fn check_transaction_status(
    _state: State<'_, Mutex<AppState>>,
    tx_hash: String,
    network: String,
) -> Result<Option<String>, String> {
    if network == "bitcoin" {
        return fetch_bitcoin_tx_status(&tx_hash).await;
    }

    let config = EVM_NETWORKS.iter().find(|c| c.id == network)
        .ok_or_else(|| format!("Unknown network: {}", network))?;
    fetch_tx_status(config, &tx_hash).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use providers::solana::parse_solana_balance;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn starter_assets(network: &str) -> Vec<Asset> {
        vec![
            Asset {
                symbol: "ETH".to_string(),
                name: "Ethereum".to_string(),
                balance: "2482100000000000000000".to_string(),
                decimals: 18,
                price_usd: 3480.62,
                change_24h: 2.84,
                network: network.to_string(),
            },
            Asset {
                symbol: "BTC".to_string(),
                name: "Bitcoin".to_string(),
                balance: "184200000000".to_string(),
                decimals: 8,
                price_usd: 102_240.12,
                change_24h: -0.62,
                network: network.to_string(),
            },
            Asset {
                symbol: "SOL".to_string(),
                name: "Solana".to_string(),
                balance: "82450000000".to_string(),
                decimals: 9,
                price_usd: 184.33,
                change_24h: 5.18,
                network: network.to_string(),
            },
            Asset {
                symbol: "USDC".to_string(),
                name: "USD Coin".to_string(),
                balance: "8420000000".to_string(),
                decimals: 6,
                price_usd: 1.0,
                change_24h: 0.01,
                network: network.to_string(),
            },
        ]
    }

    #[test]
    fn validates_asset_address_formats() {
        assert!(validate_address_for_symbol("0xdAC17F958D2ee523a2206206994597C13D831ec7", "ETH").is_ok());
        assert!(validate_address_for_symbol("0xdac17f958d2ee523a2206206994597c13d831ec7", "ETH").is_ok());
        assert!(validate_address_for_symbol("bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq", "BTC").is_ok());
        assert!(validate_address_for_symbol("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa", "BTC").is_ok());
        assert!(validate_address_for_symbol("3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy", "BTC").is_ok());
        assert!(validate_address_for_symbol("7VH1XhBY1DmFk98fBdLqEbDsKpr41whdM8EzipizyVCJ", "SOL").is_ok());
        assert!(validate_address_for_symbol("t1eB29zcZ2v3AQvAEtcNrERsWQPmxyTN4DF", "ZEC").is_ok());
        assert!(validate_address_for_symbol("f1ke28mVhmmiSdiFRybu3ak3NnEqpx3o3Bk", "FIL").is_ok());
        assert!(validate_address_for_symbol("inj1m6kmamcpqgpsgpgxquyqjyq3zgf3g9gkzz8lqn", "INJ").is_ok());
        assert!(validate_address_for_symbol("0xdAC17F958D2ee523a2206206994597C13D831ec7", "MATIC").is_ok());
        assert!(validate_address_for_symbol("0xinvalid", "ETH").is_err());
        assert!(validate_address_for_symbol("bc1q", "BTC").is_err());
        assert!(validate_address_for_symbol("invalid", "SOL").is_err());
    }

    #[test]
    fn validates_eip55_checksum() {
        assert!(validate_evm_address("0xdAC17F958D2ee523a2206206994597C13D831ec7").is_ok());
        assert!(validate_evm_address("0xdac17f958d2ee523a2206206994597c13d831ec7").is_ok());
        assert!(validate_evm_address("0xDAc17f958D2eE523a2206206994597C13D831ec7").is_err());
        assert!(validate_evm_address("0xDbC17F958D2ee523a2206206994597C13D831ec7").is_err());
        assert!(validate_evm_address("0x0000000000000000000000000000000000000000").is_ok());
        assert!(validate_evm_address("0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF").is_ok());
    }

    #[test]
    fn provider_trait_covers_all_chains() {
        for symbol in &["ETH", "BTC", "SOL", "ZEC", "FIL", "INJ", "MATIC"] {
            let provider = get_provider(symbol);
            assert!(provider.is_some(), "No provider for symbol {symbol}");
        }
    }

    #[test]
    fn selects_cached_asset_by_network_and_symbol() {
        let assets = starter_assets("ethereum");
        let cached = cached_asset(&assets, "ethereum", "ETH").unwrap();
        assert_eq!(cached.symbol, "ETH");
        assert_eq!(cached.network, "ethereum");
        assert_eq!(cached.balance, "2482100000000000000000");
        assert!(cached_asset(&assets, "polygon", "ETH").is_none());
        assert!(cached_asset(&assets, "ethereum", "MATIC").is_none());
    }

    #[test]
    fn parses_bitcoin_balance_with_mempool_values() {
        let json = serde_json::json!({
            "chain_stats": {
                "funded_txo_sum": 5000,
                "spent_txo_sum": 1200
            },
            "mempool_stats": {
                "funded_txo_sum": 700,
                "spent_txo_sum": 200
            }
        });
        assert_eq!(parse_bitcoin_balance(&json).unwrap(), 4300);
    }

    #[test]
    fn parses_bitcoin_utxos_and_fee_rate() {
        let json = serde_json::json!([
            {
                "txid": "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
                "vout": 1,
                "value": 50_000,
                "status": { "confirmed": true }
            },
            {
                "txid": "101102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d2e2f",
                "vout": 0,
                "value": 100,
                "status": { "confirmed": true }
            }
        ]);
        let utxos = parse_bitcoin_utxos(&json).unwrap();
        assert_eq!(utxos.len(), 1);
        assert_eq!(utxos[0].value, 50_000);

        let fees = serde_json::json!({ "3": 2.1, "6": 1.4 });
        assert_eq!(parse_bitcoin_fee_rate(&fees).unwrap(), 3);
    }

    #[test]
    fn selects_bitcoin_coins_with_change() {
        let utxos = vec![BitcoinUtxo {
            txid: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f".to_string(),
            vout: 0,
            value: 50_000,
            confirmed: true,
        }];
        let (selected, fee, change) = bitcoin_select_coins(&utxos, 10_000, 2).unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(fee, bitcoin_estimated_vbytes(1, 2) * 2);
        assert_eq!(change, 50_000 - 10_000 - fee);
    }

    #[test]
    fn signs_bitcoin_p2wpkh_transfer() {
        let private_key = [0x01u8; 32];
        let from = bitcoin_bech32_address(&private_key, false).unwrap();
        let utxos = vec![BitcoinUtxo {
            txid: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f".to_string(),
            vout: 0,
            value: 50_000,
            confirmed: true,
        }];
        let signed = bitcoin_signed_transfer(
            &private_key,
            &from,
            "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq",
            10_000,
            &utxos,
            2,
        ).unwrap();

        assert_eq!(signed.txid.len(), 64);
        assert!(signed.raw_tx_hex.starts_with("020000000001"));
        assert!(!signed.first_signature_hex.is_empty());
        assert_eq!(signed.fee_sats, bitcoin_estimated_vbytes(1, 2) * 2);
        assert_eq!(signed.post_balance, 50_000 - 10_000 - signed.fee_sats);
    }

    #[test]
    fn parses_solana_balance_lamports() {
        let json = serde_json::json!({
            "jsonrpc": "2.0",
            "result": {
                "context": { "slot": 1 },
                "value": 123456789u64
            },
            "id": 1
        });
        assert_eq!(parse_solana_balance(&json).unwrap(), 123456789);
    }

    #[test]
    fn derives_documented_wallet_paths_deterministically() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let addresses = derive_addresses_from_mnemonic(mnemonic).unwrap();
        assert_eq!(addresses.len(), 6);
        assert_eq!(addresses.get("evm").unwrap(), "0x9858effd232b4033e47d90003d41ec34ecaeda94");
        assert_eq!(addresses.get("bitcoin").unwrap(), "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu");
        assert_eq!(addresses.get("zcash").unwrap(), "t1XVXWCvpMgBvUaed4XDqWtgQgJSu1Ghz7F");
        assert_eq!(addresses.get("solana").unwrap(), "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk");
        assert_eq!(addresses.get("filecoin").unwrap(), "f1fFXqnEMPFe1NoAajxRKukEBLwshG1LQQC");
        assert_eq!(addresses.get("injective").unwrap(), "inj1gsvdpdxec8hsu57lhxg5xem7refr233zkczfgv");
    }

    #[test]
    fn locked_session_does_not_expose_secrets() {
        let mut state = AppState::from_storage(PathBuf::from("/nonexistent/wallet.json"));
        let wallet = Wallet {
            name: "Secret Wallet".to_string(),
            mnemonic: "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            address: "0xdAC17F958D2ee523a2206206994597C13D831ec7".to_string(),
            addresses: HashMap::new(),
            passphrase_hash: "deadbeef".to_string(),
            assets: vec![],
            activity: vec![],
        };
        state.wallet = Some(wallet);
        state.locked = true;
        state.stored_wallet = Some(StoredWalletMetadata {
            wallet_name: "Secret Wallet".to_string(),
        });
        let session = session_from_state(&state);
        assert_eq!(session.has_wallet, true);
        assert_eq!(session.locked, true);
        assert!(session.address.is_none());
        assert!(session.addresses.is_none());
        assert!(session.assets.is_empty());
        assert!(session.activity.is_empty());
    }

    #[test]
    fn constructs_valid_eip1559_signature() {
        use k256::ecdsa::SigningKey;
        let private_key = [0xabu8; 32];
        let signing_key = SigningKey::from_bytes((&private_key).into()).unwrap();
        let result = sign_eip1559_transfer(
            &signing_key,
            1,
            0,
            1_000_000_000,
            1_000_000_000,
            21000,
            "0xdAC17F958D2ee523a2206206994597C13D831ec7",
            1_000_000_000_000_000_000u128,
            &[],
        );
        assert!(result.is_ok());
        let (_raw, tx_hash, _raw_hex, r, s) = result.unwrap();
        assert!(tx_hash.starts_with("0x"));
        assert_eq!(tx_hash.len(), 66);
        assert_eq!(r.len(), 64);
        assert_eq!(s.len(), 64);
        assert!(!_raw.is_empty());
    }

    #[test]
    fn encodes_erc20_transfer_abi() {
        let recipient = "0xdAC17F958D2ee523a2206206994597C13D831ec7";
        let amount: u128 = 1_000_000_000_000_000_000;
        let data = encode_erc20_transfer(recipient, amount).unwrap();
        assert!(!data.is_empty());
        assert_eq!(data.len(), 4 + 32 + 32);
        assert_eq!(&data[..4], &[0xa9, 0x05, 0x9c, 0xbb]);
        let recip_bytes = hex::decode(recipient.trim_start_matches("0x")).unwrap();
        assert_eq!(&data[16..36], &recip_bytes[..]);
        assert_eq!(data[data.len() - 1], 0x00);
    }

    #[test]
    fn signs_erc20_transfer() {
        use k256::ecdsa::SigningKey;
        let private_key = [0xabu8; 32];
        let signing_key = SigningKey::from_bytes((&private_key).into()).unwrap();
        let data = encode_erc20_transfer("0xdAC17F958D2ee523a2206206994597C13D831ec7", 1_000_000).unwrap();
        let result = sign_eip1559_transfer(
            &signing_key,
            1,
            0,
            1_000_000_000,
            1_000_000_000,
            50000,
            "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
            0,
            &data,
        );
        assert!(result.is_ok());
        let (_raw, tx_hash, _raw_hex, r, s) = result.unwrap();
        assert!(tx_hash.starts_with("0x"));
        assert_eq!(tx_hash.len(), 66);
        assert_eq!(r.len(), 64);
        assert_eq!(s.len(), 64);
    }

    #[test]
    fn derives_same_key_with_same_salt() {
        let (key, salt) = derive_storage_key("correct horse battery staple", None).unwrap();
        let (same_key, same_salt) =
            derive_storage_key("correct horse battery staple", Some(&salt)).unwrap();
        assert_eq!(key, same_key);
        assert_eq!(salt, same_salt);
    }

    #[test]
    fn encrypts_and_decrypts_wallet_payload() {
        let passphrase = "Correct horse battery staple 42!";
        let wallet = Wallet {
            name: "Test Wallet".to_string(),
            mnemonic: "test mnemonic".to_string(),
            created_at: Utc::now().to_rfc3339(),
            address: address_from_seed("test seed"),
            addresses: HashMap::new(),
            passphrase_hash: hash_secret(passphrase),
            assets: starter_assets("ethereum"),
            activity: vec![activity("system", "Created", "Local", "1")],
        };
        let (key, salt) = derive_storage_key(passphrase, None).unwrap();
        let stored = encrypt_wallet(&wallet, &key, &salt).unwrap();
        assert!(!stored.ciphertext.contains(&wallet.address));

        let decrypted = decrypt_wallet(&stored, passphrase).unwrap();
        assert_eq!(decrypted.name, wallet.name);
        assert_eq!(decrypted.mnemonic, wallet.mnemonic);
        assert_eq!(decrypted.created_at, wallet.created_at);
    }

    #[test]
    fn looks_up_evm_network_configs() {
        let ethereum = EVM_NETWORKS.iter().find(|c| c.id == "ethereum").unwrap();
        assert_eq!(ethereum.display_name, "Ethereum");
        assert_eq!(ethereum.chain_id, 1);
        assert_eq!(ethereum.native_symbol, "ETH");
        assert_eq!(ethereum.rpc_url, "https://ethereum-rpc.publicnode.com");

        let avalanche = EVM_NETWORKS.iter().find(|c| c.id == "avalanche_c").unwrap();
        assert_eq!(avalanche.chain_id, 43114);
        assert_eq!(avalanche.native_symbol, "AVAX");
    }
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let storage_path = app
                .path()
                .app_data_dir()
                .map_err(|error| format!("failed to resolve app data directory: {error}"))?
                .join("wallet.json");
            app.manage(Mutex::new(AppState::from_storage(storage_path)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_wallet,
            refresh_prices,
            create_wallet,
            import_wallet,
            unlock_wallet,
            lock_wallet,
            clear_wallet,
            sign_transaction,
            send_transaction,
            swap_tokens,
            check_transaction_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running VaultForge Wallet");
}
