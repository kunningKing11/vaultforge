use super::{
    parse_latest_solana_blockhash, parse_solana_balance, parse_solana_fee_for_message,
    parse_solana_mint_decimals, parse_solana_rent_exemption, parse_solana_simulation,
    parse_solana_token_account_state, parse_solana_token_accounts, parse_solana_tx_status,
};

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
    let owner = "7VH1XhBY1DmFk98fBdLqEbDsKpr41whdM8EzipizyVCJ";
    let json = serde_json::json!({
        "jsonrpc": "2.0",
        "result": {
            "value": [{
                "pubkey": "4vJ9JU1bJJE96FWSJKvHsmmF3qN8oQfZ1ZTHwF3GvH2",
                "account": {
                    "owner": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
                    "data": {
                        "parsed": {
                            "type": "account",
                            "info": {
                                "mint": "So11111111111111111111111111111111111111112",
                                "owner": owner,
                                "state": "initialized",
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
    let accounts = parse_solana_token_accounts(&json, owner).unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(
        accounts[0].mint,
        "So11111111111111111111111111111111111111112"
    );
    assert_eq!(
        accounts[0].address,
        "4vJ9JU1bJJE96FWSJKvHsmmF3qN8oQfZ1ZTHwF3GvH2"
    );
    assert_eq!(accounts[0].amount, 1_234_500);
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
                        "type": "account",
                        "info": {
                            "owner": owner,
                            "mint": mint,
                            "state": "initialized",
                            "tokenAmount": {
                                "amount": "1234500",
                                "decimals": 6
                            }
                        }
                    }
                }
            }
        },
        "id": 1
    });
    let state = parse_solana_token_account_state(&existing, owner, mint)
        .unwrap()
        .unwrap();
    assert_eq!(state.amount, 1_234_500);
    assert_eq!(state.decimals, 6);

    assert!(
        parse_solana_token_account_state(
            &existing,
            owner,
            "TokenzQdBNbLqP5VEhdkAS6EP1z9kF9t79yDMQH9z"
        )
        .is_err()
    );
}

#[test]
fn parses_classic_solana_mint_and_simulation_preflight() {
    let mint = serde_json::json!({
        "jsonrpc": "2.0",
        "result": {
            "value": {
                "owner": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
                "data": {
                    "parsed": {
                        "type": "mint",
                        "info": { "decimals": 6 }
                    }
                }
            }
        },
        "id": 1
    });
    assert_eq!(parse_solana_mint_decimals(&mint).unwrap(), 6);

    let token_2022_mint = serde_json::json!({
        "jsonrpc": "2.0",
        "result": {
            "value": {
                "owner": "TokenzQdBNbLqP5VEhdkAS6EP1z9kF9t79yDMQH9z",
                "data": { "parsed": { "type": "mint", "info": { "decimals": 6 } } }
            }
        },
        "id": 1
    });
    assert!(parse_solana_mint_decimals(&token_2022_mint).is_err());

    let success = serde_json::json!({
        "jsonrpc": "2.0",
        "result": { "value": { "err": null } },
        "id": 1
    });
    assert!(parse_solana_simulation(&success).is_ok());

    let failed = serde_json::json!({
        "jsonrpc": "2.0",
        "result": { "value": { "err": { "InstructionError": [1, "Custom"] } } },
        "id": 1
    });
    assert!(parse_solana_simulation(&failed).is_err());
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
