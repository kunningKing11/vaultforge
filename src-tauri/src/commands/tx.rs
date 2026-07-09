use chrono::Utc;
use std::sync::Mutex;
use tauri::State;

use crate::activity::{activity, random_hex, short_address};
use crate::derivation::signing_key_from_mnemonic;
use crate::dto::{Activity, SignedTransaction, WalletSession};
use crate::providers::bitcoin::{
    broadcast_bitcoin_transaction, fetch_bitcoin_tx_status, sign_bitcoin_transfer,
};
use crate::providers::evm::{
    EVM_NETWORKS, broadcast_evm_tx, evm_config_by_id, evm_tokens_for_network,
    fetch_evm_estimated_gas, fetch_evm_gas_price, fetch_evm_nonce, fetch_evm_tx_status,
};
use crate::providers::solana::{broadcast_solana_transaction, fetch_solana_tx_status};
use crate::state::{AppState, session_from_state, validate_unlocked};
use crate::storage::persist_state_wallet;
use crate::tx::evm::{Eip1559TxDraft, encode_erc20_transfer, sign_eip1559_transfer};
use crate::tx::solana::{sign_solana_token_transfer, sign_solana_transfer};
use crate::validation::validate_transfer;

pub(crate) fn required_native_debit(
    is_native_transfer: bool,
    amount: u128,
    fee: u128,
    native_symbol: &str,
) -> Result<u128, String> {
    if is_native_transfer {
        amount
            .checked_add(fee)
            .ok_or_else(|| format!("{native_symbol} total debit is too large"))
    } else {
        Ok(fee)
    }
}

pub(crate) fn ensure_native_balance_covers_debit(
    balance: u128,
    required: u128,
    native_symbol: &str,
    is_native_transfer: bool,
    fee_context: &str,
) -> Result<(), String> {
    if balance >= required {
        return Ok(());
    }

    if is_native_transfer {
        Err(format!(
            "Insufficient {native_symbol} balance for amount plus fee"
        ))
    } else {
        Err(format!(
            "Insufficient {native_symbol} balance for {fee_context}"
        ))
    }
}

#[tauri::command]
pub(crate) async fn sign_transaction(
    state: State<'_, Mutex<AppState>>,
    to: String,
    symbol: String,
    network: String,
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
        validate_transfer(wallet, &to, &symbol, &network, &amount)?;
        (
            wallet.mnemonic.clone(),
            wallet.address.clone(),
            wallet.addresses.clone(),
            wallet.assets.clone(),
        )
    };

    let to = to.trim().to_string();
    let value: u128 = amount.parse().map_err(|_| "Invalid amount".to_string())?;

    let asset = assets
        .iter()
        .find(|a| a.symbol == symbol && a.network == network)
        .ok_or_else(|| format!("Asset {symbol} on {network} not found in wallet"))?;
    let network_id = asset.network.as_str();
    let decimals = asset.decimals;

    match network_id {
        "bitcoin" if symbol == "BTC" => {
            // Bitcoin signing path
            let from = addresses
                .get("bitcoin")
                .ok_or_else(|| "Wallet BTC address is not available".to_string())?
                .clone();
            let amount_sats: u64 = value
                .try_into()
                .map_err(|_| "BTC amount is too large".to_string())?;
            let signed_btc = sign_bitcoin_transfer(&mnemonic, &from, &to, amount_sats).await?;

            Ok(SignedTransaction {
                from,
                to,
                symbol: symbol.clone(),
                amount: value.to_string(),
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
                fiat_value: (value as f64) * asset.price_usd,
                raw_tx: Some(signed_btc.raw_tx_hex),
                tx_hash: Some(signed_btc.txid),
            })
        }
        "solana" => {
            let from = addresses
                .get("solana")
                .ok_or_else(|| "Wallet Solana address is not available".to_string())?
                .clone();
            let amount_u64: u64 = value
                .try_into()
                .map_err(|_| format!("{symbol} amount is too large"))?;
            let sol_balance: u128 = assets
                .iter()
                .find(|a| a.network == "solana" && a.symbol == "SOL")
                .and_then(|a| a.balance.parse().ok())
                .unwrap_or(0);

            let signed_sol = if symbol == "SOL" {
                sign_solana_transfer(&mnemonic, &from, &to, amount_u64).await?
            } else {
                let decimals_u8: u8 = decimals
                    .try_into()
                    .map_err(|_| "SPL token decimals are too large".to_string())?;
                sign_solana_token_transfer(
                    &mnemonic,
                    &from,
                    &to,
                    &asset.name,
                    amount_u64,
                    decimals_u8,
                )
                .await?
            };

            let fee_lamports = signed_sol.fee_lamports as u128;
            let required_sol = required_native_debit(symbol == "SOL", value, fee_lamports, "SOL")?;
            ensure_native_balance_covers_debit(
                sol_balance,
                required_sol,
                "SOL",
                symbol == "SOL",
                "Solana transaction fee",
            )?;

            let total_debit = if symbol == "SOL" {
                required_sol.to_string()
            } else {
                value.to_string()
            };

            let post_balance = if symbol == "SOL" {
                sol_balance.saturating_sub(required_sol).to_string()
            } else {
                let balance: u128 = asset.balance.parse().unwrap_or(0);
                balance.saturating_sub(value).to_string()
            };

            Ok(SignedTransaction {
                from,
                to,
                symbol: symbol.clone(),
                amount: value.to_string(),
                note: note.trim().to_string(),
                network: "solana".to_string(),
                nonce: signed_sol.recent_blockhash,
                signed_at: Utc::now().to_rfc3339(),
                payload_hash: signed_sol.signature.clone(),
                signature: signed_sol.signature.clone(),
                fee_amount: signed_sol.fee_lamports.to_string(),
                fee_symbol: "SOL".to_string(),
                total_debit,
                post_balance,
                decimals,
                fiat_value: (value as f64) * asset.price_usd,
                raw_tx: Some(signed_sol.raw_tx_base64),
                tx_hash: Some(signed_sol.signature),
            })
        }
        network_id if evm_config_by_id(network_id).is_some() => {
            let config = evm_config_by_id(network_id)
                .ok_or_else(|| format!("No EVM chain configured for network {network_id}"))?;

            let is_native = symbol == config.native_symbol;

            let (tx_to, tx_data, display_to) = if is_native {
                (to.clone(), Vec::new(), to.clone())
            } else {
                let token = evm_tokens_for_network(config.id)
                    .iter()
                    .find(|t| t.symbol == symbol)
                    .ok_or_else(|| format!("Token {symbol} not found on {network_id}"))?;
                (
                    token.contract.to_string(),
                    encode_erc20_transfer(&to, value)?,
                    to.clone(),
                )
            };

            let nonce = fetch_evm_nonce(config, &address).await?;
            let gas_price = fetch_evm_gas_price(config).await?;
            let gas_limit = if tx_data.is_empty() {
                fetch_evm_estimated_gas(config, &address, &tx_to, value, &[]).await?
            } else {
                fetch_evm_estimated_gas(config, &address, &tx_to, 0, &tx_data).await?
            };

            let max_priority_fee_per_gas = gas_price;
            let max_fee_per_gas = gas_price;
            let total_fee_wei: u128 = gas_limit as u128 * max_fee_per_gas as u128;

            let native_asset = assets
                .iter()
                .find(|a| a.network == config.id && a.symbol == config.native_symbol)
                .ok_or_else(|| format!("{} balance is not available", config.native_symbol))?;

            let native_balance: u128 = native_asset
                .balance
                .parse()
                .map_err(|_| format!("Invalid {} balance", config.native_symbol))?;

            let required_native =
                required_native_debit(is_native, value, total_fee_wei, config.native_symbol)?;
            ensure_native_balance_covers_debit(
                native_balance,
                required_native,
                config.native_symbol,
                is_native,
                "transaction fee",
            )?;

            let signing_key = signing_key_from_mnemonic(&mnemonic)?;
            let (_, tx_hash, raw_tx_hex, r_hex, s_hex) = sign_eip1559_transfer(&Eip1559TxDraft {
                signing_key: &signing_key,
                chain_id: config.chain_id,
                nonce,
                max_priority_fee_per_gas,
                max_fee_per_gas,
                gas_limit,
                to: &tx_to,
                value: if tx_data.is_empty() { value } else { 0 },
                data: &tx_data,
            })?;

            let fee_str = total_fee_wei.to_string();
            let total_debit_str = if is_native {
                required_native.to_string()
            } else {
                total_fee_wei.to_string()
            };
            let signature_str = format!("0x{}{}", r_hex, s_hex);

            let post_balance = if is_native {
                native_balance.saturating_sub(required_native).to_string()
            } else {
                let token_balance: u128 = asset.balance.parse().unwrap_or(0);
                token_balance.saturating_sub(value).to_string()
            };

            Ok(SignedTransaction {
                from: address,
                to: display_to,
                symbol: symbol.clone(),
                amount: value.to_string(),
                note: note.trim().to_string(),
                network: config.id.to_string(),
                nonce: nonce.to_string(),
                signed_at: Utc::now().to_rfc3339(),
                payload_hash: tx_hash.clone(),
                signature: signature_str,
                fee_amount: fee_str,
                fee_symbol: config.native_symbol.to_string(),
                total_debit: total_debit_str,
                post_balance,
                decimals,
                fiat_value: (value as f64) * asset.price_usd,
                raw_tx: Some(raw_tx_hex),
                tx_hash: Some(tx_hash),
            })
        }
        unsupported_network_id => Err(format!(
            "{} transfers on {} are not implemented yet",
            symbol, unsupported_network_id
        )),
    }
}

#[tauri::command]
pub(crate) async fn send_transaction(
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
        if signed.from != wallet.address
            && !wallet
                .addresses
                .values()
                .any(|address| address == &signed.from)
        {
            return Err("Signed transaction does not match this wallet".to_string());
        }
    }

    let raw_tx = signed
        .raw_tx
        .as_ref()
        .ok_or_else(|| "No raw transaction data".to_string())?;

    let tx_hash = match signed.network.as_str() {
        "bitcoin" if signed.symbol == "BTC" => broadcast_bitcoin_transaction(raw_tx).await?,
        "solana" => broadcast_solana_transaction(raw_tx).await?,
        network_id if evm_config_by_id(network_id).is_some() => {
            let config = evm_config_by_id(network_id)
                .ok_or_else(|| format!("No EVM chain configured for {network_id}"))?;
            broadcast_evm_tx(config, raw_tx).await?
        }
        network_id => return Err(format!("Unsupported network: {network_id}")),
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
    Ok(session_from_state(&state))
}

#[tauri::command(rename_all = "camelCase")]
pub(crate) fn swap_tokens(
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
pub(crate) async fn check_transaction_status(
    _state: State<'_, Mutex<AppState>>,
    tx_hash: String,
    network: String,
) -> Result<Option<String>, String> {
    match network.as_str() {
        "bitcoin" => fetch_bitcoin_tx_status(&tx_hash).await,
        "solana" => fetch_solana_tx_status(&tx_hash).await,
        network_id if evm_config_by_id(network_id).is_some() => {
            let config = EVM_NETWORKS
                .iter()
                .find(|c| c.id == network_id)
                .ok_or_else(|| format!("Unknown network: {}", network))?;
            fetch_evm_tx_status(config, &tx_hash).await
        }
        _ => Err(format!("Unsupported network: {}", network)),
    }
}
