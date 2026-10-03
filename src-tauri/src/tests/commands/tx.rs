use super::{
    EvmNonceReservationGuard, ensure_native_balance_covers_debit,
    ensure_token_balance_covers_amount, required_native_debit,
};
use crate::state::AppState;
use std::path::PathBuf;
use std::sync::Mutex;

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
