use chrono::Utc;
use std::sync::Mutex;
use tauri::State;

use crate::activity::{random_hex, short_address};
use crate::assets::asset_matches;
use crate::derivation::signing_key_from_mnemonic;
use crate::derivation::xrpl_private_key_from_mnemonic;
use crate::dto::{Activity, AssetIdentity, SignedTransaction, WalletSession};
use crate::operations::{
    Operation, OperationKind, OperationStep, TransactionAttempt, TransactionStatus,
    TransactionStep, create_send_operation, operation_step_network,
};
use crate::providers::bitcoin::{broadcast_bitcoin_transaction, sign_bitcoin_transfer};
use crate::providers::evm::{
    broadcast_evm_tx, estimate_evm_call, evm_config_by_id, fetch_evm_native_balance,
    fetch_evm_nonce, fetch_evm_token_balance_for_contract,
};
use crate::providers::http::ProviderClients;
use crate::providers::solana::{
    broadcast_solana_transaction, fetch_solana_mint_decimals, fetch_solana_native_balance,
    fetch_solana_token_account_rent, fetch_solana_token_account_state, fetch_solana_token_accounts,
    simulate_solana_transaction,
};
use crate::providers::tron::broadcast_tron_transaction;
use crate::providers::xrpl::{
    broadcast_xrpl_transaction, fetch_xrpl_account_info, fetch_xrpl_network_info,
};
use crate::state::{AppState, session_from_state, validate_unlocked};
use crate::storage::persist_state_wallet;
use crate::tx::evm::{
    Eip1559TxDraft, build_erc20_transfer_call, build_native_transfer_call, sign_eip1559_transfer,
};
use crate::tx::solana::{
    select_solana_token_sources, sign_solana_token_transfer, sign_solana_transfer,
    solana_associated_token_address,
};
use crate::tx::tron::sign_tron_transfer;
use crate::tx::tx_status::{Execution, Finality, TxStatus};
use crate::tx::xrpl::sign_xrpl_payment;
use crate::validation::validate_transfer;
use zeroize::{Zeroize, Zeroizing};

struct EvmNonceReservationGuard<'a> {
    state: &'a Mutex<AppState>,
    chain_id: u64,
    address: String,
    nonce: u64,
    armed: bool,
}

impl<'a> EvmNonceReservationGuard<'a> {
    fn new(state: &'a Mutex<AppState>, chain_id: u64, address: String, nonce: u64) -> Self {
        Self {
            state,
            chain_id,
            address,
            nonce,
            armed: true,
        }
    }

    fn keep(&mut self) {
        self.armed = false;
    }
}

impl Drop for EvmNonceReservationGuard<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if let Ok(mut state) = self.state.lock() {
            state
                .evm_nonces
                .cancel_unbroadcast(self.chain_id, &self.address, self.nonce);
        }
    }
}

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

pub(crate) fn ensure_token_balance_covers_amount(
    balance: u128,
    amount: u128,
    symbol: &str,
) -> Result<(), String> {
    if balance >= amount {
        Ok(())
    } else {
        Err(format!("Insufficient {symbol} balance"))
    }
}

/// An unresolved send is a signed or broadcast attempt without finalized
/// evidence; it must be reconciled before another send can be authorized.
fn is_unresolved_send(operation: &Operation, network: &str, from: &str) -> bool {
    (operation.kind == OperationKind::Send
        || (network == "solana" && operation.kind == OperationKind::Swap))
        && !crate::uniswap::local_swap_confirmed(operation)
        && operation.source_asset.network == network
        && (if network == "solana" {
            operation.sender == from
        } else {
            operation.sender.eq_ignore_ascii_case(from)
        })
        && !matches!(
            operation.step,
            OperationStep::Completed | OperationStep::Cancelled
        )
        && operation.attempts.iter().any(|attempt| {
            attempt.status != TransactionStatus::Failed
                && !attempt
                    .tx_status
                    .as_ref()
                    .is_some_and(|status| status.finality == Finality::Finalized)
        })
}

/// Creates the durable send record and persists its signed attempt before the
/// first broadcast, returning the operation id and the bytes to broadcast.
/// Bitcoin native sends are not recorded here yet; unsupported routes are also
/// left to the broadcast match to reject without leaving a stuck record.
fn prepare_durable_send(
    state: &Mutex<AppState>,
    signed: &SignedTransaction,
) -> Result<Option<(String, String)>, String> {
    let supported = evm_config_by_id(&signed.network).is_some()
        || signed.network == "solana"
        || (signed.token_address.is_none() && matches!(signed.network.as_str(), "tron" | "xrpl"));
    if !supported {
        return Ok(None);
    }
    let raw_tx = signed
        .raw_tx
        .clone()
        .ok_or_else(|| "Signed transaction is missing raw bytes".to_string())?;
    let tx_hash = signed
        .tx_hash
        .clone()
        .ok_or_else(|| "Signed transaction is missing a hash".to_string())?;
    // Retrying an already recorded hash must reuse its durable attempt instead
    // of creating a second record for the same transaction.
    if let Some((operation_id, stored)) = find_durable_send(state, &signed.network, &tx_hash)? {
        return Ok(Some((operation_id, stored.unwrap_or(raw_tx))));
    }
    let mut guard = state.lock().map_err(|_| "State lock failed")?;
    if guard.locked {
        return Err("Wallet is locked".to_string());
    }
    let wallet = guard.wallet.as_mut().ok_or("No wallet exists yet")?;
    // Non-EVM chains have no account nonce to serialize sends, so a re-signed
    // transfer could double-spend if an earlier attempt already landed.
    let is_evm = crate::registry::network_by_id(&signed.network)
        .is_some_and(|network| network.kind == "evm");
    if !is_evm
        && wallet
            .operations
            .iter()
            .any(|operation| is_unresolved_send(operation, &signed.network, &signed.from))
    {
        return Err(format!(
            "An unresolved {} send is pending; reconcile it before broadcasting another",
            signed.network
        ));
    }
    let asset = AssetIdentity::new(&signed.network, signed.token_address.as_deref());
    let amount_base_units: u128 = signed
        .amount
        .parse()
        .map_err(|_| "Invalid signed amount".to_string())?;
    let mut operation = create_send_operation(
        asset,
        signed.from.clone(),
        signed.to.clone(),
        amount_base_units,
        signed.symbol.clone(),
        signed.note.trim().to_string(),
    );
    operation.attempts.push(TransactionAttempt {
        step: TransactionStep::Send,
        status: TransactionStatus::Signed,
        tx_hash: tx_hash.clone(),
        raw_tx: Some(raw_tx.clone()),
        signature: Some(signed.signature.clone()),
        fee: Some(format!("{} {}", signed.fee_amount, signed.fee_symbol)),
        nonce_or_sequence: Some(signed.nonce.clone()),
        error: None,
        updated_at: Utc::now().to_rfc3339(),
        tx_status: None,
        evm_context: None,
        solana_context: None,
    });
    let operation_id = operation.id.clone();
    wallet.operations.push(operation);
    if let Err(error) = persist_state_wallet(&mut guard) {
        guard
            .wallet
            .as_mut()
            .ok_or("Wallet closed during submission")?
            .operations
            .pop();
        return Err(error);
    }
    let operations = guard
        .wallet
        .as_ref()
        .ok_or("Wallet closed during submission")?
        .operations
        .clone();
    guard.evm_nonces.restore_send_operations(&operations);
    Ok(Some((operation_id, raw_tx)))
}

/// Finds the durable send operation bound to a submitted hash, returning its id
/// and the raw bytes that were persisted before broadcast.
fn find_durable_send(
    state: &Mutex<AppState>,
    network: &str,
    tx_hash: &str,
) -> Result<Option<(String, Option<String>)>, String> {
    let guard = state.lock().map_err(|_| "State lock failed")?;
    let Some(wallet) = guard.wallet.as_ref() else {
        return Ok(None);
    };
    Ok(wallet
        .operations
        .iter()
        .find(|operation| {
            operation.kind == OperationKind::Send
                && operation.source_asset.network == network
                && operation
                    .attempts
                    .iter()
                    .any(|attempt| attempt.tx_hash == tx_hash)
        })
        .map(|operation| {
            let stored = operation
                .attempts
                .iter()
                .rfind(|attempt| attempt.tx_hash == tx_hash)
                .and_then(|attempt| attempt.raw_tx.clone());
            (operation.id.clone(), stored)
        }))
}

/// Applies one observation/status transition to a durable send attempt and
/// persists it, restoring the previous record when the save fails.
fn update_send_attempt<F>(
    state: &Mutex<AppState>,
    operation_id: &str,
    tx_hash: &str,
    update: F,
) -> Result<(), String>
where
    F: FnOnce(&mut TransactionAttempt),
{
    let mut guard = state.lock().map_err(|_| "State lock failed")?;
    let (index, before) = {
        let wallet = guard.wallet.as_mut().ok_or("No wallet exists yet")?;
        let index = wallet
            .operations
            .iter()
            .position(|operation| operation.id == operation_id)
            .ok_or("Send operation disappeared")?;
        let before = wallet.operations[index].clone();
        {
            let attempt = wallet.operations[index]
                .attempts
                .iter_mut()
                .rfind(|attempt| attempt.tx_hash == tx_hash)
                .ok_or("Send attempt disappeared")?;
            update(attempt);
        }
        wallet.operations[index].updated_at = Utc::now().to_rfc3339();
        (index, before)
    };
    if let Err(error) = persist_state_wallet(&mut guard) {
        if let Some(wallet) = guard.wallet.as_mut() {
            wallet.operations[index] = before;
        }
        return Err(error);
    }
    let operations = guard
        .wallet
        .as_ref()
        .ok_or("Wallet closed during send")?
        .operations
        .clone();
    guard.evm_nonces.restore_send_operations(&operations);
    Ok(())
}

/// Marks the durable attempt for an explicitly cancelled send as failed so a
/// restart does not resurrect its nonce/sequence protection.
fn mark_send_cancelled(
    state: &mut AppState,
    network: &str,
    from: &str,
    nonce: &str,
) -> Result<bool, String> {
    let Some(index) = state.wallet.as_ref().and_then(|wallet| {
        wallet.operations.iter().position(|operation| {
            operation.kind == OperationKind::Send
                && operation.source_asset.network == network
                && operation.sender.eq_ignore_ascii_case(from)
                && operation.attempts.iter().any(|attempt| {
                    attempt.nonce_or_sequence.as_deref() == Some(nonce)
                        && attempt.status == TransactionStatus::Signed
                })
        })
    }) else {
        return Ok(false);
    };
    let previous = state.wallet.as_ref().ok_or("Wallet closed")?.operations[index].clone();
    {
        let wallet = state.wallet.as_mut().ok_or("Wallet closed")?;
        let operation = &mut wallet.operations[index];
        for attempt in operation.attempts.iter_mut() {
            if attempt.nonce_or_sequence.as_deref() == Some(nonce)
                && attempt.status == TransactionStatus::Signed
            {
                attempt.status = TransactionStatus::Failed;
                attempt.error = Some("Cancelled before broadcast".to_string());
                attempt.updated_at = Utc::now().to_rfc3339();
            }
        }
        operation.updated_at = Utc::now().to_rfc3339();
    }
    if let Err(error) = persist_state_wallet(state) {
        state.wallet.as_mut().ok_or("Wallet closed")?.operations[index] = previous;
        return Err(error);
    }
    let operations = state
        .wallet
        .as_ref()
        .ok_or("Wallet closed")?
        .operations
        .clone();
    state.evm_nonces.restore_send_operations(&operations);
    Ok(true)
}

#[tauri::command]
#[allow(
    clippy::too_many_arguments,
    reason = "Tauri exposes these fields as the existing flat frontend command contract"
)]
pub(crate) async fn sign_transaction(
    state: State<'_, Mutex<AppState>>,
    clients: State<'_, ProviderClients>,
    to: String,
    symbol: String,
    network: String,
    token_address: Option<String>,
    amount: String,
    note: String,
    destination_tag: Option<u32>,
) -> Result<SignedTransaction, String> {
    validate_unlocked(&state)?;

    let (mnemonic, bip39_passphrase, addresses, assets, bitcoin_account) = {
        let state = state.lock().map_err(|_| "State lock failed")?;
        let wallet = state
            .wallet
            .as_ref()
            .ok_or_else(|| "No wallet exists yet".to_string())?;
        validate_transfer(
            wallet,
            &to,
            &symbol,
            &network,
            token_address.as_deref(),
            &amount,
        )?;
        (
            Zeroizing::new(wallet.mnemonic.clone()),
            Zeroizing::new(wallet.bip39_passphrase.clone()),
            wallet.addresses.clone(),
            wallet.assets.clone(),
            state.bitcoin_account.clone(),
        )
    };

    let to = to.trim().to_string();
    let value: u128 = amount.parse().map_err(|_| "Invalid amount".to_string())?;

    let client = clients.http();

    let asset = assets
        .iter()
        .find(|asset| asset_matches(asset, &network, token_address.as_deref()))
        .ok_or_else(|| format!("Asset {symbol} on {network} not found in wallet"))?;
    let network_id = asset.network.as_str();
    let symbol = asset.symbol.clone();
    let is_native = asset.token_address.is_none();
    let mut decimals = asset.decimals;

    if destination_tag.is_some() && network_id != "xrpl" {
        return Err("Destination tags are only supported for XRP Ledger payments".to_string());
    }

    match network_id {
        "bitcoin" if is_native => {
            let from = addresses
                .get("bitcoin")
                .ok_or_else(|| "Wallet BTC address is not available".to_string())?
                .clone();
            let amount_sats: u64 = value
                .try_into()
                .map_err(|_| "BTC amount is too large".to_string())?;
            let bitcoin_account = bitcoin_account
                .as_ref()
                .ok_or_else(|| "Bitcoin account discovery is unavailable".to_string())?;
            let signed_btc = sign_bitcoin_transfer(
                client,
                &mnemonic,
                &bip39_passphrase,
                &from,
                &to,
                amount_sats,
                bitcoin_account,
            )
            .await?;

            Ok(SignedTransaction {
                token_address: asset.token_address.clone(),
                from,
                to,
                symbol: symbol.clone(),
                amount: value.to_string(),
                note: note.trim().to_string(),
                destination_tag: None,
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
        network_id if evm_config_by_id(network_id).is_some() => {
            let config = evm_config_by_id(network_id)
                .ok_or_else(|| format!("No EVM chain configured for network {network_id}"))?;

            let native_symbol = config.native_asset.symbol.as_str();
            let address = addresses
                .get("evm")
                .cloned()
                .ok_or_else(|| "Wallet EVM address is not available".to_string())?;

            let call = if is_native {
                build_native_transfer_call(&to, value)?
            } else {
                let token_address = asset
                    .token_address
                    .as_deref()
                    .ok_or_else(|| format!("{symbol} token address is not available"))?;
                build_erc20_transfer_call(token_address, &to, value)?
            };

            let native_balance = fetch_evm_native_balance(client, config, &address).await?;
            let token_balance = if is_native {
                None
            } else {
                let token_address = asset
                    .token_address
                    .as_deref()
                    .ok_or_else(|| format!("{symbol} token address is not available"))?;
                Some(
                    fetch_evm_token_balance_for_contract(client, config, token_address, &address)
                        .await?,
                )
            };
            if let Some(token_balance) = token_balance {
                ensure_token_balance_covers_amount(token_balance, value, &symbol)?;
            }

            let rpc_pending_nonce = fetch_evm_nonce(client, config, &address).await?;
            let estimate = estimate_evm_call(client, config, &address, &call).await?;
            let gas_limit = estimate.gas_limit;
            let max_priority_fee_per_gas = estimate.fees.max_priority_fee_per_gas;
            let max_fee_per_gas = estimate.fees.max_fee_per_gas;
            let total_fee_wei = estimate.max_native_fee;

            let required_native =
                required_native_debit(is_native, value, total_fee_wei, native_symbol)?;
            ensure_native_balance_covers_debit(
                native_balance,
                required_native,
                native_symbol,
                is_native,
                "transaction fee",
            )?;
            let chain_id = config.chain_id()?;
            let signing_key = signing_key_from_mnemonic(&mnemonic, &bip39_passphrase)?;
            let nonce = {
                let mut app_state = state.lock().map_err(|_| "State lock failed")?;
                let operations = &app_state
                    .wallet
                    .as_ref()
                    .ok_or("Wallet closed during signing")?
                    .operations
                    .clone();
                app_state.evm_nonces.restore_operations(operations)?;
                app_state.evm_nonces.restore_send_operations(operations);
                app_state
                    .evm_nonces
                    .reserve(chain_id, &address, rpc_pending_nonce)?
            };
            let mut nonce_reservation =
                EvmNonceReservationGuard::new(state.inner(), chain_id, address.clone(), nonce);
            let (_, tx_hash, raw_tx_hex, r_hex, s_hex) = sign_eip1559_transfer(&Eip1559TxDraft {
                signing_key: &signing_key,
                chain_id,
                nonce,
                max_priority_fee_per_gas,
                max_fee_per_gas,
                gas_limit,
                to: &call.to,
                value: call.value,
                data: &call.data,
            })?;

            let fee_str = total_fee_wei.to_string();
            let total_debit_str = if is_native {
                required_native.to_string()
            } else {
                total_fee_wei.to_string()
            };
            let signature_str = format!("0x{}{}", r_hex, s_hex);

            let post_balance = match token_balance {
                None => native_balance
                    .checked_sub(required_native)
                    .ok_or_else(|| format!("Insufficient {native_symbol} balance"))?,
                Some(token_balance) => token_balance
                    .checked_sub(value)
                    .ok_or_else(|| format!("Insufficient {symbol} balance"))?,
            }
            .to_string();

            let signed = SignedTransaction {
                token_address: asset.token_address.clone(),
                from: address,
                to,
                symbol: symbol.clone(),
                amount: value.to_string(),
                note: note.trim().to_string(),
                destination_tag: None,
                network: config.id.to_string(),
                nonce: nonce.to_string(),
                signed_at: Utc::now().to_rfc3339(),
                payload_hash: tx_hash.clone(),
                signature: signature_str,
                fee_amount: fee_str,
                fee_symbol: native_symbol.to_string(),
                total_debit: total_debit_str,
                post_balance,
                decimals,
                fiat_value: (value as f64) * asset.price_usd,
                raw_tx: Some(raw_tx_hex),
                tx_hash: Some(tx_hash),
            };
            nonce_reservation.keep();
            Ok(signed)
        }
        "solana" => {
            let from = addresses
                .get("solana")
                .ok_or_else(|| "Wallet Solana address is not available".to_string())?
                .clone();
            let amount_u64: u64 = value
                .try_into()
                .map_err(|_| format!("{symbol} amount is too large"))?;
            let sol_balance = fetch_solana_native_balance(client, &from).await?;
            let mut extra_sol_lamports = 0u64;
            let mut source_token_balance = None;

            let signed_sol = if is_native {
                sign_solana_transfer(client, &mnemonic, &bip39_passphrase, &from, &to, amount_u64)
                    .await?
            } else {
                let mint = asset
                    .token_address
                    .as_deref()
                    .ok_or_else(|| format!("{symbol} mint address is not available"))?;
                let mint_decimals = fetch_solana_mint_decimals(client, mint).await?;
                decimals = u32::from(mint_decimals);

                let source_accounts = fetch_solana_token_accounts(client, &from).await?;
                let (sources, total_balance) = select_solana_token_sources(
                    &from,
                    mint,
                    mint_decimals,
                    amount_u64,
                    &source_accounts,
                )?;
                source_token_balance = Some(total_balance);

                let destination_ata = solana_associated_token_address(&to, mint)?;
                let ata_exists =
                    fetch_solana_token_account_state(client, &destination_ata, &to, mint)
                        .await?
                        .is_some();
                if !ata_exists {
                    extra_sol_lamports = fetch_solana_token_account_rent(client).await?;
                }

                sign_solana_token_transfer(
                    client,
                    &mnemonic,
                    &bip39_passphrase,
                    &from,
                    &to,
                    mint,
                    &sources,
                    mint_decimals,
                )
                .await?
            };

            let fee_lamports = signed_sol.fee_lamports as u128;
            let extra_sol_lamports = extra_sol_lamports as u128;
            let required_sol = if is_native {
                required_native_debit(true, value, fee_lamports, "SOL")?
            } else {
                fee_lamports
                    .checked_add(extra_sol_lamports)
                    .ok_or_else(|| "SOL total debit is too large".to_string())?
            };

            if sol_balance < required_sol {
                return Err(if is_native {
                    "Insufficient SOL balance for amount plus fee".to_string()
                } else if extra_sol_lamports > 0 {
                    "Insufficient SOL balance for Solana transaction fee and token account rent"
                        .to_string()
                } else {
                    "Insufficient SOL balance for Solana transaction fee".to_string()
                });
            }

            simulate_solana_transaction(client, &signed_sol.raw_tx_base64).await?;

            let total_debit = if is_native {
                required_sol.to_string()
            } else {
                value.to_string()
            };

            let post_balance = if is_native {
                sol_balance.saturating_sub(required_sol).to_string()
            } else {
                let balance = source_token_balance
                    .ok_or_else(|| "Solana token balance was not preflighted".to_string())?;
                balance.saturating_sub(value).to_string()
            };

            let signed = SignedTransaction {
                token_address: asset.token_address.clone(),
                from,
                to,
                symbol: symbol.clone(),
                amount: value.to_string(),
                note: note.trim().to_string(),
                destination_tag: None,
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
            };
            Ok(signed)
        }
        "tron" if is_native => {
            let from = addresses
                .get("tron")
                .ok_or_else(|| "Wallet Tron address is not available".to_string())?
                .clone();
            let amount_sun: u64 = value
                .try_into()
                .map_err(|_| "TRX amount is too large".to_string())?;
            let trx_balance: u128 = asset
                .balance
                .parse()
                .map_err(|_| "Invalid TRX balance".to_string())?;
            let signed_trx =
                sign_tron_transfer(client, &mnemonic, &bip39_passphrase, &from, &to, amount_sun)
                    .await?;
            let fee_sun = signed_trx.fee_sun as u128;
            let required_trx = required_native_debit(true, value, fee_sun, "TRX")?;
            ensure_native_balance_covers_debit(
                trx_balance,
                required_trx,
                "TRX",
                true,
                "Tron transaction fee",
            )?;
            let raw_tx = serde_json::to_string(&signed_trx.raw_tx)
                .map_err(|_| "Failed to serialize signed Tron transaction".to_string())?;

            let signed = SignedTransaction {
                token_address: asset.token_address.clone(),
                from,
                to,
                symbol: symbol.clone(),
                amount: value.to_string(),
                note: note.trim().to_string(),
                destination_tag: None,
                network: "tron".to_string(),
                nonce: "resource".to_string(),
                signed_at: Utc::now().to_rfc3339(),
                payload_hash: signed_trx.txid.clone(),
                signature: signed_trx.signature,
                fee_amount: signed_trx.fee_sun.to_string(),
                fee_symbol: "TRX".to_string(),
                total_debit: required_trx.to_string(),
                post_balance: trx_balance.saturating_sub(required_trx).to_string(),
                decimals,
                fiat_value: (value as f64) * asset.price_usd,
                raw_tx: Some(raw_tx),
                tx_hash: Some(signed_trx.txid),
            };
            Ok(signed)
        }
        "xrpl" if is_native => {
            let from = addresses
                .get("xrpl")
                .ok_or_else(|| "Wallet XRP Ledger address is not available".to_string())?
                .clone();
            let amount_drops: u64 = value
                .try_into()
                .map_err(|_| "XRP amount is too large".to_string())?;
            let account = fetch_xrpl_account_info(client, &from, "current")
                .await?
                .ok_or_else(|| "The XRP Ledger account is not funded yet".to_string())?;
            let network_info = fetch_xrpl_network_info(client).await?;
            let recipient_exists = fetch_xrpl_account_info(client, &to, "validated")
                .await?
                .is_some();
            if !recipient_exists && amount_drops < network_info.base_reserve_drops {
                return Err(format!(
                    "A new XRP Ledger account must receive at least {} drops",
                    network_info.base_reserve_drops
                ));
            }

            let mut private_key = xrpl_private_key_from_mnemonic(&mnemonic, &bip39_passphrase)?;
            let signed = sign_xrpl_payment(
                &private_key,
                &from,
                &to,
                amount_drops,
                destination_tag,
                &note,
                &account,
                &network_info,
            );
            private_key.zeroize();
            let signed = signed?;

            let signed_tx = SignedTransaction {
                token_address: asset.token_address.clone(),
                from,
                to,
                symbol: symbol.clone(),
                amount: value.to_string(),
                note: note.trim().to_string(),
                destination_tag,
                network: "xrpl".to_string(),
                nonce: signed.sequence.to_string(),
                signed_at: Utc::now().to_rfc3339(),
                payload_hash: signed.tx_hash.clone(),
                signature: signed.signature,
                fee_amount: signed.fee_drops.to_string(),
                fee_symbol: "XRP".to_string(),
                total_debit: amount_drops
                    .checked_add(signed.fee_drops)
                    .ok_or_else(|| "XRP amount plus fee is too large".to_string())?
                    .to_string(),
                post_balance: signed.post_balance_drops.to_string(),
                decimals,
                fiat_value: (value as f64) * asset.price_usd,
                raw_tx: Some(signed.raw_tx_hex),
                tx_hash: Some(signed.tx_hash),
            };
            Ok(signed_tx)
        }
        unsupported_network_id => Err(format!(
            "{} transfers on {} are not implemented yet",
            symbol, unsupported_network_id
        )),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub(crate) fn cancel_signed_transaction(
    state: State<'_, Mutex<AppState>>,
    network: String,
    from: String,
    nonce: String,
) -> Result<(), String> {
    let mut state = state.lock().map_err(|_| "State lock failed")?;
    if let Some(config) = evm_config_by_id(&network) {
        let chain_id = config.chain_id()?;
        let parsed = nonce
            .parse::<u64>()
            .map_err(|_| "Invalid EVM nonce in canceled transaction".to_string())?;
        if !state.evm_nonces.cancel_unbroadcast(chain_id, &from, parsed) {
            return Err("EVM transaction is no longer awaiting broadcast".to_string());
        }
        mark_send_cancelled(&mut state, &network, &from, &nonce)?;
        return Ok(());
    }
    // Non-EVM sends are recorded only once submission is authorized, so there
    // is nothing to release here yet; treat cancellation as a no-op.
    mark_send_cancelled(&mut state, &network, &from, &nonce)?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn send_transaction(
    state: State<'_, Mutex<AppState>>,
    clients: State<'_, ProviderClients>,
    signed: SignedTransaction,
) -> Result<WalletSession, String> {
    validate_unlocked(&state)?;

    {
        let state = state.lock().map_err(|_| "State lock failed")?;
        let wallet = state
            .wallet
            .as_ref()
            .ok_or_else(|| "No wallet exists yet".to_string())?;
        if !wallet
            .addresses
            .values()
            .any(|address| address == &signed.from)
        {
            return Err("Signed transaction does not match this wallet".to_string());
        }
    }

    let expected_hash = signed
        .tx_hash
        .clone()
        .ok_or_else(|| "Signed transaction is missing a hash".to_string())?;
    let durable = prepare_durable_send(&state, &signed)?;
    let mut raw_tx = signed
        .raw_tx
        .clone()
        .ok_or_else(|| "No raw transaction data".to_string())?;
    if let Some((operation_id, stored)) = &durable {
        // The attempt is now durable; prefer its bytes and record submission
        // intent before any network request.
        raw_tx = stored.clone();
        update_send_attempt(&state, operation_id, &expected_hash, |attempt| {
            attempt.status = TransactionStatus::SubmissionUnknown;
            attempt.error = Some("Submission authorized; reconcile before retry".to_string());
        })?;
    }

    let client = clients.http();

    let broadcast = match signed.network.as_str() {
        "bitcoin" if signed.token_address.is_none() => {
            broadcast_bitcoin_transaction(client, &raw_tx).await
        }
        network_id if evm_config_by_id(network_id).is_some() => {
            let config = evm_config_by_id(network_id)
                .ok_or_else(|| format!("No EVM chain configured for {network_id}"))?;
            let chain_id = config.chain_id()?;
            let nonce = signed
                .nonce
                .parse::<u64>()
                .map_err(|_| "Invalid EVM nonce in signed transaction".to_string())?;
            {
                let mut state = state.lock().map_err(|_| "State lock failed")?;
                state
                    .evm_nonces
                    .begin_broadcast(chain_id, &signed.from, nonce)?;
            }
            match broadcast_evm_tx(client, config, &raw_tx).await {
                Ok(tx_hash) => {
                    let mut state = state.lock().map_err(|_| "State lock failed")?;
                    state
                        .evm_nonces
                        .mark_broadcast(chain_id, &signed.from, nonce)?;
                    Ok(tx_hash)
                }
                Err(error) => Err(error),
            }
        }
        "solana" => broadcast_solana_transaction(client, &raw_tx).await,
        "tron" if signed.token_address.is_none() => {
            let tx: serde_json::Value = serde_json::from_str(&raw_tx)
                .map_err(|_| "Invalid signed Tron transaction JSON".to_string())?;
            broadcast_tron_transaction(client, &tx).await
        }
        "xrpl" if signed.token_address.is_none() => {
            broadcast_xrpl_transaction(client, &raw_tx, &signed.payload_hash).await
        }
        network_id => return Err(format!("Unsupported network: {network_id}")),
    };

    let broadcast = if durable.is_some() {
        // Bind acceptance to the persisted hash so a mismatched provider
        // response never overwrites the attempt's expected transaction id.
        broadcast.and_then(|hash| {
            if hash.eq_ignore_ascii_case(&expected_hash) {
                Ok(hash)
            } else {
                Err(format!(
                    "Provider returned an unexpected transaction hash {hash}"
                ))
            }
        })
    } else {
        broadcast
    };

    if let Some((operation_id, _)) = &durable {
        let error = broadcast.as_ref().err().cloned();
        update_send_attempt(
            &state,
            operation_id,
            &expected_hash,
            |attempt| match &error {
                None => {
                    attempt.status = TransactionStatus::Submitted;
                    attempt.error = None;
                }
                Some(error) => {
                    attempt.status = TransactionStatus::SubmissionUnknown;
                    attempt.error = Some(error.clone());
                }
            },
        )?;
        if let Some(error) = error {
            return Err(format!(
                "Submission outcome is uncertain; saved hash {expected_hash}: {error}"
            ));
        }
        let state = state.lock().map_err(|_| "State lock failed")?;
        return Ok(session_from_state(&state));
    }

    let tx_hash = broadcast?;
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
                asset: Some(AssetIdentity::new(
                    &signed.network,
                    signed.token_address.as_deref(),
                )),
                counter_asset: None,
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
                tx_status: None,
            },
        );
    }
    persist_state_wallet(&mut state)?;
    Ok(session_from_state(&state))
}

/// Returns the unchanged session until swaps are implemented.
#[tauri::command(rename_all = "camelCase")]
pub(crate) fn swap_tokens(
    state: State<'_, Mutex<AppState>>,
    from_asset: AssetIdentity,
    to_asset: AssetIdentity,
    amount: String,
) -> Result<WalletSession, String> {
    let _ = (from_asset, to_asset, amount);
    let state = state.lock().map_err(|_| "State lock failed")?;
    Ok(session_from_state(&state))
}

#[tauri::command]
pub(crate) async fn check_transaction_status(
    state: State<'_, Mutex<AppState>>,
    clients: State<'_, ProviderClients>,
    tx_hash: String,
    network: String,
) -> Result<Option<TxStatus>, String> {
    reconcile_send_status_with(state.inner(), &tx_hash, &network, || {
        crate::providers::fetch_tx_observation(clients.http(), &network, &tx_hash)
    })
    .await
}

enum SendStatusTarget {
    Activity(String),
    Operation(String),
}

async fn reconcile_send_status_with<F, Fut>(
    state: &Mutex<AppState>,
    tx_hash: &str,
    network: &str,
    lookup: F,
) -> Result<Option<TxStatus>, String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<Option<TxStatus>, String>>,
{
    let (generation, target, expected_observation) = {
        let app_state = state.lock().map_err(|_| "State lock failed")?;

        if app_state.locked {
            return Err("Wallet is locked".to_string());
        }

        let wallet = app_state
            .wallet
            .as_ref()
            .ok_or_else(|| "No wallet exists yet".to_string())?;

        // Durable operations (sends and CCTP legs) are the source of truth; the
        // legacy activity cache remains readable for older sends.
        if let Some((operation, attempt)) = wallet.operations.iter().find_map(|operation| {
            operation
                .attempts
                .iter()
                .rfind(|attempt| {
                    attempt.tx_hash == tx_hash
                        && operation_step_network(operation, attempt.step) == network
                })
                .map(|attempt| (operation, attempt))
        }) {
            (
                app_state.wallet_generation,
                SendStatusTarget::Operation(operation.id.clone()),
                attempt.tx_status.clone(),
            )
        } else {
            let activity = wallet
                .activity
                .iter()
                .find(|item| {
                    item.kind == "send"
                        && item.hash == tx_hash
                        && item.network.as_deref() == Some(network)
                })
                .ok_or_else(|| "Transaction is not recorded in this wallet".to_string())?;
            (
                app_state.wallet_generation,
                SendStatusTarget::Activity(activity.id.clone()),
                activity.tx_status.clone(),
            )
        }
    };
    let observed_status = lookup().await?;

    let mut app_state = state.lock().map_err(|_| "State lock failed")?;
    commit_send_status(
        &mut app_state,
        generation,
        &target,
        tx_hash,
        network,
        observed_status,
        expected_observation,
    )
}

fn commit_send_status(
    app_state: &mut AppState,
    generation: u64,
    target: &SendStatusTarget,
    tx_hash: &str,
    network: &str,
    observed_status: Option<TxStatus>,
    expected_observation: Option<TxStatus>,
) -> Result<Option<TxStatus>, String> {
    if !app_state.can_commit_refresh(generation) {
        return Err("Wallet session changed during status lookup".to_string());
    }
    if let SendStatusTarget::Activity(activity_id) = target {
        let wallet = app_state
            .wallet
            .as_mut()
            .ok_or_else(|| "No wallet exists yet".to_string())?;
        let index = wallet
            .activity
            .iter()
            .position(|item| {
                item.id == *activity_id
                    && item.kind == "send"
                    && item.hash == tx_hash
                    && item.network.as_deref() == Some(network)
            })
            .ok_or_else(|| "Transaction changed during status lookup".to_string())?;
        let previous_status = wallet.activity[index].status.clone();
        let previous_observation = wallet.activity[index].tx_status.clone();
        if previous_observation
            .as_ref()
            .is_some_and(|status| status.finality == Finality::Finalized)
        {
            return Ok(previous_observation);
        }
        if previous_observation != expected_observation {
            return Ok(previous_observation);
        }
        let label = observed_status
            .as_ref()
            .map(|status| status.legacy_label().to_string())
            .unwrap_or_else(|| "pending".to_string());
        if previous_observation == observed_status && label == previous_status {
            return Ok(observed_status);
        }
        wallet.activity[index].status = label;
        wallet.activity[index].tx_status = observed_status.clone();
        if let Err(error) = persist_state_wallet(app_state) {
            // Keep failed saves retryable instead of treating the in-memory result as durable.
            if let Some(wallet) = app_state.wallet.as_mut() {
                wallet.activity[index].status = previous_status;
                wallet.activity[index].tx_status = previous_observation;
            }
            return Err(error);
        }
        return Ok(observed_status);
    }

    let SendStatusTarget::Operation(operation_id) = target else {
        return Err("Unknown send status target".to_string());
    };
    let wallet = app_state
        .wallet
        .as_mut()
        .ok_or_else(|| "No wallet exists yet".to_string())?;
    let op_index = wallet
        .operations
        .iter()
        .position(|operation| operation.id == *operation_id)
        .ok_or_else(|| "Transaction changed during status lookup".to_string())?;
    let attempt_index = wallet.operations[op_index]
        .attempts
        .iter()
        .rposition(|attempt| attempt.tx_hash == tx_hash)
        .ok_or_else(|| "Transaction changed during status lookup".to_string())?;
    let previous_observation = wallet.operations[op_index].attempts[attempt_index]
        .tx_status
        .clone();
    if previous_observation
        .as_ref()
        .is_some_and(|status| status.finality == Finality::Finalized)
    {
        return Ok(previous_observation);
    }
    if previous_observation != expected_observation {
        return Ok(previous_observation);
    }
    if previous_observation == observed_status {
        return Ok(observed_status);
    }
    let status = observed_status
        .as_ref()
        .map(
            |observation| match (&observation.execution, &observation.finality) {
                (Execution::Failed, _) => TransactionStatus::Failed,
                (Execution::Succeeded, Finality::Finalized) => TransactionStatus::Confirmed,
                (Execution::Succeeded, Finality::Included) => TransactionStatus::Submitted,
            },
        )
        .unwrap_or(TransactionStatus::SubmissionUnknown);
    let previous = wallet.operations[op_index].clone();
    let now = Utc::now().to_rfc3339();
    wallet.operations[op_index].attempts[attempt_index].tx_status = observed_status.clone();
    wallet.operations[op_index].attempts[attempt_index].status = status;
    wallet.operations[op_index].attempts[attempt_index].updated_at = now.clone();
    wallet.operations[op_index].updated_at = now;
    if let Err(error) = persist_state_wallet(app_state) {
        if let Some(wallet) = app_state.wallet.as_mut() {
            wallet.operations[op_index] = previous;
        }
        return Err(error);
    }
    Ok(observed_status)
}

#[cfg(test)]
#[path = "../tests/commands/tx.rs"]
mod tests;
