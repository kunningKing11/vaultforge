use super::{
    EvmNonceReservationGuard, ensure_native_balance_covers_debit,
    ensure_token_balance_covers_amount, reconcile_send_status_with, required_native_debit,
};
use crate::activity::{activity, hash_secret, random_hex};
use crate::dto::{FiatCurrency, Wallet};
use crate::state::AppState;
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
