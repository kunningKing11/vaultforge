use crate::derivation::{BitcoinAccount, BitcoinDerivedAddress, BitcoinKeyOrigin};
use crate::dto::Asset;
use crate::providers::bitcoin::BitcoinUtxo;

pub(crate) fn starter_assets(network: &str) -> Vec<Asset> {
    vec![
        Asset {
            symbol: "ETH".to_string(),
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
    BitcoinAccount::from_mnemonic(BITCOIN_TEST_MNEMONIC)
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
