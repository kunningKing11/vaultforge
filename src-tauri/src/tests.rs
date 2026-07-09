use chrono::Utc;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::activity::{activity, hash_secret};
use crate::assets::cached_asset;
use crate::commands::tx::{ensure_native_balance_covers_debit, required_native_debit};
use crate::derivation::{
    address_from_seed, bitcoin_bech32_address, derive_addresses_from_mnemonic,
};
use crate::dto::{Asset, Wallet};
use crate::providers::bitcoin::{
    BitcoinUtxo, parse_bitcoin_balance, parse_bitcoin_fee_rate, parse_bitcoin_utxos,
};
use crate::providers::evm::EVM_NETWORKS;
use crate::providers::get_provider;
use crate::providers::solana::{
    parse_latest_solana_blockhash, parse_solana_balance, parse_solana_fee_for_message,
    parse_solana_rent_exemption, parse_solana_token_account_state, parse_solana_token_accounts,
    parse_solana_tx_status,
};
use crate::state::{AppState, StoredWalletMetadata, session_from_state};
use crate::storage::{decrypt_wallet, derive_storage_key, encrypt_wallet};
use crate::tx::bitcoin::{bitcoin_estimated_vbytes, bitcoin_select_coins, bitcoin_signed_transfer};
use crate::tx::evm::{Eip1559TxDraft, encode_erc20_transfer, sign_eip1559_transfer};
use crate::tx::solana::{
    sign_solana_token_transfer_with_blockhash, sign_solana_transfer_with_blockhash,
    solana_associated_token_address,
};
use crate::validation::{validate_address_for_symbol, validate_evm_address, validate_transfer};

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
fn validates_solana_token_transfer_recipient_as_solana_address() {
    let wallet = Wallet {
        name: "Test Wallet".to_string(),
        mnemonic: "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".to_string(),
        created_at: "2025-01-01T00:00:00Z".to_string(),
        address: "0xdAC17F958D2ee523a2206206994597C13D831ec7".to_string(),
        addresses: HashMap::new(),
        passphrase_hash: "deadbeef".to_string(),
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
    };

    assert!(
        validate_transfer(
            &wallet,
            "7VH1XhBY1DmFk98fBdLqEbDsKpr41whdM8EzipizyVCJ",
            "SPL-So1111",
            "solana",
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
fn parses_solana_token_accounts() {
    let json = serde_json::json!({
        "jsonrpc": "2.0",
        "result": {
            "value": [{
                "account": {
                    "data": {
                        "parsed": {
                            "info": {
                                "mint": "So11111111111111111111111111111111111111112",
                                "tokenAmount": {
                                    "amount": "1234500",
                                    "decimals": 6
                                }
                            }
                        }
                    }
                }
            }]
        },
        "id": 1
    });
    let accounts = parse_solana_token_accounts(&json).unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(
        accounts[0].mint,
        "So11111111111111111111111111111111111111112"
    );
    assert_eq!(accounts[0].amount, "1234500");
    assert_eq!(accounts[0].decimals, 6);
}

#[test]
fn parses_solana_status_and_fee() {
    let pending = serde_json::json!({
        "jsonrpc": "2.0",
        "result": { "value": [null] },
        "id": 1
    });
    assert_eq!(parse_solana_tx_status(&pending).unwrap(), None);

    let confirmed = serde_json::json!({
        "jsonrpc": "2.0",
        "result": { "value": [{ "err": null, "confirmationStatus": "finalized" }] },
        "id": 1
    });
    assert_eq!(
        parse_solana_tx_status(&confirmed).unwrap(),
        Some("confirmed".to_string())
    );

    let failed = serde_json::json!({
        "jsonrpc": "2.0",
        "result": { "value": [{ "err": { "InstructionError": [0, "Custom"] } }] },
        "id": 1
    });
    assert_eq!(
        parse_solana_tx_status(&failed).unwrap(),
        Some("failed".to_string())
    );

    let fee = serde_json::json!({
        "jsonrpc": "2.0",
        "result": { "value": 5000 },
        "id": 1
    });
    assert_eq!(parse_solana_fee_for_message(&fee).unwrap(), 5000);
}

#[test]
fn parses_solana_token_account_state() {
    let owner = "7VH1XhBY1DmFk98fBdLqEbDsKpr41whdM8EzipizyVCJ";
    let mint = "So11111111111111111111111111111111111111112";

    let missing = serde_json::json!({
        "jsonrpc": "2.0",
        "result": { "value": null },
        "id": 1
    });
    assert_eq!(
        parse_solana_token_account_state(&missing, owner, mint).unwrap(),
        None
    );

    let existing = serde_json::json!({
        "jsonrpc": "2.0",
        "result": {
            "value": {
                "owner": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
                "data": {
                    "parsed": {
                        "info": {
                            "owner": owner,
                            "mint": mint
                        }
                    }
                }
            }
        },
        "id": 1
    });
    assert!(parse_solana_token_account_state(&existing, owner, mint)
        .unwrap()
        .is_some());

    assert!(parse_solana_token_account_state(
        &existing,
        owner,
        "TokenzQdBNbLqP5VEhdkAS6EP1z9kF9t79yDMQH9z"
    )
    .is_err());
}

#[test]
fn parses_solana_rent_exemption() {
    let json = serde_json::json!({
        "jsonrpc": "2.0",
        "result": 2039280,
        "id": 1
    });
    assert_eq!(parse_solana_rent_exemption(&json).unwrap(), 2039280);
}

#[test]
fn parses_latest_solana_blockhash() {
    let json = serde_json::json!({
        "jsonrpc": "2.0",
        "result": {
            "context": { "slot": 1 },
            "value": {
                "blockhash": "11111111111111111111111111111111",
                "lastValidBlockHeight": 123
            }
        },
        "id": 1
    });
    assert_eq!(
        parse_latest_solana_blockhash(&json).unwrap(),
        "11111111111111111111111111111111"
    );
}

#[test]
fn signs_solana_native_transfer() {
    let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    let addresses = derive_addresses_from_mnemonic(mnemonic).unwrap();
    let from = addresses.get("solana").unwrap();
    let signed = sign_solana_transfer_with_blockhash(
        mnemonic,
        from,
        "7VH1XhBY1DmFk98fBdLqEbDsKpr41whdM8EzipizyVCJ",
        1_000_000,
        "11111111111111111111111111111111",
        5000,
    )
    .unwrap();

    assert!(!signed.signature.is_empty());
    assert!(!signed.raw_tx_base64.is_empty());
    assert_eq!(signed.recent_blockhash, "11111111111111111111111111111111");
    assert_eq!(signed.fee_lamports, 5000);
}

#[test]
fn signs_solana_spl_token_transfer() {
    let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
    let addresses = derive_addresses_from_mnemonic(mnemonic).unwrap();
    let from = addresses.get("solana").unwrap();
    let signed = sign_solana_token_transfer_with_blockhash(
        mnemonic,
        from,
        "7VH1XhBY1DmFk98fBdLqEbDsKpr41whdM8EzipizyVCJ",
        "So11111111111111111111111111111111111111112",
        1_000_000,
        9,
        "11111111111111111111111111111111",
        5000,
    )
    .unwrap();

    assert!(!signed.signature.is_empty());
    assert!(!signed.raw_tx_base64.is_empty());
    assert_eq!(signed.fee_lamports, 5000);
}

#[test]
fn derives_solana_associated_token_address() {
    let ata = solana_associated_token_address(
        "7VH1XhBY1DmFk98fBdLqEbDsKpr41whdM8EzipizyVCJ",
        "So11111111111111111111111111111111111111112",
    )
    .unwrap();

    assert!(!ata.is_empty());
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
    let amount = 1_000_000_000_000_000_000;
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
