use super::{
    parse_latest_solana_blockhash, parse_solana_balance, parse_solana_fee_for_message,
    parse_solana_mint_decimals, parse_solana_rent_exemption, parse_solana_simulation,
    parse_solana_token_account_state, parse_solana_token_accounts, parse_solana_tx_status,
};
use crate::tx::tx_status::{Execution, Finality, legacy_tx_status};

#[test]
fn merges_live_solana_balances_with_trusted_metadata_and_confirmed_zeros() {
    let config = super::solana_config().unwrap();
    let jup = "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN";
    let usdc = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    let unknown = bs58::encode([42u8; 32]).into_string();
    let cached = vec![
        crate::dto::Asset {
            solana_token: None,
            symbol: "stale symbol".into(),
            unicode_symbol: None,
            name: "stale name".into(),
            balance: "999999999".into(),
            decimals: 6,
            price_usd: 0.5,
            change_24h: 2.0,
            network: "solana".into(),
            token_address: Some(jup.into()),
        },
        crate::dto::Asset {
            solana_token: None,
            symbol: "USDC".into(),
            unicode_symbol: None,
            name: "USD Coin".into(),
            balance: "888888888".into(),
            decimals: 6,
            price_usd: 1.0,
            change_24h: 0.0,
            network: "solana".into(),
            token_address: Some(usdc.into()),
        },
    ];
    let accounts = [(jup, 100, 6), (jup, 200, 6), (unknown.as_str(), 42, 9)]
        .into_iter()
        .map(|(mint, amount, decimals)| super::SolanaTokenAccount {
            capabilities: None,
            address: String::new(),
            mint: mint.into(),
            amount,
            decimals,
        })
        .collect();
    let assets = super::solana_token_assets(config, accounts, &cached);
    assert_eq!(assets.len(), config.tokens.len() + 1);
    let find = |mint: &str| {
        assets
            .iter()
            .find(|asset| asset.token_address.as_deref() == Some(mint))
            .unwrap()
    };
    assert_eq!(find(jup).balance, "300");
    assert_eq!(find(jup).symbol, "JUP");
    assert_eq!(find(jup).name, "Jupiter");
    assert_eq!(find(jup).decimals, 6);
    assert_eq!(find(jup).price_usd, 0.5);
    assert_eq!(find(usdc).balance, "0");
    assert_eq!(find(&unknown).balance, "42");
    assert_eq!(find(&unknown).decimals, 9);
    assert_eq!(find(&unknown).symbol, format!("SPL-{}", &unknown[..6]));

    let mut changed = config.clone();
    changed
        .tokens
        .iter_mut()
        .find(|token| token.symbol == "JUP")
        .unwrap()
        .decimals = 9;
    for amount in [0, 100] {
        let assets = super::solana_token_assets(
            &changed,
            vec![super::SolanaTokenAccount {
                capabilities: None,
                address: String::new(),
                mint: jup.into(),
                amount,
                decimals: 6,
            }],
            &[],
        );
        assert_eq!(
            assets
                .iter()
                .find(|asset| asset.symbol == "JUP")
                .unwrap()
                .decimals,
            6
        );
    }
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

    let error = serde_json::json!({
        "jsonrpc": "2.0",
        "error": { "code": -32005, "message": "node unhealthy" },
        "id": 1
    });
    assert_eq!(
        parse_solana_balance(&error).unwrap_err(),
        "Solana balance RPC error: {\"code\":-32005,\"message\":\"node unhealthy\"}"
    );
}

#[test]
fn parses_solana_commitment_without_using_slots_as_heights() {
    for commitment in ["processed", "confirmed", "finalized"] {
        let json = serde_json::json!({"result":{"value":[{"slot":42,"err":null,"confirmationStatus":commitment,"confirmations":if commitment=="finalized" {serde_json::Value::Null} else {serde_json::json!(0)}}]}});
        let observation = super::parse_solana_tx_observation(&json).unwrap().unwrap();
        assert_eq!(observation.execution, Execution::Succeeded);
        assert_eq!(
            observation.finality,
            if commitment == "finalized" {
                Finality::Finalized
            } else {
                Finality::Included
            }
        );
        assert_eq!(observation.slot, Some(42));
        assert_eq!(observation.block_height, None);
        assert_eq!(observation.block_hash, None);
        assert_eq!(
            observation.confirmations,
            if commitment == "finalized" {
                None
            } else {
                Some(0)
            }
        );
        assert_eq!(
            legacy_tx_status(Some(observation)).as_deref(),
            Some("confirmed")
        );
    }
    let failure = serde_json::json!({"result":{"value":[{"slot":42,"err":{"InstructionError":[0,"Custom"]},"confirmationStatus":"finalized"}]}});
    let observation = super::parse_solana_tx_observation(&failure)
        .unwrap()
        .unwrap();
    assert_eq!(observation.execution, Execution::Failed);
    assert_eq!(observation.finality, Finality::Finalized);
    assert_eq!(
        legacy_tx_status(Some(observation)).as_deref(),
        Some("failed")
    );
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
fn parses_solana_status_and_fee() {
    let pending = serde_json::json!({
        "jsonrpc": "2.0",
        "result": { "value": [null] },
        "id": 1
    });
    assert_eq!(parse_solana_tx_status(&pending).unwrap(), None);

    let confirmed = serde_json::json!({
        "jsonrpc": "2.0",
        "result": { "value": [{ "slot": 42, "err": null, "confirmationStatus": "finalized" }] },
        "id": 1
    });
    assert_eq!(
        parse_solana_tx_status(&confirmed).unwrap(),
        Some("confirmed".to_string())
    );

    let failed = serde_json::json!({
        "jsonrpc": "2.0",
        "result": { "value": [{ "slot": 42, "confirmationStatus": "confirmed", "err": { "InstructionError": [0, "Custom"] } }] },
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
fn portfolio_discovers_both_programs_and_verifies_live_mints() {
    use crate::solana_tokens::{
        CLASSIC, TOKEN_2022,
        tests::{extend, holding_bytes, mint_bytes, value},
    };
    use serde_json::json;
    use solana_pubkey::Pubkey;
    use spl_token_2022_interface::extension::ExtensionType;
    let owner = Pubkey::from([8; 32]).to_string();
    let mut token = holding_bytes([7; 32], [8; 32], 123, true);
    extend(
        &mut token,
        2,
        ExtensionType::TransferFeeAmount,
        &999u64.to_le_bytes(),
    );
    let mut mint = mint_bytes(9);
    extend(&mut mint, 1, ExtensionType::TransferFeeConfig, &[0; 108]);
    let (url, server) = portfolio_rpc(vec![
        json!({"result":{"value":[{"pubkey":Pubkey::from([2;32]).to_string(),"account":value(CLASSIC, &holding_bytes([3;32],[8;32],50,false))}]}}),
        json!({"result":{"value":[{"pubkey":Pubkey::from([4;32]).to_string(),"account":value(TOKEN_2022, &token)}]}}),
        json!({"result":{"value":[value(CLASSIC,&mint_bytes(6)),value(TOKEN_2022,&mint)]}}),
    ]);
    let accounts = super::super::http::tests::runtime()
        .block_on(super::fetch_solana_portfolio_accounts_at(
            &reqwest::Client::new(),
            &url,
            &owner,
        ))
        .unwrap();
    assert_eq!(accounts.len(), 2);
    assert_eq!((accounts[1].amount, accounts[1].decimals), (123, 9));
    let requests = server.join().unwrap();
    assert_eq!(requests[0]["params"][1]["programId"], CLASSIC);
    assert_eq!(requests[1]["params"][1]["programId"], TOKEN_2022);
    assert_eq!(requests[2]["method"], "getMultipleAccounts");
}

fn portfolio_rpc(
    responses: Vec<serde_json::Value>,
) -> (String, std::thread::JoinHandle<Vec<serde_json::Value>>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let thread = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for response in responses {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let body = loop {
                let mut buffer = [0; 4096];
                let n = stream.read(&mut buffer).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buffer[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                    let len: usize = headers
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length: "))
                        .unwrap()
                        .parse()
                        .unwrap();
                    if bytes.len() >= end + 4 + len {
                        break bytes[end + 4..end + 4 + len].to_vec();
                    }
                }
            };
            requests.push(serde_json::from_slice(&body).unwrap());
            let body = response.to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        }
        requests
    });
    (url, thread)
}

#[test]
fn portfolio_rejects_incomplete_discovery_and_mint_program_mismatch() {
    use crate::solana_tokens::{
        CLASSIC, TOKEN_2022,
        tests::{holding_bytes, mint_bytes, value},
    };
    use serde_json::json;
    use solana_pubkey::Pubkey;
    let owner = Pubkey::from([8; 32]).to_string();
    let runtime = super::super::http::tests::runtime();
    for mismatch in [false, true] {
        let mut replies = vec![json!({"result":{"value":[]}})];
        if mismatch {
            replies.push(json!({"result":{"value":[{"pubkey":Pubkey::from([4;32]).to_string(),"account":value(TOKEN_2022,&holding_bytes([7;32],[8;32],5,false))}]}}));
            replies.push(json!({"result":{"value":[value(CLASSIC,&mint_bytes(6))]}}));
        } else {
            replies.push(json!({"error":{"code":-32005,"message":"unavailable"}}));
        }
        let (url, server) = portfolio_rpc(replies);
        assert!(
            runtime
                .block_on(super::fetch_solana_portfolio_accounts_at(
                    &reqwest::Client::new(),
                    &url,
                    &owner
                ))
                .is_err()
        );
        server.join().unwrap();
    }
}

#[test]
fn portfolio_rejects_missing_nonzero_mints_and_malformed_zero_mints() {
    use crate::solana_tokens::{
        TOKEN_2022,
        tests::{holding_bytes, value},
    };
    use serde_json::json;
    use solana_pubkey::Pubkey;
    let owner = Pubkey::from([8; 32]).to_string();
    let runtime = super::super::http::tests::runtime();
    for (amount, mint) in [(1, json!(null)), (0, json!({}))] {
        let (url, server) = portfolio_rpc(vec![
            json!({"result":{"value":[]}}),
            json!({"result":{"value":[{"pubkey":Pubkey::from([4;32]).to_string(),"account":value(TOKEN_2022,&holding_bytes([7;32],[8;32],amount,false))}]}}),
            json!({"result":{"value":[mint]}}),
        ]);
        let error = runtime
            .block_on(super::fetch_solana_portfolio_accounts_at(
                &reqwest::Client::new(),
                &url,
                &owner,
            ))
            .unwrap_err();
        assert_eq!(
            error,
            if amount > 0 {
                "Solana mint descriptor missing"
            } else {
                "Missing Solana account program"
            }
        );
        server.join().unwrap();
    }
}

#[test]
fn portfolio_retains_cached_tokens_when_either_program_fails() {
    use serde_json::json;
    let owner = solana_pubkey::Pubkey::from([8; 32]).to_string();
    let cached = vec![crate::dto::Asset {
        solana_token: None,
        symbol: "cached".into(),
        unicode_symbol: None,
        name: "Cached token".into(),
        balance: "123".into(),
        decimals: 6,
        price_usd: 1.0,
        change_24h: 0.0,
        network: "solana".into(),
        token_address: Some(solana_pubkey::Pubkey::from([7; 32]).to_string()),
    }];
    let runtime = super::super::http::tests::runtime();
    for first_fails in [false, true] {
        let mut replies = vec![json!({"result":{"value":50}})];
        if !first_fails {
            replies.push(json!({"result":{"value":[]}}));
        }
        replies.push(json!({"error":{"code":-32602,"message":"program unavailable"}}));
        let (url, server) = portfolio_rpc(replies);
        let refresh = runtime.block_on(super::fetch_solana_assets_at(
            &reqwest::Client::new(),
            &url,
            &owner,
            &cached,
        ));
        assert!(refresh.balance_failed);
        assert_eq!(refresh.assets.len(), 2); // native + cache; no fabricated registry zeros
        assert_eq!(refresh.assets[1].balance, "123");
        assert_eq!(refresh.assets[1].token_address, cached[0].token_address);
        server.join().unwrap();
    }
}

#[test]
fn portfolio_skips_closed_mints_with_zero_balance_and_keeps_live_holdings() {
    use crate::solana_tokens::{
        CLASSIC, TOKEN_2022,
        tests::{holding_bytes, mint_bytes, value},
    };
    use serde_json::json;
    use solana_pubkey::Pubkey;
    let owner = Pubkey::from([8; 32]).to_string();
    let live_mint = Pubkey::from([3; 32]).to_string();
    let (url, server) = portfolio_rpc(vec![
        json!({"result":{"value":[{"pubkey":Pubkey::from([2;32]).to_string(),"account":value(CLASSIC,&holding_bytes([3;32],[8;32],50,false))}]}}),
        json!({"result":{"value":[{"pubkey":Pubkey::from([4;32]).to_string(),"account":value(TOKEN_2022,&holding_bytes([7;32],[8;32],0,false))}]}}),
        json!({"result":{"value":[value(CLASSIC,&mint_bytes(6)),null]}}),
    ]);
    let accounts = super::super::http::tests::runtime()
        .block_on(super::fetch_solana_portfolio_accounts_at(
            &reqwest::Client::new(),
            &url,
            &owner,
        ))
        .unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].mint, live_mint);
    assert_eq!((accounts[0].amount, accounts[0].decimals), (50, 6));
    server.join().unwrap();
}

#[test]
fn rejects_malformed_solana_observation_fields_and_rpc_errors() {
    let valid = serde_json::json!({"result":{"value":[{"slot":42,"err":null,"confirmationStatus":"confirmed","confirmations":1}]}});
    for (field, value) in [
        ("slot", serde_json::json!(-1)),
        ("slot", serde_json::Value::Null),
        ("err", serde_json::json!(false)),
        ("confirmationStatus", serde_json::json!("unknown")),
        ("confirmations", serde_json::json!("3")),
    ] {
        let mut json = valid.clone();
        json["result"]["value"][0][field] = value;
        assert!(super::parse_solana_tx_observation(&json).is_err());
    }
    for json in [
        serde_json::json!({"error":{"code":-32000,"message":"RPC unavailable"}}),
        serde_json::json!({"result":{"value":[]}}),
    ] {
        assert!(super::parse_solana_tx_observation(&json).is_err());
    }
}
