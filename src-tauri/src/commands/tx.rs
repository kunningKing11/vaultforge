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
    EVM_NETWORKS, broadcast_evm_transaction, evm_config_by_id, evm_config_for_symbol,
    evm_tokens_for_network, fetch_evm_estimate_gas, fetch_evm_gas_price, fetch_evm_nonce,
    fetch_tx_status,
};
use crate::state::{AppState, session_from_state, validate_unlocked};
use crate::storage::persist_state_wallet;
use crate::tx::evm::{Eip1559TxDraft, encode_erc20_transfer, sign_eip1559_transfer};
use crate::validation::validate_transfer;

#[tauri::command]
pub(crate) async fn sign_transaction(
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

    let asset = assets
        .iter()
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
        let token = evm_tokens_for_network(config.id)
            .iter()
            .find(|t| t.symbol == symbol)
            .ok_or_else(|| format!("Token {symbol} not found on {network_id}"))?;
        (
            token.contract.to_string(),
            encode_erc20_transfer(&to, value_wei)?,
            to.clone(),
        )
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
    let (_, tx_hash, raw_tx_hex, r_hex, s_hex) = sign_eip1559_transfer(&Eip1559TxDraft {
        signing_key: &signing_key,
        chain_id: config.chain_id,
        nonce,
        max_priority_fee_per_gas,
        max_fee_per_gas,
        gas_limit,
        to: &tx_to,
        value: if tx_data.is_empty() { value_wei } else { 0 },
        data: &tx_data,
    })?;

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

    Ok(SignedTransaction {
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
    })
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
    if network == "bitcoin" {
        return fetch_bitcoin_tx_status(&tx_hash).await;
    }

    let config = EVM_NETWORKS
        .iter()
        .find(|c| c.id == network)
        .ok_or_else(|| format!("Unknown network: {}", network))?;
    fetch_tx_status(config, &tx_hash).await
}
