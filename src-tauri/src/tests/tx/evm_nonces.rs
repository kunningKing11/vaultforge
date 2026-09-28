use super::EvmNonceManager;

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
fn cancel_all_unbroadcast_releases_pending_and_preserves_broadcast_attempts() {
    let mut manager = EvmNonceManager::default();
    manager.reserve(1, "0xabc", 8).unwrap();
    let attempted_nonce = manager.reserve(1, "0xdef", 8).unwrap();
    manager
        .begin_broadcast(1, "0xdef", attempted_nonce)
        .unwrap();
    let broadcast_nonce = manager.reserve(1, "0xghi", 8).unwrap();
    manager
        .begin_broadcast(1, "0xghi", broadcast_nonce)
        .unwrap();
    manager.mark_broadcast(1, "0xghi", broadcast_nonce).unwrap();

    manager.cancel_all_unbroadcast();

    assert_eq!(manager.reserve(1, "0xabc", 8).unwrap(), 8);
    assert_eq!(manager.reserve(1, "0xdef", 8).unwrap(), 9);
    assert_eq!(manager.reserve(1, "0xghi", 8).unwrap(), 9);
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
