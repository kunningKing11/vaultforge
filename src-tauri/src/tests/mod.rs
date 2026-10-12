use crate::derivation::{BitcoinAccount, BitcoinDerivedAddress, BitcoinKeyOrigin};
use crate::dto::Asset;
use crate::providers::bitcoin::BitcoinUtxo;

pub(crate) fn sample_operation() -> crate::operations::Operation {
    crate::operations::create_operation(
        crate::dto::AssetIdentity::new(
            "ethereum",
            Some("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
        ),
        crate::dto::AssetIdentity::new(
            "arbitrum_one",
            Some("0xaf88d065e77c8cC2239327C5EDb3A432268e5831"),
        ),
        "0x3333333333333333333333333333333333333333".to_string(),
        "0x4444444444444444444444444444444444444444".to_string(),
        1_000_000,
    )
    .unwrap()
}

pub(crate) fn sample_attempt(
    step: crate::operations::TransactionStep,
    status: crate::operations::TransactionStatus,
) -> crate::operations::TransactionAttempt {
    crate::operations::TransactionAttempt {
        step,
        status,
        tx_hash: format!("0x{}", "ab".repeat(32)),
        raw_tx: Some("0xdeadbeef".to_string()),
        evm_context: None,
        signature: Some("test signature".to_string()),
        fee: Some("100".to_string()),
        nonce_or_sequence: Some("7".to_string()),
        error: None,
        updated_at: "2026-10-07T00:00:00Z".to_string(),
        tx_status: None,
    }
}

pub(crate) fn starter_assets(network: &str) -> Vec<Asset> {
    vec![
        Asset {
            symbol: "ETH".to_string(),
            unicode_symbol: Some("Ξ".to_string()),
            name: "Ethereum".to_string(),
            balance: "2482100000000000000000".to_string(),
            decimals: 18,
            price_usd: 3480.62,
            change_24h: 2.84,
            network: network.to_string(),
            token_address: None,
        },
        Asset {
            symbol: "BTC".to_string(),
            unicode_symbol: Some("₿".to_string()),
            name: "Bitcoin".to_string(),
            balance: "184200000000".to_string(),
            decimals: 8,
            price_usd: 102_240.12,
            change_24h: -0.62,
            network: network.to_string(),
            token_address: None,
        },
        Asset {
            symbol: "SOL".to_string(),
            unicode_symbol: None,
            name: "Solana".to_string(),
            balance: "82450000000".to_string(),
            decimals: 9,
            price_usd: 184.33,
            change_24h: 5.18,
            network: network.to_string(),
            token_address: None,
        },
        Asset {
            symbol: "USDC".to_string(),
            unicode_symbol: None,
            name: "USD Coin".to_string(),
            balance: "8420000000".to_string(),
            decimals: 6,
            price_usd: 1.0,
            change_24h: 0.01,
            network: network.to_string(),
            token_address: Some("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string()),
        },
    ]
}

pub(crate) const BITCOIN_TEST_MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

pub(crate) fn bitcoin_test_owner(origin: BitcoinKeyOrigin) -> BitcoinDerivedAddress {
    BitcoinAccount::from_mnemonic(BITCOIN_TEST_MNEMONIC, "")
        .unwrap()
        .derive_address(origin)
        .unwrap()
}

pub(crate) fn bitcoin_test_utxo(
    txid_byte: u8,
    value: u64,
    confirmed: bool,
    origin: BitcoinKeyOrigin,
) -> BitcoinUtxo {
    BitcoinUtxo {
        txid: format!("{txid_byte:02x}").repeat(32),
        vout: 0,
        value,
        confirmed,
        owner: bitcoin_test_owner(origin),
    }
}
