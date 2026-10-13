use super::EvmNonceManager;
use crate::dto::AssetIdentity;
use crate::operations::{
    TransactionAttempt, TransactionStatus, TransactionStep, create_send_operation,
};

fn send_attempt(nonce: &str) -> TransactionAttempt {
    TransactionAttempt {
        step: TransactionStep::Send,
        status: TransactionStatus::Signed,
        tx_hash: "0xhash".to_string(),
        raw_tx: Some("0xraw".to_string()),
        signature: None,
        fee: None,
        nonce_or_sequence: Some(nonce.to_string()),
        error: None,
        updated_at: "2026-10-08T00:00:00Z".to_string(),
        tx_status: None,
        evm_context: None,
        solana_context: None,
    }
}

#[test]
fn broadcast_attempt_blocks_cancellation() {
    let mut manager = EvmNonceManager::default();
    let nonce = manager.reserve(1, "0xabc", 8).unwrap();

    manager.begin_broadcast(1, "0xabc", nonce).unwrap();
    assert!(!manager.cancel_unbroadcast(1, "0xabc", nonce));
    manager.mark_broadcast(1, "0xabc", nonce).unwrap();
    assert!(!manager.cancel_unbroadcast(1, "0xabc", nonce));
    assert_eq!(manager.reserve(1, "0xabc", 8).unwrap(), 9);
}

#[test]
fn cancel_releases_tail_nonce() {
    let mut manager = EvmNonceManager::default();
    let nonce = manager.reserve(1, "0xabc", 12).unwrap();

    assert!(manager.cancel_unbroadcast(1, "0xabc", nonce));
    assert_eq!(manager.reserve(1, "0xabc", 12).unwrap(), nonce);
}

#[test]
fn reserves_separate_nonce_queues_per_chain_and_address() {
    let mut manager = EvmNonceManager::default();

    assert_eq!(manager.reserve(1, "0xabc", 3).unwrap(), 3);
    assert_eq!(manager.reserve(8453, "0xabc", 3).unwrap(), 3);
    assert_eq!(manager.reserve(1, "0xdef", 3).unwrap(), 3);
}

#[test]
fn restore_operations_ignores_ordinary_send_operations() {
    let mut manager = EvmNonceManager::default();
    let mut operation = create_send_operation(
        AssetIdentity::new("ethereum", None),
        "0xabc".to_string(),
        "0xdef".to_string(),
        1,
        "ETH".to_string(),
        String::new(),
    );
    operation.attempts.push(send_attempt("7"));

    manager.restore_operations(&[operation]).unwrap();

    // Ordinary sends use per-nonce protection, not account-wide CCTP blocking.
    assert_eq!(manager.reserve(1, "0xabc", 7).unwrap(), 7);
}
