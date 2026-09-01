use bech32::{Bech32, Bech32m, Hrp};
use chrono::Utc;
use std::collections::HashMap;

use super::{validate_address_for_network, validate_transfer};
use crate::address::evm::validate_address as validate_evm_address;
use crate::dto::{Asset, FiatCurrency, Wallet};

#[test]
fn validates_asset_address_formats() {
    assert!(
        validate_address_for_network("0xdAC17F958D2ee523a2206206994597C13D831ec7", "ethereum")
            .is_ok()
    );
    assert!(
        validate_address_for_network("0xdac17f958d2ee523a2206206994597c13d831ec7", "ethereum")
            .is_ok()
    );
    assert!(
        validate_address_for_network("bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq", "bitcoin")
            .is_ok()
    );
    assert!(validate_address_for_network("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa", "bitcoin").is_ok());
    assert!(validate_address_for_network("3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy", "bitcoin").is_ok());
    assert!(
        validate_address_for_network("7VH1XhBY1DmFk98fBdLqEbDsKpr41whdM8EzipizyVCJ", "solana")
            .is_ok()
    );
    assert!(validate_address_for_network("t1eB29zcZ2v3AQvAEtcNrERsWQPmxyTN4DF", "zcash").is_ok());
    assert!(
        validate_address_for_network("f17uoq6tp427uzv7fztkbsnn64iwotfrristwpryy", "filecoin")
            .is_ok()
    );
    assert!(
        validate_address_for_network("inj1m6kmamcpqgpsgpgxquyqjyq3zgf3g9gkzz8lqn", "injective")
            .is_ok()
    );
    assert!(
        validate_address_for_network("0xdAC17F958D2ee523a2206206994597C13D831ec7", "polygon")
            .is_ok()
    );
    assert!(validate_address_for_network("0xinvalid", "ethereum").is_err());
    assert!(validate_address_for_network("bc1q", "bitcoin").is_err());
    assert!(validate_address_for_network("invalid", "solana").is_err());
    assert!(
        validate_address_for_network(
            "0xdAC17F958D2ee523a2206206994597C13D831ec7",
            "not-a-network"
        )
        .is_err()
    );
}

#[test]
fn validates_filecoin_protocols_and_checksums() {
    for address in [
        "f00",
        "f0150",
        "f17uoq6tp427uzv7fztkbsnn64iwotfrristwpryy",
        "f24vg6ut43yw2h2jqydgbg2xq7x6f4kub3bg6as6i",
        "f3vvmn62lofvhjd2ugzca6sof2j2ubwok6cj4xxbfzz4yuxfkgobpihhd2thlanmsh3w2ptld2gqkn2jvlss4a",
        "f410fu7h6rd7gqwhcxip6t2xmc5f6odjy5yvxaih7xey",
    ] {
        assert!(
            validate_address_for_network(address, "filecoin").is_ok(),
            "{address}"
        );
    }
    assert!(
        validate_address_for_network("t17uoq6tp427uzv7fztkbsnn64iwotfrristwpryy", "filecoin")
            .is_err()
    );
    assert!(
        validate_address_for_network("f17uoq6tp427uzv7fztkbsnn64iwotfrristwpryz", "filecoin")
            .is_err()
    );
    assert!(
        validate_address_for_network("f410fU7h6rd7gqwhcxip6t2xmc5f6odjy5yvxaih7xey", "filecoin")
            .is_err()
    );
}

#[test]
fn injective_requires_the_standard_bech32_account_encoding() {
    let wrong_hrp = bech32::encode::<Bech32>(Hrp::parse("cosmos").unwrap(), &[0; 20]).unwrap();
    let wrong_length = bech32::encode::<Bech32>(Hrp::parse("inj").unwrap(), &[0; 19]).unwrap();
    let bech32m = bech32::encode::<Bech32m>(Hrp::parse("inj").unwrap(), &[0; 20]).unwrap();
    assert!(validate_address_for_network(&wrong_hrp, "injective").is_err());
    assert!(validate_address_for_network(&wrong_length, "injective").is_err());
    assert!(validate_address_for_network(&bech32m, "injective").is_err());
    assert!(
        validate_address_for_network("inj1m6kmamcpqgpsgpgxquyqjyq3zgf3g9gkzz8lqn", "injective")
            .is_ok()
    );
}

#[test]
fn validates_solana_token_transfer_recipient_as_solana_address() {
    let wallet = Wallet {
        name: "Test Wallet".to_string(),
        mnemonic: "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".to_string(),
        created_at: "2025-01-01T00:00:00Z".to_string(),
        addresses: HashMap::new(),
        wallet_password_hash: "deadbeef".to_string(),
        fiat_currency: FiatCurrency::Usd,
        usd_exchange_rate: 1.0,
        assets: vec![Asset {
            symbol: "SPL-So1111".to_string(),
            name: "So11111111111111111111111111111111111111112".to_string(),
            balance: "1000000".to_string(),
            decimals: 9,
            price_usd: 0.0,
            change_24h: 0.0,
            network: "solana".to_string(),
            token_address: Some("So11111111111111111111111111111111111111112".to_string()),
        }],
        activity: vec![],
        enabled_networks: vec!["solana".to_string()],
        auto_lock_timeout_secs: None,
    };

    assert!(
        validate_transfer(
            &wallet,
            "7VH1XhBY1DmFk98fBdLqEbDsKpr41whdM8EzipizyVCJ",
            "SPL-So1111",
            "solana",
            Some("So11111111111111111111111111111111111111112"),
            "1",
        )
        .is_ok()
    );
}

#[test]
fn token_transfer_validation_uses_contract_or_mint_identity() {
    let wallet = Wallet {
        name: "Test".to_string(),
        mnemonic: "test mnemonic".to_string(),
        created_at: Utc::now().to_rfc3339(),
        addresses: HashMap::new(),
        wallet_password_hash: "deadbeef".to_string(),
        fiat_currency: FiatCurrency::Usd,
        usd_exchange_rate: 1.0,
        assets: vec![
            Asset {
                symbol: "DUP".to_string(),
                name: "First".to_string(),
                balance: "0".to_string(),
                decimals: 6,
                price_usd: 0.0,
                change_24h: 0.0,
                network: "solana".to_string(),
                token_address: Some("So11111111111111111111111111111111111111112".to_string()),
            },
            Asset {
                symbol: "DUP".to_string(),
                name: "Second".to_string(),
                balance: "10".to_string(),
                decimals: 6,
                price_usd: 0.0,
                change_24h: 0.0,
                network: "solana".to_string(),
                token_address: Some("7VH1XhBY1DmFk98fBdLqEbDsKpr41whdM8EzipizyVCJ".to_string()),
            },
        ],
        activity: vec![],
        enabled_networks: vec!["solana".to_string()],
        auto_lock_timeout_secs: None,
    };

    assert!(
        validate_transfer(
            &wallet,
            "7VH1XhBY1DmFk98fBdLqEbDsKpr41whdM8EzipizyVCJ",
            "DUP",
            "solana",
            Some("7VH1XhBY1DmFk98fBdLqEbDsKpr41whdM8EzipizyVCJ"),
            "1",
        )
        .is_ok()
    );
    // Solana token balances are rechecked against the source ATA before signing, so this
    // static validator must not reject a send solely because a portfolio cache is stale.
    assert!(
        validate_transfer(
            &wallet,
            "7VH1XhBY1DmFk98fBdLqEbDsKpr41whdM8EzipizyVCJ",
            "DUP",
            "solana",
            Some("So11111111111111111111111111111111111111112"),
            "1",
        )
        .is_ok()
    );
}

#[test]
fn validates_eip55_checksum() {
    assert!(validate_evm_address("0xdAC17F958D2ee523a2206206994597C13D831ec7").is_ok());
    assert!(validate_evm_address("0xdac17f958d2ee523a2206206994597c13d831ec7").is_ok());
    assert!(validate_evm_address("0xDAc17f958D2eE523a2206206994597C13D831ec7").is_err());
    assert!(validate_evm_address("0xDbC17F958D2ee523a2206206994597C13D831ec7").is_err());
    assert!(validate_evm_address("0x0000000000000000000000000000000000000000").is_ok());
    assert!(validate_evm_address("0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF").is_ok());
    assert!(validate_evm_address("dAC17F958D2ee523a2206206994597C13D831ec7").is_err());
}

#[test]
fn rejects_wrong_network_and_unsupported_bitcoin_recipients() {
    assert!(
        validate_address_for_network("tb1qfm5q7m0s5p8fyj5h6q8w9zqv0cq7cx8xg42n94", "bitcoin")
            .is_err()
    );
    assert!(validate_address_for_network("mipcBbFg9gMiCh81Kj8tqqdgoZub1ZJRfn", "bitcoin").is_err());
    assert!(
        validate_address_for_network("bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq", "bitcoin")
            .is_ok()
    );
}

#[test]
fn transfer_validation_rejects_malformed_amounts_and_mints() {
    let wallet = Wallet {
        name: "Test Wallet".to_string(),
        mnemonic: "test mnemonic".to_string(),
        created_at: "2025-01-01T00:00:00Z".to_string(),
        addresses: HashMap::new(),
        wallet_password_hash: "unused".to_string(),
        fiat_currency: FiatCurrency::Usd,
        usd_exchange_rate: 1.0,
        assets: vec![Asset {
            symbol: "SPL".to_string(),
            name: "SPL".to_string(),
            balance: "10".to_string(),
            decimals: 6,
            price_usd: 0.0,
            change_24h: 0.0,
            network: "solana".to_string(),
            token_address: Some("So11111111111111111111111111111111111111112".to_string()),
        }],
        activity: vec![],
        enabled_networks: vec!["solana".to_string()],
        auto_lock_timeout_secs: None,
    };
    let recipient = "7VH1XhBY1DmFk98fBdLqEbDsKpr41whdM8EzipizyVCJ";
    let mint = "So11111111111111111111111111111111111111112";
    assert!(validate_transfer(&wallet, recipient, "SPL", "solana", Some(mint), "01").is_ok());
    assert!(validate_transfer(&wallet, recipient, "SPL", "solana", Some(mint), "+1").is_err());
    assert!(validate_transfer(&wallet, recipient, "SPL", "solana", Some(mint), "0").is_err());
    assert!(validate_transfer(&wallet, recipient, "SPL", "solana", Some("invalid"), "1").is_err());
}
