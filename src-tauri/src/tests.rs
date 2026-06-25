use chrono::Utc;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::activity::{activity, hash_secret};
use crate::derivation::{
    address_from_seed, bitcoin_bech32_address, derive_addresses_from_mnemonic,
};
use crate::dto::{Asset, Wallet};
use crate::providers::bitcoin::{
    BitcoinUtxo, bitcoin_estimated_vbytes, bitcoin_select_coins, bitcoin_signed_transfer,
    parse_bitcoin_balance, parse_bitcoin_fee_rate, parse_bitcoin_utxos,
};
use crate::providers::evm::EVM_NETWORKS;
use crate::providers::solana::parse_solana_balance;
use crate::providers::{cached_asset, get_provider};
use crate::state::{AppState, StoredWalletMetadata, session_from_state};
use crate::storage::{decrypt_wallet, derive_storage_key, encrypt_wallet};
use crate::tx::evm::{Eip1559TxDraft, encode_erc20_transfer, sign_eip1559_transfer};
use crate::validation::{validate_address_for_symbol, validate_evm_address};

fn starter_assets(network: &str) -> Vec<Asset> {
    vec![
        Asset {
            symbol: "ETH".to_string(),
            name: "Ethereum".to_string(),
            balance: "2482100000000000000000".to_string(),
            decimals: 18,
            price_usd: 3480.62,
            change_24h: 2.84,
            network: network.to_string(),
        },
        Asset {
            symbol: "BTC".to_string(),
            name: "Bitcoin".to_string(),
            balance: "184200000000".to_string(),
            decimals: 8,
            price_usd: 102_240.12,
            change_24h: -0.62,
            network: network.to_string(),
        },
        Asset {
            symbol: "SOL".to_string(),
            name: "Solana".to_string(),
            balance: "82450000000".to_string(),
            decimals: 9,
            price_usd: 184.33,
            change_24h: 5.18,
            network: network.to_string(),
        },
        Asset {
            symbol: "USDC".to_string(),
            name: "USD Coin".to_string(),
            balance: "8420000000".to_string(),
            decimals: 6,
            price_usd: 1.0,
            change_24h: 0.01,
            network: network.to_string(),
        },
    ]
}

#[test]
fn validates_asset_address_formats() {
    assert!(
        validate_address_for_symbol("0xdAC17F958D2ee523a2206206994597C13D831ec7", "ETH").is_ok()
    );
    assert!(
        validate_address_for_symbol("0xdac17f958d2ee523a2206206994597c13d831ec7", "ETH").is_ok()
    );
    assert!(
        validate_address_for_symbol("bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq", "BTC").is_ok()
    );
    assert!(validate_address_for_symbol("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa", "BTC").is_ok());
    assert!(validate_address_for_symbol("3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy", "BTC").is_ok());
    assert!(
        validate_address_for_symbol("7VH1XhBY1DmFk98fBdLqEbDsKpr41whdM8EzipizyVCJ", "SOL").is_ok()
    );
    assert!(validate_address_for_symbol("t1eB29zcZ2v3AQvAEtcNrERsWQPmxyTN4DF", "ZEC").is_ok());
    assert!(validate_address_for_symbol("f1ke28mVhmmiSdiFRybu3ak3NnEqpx3o3Bk", "FIL").is_ok());
    assert!(
        validate_address_for_symbol("inj1m6kmamcpqgpsgpgxquyqjyq3zgf3g9gkzz8lqn", "INJ").is_ok()
    );
    assert!(
        validate_address_for_symbol("0xdAC17F958D2ee523a2206206994597C13D831ec7", "MATIC").is_ok()
    );
    assert!(validate_address_for_symbol("0xinvalid", "ETH").is_err());
    assert!(validate_address_for_symbol("bc1q", "BTC").is_err());
    assert!(validate_address_for_symbol("invalid", "SOL").is_err());
}

#[test]
fn validates_eip55_checksum() {
    assert!(validate_evm_address("0xdAC17F958D2ee523a2206206994597C13D831ec7").is_ok());
    assert!(validate_evm_address("0xdac17f958d2ee523a2206206994597c13d831ec7").is_ok());
    assert!(validate_evm_address("0xDAc17f958D2eE523a2206206994597C13D831ec7").is_err());
    assert!(validate_evm_address("0xDbC17F958D2ee523a2206206994597C13D831ec7").is_err());
    assert!(validate_evm_address("0x0000000000000000000000000000000000000000").is_ok());
    assert!(validate_evm_address("0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF").is_ok());
}

#[test]
fn provider_trait_covers_all_chains() {
    for symbol in &["ETH", "BTC", "SOL", "ZEC", "FIL", "INJ", "MATIC"] {
        let provider = get_provider(symbol);
        assert!(provider.is_some(), "No provider for symbol {symbol}");
    }
}

#[test]
fn selects_cached_asset_by_network_and_symbol() {
    let assets = starter_assets("ethereum");
    let cached = cached_asset(&assets, "ethereum", "ETH").unwrap();
    assert_eq!(cached.symbol, "ETH");
    assert_eq!(cached.network, "ethereum");
    assert_eq!(cached.balance, "2482100000000000000000");
    assert!(cached_asset(&assets, "polygon", "ETH").is_none());
    assert!(cached_asset(&assets, "ethereum", "MATIC").is_none());
}

#[test]
fn parses_bitcoin_balance_with_mempool_values() {
    let json = serde_json::json!({
        "chain_stats": {
            "funded_txo_sum": 5000,
            "spent_txo_sum": 1200
        },
        "mempool_stats": {
            "funded_txo_sum": 700,
            "spent_txo_sum": 200
        }
    });
    assert_eq!(parse_bitcoin_balance(&json).unwrap(), 4300);
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
    let utxos = parse_bitcoin_utxos(&json).unwrap();
    assert_eq!(utxos.len(), 1);
    assert_eq!(utxos[0].value, 50_000);

    let fees = serde_json::json!({ "3": 2.1, "6": 1.4 });
    assert_eq!(parse_bitcoin_fee_rate(&fees).unwrap(), 3);
}

#[test]
fn selects_bitcoin_coins_with_change() {
    let utxos = vec![BitcoinUtxo {
        txid: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f".to_string(),
        vout: 0,
        value: 50_000,
        confirmed: true,
    }];
    let (selected, fee, change) = bitcoin_select_coins(&utxos, 10_000, 2).unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(fee, bitcoin_estimated_vbytes(1, 2) * 2);
    assert_eq!(change, 50_000 - 10_000 - fee);
}

#[test]
fn signs_bitcoin_p2wpkh_transfer() {
    let private_key = [0x01u8; 32];
    let from = bitcoin_bech32_address(&private_key, false).unwrap();
    let utxos = vec![BitcoinUtxo {
        txid: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f".to_string(),
        vout: 0,
        value: 50_000,
        confirmed: true,
    }];
    let signed = bitcoin_signed_transfer(
        &private_key,
        &from,
        "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq",
        10_000,
        &utxos,
        2,
    )
    .unwrap();

    assert_eq!(signed.txid.len(), 64);
    assert!(signed.raw_tx_hex.starts_with("020000000001"));
    assert!(!signed.first_signature_hex.is_empty());
    assert_eq!(signed.fee_sats, bitcoin_estimated_vbytes(1, 2) * 2);
    assert_eq!(signed.post_balance, 50_000 - 10_000 - signed.fee_sats);
}

#[test]
fn parses_solana_balance_lamports() {
    let json = serde_json::json!({
        "jsonrpc": "2.0",
        "result": {
            "context": { "slot": 1 },
            "value": 123456789u64
        },
        "id": 1
    });
    assert_eq!(parse_solana_balance(&json).unwrap(), 123456789);
}

#[test]
fn derives_documented_wallet_paths_deterministically() {
    let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    let addresses = derive_addresses_from_mnemonic(mnemonic).unwrap();
    assert_eq!(addresses.len(), 6);
    assert_eq!(
        addresses.get("evm").unwrap(),
        "0x9858effd232b4033e47d90003d41ec34ecaeda94"
    );
    assert_eq!(
        addresses.get("bitcoin").unwrap(),
        "bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu"
    );
    assert_eq!(
        addresses.get("zcash").unwrap(),
        "t1XVXWCvpMgBvUaed4XDqWtgQgJSu1Ghz7F"
    );
    assert_eq!(
        addresses.get("solana").unwrap(),
        "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk"
    );
    assert_eq!(
        addresses.get("filecoin").unwrap(),
        "f1fFXqnEMPFe1NoAajxRKukEBLwshG1LQQC"
    );
    assert_eq!(
        addresses.get("injective").unwrap(),
        "inj1gsvdpdxec8hsu57lhxg5xem7refr233zkczfgv"
    );
}

#[test]
fn locked_session_does_not_expose_secrets() {
    let mut state = AppState::from_storage(PathBuf::from("/nonexistent/wallet.json"));
    let wallet = Wallet {
        name: "Secret Wallet".to_string(),
        mnemonic: "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".to_string(),
        created_at: "2025-01-01T00:00:00Z".to_string(),
        address: "0xdAC17F958D2ee523a2206206994597C13D831ec7".to_string(),
        addresses: HashMap::new(),
        passphrase_hash: "deadbeef".to_string(),
        assets: vec![],
        activity: vec![],
    };
    state.wallet = Some(wallet);
    state.locked = true;
    state.stored_wallet = Some(StoredWalletMetadata {
        wallet_name: "Secret Wallet".to_string(),
    });
    let session = session_from_state(&state);
    assert_eq!(session.has_wallet, true);
    assert_eq!(session.locked, true);
    assert!(session.address.is_none());
    assert!(session.addresses.is_none());
    assert!(session.assets.is_empty());
    assert!(session.activity.is_empty());
}

#[test]
fn constructs_valid_eip1559_signature() {
    use k256::ecdsa::SigningKey;
    let private_key = [0xabu8; 32];
    let signing_key = SigningKey::from_bytes((&private_key).into()).unwrap();
    let result = sign_eip1559_transfer(&Eip1559TxDraft {
        signing_key: &signing_key,
        chain_id: 1,
        nonce: 0,
        max_priority_fee_per_gas: 1_000_000_000,
        max_fee_per_gas: 1_000_000_000,
        gas_limit: 21000,
        to: "0xdAC17F958D2ee523a2206206994597C13D831ec7",
        value: 1_000_000_000_000_000_000u128,
        data: &[],
    });
    assert!(result.is_ok());
    let (_raw, tx_hash, _raw_hex, r, s) = result.unwrap();
    assert!(tx_hash.starts_with("0x"));
    assert_eq!(tx_hash.len(), 66);
    assert_eq!(r.len(), 64);
    assert_eq!(s.len(), 64);
    assert!(!_raw.is_empty());
}

#[test]
fn encodes_erc20_transfer_abi() {
    let recipient = "0xdAC17F958D2ee523a2206206994597C13D831ec7";
    let amount: u128 = 1_000_000_000_000_000_000;
    let data = encode_erc20_transfer(recipient, amount).unwrap();
    assert!(!data.is_empty());
    assert_eq!(data.len(), 4 + 32 + 32);
    assert_eq!(&data[..4], &[0xa9, 0x05, 0x9c, 0xbb]);
    let recip_bytes = hex::decode(recipient.trim_start_matches("0x")).unwrap();
    assert_eq!(&data[16..36], &recip_bytes[..]);
    assert_eq!(data[data.len() - 1], 0x00);
}

#[test]
fn signs_erc20_transfer() {
    use k256::ecdsa::SigningKey;
    let private_key = [0xabu8; 32];
    let signing_key = SigningKey::from_bytes((&private_key).into()).unwrap();
    let data =
        encode_erc20_transfer("0xdAC17F958D2ee523a2206206994597C13D831ec7", 1_000_000).unwrap();
    let result = sign_eip1559_transfer(&Eip1559TxDraft {
        signing_key: &signing_key,
        chain_id: 1,
        nonce: 0,
        max_priority_fee_per_gas: 1_000_000_000,
        max_fee_per_gas: 1_000_000_000,
        gas_limit: 50000,
        to: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
        value: 0,
        data: &data,
    });
    assert!(result.is_ok());
    let (_raw, tx_hash, _raw_hex, r, s) = result.unwrap();
    assert!(tx_hash.starts_with("0x"));
    assert_eq!(tx_hash.len(), 66);
    assert_eq!(r.len(), 64);
    assert_eq!(s.len(), 64);
}

#[test]
fn derives_same_key_with_same_salt() {
    let (key, salt) = derive_storage_key("correct horse battery staple", None).unwrap();
    let (same_key, same_salt) =
        derive_storage_key("correct horse battery staple", Some(&salt)).unwrap();
    assert_eq!(key, same_key);
    assert_eq!(salt, same_salt);
}

#[test]
fn encrypts_and_decrypts_wallet_payload() {
    let passphrase = "Correct horse battery staple 42!";
    let wallet = Wallet {
        name: "Test Wallet".to_string(),
        mnemonic: "test mnemonic".to_string(),
        created_at: Utc::now().to_rfc3339(),
        address: address_from_seed("test seed"),
        addresses: HashMap::new(),
        passphrase_hash: hash_secret(passphrase),
        assets: starter_assets("ethereum"),
        activity: vec![activity("system", "Created", "Local", "1")],
    };
    let (key, salt) = derive_storage_key(passphrase, None).unwrap();
    let stored = encrypt_wallet(&wallet, &key, &salt).unwrap();
    assert!(!stored.ciphertext.contains(&wallet.address));

    let decrypted = decrypt_wallet(&stored, passphrase).unwrap();
    assert_eq!(decrypted.name, wallet.name);
    assert_eq!(decrypted.mnemonic, wallet.mnemonic);
    assert_eq!(decrypted.created_at, wallet.created_at);
}

#[test]
fn looks_up_evm_network_configs() {
    let ethereum = EVM_NETWORKS.iter().find(|c| c.id == "ethereum").unwrap();
    assert_eq!(ethereum.display_name, "Ethereum");
    assert_eq!(ethereum.chain_id, 1);
    assert_eq!(ethereum.native_symbol, "ETH");
    assert_eq!(ethereum.rpc_url, "https://ethereum-rpc.publicnode.com");

    let avalanche = EVM_NETWORKS.iter().find(|c| c.id == "avalanche_c").unwrap();
    assert_eq!(avalanche.chain_id, 43114);
    assert_eq!(avalanche.native_symbol, "AVAX");
}
