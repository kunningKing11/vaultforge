//! Native Solana swap execution and durable recovery. Keys never leave Rust.
use crate::cctp_execution::{check_snapshot, save_operation};
use crate::dto::WalletSession;
use crate::operations::{
    Operation, OperationStep, SolanaAttemptContext, SolanaLookup, TransactionAttempt,
    TransactionStatus, TransactionStep,
};
use crate::providers::http::{json_rpc_result, rpc_post};
use crate::state::{AppState, session_from_state};
use crate::swap_routes::{ApiSwap, RouteTransaction};
use crate::tx::swap_solana::{self as encoding, key};
use crate::tx::tx_status::Finality;
use crate::uniswap::UniswapPreparedStep;
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::Utc;
use serde_json::{Value, json};
use solana_message::{AddressLookupTableAccount, VersionedMessage};
use solana_signer::Signer;
use solana_transaction::versioned::VersionedTransaction;
use std::sync::Mutex;

pub(crate) trait SolanaRpc {
    fn execute_jupiter(
        &self,
        _request: &crate::providers::jupiter::ExecuteRequest<'_>,
    ) -> impl std::future::Future<
        Output = Result<crate::providers::jupiter::JupiterExecution, String>,
    > + Send {
        async { Err("Jupiter executor unavailable".into()) }
    }

    fn swap_rpc(
        &self,
        method: &str,
        params: Value,
    ) -> impl std::future::Future<Output = Result<Value, String>> + Send;
}
impl SolanaRpc for reqwest::Client {
    async fn execute_jupiter(
        &self,
        request: &crate::providers::jupiter::ExecuteRequest<'_>,
    ) -> Result<crate::providers::jupiter::JupiterExecution, String> {
        crate::providers::jupiter::execute_order(self, request).await
    }

    async fn swap_rpc(&self, method: &str, params: Value) -> Result<Value, String> {
        let config = crate::registry::network_by_id("solana").ok_or("Solana network missing")?;
        let response = rpc_post(
            self,
            config.rpc_url()?,
            &json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}),
        )
        .await?;
        Ok(json_rpc_result(&response, "Solana swap")?.clone())
    }
}
async fn rpc(
    client: &(impl SolanaRpc + Sync),
    method: &str,
    params: Value,
) -> Result<Value, String> {
    client.swap_rpc(method, params).await
}
async fn latest(client: &(impl SolanaRpc + Sync)) -> Result<(solana_hash::Hash, u64), String> {
    let value = rpc(
        client,
        "getLatestBlockhash",
        json!([{"commitment":"confirmed"}]),
    )
    .await?;
    let hash = value["value"]["blockhash"]
        .as_str()
        .ok_or("Solana blockhash missing")?
        .parse()
        .map_err(|_| "Invalid Solana blockhash")?;
    Ok((
        hash,
        value["value"]["lastValidBlockHeight"]
            .as_u64()
            .ok_or("Solana expiry missing")?,
    ))
}
async fn lookup(client: &(impl SolanaRpc + Sync), address: &str) -> Result<SolanaLookup, String> {
    key(address)?;
    let value = rpc(
        client,
        "getAccountInfo",
        json!([address,{"encoding":"base64","commitment":"confirmed"}]),
    )
    .await?;
    if value["value"]["owner"] != "AddressLookupTab1e1111111111111111111111111" {
        return Err("Solana lookup owner mismatch".into());
    }
    let data = STANDARD
        .decode(
            value["value"]["data"][0]
                .as_str()
                .ok_or("Lookup data missing")?,
        )
        .map_err(|_| "Invalid lookup data")?;
    if data.len() < 56
        || data[..4] != [1, 0, 0, 0]
        || data[4..12] != [255; 8]
        || (data.len() - 56) % 32 != 0
    {
        return Err("Invalid or deactivated Solana lookup table".into());
    }
    Ok(SolanaLookup {
        address: address.into(),
        addresses: data[56..]
            .chunks_exact(32)
            .map(|v| bs58::encode(v).into_string())
            .collect(),
    })
}
/// Fetch lookup tables and check the original order blockhash.
pub(crate) async fn order_lookups(
    client: &(impl SolanaRpc + Sync),
    raw: &str,
) -> Result<Vec<SolanaLookup>, String> {
    let tx = encoding::decode(raw)?;
    let mut tables = Vec::new();
    if let Some(lookups) = tx.message.address_table_lookups() {
        if lookups.len() > 8 {
            return Err("Too many Jupiter lookup tables".into());
        }
        for item in lookups {
            tables.push(lookup(client, &item.account_key.to_string()).await?);
        }
    }
    check_blockhash(
        client,
        &tx.message.recent_blockhash().to_string(),
        "confirmed",
    )
    .await?;
    Ok(tables)
}
async fn check_blockhash(
    client: &(impl SolanaRpc + Sync),
    hash: &str,
    commitment: &str,
) -> Result<(), String> {
    if rpc(
        client,
        "isBlockhashValid",
        json!([hash, {"commitment":commitment}]),
    )
    .await?["value"]
        .as_bool()
        != Some(true)
    {
        return Err("Jupiter order blockhash expired or unavailable; request a fresh quote".into());
    }
    Ok(())
}
pub(crate) async fn transaction(
    client: &(impl SolanaRpc + Sync),
    data: &Value,
    sender: &str,
) -> Result<(String, Vec<SolanaLookup>), String> {
    let mut tx = if let Some(raw) = data.as_str() {
        encoding::decode(raw)?
    } else {
        let instructions = data["instructions"]
            .as_array()
            .filter(|v| v.len() <= 16)
            .ok_or("No supported serialized Solana swap transaction")?;
        let instructions = instructions
            .iter()
            .map(|instruction| {
                let program_id = key(instruction["programId"]
                    .as_str()
                    .ok_or("Solana program missing")?)?;
                let accounts = instruction["keys"]
                    .as_array()
                    .ok_or("Solana account list missing")?
                    .iter()
                    .map(|account| {
                        let pubkey =
                            key(account["pubkey"].as_str().ok_or("Solana account missing")?)?;
                        let signer = account["isSigner"]
                            .as_bool()
                            .ok_or("Solana signer flag missing")?;
                        let writable = account["isWritable"]
                            .as_bool()
                            .ok_or("Solana writable flag missing")?;
                        Ok(if writable {
                            solana_instruction::AccountMeta::new(pubkey, signer)
                        } else {
                            solana_instruction::AccountMeta::new_readonly(pubkey, signer)
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let encoded = instruction["data"]
                    .as_str()
                    .ok_or("Solana instruction data missing")?;
                let bytes = hex::decode(encoded.strip_prefix("0x").unwrap_or(encoded))
                    .map_err(|_| "Invalid Relay Solana instruction hex")?;
                Ok(solana_instruction::Instruction {
                    program_id,
                    accounts,
                    data: bytes,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut tables = Vec::new();
        if let Some(addresses) = data["addressLookupTableAddresses"].as_array() {
            if addresses.len() > 8 {
                return Err("Too many Solana lookup tables".into());
            }
            for address in addresses {
                let table =
                    lookup(client, address.as_str().ok_or("Lookup address missing")?).await?;
                tables.push(AddressLookupTableAccount {
                    key: key(&table.address)?,
                    addresses: table
                        .addresses
                        .iter()
                        .map(|a| key(a))
                        .collect::<Result<Vec<_>, _>>()?,
                });
            }
        }
        let (hash, _) = latest(client).await?;
        let message =
            solana_message::v0::Message::try_compile(&key(sender)?, &instructions, &tables, hash)
                .map_err(|_| "Solana swap could not compile")?;
        VersionedTransaction {
            signatures: vec![Default::default()],
            message: VersionedMessage::V0(message),
        }
    };
    if tx
        .signatures
        .iter()
        .any(|signature| signature.as_ref().iter().any(|b| *b != 0))
    {
        return Err("Provider supplied a pre-signed Solana transaction".into());
    }
    let (hash, _) = latest(client).await?;
    match &mut tx.message {
        VersionedMessage::Legacy(m) => m.recent_blockhash = hash.clone(),
        VersionedMessage::V0(m) => m.recent_blockhash = hash.clone(),
        _ => return Err("Solana v1 swaps are not supported".into()),
    }
    let mut tables = Vec::new();
    if let Some(lookups) = tx.message.address_table_lookups() {
        for item in lookups {
            tables.push(lookup(client, &item.account_key.to_string()).await?);
        }
    }
    let raw = STANDARD.encode(wincode::serialize(&tx).map_err(|_| "Solana serialization failed")?);
    Ok((raw, tables))
}
pub(crate) fn validate_operation(operation: &Operation) -> Result<(), String> {
    if operation.kind != crate::operations::OperationKind::Swap
        || operation.source_asset.network != "solana"
        || operation.destination_asset.network != "solana"
        || operation.sender != operation.recipient
    {
        return Err("Unsupported Solana swap operation".into());
    }
    let terms = operation.swap.as_ref().ok_or("Saved Solana swap missing")?;
    let expected: u128 = terms
        .quoted_output
        .parse()
        .map_err(|_| "Invalid native expected output")?;
    let minimum: u128 = terms
        .minimum_received
        .parse()
        .map_err(|_| "Invalid native minimum output")?;
    if minimum == 0
        || minimum > expected
        || minimum < crate::providers::uniswap::minimum_output(expected, terms.slippage_bps)?
    {
        return Err("Saved native swap minimum or slippage changed".into());
    }
    if terms.chain_id.is_some()
        || terms.relay.is_some()
        || terms.router != encoding::JUPITER
        || !terms.quoter.is_empty()
        || terms.pool_fee.is_some()
    {
        return Err("Solana swap contains EVM execution terms".into());
    }
    let api = terms.api.as_ref().ok_or("Native Solana provider missing")?;
    if crate::jupiter::is_jupiter(operation) {
        return crate::jupiter::validate_operation(operation);
    } else if api.jupiter.is_some() {
        return Err("Foreign provider contains Jupiter execution terms".into());
    }

    let RouteTransaction::Solana {
        transaction,
        lookups,
    } = &api.transaction
    else {
        return Err("Solana swap transaction family mismatch".into());
    };
    encoding::validate_with_tokens(
        transaction,
        lookups,
        &operation.sender,
        operation
            .source_asset
            .token_address
            .as_deref()
            .unwrap_or(encoding::NATIVE),
        operation
            .destination_asset
            .token_address
            .as_deref()
            .unwrap_or(encoding::NATIVE),
        operation
            .amount_base_units
            .parse()
            .map_err(|_| "Invalid swap input")?,
        terms
            .minimum_received
            .parse()
            .map_err(|_| "Invalid swap minimum")?,
        terms.slippage_bps,
        api.solana_tokens.as_ref(),
    )?;
    Ok(())
}
async fn fee_and_simulation(
    client: &(impl SolanaRpc + Sync),
    tx: &VersionedTransaction,
    lookups: &[SolanaLookup],
    tokens: Option<&crate::solana_swap_tokens::SwapTokens>,
) -> Result<(u64, u64), String> {
    // Pin mainnet and simulate the exact message before it is signed or broadcast.
    if rpc(client, "getGenesisHash", json!([])).await?.as_str()
        != Some("5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d")
    {
        return Err("Solana swap RPC is not mainnet".into());
    }
    let fee = rpc(
        client,
        "getFeeForMessage",
        json!([STANDARD.encode(tx.message.serialize()),{"commitment":"confirmed"}]),
    )
    .await?["value"]
        .as_u64()
        .ok_or("Solana swap fee unavailable")?;
    if fee == 0 {
        return Err("Solana swap fee unavailable".into());
    }
    let keys = encoding::resolved_keys(tx, lookups)?;
    let payer = keys.first().ok_or("Solana payer missing")?;
    let mut new_accounts = std::collections::BTreeMap::new();
    let mut native_spend = 0u64;
    for instruction in tx.message.instructions() {
        let program = keys
            .get(usize::from(instruction.program_id_index))
            .ok_or("Invalid Solana program index")?
            .to_string();
        if program == "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL" {
            let account = keys
                .get(usize::from(
                    *instruction.accounts.get(1).ok_or("ATA missing")?,
                ))
                .ok_or("Invalid ATA index")?;
            let state = rpc(
                client,
                "getAccountInfo",
                json!([account.to_string(),{"encoding":"base64","commitment":"confirmed"}]),
            )
            .await?;
            let value = state.get("value").ok_or("ATA state missing")?;
            if !value.is_null() {
                if let Some(tokens) = tokens {
                    let owner = keys
                        .get(usize::from(
                            *instruction.accounts.get(2).ok_or("ATA owner missing")?,
                        ))
                        .ok_or("ATA owner index missing")?
                        .to_string();
                    let mint = keys
                        .get(usize::from(
                            *instruction.accounts.get(3).ok_or("ATA mint missing")?,
                        ))
                        .ok_or("ATA mint index missing")?
                        .to_string();
                    let token = tokens.get(&mint)?;
                    let holding =
                        crate::solana_tokens::inspect_holding(&account.to_string(), &owner, value)?;
                    if holding.program.id() != token.program
                        || holding.mint != mint
                        || holding.unsupported_reason().is_some()
                    {
                        return Err(
                            "Swap account creation targets an invalid existing token account"
                                .into(),
                        );
                    }
                }
            }
            if value.is_null() {
                let mint = keys
                    .get(usize::from(
                        *instruction.accounts.get(3).ok_or("ATA mint missing")?,
                    ))
                    .ok_or("Invalid ATA mint index")?
                    .to_string();
                let size = tokens.map_or(Ok(165), |tokens| {
                    Ok::<_, String>(tokens.get(&mint)?.account_len)
                })?;
                new_accounts.insert(account.to_string(), size);
            }
        } else if program == encoding::NATIVE {
            let amount = u64::from_le_bytes(
                instruction
                    .data
                    .get(4..12)
                    .ok_or("Malformed native swap transfer")?
                    .try_into()
                    .map_err(|_| "Malformed native amount")?,
            );
            native_spend = native_spend
                .checked_add(amount)
                .ok_or("Native swap amount overflow")?;
        }
    }
    let mut rent = 0u64;
    for size in new_accounts.values() {
        let amount = rpc(
            client,
            "getMinimumBalanceForRentExemption",
            json!([size,{"commitment":"confirmed"}]),
        )
        .await?
        .as_u64()
        .ok_or("Solana rent unavailable")?;
        rent = rent.checked_add(amount).ok_or("Solana rent overflow")?;
    }
    let balance = rpc(
        client,
        "getBalance",
        json!([payer.to_string(),{"commitment":"confirmed"}]),
    )
    .await?["value"]
        .as_u64()
        .ok_or("Solana native balance unavailable")?;
    let required = native_spend
        .checked_add(fee)
        .and_then(|v| v.checked_add(rent))
        .ok_or("Solana swap budget overflow")?;
    if balance < required {
        return Err("Insufficient SOL for the swap amount, network fee, and account rent".into());
    }
    let raw = STANDARD.encode(wincode::serialize(tx).map_err(|_| "Solana serialization failed")?);
    let simulated = rpc(
        client,
        "simulateTransaction",
        json!([raw,{"encoding":"base64","sigVerify":false,"commitment":"confirmed"}]),
    )
    .await?;
    if simulated["value"].get("err") != Some(&Value::Null) {
        return Err(format!(
            "Solana swap simulation failed: {}",
            simulated["value"]["err"]
        ));
    }
    Ok((fee, rent))
}
async fn validated_api_transaction(
    client: &(impl SolanaRpc + Sync),
    api: &ApiSwap,
    sender: &str,
    input: &str,
    output: &str,
    amount: u128,
    minimum: u128,
    slippage: u16,
) -> Result<VersionedTransaction, String> {
    let RouteTransaction::Solana {
        transaction,
        lookups,
    } = &api.transaction
    else {
        return Err("Native swap required".into());
    };
    if let Some(tokens) = &api.solana_tokens {
        crate::solana_swap_tokens::recheck(client, tokens, sender, input, output, amount).await?;
    }
    if api.provider != crate::swap_routes::RouteProvider::Jupiter {
        return encoding::validate_with_tokens(
            transaction,
            lookups,
            sender,
            input,
            output,
            amount,
            minimum,
            slippage,
            api.solana_tokens.as_ref(),
        );
    }
    let saved = api.jupiter.as_ref().ok_or("Jupiter terms missing")?;
    if saved.order.transaction_base64 != *transaction
        || saved.order.minimum_output_base_units.parse::<u128>().ok() != Some(minimum)
        || api.request_id != saved.order.request_id
    {
        return Err("Jupiter transaction or minimum differs from the reviewed order".into());
    }
    let request = crate::providers::jupiter::OrderRequest {
        taker: sender.into(),
        input_mint: input.into(),
        output_mint: output.into(),
        amount_base_units: amount.to_string(),
        slippage_bps: slippage,
    };
    let validated = crate::tx::jupiter::validate_order_with_tokens(
        &saved.order,
        &request,
        lookups,
        api.solana_tokens.as_ref(),
    )?;
    if validated.message_hash != saved.message_hash {
        return Err("Jupiter message changed".into());
    }
    for (account, mint) in &validated.fee_token_accounts {
        if let Some(tokens) = &api.solana_tokens {
            let token = tokens.get(mint)?;
            let response = rpc(
                client,
                "getAccountInfo",
                json!([account,{"encoding":"base64","commitment":"confirmed"}]),
            )
            .await?;
            let (program, bytes) = crate::solana_tokens::account_data(&response["value"])?;
            let owner = bytes
                .get(32..64)
                .ok_or("Jupiter fee collector owner missing")?;
            let owner = bs58::encode(owner).into_string();
            let holding =
                crate::solana_tokens::inspect_holding(account, &owner, &response["value"])?;
            if program.id() != token.program
                || holding.mint != *mint
                || holding.unsupported_reason().is_some()
            {
                return Err("Jupiter fee collector program, mint or extensions changed".into());
            }
            continue;
        }
        let response = rpc(
            client,
            "getAccountInfo",
            json!([account, {"encoding":"jsonParsed", "commitment":"confirmed"}]),
        )
        .await?;
        let value = &response["value"];
        let parsed = &value["data"]["parsed"];
        if value["owner"] != "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
            || parsed["type"] != "account"
            || parsed["info"]["mint"].as_str() != Some(mint)
            || parsed["info"]["state"] != "initialized"
        {
            return Err("Jupiter fee collector is not an initialized classic token account for the quoted fee mint".into());
        }
    }
    Ok(validated.transaction)
}

pub(crate) async fn quote_fee(
    client: &(impl SolanaRpc + Sync),
    api: &ApiSwap,
    sender: &str,
    input: &str,
    output: &str,
    amount: u128,
    minimum: u128,
    slippage: u16,
) -> Result<u64, String> {
    let RouteTransaction::Solana { lookups, .. } = &api.transaction else {
        return Err("Native swap required".into());
    };
    let tx = validated_api_transaction(
        client, api, sender, input, output, amount, minimum, slippage,
    )
    .await?;
    let (fee, rent) = fee_and_simulation(client, &tx, lookups, api.solana_tokens.as_ref()).await?;
    fee.checked_add(rent)
        .ok_or("Solana fee and rent overflow".into())
}
fn view(
    operation: &Operation,
    attempt: &TransactionAttempt,
) -> Result<UniswapPreparedStep, String> {
    Ok(UniswapPreparedStep {
        operation_id: operation.id.clone(),
        step: "swap".into(),
        tx_hash: attempt.tx_hash.clone(),
        network: "solana".into(),
        max_native_fee: attempt.fee.clone().ok_or("Saved native fee missing")?,
    })
}
async fn reconcile(
    state: &Mutex<AppState>,
    client: &(impl SolanaRpc + Sync),
    generation: u64,
    original: &Operation,
) -> Result<Operation, String> {
    let mut next = original.clone();
    crate::solana_delivery::reconcile(&mut next, client).await?;
    let expiry_candidate = next.attempts.last().and_then(|attempt| {
        (attempt.tx_status.is_none()
            && matches!(
                attempt.status,
                TransactionStatus::Signed
                    | TransactionStatus::Submitted
                    | TransactionStatus::SubmissionUnknown
            )
            && !attempt
                .error
                .as_deref()
                .is_some_and(|error| error.starts_with("Status check failed:")))
        .then(|| {
            attempt
                .solana_context
                .as_ref()
                .map(|context| (attempt.tx_hash.clone(), context.last_valid_height))
        })
        .flatten()
    });
    if let Some((hash, last_valid_height)) = expiry_candidate {
        if let Ok(height) = rpc(
            client,
            "getBlockHeight",
            json!([{"commitment":"finalized"}]),
        )
        .await
        {
            if height
                .as_u64()
                .is_some_and(|height| height > last_valid_height)
            {
                let blockhash = next
                    .attempts
                    .last()
                    .and_then(|a| a.solana_context.as_ref())
                    .map(|c| c.blockhash.clone())
                    .ok_or("Saved Solana expiry blockhash missing")?;
                let invalid = rpc(
                    client,
                    "isBlockhashValid",
                    json!([blockhash,{"commitment":"finalized"}]),
                )
                .await
                .ok()
                .and_then(|v| v["value"].as_bool())
                    == Some(false);
                let invalid_confirmed = rpc(
                    client,
                    "isBlockhashValid",
                    json!([blockhash,{"commitment":"confirmed"}]),
                )
                .await
                .ok()
                .and_then(|v| v["value"].as_bool())
                    == Some(false);
                if !invalid || !invalid_confirmed {
                    save_operation(
                        &mut *state.lock().map_err(|_| "State lock failed")?,
                        generation,
                        original,
                        &next,
                    )?;
                    return Ok(next);
                }
                if let Ok(finalized_slot) =
                    rpc(client, "getSlot", json!([{"commitment":"finalized"}])).await
                {
                    if let Some(finalized_slot) = finalized_slot.as_u64() {
                        // Read history again after the finalized head. A stale cached
                        // missing signature cannot prove a transaction expired.
                        if let Ok(status) = rpc(
                            client,
                            "getSignatureStatuses",
                            json!([[hash],{"searchTransactionHistory":true}]),
                        )
                        .await
                        {
                            if status["context"]["slot"]
                                .as_u64()
                                .is_some_and(|slot| slot >= finalized_slot)
                            {
                                if let Ok(observed) =
                                    crate::providers::solana::parse_solana_tx_observation(
                                        &json!({"result":status}),
                                    )
                                {
                                    if observed.is_some() {
                                        crate::solana_delivery::reconcile(&mut next, client)
                                            .await?;
                                    } else if let Some(attempt) = next.attempts.last_mut() {
                                        attempt.status = TransactionStatus::Superseded;
                                        attempt.error = Some(
                                            "Expired without inclusion on the finalized Solana chain".into(),
                                        );
                                        attempt.updated_at = Utc::now().to_rfc3339();
                                        next.last_error = attempt.error.clone();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    save_operation(
        &mut *state.lock().map_err(|_| "State lock failed")?,
        generation,
        original,
        &next,
    )?;
    Ok(next)
}
pub(crate) async fn prepare(
    state: &Mutex<AppState>,
    client: &(impl SolanaRpc + Sync),
    id: &str,
) -> Result<UniswapPreparedStep, String> {
    let (_guard, generation, original) = crate::uniswap::begin_action(state, id)?;
    let original = reconcile(state, client, generation, &original).await?;
    validate_operation(&original)?;
    if crate::uniswap::terminal(&original) {
        return Err("Swap is complete, failed, or cancelled".into());
    }
    if let Some(attempt) = original.attempts.last() {
        if attempt.status == TransactionStatus::Signed && attempt.tx_status.is_none() {
            return view(&original, attempt);
        }
        return Err("Track the saved Solana swap before preparing another transaction".into());
    }
    let terms = original.swap.as_ref().ok_or("Swap terms missing")?;
    if terms.deadline <= crate::uniswap::now() {
        return Err("Solana swap quote expired; cancel and request a fresh quote".into());
    }
    let mut api = terms.api.clone().ok_or("Swap provider missing")?;
    let RouteTransaction::Solana {
        transaction: raw, ..
    } = &api.transaction
    else {
        return Err("Native transaction missing".into());
    };
    let (raw, lookups) = if crate::jupiter::is_jupiter(&original) {
        (raw.clone(), order_lookups(client, raw).await?)
    } else {
        transaction(client, &Value::String(raw.clone()), &original.sender).await?
    };
    api.transaction = RouteTransaction::Solana {
        transaction: raw.clone(),
        lookups: lookups.clone(),
    };
    let mut tx = validated_api_transaction(
        client,
        &api,
        &original.sender,
        original
            .source_asset
            .token_address
            .as_deref()
            .unwrap_or(encoding::NATIVE),
        original
            .destination_asset
            .token_address
            .as_deref()
            .unwrap_or(encoding::NATIVE),
        original
            .amount_base_units
            .parse()
            .map_err(|_| "Invalid amount")?,
        terms
            .minimum_received
            .parse()
            .map_err(|_| "Invalid minimum")?,
        terms.slippage_bps,
    )
    .await?;
    let (hash, last_valid) = if let Some(jupiter) = crate::jupiter::terms(&original) {
        let height = rpc(
            client,
            "getBlockHeight",
            json!([{"commitment":"confirmed"}]),
        )
        .await?
        .as_u64()
        .ok_or("Solana height missing")?;
        if height > jupiter.last_valid_height {
            return Err("Jupiter order expired".into());
        }
        (
            tx.message.recent_blockhash().to_owned(),
            jupiter.last_valid_height,
        )
    } else {
        let (hash, height) = latest(client).await?;
        match &mut tx.message {
            VersionedMessage::Legacy(m) => m.recent_blockhash = hash.clone(),
            VersionedMessage::V0(m) => m.recent_blockhash = hash.clone(),
            _ => return Err("Solana v1 swaps are not supported".into()),
        }
        (hash, height)
    };
    let (fee, rent) = fee_and_simulation(client, &tx, &lookups, api.solana_tokens.as_ref()).await?;
    if let Some(tokens) = &api.solana_tokens {
        crate::solana_swap_tokens::recheck(
            client,
            tokens,
            &original.sender,
            original
                .source_asset
                .token_address
                .as_deref()
                .unwrap_or(encoding::NATIVE),
            original
                .destination_asset
                .token_address
                .as_deref()
                .unwrap_or(encoding::NATIVE),
            original
                .amount_base_units
                .parse()
                .map_err(|_| "Invalid swap amount")?,
        )
        .await?;
    }
    let mut lock = state.lock().map_err(|_| "State lock failed")?;
    check_snapshot(&lock, generation, &original)?;
    let wallet = lock.wallet.as_ref().ok_or("Wallet closed")?;
    if wallet.operations.iter().any(|o| {
        o.id != original.id
            && !crate::uniswap::local_swap_confirmed(o)
            && o.source_asset.network == "solana"
            && o.sender == original.sender
            && o.attempts.iter().any(|a| {
                a.raw_tx.is_some()
                    && a.status != TransactionStatus::Superseded
                    && !a
                        .tx_status
                        .as_ref()
                        .is_some_and(|s| s.finality == Finality::Finalized)
            })
    }) {
        return Err("Another Solana transaction is unresolved; reconcile it first".into());
    }
    let secret = crate::derivation::solana_secret_key_from_mnemonic(
        &wallet.mnemonic,
        &wallet.bip39_passphrase,
    )?;
    let keypair =
        solana_keypair::keypair_from_seed(&secret).map_err(|_| "Solana signing key unavailable")?;
    if keypair.pubkey() != key(&original.sender)? {
        return Err("Solana signing key does not match wallet".into());
    }
    tx = VersionedTransaction::try_new(tx.message, &[&keypair])
        .map_err(|_| "Solana swap signing failed")?;
    let signature = tx.signatures[0].to_string();
    if wallet.operations.iter().any(|operation| {
        operation
            .attempts
            .iter()
            .any(|attempt| attempt.tx_hash == signature)
    }) {
        return Err("Solana RPC returned a blockhash already used for this swap; wait for a newer block and retry".into());
    }
    let mut next = original.clone();
    next.step = OperationStep::Send;
    next.swap.as_mut().ok_or("Swap terms missing")?.api = Some(api);
    let timestamp = Utc::now().to_rfc3339();
    next.attempts.push(TransactionAttempt {
        step: TransactionStep::Send,
        status: TransactionStatus::Signed,
        tx_hash: signature.clone(),
        raw_tx: Some(
            STANDARD
                .encode(wincode::serialize(&tx).map_err(|_| "Solana signed bytes unavailable")?),
        ),
        signature: Some(signature),
        fee: Some(
            fee.checked_add(rent)
                .ok_or("Solana budget overflow")?
                .to_string(),
        ),
        nonce_or_sequence: Some(hash.to_string()),
        error: None,
        updated_at: timestamp.clone(),
        tx_status: None,
        evm_context: None,
        solana_context: Some(SolanaAttemptContext {
            payer: original.sender.clone(),
            blockhash: hash.to_string(),
            last_valid_height: last_valid,
            fee_lamports: fee,
            rent_lamports: rent,
            event_account: None,
            fee_recipient: None,
            lookup: None,
            setup_slot: None,
            create_ata: false,
            provisional_destination_fee: None,
        }),
    });
    next.updated_at = timestamp;
    save_operation(&mut lock, generation, &original, &next)?;
    view(&next, next.attempts.last().ok_or("Signed attempt missing")?)
}
pub(crate) async fn submit(
    state: &Mutex<AppState>,
    client: &(impl SolanaRpc + Sync),
    id: &str,
    expected: &str,
) -> Result<WalletSession, String> {
    let (_guard, generation, original) = crate::uniswap::begin_action(state, id)?;
    let original = reconcile(state, client, generation, &original).await?;
    validate_operation(&original)?;
    let attempt = original.attempts.last().ok_or("Prepare the swap first")?;
    if attempt.tx_hash != expected {
        return Err("Reviewed Solana transaction changed".into());
    }
    if attempt.tx_status.is_some() {
        return Ok(session_from_state(
            &*state.lock().map_err(|_| "State lock failed")?,
        ));
    }
    if !matches!(
        attempt.status,
        TransactionStatus::Signed
            | TransactionStatus::Submitted
            | TransactionStatus::SubmissionUnknown
    ) {
        return Err("Saved Solana transaction cannot be broadcast".into());
    }
    if crate::uniswap::terminal(&original)
        || attempt
            .error
            .as_deref()
            .is_some_and(|e| e.starts_with("Status check failed:"))
    {
        return Err("Solana swap cannot be submitted until reconciliation succeeds".into());
    }
    if crate::jupiter::is_jupiter(&original) && attempt.status != TransactionStatus::Signed {
        return Err("Jupiter submission already attempted; reconcile the saved signature without re-executing".into());
    }
    let context = attempt
        .solana_context
        .as_ref()
        .ok_or("Native execution context missing")?;
    let height = rpc(
        client,
        "getBlockHeight",
        json!([{"commitment":"confirmed"}]),
    )
    .await?
    .as_u64()
    .ok_or("Solana block height missing")?;
    if height > context.last_valid_height {
        return Err(
            "Saved Solana transaction expired; check its final status before cancelling".into(),
        );
    }
    let raw = attempt
        .raw_tx
        .as_deref()
        .ok_or("Signed native bytes missing")?;
    let tx = encoding::decode(raw)?;
    let terms = original.swap.as_ref().ok_or("Swap terms missing")?;
    let api = terms.api.as_ref().ok_or("Native provider missing")?;
    let RouteTransaction::Solana { lookups, .. } = &api.transaction else {
        return Err("Native transaction missing".into());
    };
    let mut quoted = validated_api_transaction(
        client,
        api,
        &original.sender,
        original
            .source_asset
            .token_address
            .as_deref()
            .unwrap_or(encoding::NATIVE),
        original
            .destination_asset
            .token_address
            .as_deref()
            .unwrap_or(encoding::NATIVE),
        original
            .amount_base_units
            .parse()
            .map_err(|_| "Invalid swap input")?,
        terms
            .minimum_received
            .parse()
            .map_err(|_| "Invalid swap minimum")?,
        terms.slippage_bps,
    )
    .await?;

    if !crate::jupiter::is_jupiter(&original) {
        match &mut quoted.message {
            VersionedMessage::Legacy(m) => {
                m.recent_blockhash = tx.message.recent_blockhash().to_owned()
            }
            VersionedMessage::V0(m) => {
                m.recent_blockhash = tx.message.recent_blockhash().to_owned()
            }
            _ => return Err("Solana v1 swaps are not supported".into()),
        }
    }
    if let Some(jupiter) = crate::jupiter::terms(&original) {
        crate::tx::jupiter::verify_signed_order(&jupiter.order, raw, &original.sender)?;
        check_blockhash(client, &context.blockhash, "confirmed").await?;
        if terms.deadline <= crate::uniswap::now() {
            return Err("Jupiter order expired".into());
        }
    }
    if quoted.message != tx.message
        || tx.message.recent_blockhash().to_string() != context.blockhash
        || tx.signatures[0].to_string() != expected
        || !tx.signatures[0].verify(key(&original.sender)?.as_ref(), &tx.message.serialize())
    {
        return Err("Signed Solana swap differs from reviewed intent".into());
    }
    let (fee, rent) = fee_and_simulation(client, &tx, lookups, api.solana_tokens.as_ref()).await?;
    if fee.checked_add(rent).ok_or("Native budget overflow")?
        > context
            .fee_lamports
            .checked_add(context.rent_lamports)
            .ok_or("Saved budget overflow")?
    {
        return Err("Solana swap fee increased; review a fresh quote".into());
    }
    if let Some(tokens) = &api.solana_tokens {
        crate::solana_swap_tokens::recheck(
            client,
            tokens,
            &original.sender,
            original
                .source_asset
                .token_address
                .as_deref()
                .unwrap_or(encoding::NATIVE),
            original
                .destination_asset
                .token_address
                .as_deref()
                .unwrap_or(encoding::NATIVE),
            original
                .amount_base_units
                .parse()
                .map_err(|_| "Invalid swap amount")?,
        )
        .await?;
    }
    let mut submitting = original.clone();
    let next = submitting.attempts.last_mut().ok_or("Attempt missing")?;
    next.status = TransactionStatus::SubmissionUnknown;
    next.error = Some("Submission authorized; reconcile before retry".into());
    next.updated_at = Utc::now().to_rfc3339();
    save_operation(
        &mut *state.lock().map_err(|_| "State lock failed")?,
        generation,
        &original,
        &submitting,
    )?;
    let params = json!([raw, {
        "encoding": "base64", "skipPreflight": false,
        "preflightCommitment": "confirmed", "maxRetries": 0
    }]);
    let result = if let Some(jupiter) = crate::jupiter::terms(&original) {
        client
            .execute_jupiter(&crate::providers::jupiter::ExecuteRequest {
                request_id: &jupiter.order.request_id,
                signed_transaction_base64: raw,
            })
            .await
            .and_then(|report| {
                // Service success records submission; chain evidence establishes delivery.
                let response = report.raw_response;
                if response["status"] != "Success" || response["code"].as_i64() != Some(0) {
                    return Err("Jupiter reported a failure; awaiting chain evidence".into());
                }
                response["signature"]
                    .as_str()
                    .map(String::from)
                    .ok_or("Jupiter execution signature missing".into())
            })
    } else {
        rpc(client, "sendTransaction", params)
            .await
            .and_then(|value| {
                value
                    .as_str()
                    .map(String::from)
                    .ok_or("Solana broadcast signature missing".into())
            })
    };
    let mut after = submitting.clone();
    let next = after.attempts.last_mut().ok_or("Attempt missing")?;
    match result {
        Ok(hash) if hash == expected => {
            next.status = TransactionStatus::Submitted;
            next.error = None;
        }
        Ok(_) => {
            next.error = Some(
                "Solana provider returned a different signature; submission is uncertain".into(),
            )
        }
        Err(error) => next.error = Some(format!("Submission uncertain: {error}")),
    }
    next.updated_at = Utc::now().to_rfc3339();
    after.last_error = next.error.clone();
    after.updated_at = next.updated_at.clone();
    let mut lock = state.lock().map_err(|_| "State lock failed")?;
    save_operation(&mut lock, generation, &submitting, &after)?;
    Ok(session_from_state(&lock))
}
pub(crate) async fn sync(
    state: &Mutex<AppState>,
    client: &(impl SolanaRpc + Sync),
    id: &str,
) -> Result<WalletSession, String> {
    let (_guard, generation, original) = crate::uniswap::begin_action(state, id)?;
    let operation = reconcile(state, client, generation, &original).await?;
    let native = rpc(
        client,
        "getBalance",
        json!([operation.sender,{"commitment":"confirmed"}]),
    )
    .await
    .ok()
    .and_then(|v| v["value"].as_u64());
    let tokens = crate::providers::solana::fetch_solana_portfolio_accounts_with(
        &operation.sender,
        |method, params| async move { Ok(json!({"result":rpc(client,method,params).await?})) },
    )
    .await
    .ok();
    let mut lock = state.lock().map_err(|_| "State lock failed")?;
    check_snapshot(&lock, generation, &operation)?;
    if let Some(wallet) = lock.wallet.as_mut() {
        for asset in wallet
            .assets
            .iter_mut()
            .filter(|asset| asset.network == "solana")
        {
            match &asset.token_address {
                None => {
                    if let Some(balance) = native {
                        asset.balance = balance.to_string();
                    }
                }
                Some(_) => {}
            }
        }
    }
    if let (Some(accounts), Some(wallet)) = (tokens, lock.wallet.as_mut()) {
        let config = crate::registry::network_by_id("solana").ok_or("Solana network missing")?;
        let refreshed =
            crate::providers::solana::solana_token_assets(config, accounts, &wallet.assets);
        wallet
            .assets
            .retain(|asset| asset.network != "solana" || asset.token_address.is_none());
        wallet.assets.extend(refreshed);
    }
    Ok(session_from_state(&lock))
}

pub(crate) fn expired(operation: &Operation) -> bool {
    operation.source_asset.network == "solana"
        && operation.attempts.last().is_some_and(|attempt| {
            attempt.status == TransactionStatus::Superseded
                && attempt.error.as_deref()
                    == Some("Expired without inclusion on the finalized Solana chain")
        })
}

#[cfg(test)]
#[path = "tests/swap_solana.rs"]
mod tests;
