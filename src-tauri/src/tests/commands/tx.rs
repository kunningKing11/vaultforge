use super::{
    EvmNonceReservationGuard, ensure_native_balance_covers_debit,
    ensure_token_balance_covers_amount, mark_send_cancelled, prepare_durable_send,
    reconcile_send_status_with, required_native_debit, update_send_attempt,
};
use crate::activity::{activity, hash_secret, random_hex};
use crate::dto::{AssetIdentity, FiatCurrency, SignedTransaction, Wallet};
use crate::operations::{
    OperationKind, TransactionAttempt, TransactionStatus, TransactionStep, create_operation,
};
use crate::state::AppState;
use crate::tx::evm_nonces::EvmNonceManager;
use crate::tx::tx_status::{Execution, Finality, TxStatus};

fn observation(label: &str) -> TxStatus {
    TxStatus::new(
        if label == "failed" {
            Execution::Failed
        } else {
            Execution::Succeeded
        },
        Finality::Finalized,
    )
}

use crate::storage::{decrypt_wallet, derive_storage_key, read_stored_wallet};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

fn send_state(path: PathBuf) -> Mutex<AppState> {
    let mut state = AppState::from_storage(path);
    let mut send = activity("send", "Transfer sent", "", "1 ETH");
    send.status = "pending".to_string();
    send.hash = "0xabc".to_string();
    send.network = Some("ethereum".to_string());
    state.wallet = Some(Wallet {
        name: "Send status fixture".to_string(),
        mnemonic: "test mnemonic".to_string(),
        bip39_passphrase: String::new(),
        created_at: "2026-10-08T00:00:00Z".to_string(),
        addresses: HashMap::new(),
        wallet_password_hash: "test hash".to_string(),
        fiat_currency: FiatCurrency::Usd,
        usd_exchange_rate: 1.0,
        assets: vec![],
        activity: vec![send],
        operations: vec![],
        enabled_networks: vec!["ethereum".to_string(), "base".to_string()],
        auto_lock_timeout_secs: Some(60),
        use_crypto_symbols: false,
        hide_below_usd: crate::dto::default_hide_below_usd(),
        group_assets_by_chain: false,
    });
    state.encryption_key = Some([7; 32]);
    state.storage_salt = Some(vec![1; 16]);
    state.wallet_generation = 12;
    state.locked = false;
    Mutex::new(state)
}

fn test_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
}

#[test]
fn cancel_marks_durable_send_failed_and_ignores_submitted_attempts() {
    let directory = send_directory("send-cancel");
    let state = send_state(directory.join("wallet.json"));
    let signed = signed_send("solana", "SolFrom", "SolTo", "blockhash", "0xhash");
    let (operation_id, _) = prepare_durable_send(&state, &signed, None)
        .unwrap()
        .unwrap();

    {
        let mut guard = state.lock().unwrap();
        assert!(mark_send_cancelled(&mut guard, "solana", "SolFrom", "blockhash").unwrap());
    }
    update_send_attempt(&state, &operation_id, "0xhash", |attempt| {
        attempt.status = TransactionStatus::Submitted;
    })
    .unwrap();
    {
        let mut guard = state.lock().unwrap();
        assert!(!mark_send_cancelled(&mut guard, "solana", "SolFrom", "blockhash").unwrap());
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn disappeared_receipts_clear_nonfinal_evidence_but_rpc_errors_do_not() {
    let directory =
        std::env::temp_dir().join(format!("vaultforge-missing-receipt-{}", random_hex(16)));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("wallet.json");
    let state = send_state(path.clone());
    let mut included = observation("confirmed");
    included.finality = Finality::Included;
    test_runtime()
        .block_on(reconcile_send_status_with(
            &state,
            "0xabc",
            "ethereum",
            || std::future::ready(Ok(Some(included.clone()))),
        ))
        .unwrap();
    let saved = std::fs::read(&path).unwrap();
    assert!(
        test_runtime()
            .block_on(reconcile_send_status_with(
                &state,
                "0xabc",
                "ethereum",
                || std::future::ready(Err("RPC unavailable".to_string()))
            ))
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), saved);
    assert_eq!(
        state.lock().unwrap().wallet.as_ref().unwrap().activity[0].tx_status,
        Some(included)
    );
    assert!(
        test_runtime()
            .block_on(reconcile_send_status_with(
                &state,
                "0xabc",
                "ethereum",
                || std::future::ready(Ok(None))
            ))
            .unwrap()
            .is_none()
    );
    assert_ne!(std::fs::read(&path).unwrap(), saved);
    let state = state.lock().unwrap();
    let item = &state.wallet.as_ref().unwrap().activity[0];
    assert_eq!(item.status, "pending");
    assert!(item.tx_status.is_none());
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn erc20_send_requires_live_token_amount() {
    assert_eq!(
        ensure_token_balance_covers_amount(999, 1_000, "USDC").unwrap_err(),
        "Insufficient USDC balance"
    );
    assert!(ensure_token_balance_covers_amount(1_000, 1_000, "USDC").is_ok());
}

#[test]
fn erc20_transfer_requires_native_fee_balance() {
    let required = required_native_debit(false, 1_000_000, 21_000, "ETH").unwrap();
    assert_eq!(required, 21_000);
    assert_eq!(
        ensure_native_balance_covers_debit(20_999, required, "ETH", false, "transaction fee")
            .unwrap_err(),
        "Insufficient ETH balance for transaction fee"
    );
    assert!(
        ensure_native_balance_covers_debit(21_000, required, "ETH", false, "transaction fee",)
            .is_ok()
    );
}

#[test]
fn evm_native_requires_amount_plus_fee() {
    let required = required_native_debit(true, 1_000_000, 21_000, "ETH").unwrap();
    assert_eq!(required, 1_021_000);
    assert_eq!(
        ensure_native_balance_covers_debit(1_020_999, required, "ETH", true, "transaction fee")
            .unwrap_err(),
        "Insufficient ETH balance for amount plus fee"
    );
    assert!(
        ensure_native_balance_covers_debit(1_021_000, required, "ETH", true, "transaction fee",)
            .is_ok()
    );
}

#[test]
fn evm_nonce_reservation_guard_keeps_committed_reservation() {
    let state = Mutex::new(AppState::from_storage(PathBuf::from(
        "/nonexistent/wallet.json",
    )));
    state
        .lock()
        .unwrap()
        .evm_nonces
        .reserve(1, "0xabc", 7)
        .unwrap();

    let mut reservation = EvmNonceReservationGuard::new(&state, 1, "0xabc".to_string(), 7);
    reservation.keep();
    drop(reservation);

    assert!(
        state
            .lock()
            .unwrap()
            .evm_nonces
            .reserve(1, "0xabc", 7)
            .is_err()
    );
}

#[test]
fn evm_nonce_reservation_guard_releases_on_drop() {
    let state = Mutex::new(AppState::from_storage(PathBuf::from(
        "/nonexistent/wallet.json",
    )));
    state
        .lock()
        .unwrap()
        .evm_nonces
        .reserve(1, "0xabc", 7)
        .unwrap();

    drop(EvmNonceReservationGuard::new(
        &state,
        1,
        "0xabc".to_string(),
        7,
    ));

    assert_eq!(
        state
            .lock()
            .unwrap()
            .evm_nonces
            .reserve(1, "0xabc", 7)
            .unwrap(),
        7
    );
}

#[test]
fn evm_send_operation_advances_recovered_nonce() {
    let directory = send_directory("send-nonce");
    let state = send_state(directory.join("wallet.json"));
    let signed = signed_send("ethereum", "0xabc", "0xdef", "7", "0xhash");
    let (operation_id, _) = prepare_durable_send(&state, &signed, None)
        .unwrap()
        .unwrap();
    update_send_attempt(&state, &operation_id, "0xhash", |attempt| {
        attempt.status = TransactionStatus::Submitted;
    })
    .unwrap();

    let mut manager = EvmNonceManager::default();
    manager.restore_send_operations(&operations(&state));
    assert_eq!(manager.reserve(1, "0xabc", 7).unwrap(), 8);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn evm_signed_send_blocks_nonce_until_resolved() {
    let directory = send_directory("send-signed-block");
    let state = send_state(directory.join("wallet.json"));
    let signed = signed_send("ethereum", "0xabc", "0xdef", "7", "0xhash");
    prepare_durable_send(&state, &signed, None)
        .unwrap()
        .unwrap();
    let mut manager = EvmNonceManager::default();
    manager.restore_send_operations(&operations(&state));
    assert!(manager.reserve(1, "0xabc", 7).is_err());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn finalized_receipts_are_not_cleared_when_a_lookup_returns_none() {
    let state = send_state(PathBuf::from("/nonexistent/wallet.json"));
    state.lock().unwrap().wallet.as_mut().unwrap().activity[0].tx_status =
        Some(observation("confirmed"));
    let result = test_runtime()
        .block_on(reconcile_send_status_with(
            &state,
            "0xabc",
            "ethereum",
            || std::future::ready(Ok(None)),
        ))
        .unwrap();
    assert_eq!(result, Some(observation("confirmed")));
}

#[test]
fn send_observations_preserve_concurrent_newer_evidence() {
    let state = send_state(PathBuf::from("/nonexistent/wallet.json"));
    let mut newer = observation("confirmed");
    newer.finality = Finality::Included;
    newer.block_height = Some(42);
    let mut older = newer.clone();
    older.block_height = Some(41);
    test_runtime()
        .block_on(reconcile_send_status_with(
            &state,
            "0xabc",
            "ethereum",
            || {
                let mut state = state.lock().unwrap();
                state.wallet.as_mut().unwrap().activity[0].tx_status = Some(newer.clone());
                std::future::ready(Ok(Some(older.clone())))
            },
        ))
        .unwrap();
    assert_eq!(
        state.lock().unwrap().wallet.as_ref().unwrap().activity[0].tx_status,
        Some(newer)
    );
}

#[test]
fn send_observations_restore_both_fields_after_failed_save() {
    let directory =
        std::env::temp_dir().join(format!("vaultforge-observation-failure-{}", random_hex(16)));
    std::fs::create_dir(&directory).unwrap();
    let state = send_state(directory.clone());
    let mut previous = observation("failed");
    previous.finality = Finality::Included;
    previous.block_height = Some(41);
    {
        let mut state = state.lock().unwrap();
        let item = &mut state.wallet.as_mut().unwrap().activity[0];
        item.status = "failed".to_string();
        item.tx_status = Some(previous.clone());
    }
    assert!(
        test_runtime()
            .block_on(reconcile_send_status_with(
                &state,
                "0xabc",
                "ethereum",
                || std::future::ready(Ok(Some(observation("confirmed"))))
            ))
            .is_err()
    );
    let state = state.lock().unwrap();
    let item = &state.wallet.as_ref().unwrap().activity[0];
    assert_eq!(item.status, "failed");
    assert_eq!(item.tx_status, Some(previous));
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn send_observations_save_changed_block_evidence_with_the_same_label() {
    let directory =
        std::env::temp_dir().join(format!("vaultforge-observation-update-{}", random_hex(16)));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("wallet.json");
    let state = send_state(path.clone());
    let mut first = observation("confirmed");
    first.finality = Finality::Included;
    first.block_height = Some(41);
    first.block_hash = Some(format!("0x{}", "ab".repeat(32)));
    test_runtime()
        .block_on(reconcile_send_status_with(
            &state,
            "0xabc",
            "ethereum",
            || std::future::ready(Ok(Some(first.clone()))),
        ))
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    test_runtime()
        .block_on(reconcile_send_status_with(
            &state,
            "0xabc",
            "ethereum",
            || std::future::ready(Ok(Some(first.clone()))),
        ))
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let mut second = first.clone();
    second.block_height = Some(42);
    second.block_hash = Some(format!("0x{}", "cd".repeat(32)));
    second.confirmations = Some(2);
    test_runtime()
        .block_on(reconcile_send_status_with(
            &state,
            "0xabc",
            "ethereum",
            || std::future::ready(Ok(Some(second.clone()))),
        ))
        .unwrap();
    assert_ne!(std::fs::read(&path).unwrap(), before);
    let state = state.lock().unwrap();
    let item = &state.wallet.as_ref().unwrap().activity[0];
    assert_eq!(item.status, "confirmed");
    assert_eq!(item.tx_status, Some(second));
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn send_operation_activity_uses_send_kind_and_symbol() {
    let directory = send_directory("send-activity");
    let state = send_state(directory.join("wallet.json"));
    let signed = signed_send("solana", "SolFrom", "SolTo", "blockhash", "0xhash");
    let (operation_id, _) = prepare_durable_send(&state, &signed, None)
        .unwrap()
        .unwrap();
    update_send_attempt(&state, &operation_id, "0xhash", |attempt| {
        attempt.status = TransactionStatus::Submitted;
    })
    .unwrap();

    let guard = state.lock().unwrap();
    let operation = guard
        .wallet
        .as_ref()
        .unwrap()
        .operations
        .iter()
        .find(|operation| operation.id == operation_id)
        .unwrap();
    assert_eq!(operation.kind, OperationKind::Send);
    let rendered = crate::operations::operation_activity(operation).unwrap();
    assert_eq!(rendered.kind, "send");
    assert_eq!(rendered.amount, "-1000000000 SOL");
    assert_eq!(rendered.network.as_deref(), Some("solana"));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn send_operation_blocks_second_broadcast_until_reconciled() {
    let directory = send_directory("send-conflict");
    let state = send_state(directory.join("wallet.json"));
    let first = signed_send("solana", "SolFrom", "SolTo", "blockhash-1", "0xhash1");
    let (operation_id, raw) = prepare_durable_send(&state, &first, None).unwrap().unwrap();
    assert_eq!(raw, "raw-0xhash1");
    let second = signed_send("solana", "SolFrom", "SolTo2", "blockhash-2", "0xhash2");
    assert!(prepare_durable_send(&state, &second, None).is_err());
    // Retrying the same hash reuses the durable attempt.
    let retry = prepare_durable_send(&state, &first, None).unwrap().unwrap();
    assert_eq!(retry.0, operation_id);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn send_operation_persists_signed_attempt_for_recovery() {
    let directory = send_directory("send-persist");
    let path = directory.join("wallet.json");
    let state = send_state(path.clone());
    let signed = signed_send("xrpl", "rFrom", "rTo", "42", "0xhash");
    let (operation_id, _) = prepare_durable_send(&state, &signed, None)
        .unwrap()
        .unwrap();

    assert!(path.exists());
    let guard = state.lock().unwrap();
    let operation = guard
        .wallet
        .as_ref()
        .unwrap()
        .operations
        .iter()
        .find(|operation| operation.id == operation_id)
        .unwrap();
    assert_eq!(operation.kind, OperationKind::Send);
    assert_eq!(operation.attempts[0].status, TransactionStatus::Signed);
    assert_eq!(operation.attempts[0].raw_tx.as_deref(), Some("raw-0xhash"));
    assert_eq!(
        operation.attempts[0].nonce_or_sequence.as_deref(),
        Some("42")
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn send_save_failure_leaves_no_operation() {
    let directory = send_directory("send-save-failure");
    let state = send_state(directory.clone());
    // Point storage at a directory so the atomic rename cannot replace it.
    state.lock().unwrap().storage_path = directory.clone();
    let signed = signed_send("solana", "SolFrom", "SolTo", "blockhash", "0xhash");
    assert!(prepare_durable_send(&state, &signed, None).is_err());
    assert!(
        state
            .lock()
            .unwrap()
            .wallet
            .as_ref()
            .unwrap()
            .operations
            .is_empty()
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn send_status_commit_updates_durable_cctp_operation() {
    let directory = send_directory("send-cctp-status");
    let state = send_state(directory.join("wallet.json"));
    let mut operation = create_operation(
        AssetIdentity::new(
            "ethereum",
            Some("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
        ),
        AssetIdentity::new(
            "arbitrum_one",
            Some("0xaf88d065e77c8cC2239327C5EDb3A432268e5831"),
        ),
        "0x1111111111111111111111111111111111111111".to_string(),
        "0x2222222222222222222222222222222222222222".to_string(),
        1_000_000,
    )
    .unwrap();
    operation.attempts.push(TransactionAttempt {
        step: TransactionStep::Burn,
        status: TransactionStatus::Submitted,
        tx_hash: "0xcctp".to_string(),
        raw_tx: Some("0xraw".to_string()),
        signature: None,
        fee: None,
        nonce_or_sequence: Some("1".to_string()),
        error: None,
        updated_at: "2026-10-08T00:00:00Z".to_string(),
        tx_status: None,
        evm_context: None,
        solana_context: None,
    });
    state
        .lock()
        .unwrap()
        .wallet
        .as_mut()
        .unwrap()
        .operations
        .push(operation);

    let observed = test_runtime()
        .block_on(reconcile_send_status_with(
            &state,
            "0xcctp",
            "ethereum",
            || std::future::ready(Ok(Some(observation("confirmed")))),
        ))
        .unwrap()
        .unwrap();
    assert_eq!(observed.finality, Finality::Finalized);
    let guard = state.lock().unwrap();
    let attempt = &guard.wallet.as_ref().unwrap().operations[0].attempts[0];
    assert_eq!(attempt.status, TransactionStatus::Confirmed);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn send_status_commit_updates_durable_operation() {
    let directory = send_directory("send-status");
    let state = send_state(directory.join("wallet.json"));
    let signed = signed_send("solana", "SolFrom", "SolTo", "blockhash", "0xhash");
    let (operation_id, _) = prepare_durable_send(&state, &signed, None)
        .unwrap()
        .unwrap();

    let observed = test_runtime()
        .block_on(reconcile_send_status_with(
            &state,
            "0xhash",
            "solana",
            || std::future::ready(Ok(Some(observation("confirmed")))),
        ))
        .unwrap()
        .unwrap();
    assert_eq!(observed.execution, Execution::Succeeded);

    let guard = state.lock().unwrap();
    let attempt = &guard
        .wallet
        .as_ref()
        .unwrap()
        .operations
        .iter()
        .find(|operation| operation.id == operation_id)
        .unwrap()
        .attempts[0];
    assert_eq!(attempt.status, TransactionStatus::Confirmed);
    assert_eq!(
        attempt.tx_status.as_ref().unwrap().finality,
        Finality::Finalized
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn send_status_does_not_downgrade_a_concurrently_settled_record() {
    let state = send_state(PathBuf::from("/nonexistent/wallet.json"));
    let result = test_runtime()
        .block_on(reconcile_send_status_with(
            &state,
            "0xabc",
            "ethereum",
            || {
                state.lock().unwrap().wallet.as_mut().unwrap().activity[0].status =
                    "confirmed".to_string();
                state.lock().unwrap().wallet.as_mut().unwrap().activity[0].tx_status =
                    Some(observation("confirmed"));
                std::future::ready(Ok(Some(observation("failed"))))
            },
        ))
        .unwrap();
    assert_eq!(
        result.as_ref().map(TxStatus::legacy_label),
        Some("confirmed")
    );
    assert_eq!(
        state.lock().unwrap().wallet.as_ref().unwrap().activity[0].status,
        "confirmed"
    );
}

fn signed_send(network: &str, from: &str, to: &str, nonce: &str, hash: &str) -> SignedTransaction {
    SignedTransaction {
        solana_token_send: None,
        token_address: None,
        from: from.to_string(),
        to: to.to_string(),
        symbol: "SOL".to_string(),
        amount: "1000000000".to_string(),
        note: String::new(),
        destination_tag: None,
        network: network.to_string(),
        nonce: nonce.to_string(),
        signed_at: "2026-10-08T00:00:00Z".to_string(),
        payload_hash: hash.to_string(),
        signature: "signature".to_string(),
        fee_amount: "5000".to_string(),
        fee_symbol: "SOL".to_string(),
        total_debit: "1000005000".to_string(),
        post_balance: "1".to_string(),
        decimals: 9,
        fiat_value: 0.0,
        raw_tx: Some(format!("raw-{hash}")),
        tx_hash: Some(hash.to_string()),
    }
}

fn send_directory(name: &str) -> std::path::PathBuf {
    let directory = std::env::temp_dir().join(format!("vaultforge-{name}-{}", random_hex(16)));
    std::fs::create_dir(&directory).unwrap();
    directory
}

fn operations(state: &Mutex<AppState>) -> Vec<crate::operations::Operation> {
    state
        .lock()
        .unwrap()
        .wallet
        .as_ref()
        .unwrap()
        .operations
        .clone()
}

#[test]
fn send_status_ignores_missing_receipts_errors_and_unchanged_results() {
    let path = std::env::temp_dir().join(format!("vaultforge-send-no-write-{}", random_hex(16)));
    let state = send_state(path.clone());
    for observation in [Ok(None), Err("RPC unavailable".to_string())] {
        let result = test_runtime().block_on(reconcile_send_status_with(
            &state,
            "0xabc",
            "ethereum",
            || std::future::ready(observation.clone()),
        ));
        assert_eq!(result.is_err(), observation.is_err());
        assert_eq!(
            state.lock().unwrap().wallet.as_ref().unwrap().activity[0].status,
            "pending"
        );
        assert!(!path.exists());
    }
}

#[test]
fn send_status_matches_network_and_hash_and_excludes_operation_activity() {
    let directory =
        std::env::temp_dir().join(format!("vaultforge-send-identity-{}", random_hex(16)));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("wallet.json");
    let state = send_state(path.clone());
    {
        let mut state = state.lock().unwrap();
        let wallet = state.wallet.as_mut().unwrap();
        let mut other = wallet.activity[0].clone();
        other.id = "base-send".to_string();
        other.network = Some("base".to_string());
        wallet.activity.push(other);
    }
    test_runtime()
        .block_on(reconcile_send_status_with(&state, "0xabc", "base", || {
            std::future::ready(Ok(Some(observation("confirmed"))))
        }))
        .unwrap();
    {
        let mut state = state.lock().unwrap();
        let wallet = state.wallet.as_mut().unwrap();
        assert_eq!(wallet.activity[0].status, "pending");
        assert_eq!(wallet.activity[1].status, "confirmed");
        wallet.activity[0].kind = "cctp".to_string();
    }
    let mut queried = false;
    let result = test_runtime().block_on(reconcile_send_status_with(
        &state,
        "0xabc",
        "ethereum",
        || {
            queried = true;
            std::future::ready(Ok(None))
        },
    ));
    assert!(result.is_err());
    assert!(!queried);
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn send_status_persists_confirmed_and_failed_results_for_reload() {
    let password = "Status persistence test password 42!";
    let (key, salt) = derive_storage_key(password, None).unwrap();
    for status in ["confirmed", "failed"] {
        let directory =
            std::env::temp_dir().join(format!("vaultforge-send-persist-{}", random_hex(16)));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("wallet.json");
        let state = send_state(path.clone());
        {
            let mut state = state.lock().unwrap();
            state.encryption_key = Some(key);
            state.storage_salt = Some(salt.clone());
            state.wallet.as_mut().unwrap().wallet_password_hash = hash_secret(password);
        }
        let result = test_runtime()
            .block_on(reconcile_send_status_with(
                &state,
                "0xabc",
                "ethereum",
                || {
                    assert!(state.try_lock().is_ok());
                    std::future::ready(Ok(Some(observation(status))))
                },
            ))
            .unwrap();
        assert_eq!(result.as_ref().map(TxStatus::legacy_label), Some(status));
        drop(state);
        let saved = read_stored_wallet(&path).unwrap().unwrap();
        let restored = decrypt_wallet(&saved, password).unwrap();
        assert_eq!(restored.wallet().activity[0].status, status);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}

#[test]
fn send_status_rechecks_record_identity_after_the_lookup() {
    for change in ["id", "hash", "network"] {
        let state = send_state(PathBuf::from("/nonexistent/wallet.json"));
        let result = test_runtime().block_on(reconcile_send_status_with(
            &state,
            "0xabc",
            "ethereum",
            || {
                let mut state = state.lock().unwrap();
                let item = &mut state.wallet.as_mut().unwrap().activity[0];
                match change {
                    "id" => item.id = "different".to_string(),
                    "hash" => item.hash = "0xdef".to_string(),
                    _ => item.network = Some("base".to_string()),
                }
                std::future::ready(Ok(Some(observation("confirmed"))))
            },
        ));
        assert!(result.is_err());
        assert_eq!(
            state.lock().unwrap().wallet.as_ref().unwrap().activity[0].status,
            "pending"
        );
    }
}

#[test]
fn send_status_rejects_locked_missing_or_unrecorded_wallets_without_lookup() {
    for condition in ["locked", "missing", "unrecorded"] {
        let state = send_state(PathBuf::from("/nonexistent/wallet.json"));
        {
            let mut state = state.lock().unwrap();
            match condition {
                "locked" => state.locked = true,
                "missing" => state.wallet = None,
                _ => state.wallet.as_mut().unwrap().activity.clear(),
            }
        }
        let mut queried = false;
        let result = test_runtime().block_on(reconcile_send_status_with(
            &state,
            "0xabc",
            "ethereum",
            || {
                queried = true;
                std::future::ready(Ok(None))
            },
        ));
        assert!(result.is_err());
        assert!(!queried);
    }
}

#[test]
fn send_status_rejects_stale_locked_deleted_and_replaced_sessions() {
    for condition in ["generation", "locked", "deleted", "replaced"] {
        let state = send_state(PathBuf::from("/nonexistent/wallet.json"));
        let result = test_runtime().block_on(reconcile_send_status_with(
            &state,
            "0xabc",
            "ethereum",
            || {
                let mut state = state.lock().unwrap();
                match condition {
                    "generation" => state.advance_wallet_generation(),
                    "locked" => state.locked = true,
                    "replaced" => {
                        let generation = state.wallet_generation.wrapping_add(1);
                        *state = send_state(PathBuf::from("/nonexistent/replacement.json"))
                            .into_inner()
                            .unwrap();
                        state.wallet_generation = generation;
                    }
                    _ => state.wallet = None,
                }
                std::future::ready(Ok(Some(observation("confirmed"))))
            },
        ));
        assert!(result.is_err());
        if let Some(wallet) = state.lock().unwrap().wallet.as_ref() {
            assert_eq!(wallet.activity[0].status, "pending");
        }
    }
}

#[test]
fn send_status_save_failures_restore_pending_and_remain_retryable() {
    let directory =
        std::env::temp_dir().join(format!("vaultforge-send-write-failure-{}", random_hex(16)));
    std::fs::create_dir(&directory).unwrap();
    let state = send_state(directory.clone());
    for _ in 0..2 {
        let result = test_runtime().block_on(reconcile_send_status_with(
            &state,
            "0xabc",
            "ethereum",
            || std::future::ready(Ok(Some(observation("confirmed")))),
        ));
        assert_eq!(result.unwrap_err(), "Failed to save wallet");
        assert_eq!(
            state.lock().unwrap().wallet.as_ref().unwrap().activity[0].status,
            "pending"
        );
        assert!(!state.lock().unwrap().locked);
    }
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn solana_native_requires_amount_plus_fee() {
    let required = required_native_debit(true, 10_000, 5_000, "SOL").unwrap();
    assert_eq!(required, 15_000);
    assert_eq!(
        ensure_native_balance_covers_debit(
            14_999,
            required,
            "SOL",
            true,
            "Solana transaction fee",
        )
        .unwrap_err(),
        "Insufficient SOL balance for amount plus fee"
    );
    assert!(ensure_native_balance_covers_debit(
        15_000,
        required,
        "SOL",
        true,
        "Solana transaction fee",
    )
    .is_ok());
}

#[test]
fn solana_token_requires_sol_for_fee() {
    let required = required_native_debit(false, 10_000, 5_000, "SOL").unwrap();
    assert_eq!(required, 5_000);
    assert_eq!(
        ensure_native_balance_covers_debit(
            4_999,
            required,
            "SOL",
            false,
            "Solana transaction fee",
        )
        .unwrap_err(),
        "Insufficient SOL balance for Solana transaction fee"
    );
    assert!(ensure_native_balance_covers_debit(
        5_000,
        required,
        "SOL",
        false,
        "Solana transaction fee",
    )
    .is_ok());
}

#[test]
fn token_send_persists_review_and_unknown_submission_and_rejects_resubmission() {
    let directory = send_directory("token2022-review");
    let path = directory.join("wallet.json");
    let state = send_state(path.clone());
    let password = "Token review recovery 42!";
    let (key, salt) = derive_storage_key(password, Some(&[2; 16])).unwrap();
    {
        let mut guard = state.lock().unwrap();
        guard.encryption_key = Some(key);
        guard.storage_salt = Some(salt);
    }
    let mut signed = signed_send("solana", "SolFrom", "SolTo", "blockhash", "signature");
    signed.token_address = Some("mint".into());
    signed.solana_token_send = Some(crate::dto::SolanaTokenSendReview {
        token_program: crate::solana_tokens::TOKEN_2022.into(),
        mint_extensions: vec![1],
        fee_schedule: Some(crate::dto::SolanaTransferFeeSchedule {
            epoch: "4".into(),
            basis_points: 100,
            maximum_fee: "10".into(),
        }),
        sources: vec![crate::dto::SolanaTokenSendSource {
            address: "source".into(),
            amount: "100".into(),
            fee: "1".into(),
        }],
        transfer_fee: "1".into(),
        recipient_amount: "99".into(),
        account_rent_lamports: "2100000".into(),
    });
    let legacy: SignedTransaction = serde_json::from_value({
        let mut json = serde_json::to_value(&signed).unwrap();
        json.as_object_mut().unwrap().remove("solanaTokenSend");
        json
    })
    .unwrap();
    assert!(legacy.solana_token_send.is_none());
    let (id, raw) = prepare_durable_send(&state, &signed, Some(12))
        .unwrap()
        .unwrap();
    assert_eq!(raw, signed.raw_tx.clone().unwrap());
    let stored = read_stored_wallet(&path).unwrap().unwrap();
    assert_eq!(stored.version, 17);
    assert!(
        !serde_json::to_string(&stored)
            .unwrap()
            .contains("token_program")
    );
    let restored = decrypt_wallet(&stored, password).unwrap();
    assert_eq!(
        restored.wallet().operations[0].solana_token_send,
        signed.solana_token_send
    );
    assert_eq!(
        restored.wallet().operations[0].attempts[0].status,
        TransactionStatus::Signed
    );
    update_send_attempt(&state, &id, "signature", |attempt| {
        attempt.status = TransactionStatus::SubmissionUnknown
    })
    .unwrap();
    assert!(
        prepare_durable_send(&state, &signed, Some(12))
            .unwrap_err()
            .contains("already recorded")
    );
    let stored = read_stored_wallet(&path).unwrap().unwrap();
    let restored = decrypt_wallet(&stored, password).unwrap();
    let operation = &restored.wallet().operations[0];
    assert_eq!(operation.solana_token_send, signed.solana_token_send);
    assert_eq!(
        operation.attempts[0].status,
        TransactionStatus::SubmissionUnknown
    );
    assert!(super::is_unresolved_send(operation, "solana", "SolFrom"));
    let mut legacy = serde_json::to_value(operation).unwrap();
    legacy.as_object_mut().unwrap().remove("solana_token_send");
    let legacy: crate::operations::Operation = serde_json::from_value(legacy).unwrap();
    assert!(legacy.solana_token_send.is_none());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn token_send_rejects_generation_changes_before_persistence() {
    let directory = send_directory("token2022-generation");
    let path = directory.join("wallet.json");
    let state = send_state(path.clone());
    let signed = signed_send("solana", "SolFrom", "SolTo", "blockhash", "signature");
    state.lock().unwrap().advance_wallet_generation();
    assert!(
        prepare_durable_send(&state, &signed, Some(12))
            .unwrap_err()
            .contains("Wallet changed")
    );
    assert!(!path.exists());
    assert!(operations(&state).is_empty());
    std::fs::remove_dir_all(directory).unwrap();
}
