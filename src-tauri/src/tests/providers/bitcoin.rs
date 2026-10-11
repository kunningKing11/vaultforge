use super::{
    BitcoinAccountSnapshot, BitcoinAddressSnapshot, BitcoinAddressStats, BitcoinBranchScan,
    parse_bitcoin_address_stats, parse_bitcoin_fee_rate, parse_bitcoin_tx_observation,
    parse_bitcoin_utxos,
};
use crate::derivation::{BitcoinAccount, BitcoinBranch, BitcoinDerivedAddress, BitcoinKeyOrigin};
use crate::tests::bitcoin_test_owner;
use crate::tx::tx_status::{Execution, Finality, legacy_tx_status};

#[test]
fn bitcoin_account_balance_includes_receive_and_change_branches() {
    let account = BitcoinAccount::from_mnemonic(MNEMONIC, "").unwrap();
    let receive = account.primary_address().unwrap();
    let next_receive = account
        .derive_address(BitcoinKeyOrigin::external(1))
        .unwrap();
    let change = account.derive_address(BitcoinKeyOrigin::change(1)).unwrap();
    let snapshot = BitcoinAccountSnapshot {
        account,
        addresses: vec![
            BitcoinAddressSnapshot {
                derived: receive,
                balance: 0,
                used: true,
            },
            BitcoinAddressSnapshot {
                derived: next_receive.clone(),
                balance: 0,
                used: false,
            },
            BitcoinAddressSnapshot {
                derived: change,
                balance: 2_479,
                used: true,
            },
        ],
    };

    assert_eq!(snapshot.total_balance().unwrap(), 2_479);
    assert_eq!(snapshot.next_receive_address(), Some(&next_receive));
}

#[test]
fn bitcoin_gap_limit_resets_after_a_used_address() {
    let mut scan = BitcoinBranchScan::new(BitcoinBranch::External);
    for index in 0..19 {
        scan.record(
            derived(index),
            BitcoinAddressStats {
                balance: 0,
                used: false,
            },
        )
        .unwrap();
    }
    assert!(!scan.complete);

    scan.record(
        derived(19),
        BitcoinAddressStats {
            balance: 0,
            used: true,
        },
    )
    .unwrap();
    assert!(!scan.complete);

    for index in 20..40 {
        scan.record(
            derived(index),
            BitcoinAddressStats {
                balance: 0,
                used: false,
            },
        )
        .unwrap();
    }
    assert!(scan.complete);
    assert_eq!(scan.addresses.len(), 40);
}

#[test]
fn bitcoin_history_marks_a_spent_address_as_used() {
    let json = serde_json::json!({
        "chain_stats": {
            "funded_txo_sum": 155_856,
            "spent_txo_sum": 155_856,
            "tx_count": 2
        },
        "mempool_stats": {
            "funded_txo_sum": 0,
            "spent_txo_sum": 0,
            "tx_count": 0
        }
    });
    let stats = parse_bitcoin_address_stats(&json).unwrap();
    assert_eq!(stats.balance, 0);
    assert!(stats.used);
}

#[test]
fn finalizes_bitcoin_at_six_confirmations_only_on_the_best_chain() {
    let observation = parse_bitcoin_tx_observation(
        &serde_json::json!({"confirmed":true,"block_height":42,"block_hash":"ab".repeat(32)}),
    )
    .unwrap()
    .unwrap();
    let canonical = serde_json::json!({"in_best_chain":true,"height":42});
    for (tip, finality, count) in [
        (42, Finality::Included, 1),
        (46, Finality::Included, 5),
        (47, Finality::Finalized, 6),
    ] {
        let observed = super::finalize_bitcoin_observation(
            observation.clone(),
            &canonical,
            &serde_json::json!(tip),
        )
        .unwrap()
        .unwrap();
        assert_eq!(observed.finality, finality);
        assert_eq!(observed.confirmations, Some(count));
    }
    assert!(
        super::finalize_bitcoin_observation(
            observation.clone(),
            &serde_json::json!({"in_best_chain":false}),
            &serde_json::json!(50)
        )
        .unwrap()
        .is_none()
    );
    for tip in [
        serde_json::json!(41),
        serde_json::json!("50"),
        serde_json::Value::Null,
    ] {
        assert!(
            super::finalize_bitcoin_observation(observation.clone(), &canonical, &tip).is_err()
        );
    }
    assert!(
        super::finalize_bitcoin_observation(
            observation,
            &serde_json::json!({"in_best_chain":true,"height":43}),
            &serde_json::json!(50)
        )
        .is_err()
    );
}

#[test]
fn parses_bitcoin_balance_with_mempool_values() {
    let json = serde_json::json!({
        "chain_stats": {
            "funded_txo_sum": 5000,
            "spent_txo_sum": 1200,
            "tx_count": 2
        },
        "mempool_stats": {
            "funded_txo_sum": 700,
            "spent_txo_sum": 200,
            "tx_count": 1
        }
    });
    let stats = parse_bitcoin_address_stats(&json).unwrap();
    assert_eq!(stats.balance, 4300);
    assert!(stats.used);
}

#[test]
fn parses_bitcoin_inclusion_without_inventing_finality_or_confirmations() {
    let observation = parse_bitcoin_tx_observation(
        &serde_json::json!({"confirmed":true,"block_height":42,"block_hash":"ab".repeat(32)}),
    )
    .unwrap()
    .unwrap();
    assert_eq!(observation.execution, Execution::Succeeded);
    assert_eq!(observation.finality, Finality::Included);
    assert_eq!(observation.block_height, Some(42));
    assert_eq!(
        observation.block_hash.as_deref(),
        Some("ab".repeat(32).as_str())
    );
    assert_eq!(observation.confirmations, None);
    assert_eq!(
        legacy_tx_status(Some(observation)).as_deref(),
        Some("confirmed")
    );
    let missing = parse_bitcoin_tx_observation(&serde_json::json!({"confirmed":true}))
        .unwrap()
        .unwrap();
    assert_eq!(missing.block_height, None);
    assert_eq!(missing.block_hash, None);
    assert!(
        parse_bitcoin_tx_observation(&serde_json::json!({"confirmed":false}))
            .unwrap()
            .is_none()
    );
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
    let owner = bitcoin_test_owner(BitcoinKeyOrigin::external(0));
    let utxos = parse_bitcoin_utxos(&json, &owner).unwrap();
    assert_eq!(utxos.len(), 2);
    assert_eq!(utxos[0].value, 50_000);
    assert_eq!(utxos[1].value, 100);
    assert_eq!(utxos[0].owner, owner);

    let fees = serde_json::json!({ "3": 2.1, "6": 1.4 });
    assert_eq!(parse_bitcoin_fee_rate(&fees).unwrap(), 3);
}

const MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

fn derived(index: u32) -> BitcoinDerivedAddress {
    BitcoinDerivedAddress {
        origin: BitcoinKeyOrigin::external(index),
        address: format!("address-{index}"),
    }
}

#[test]
fn rejects_incomplete_bitcoin_address_stats() {
    let json = serde_json::json!({
        "chain_stats": {
            "funded_txo_sum": 5000,
            "spent_txo_sum": 1200,
            "tx_count": 2
        },
        "mempool_stats": {
            "funded_txo_sum": 0,
            "tx_count": 0
        }
    });
    assert!(parse_bitcoin_address_stats(&json).is_err());
}

#[test]
fn rejects_malformed_bitcoin_status_metadata() {
    for value in [
        serde_json::json!({}),
        serde_json::json!({"confirmed":"true"}),
        serde_json::json!({"confirmed":true,"block_height":-1}),
        serde_json::json!({"confirmed":true,"block_height":"42"}),
        serde_json::json!({"confirmed":true,"block_hash":"bad"}),
    ] {
        assert!(parse_bitcoin_tx_observation(&value).is_err());
    }
}
