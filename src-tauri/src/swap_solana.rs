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
    fn swap_rpc(
        &self,
        method: &str,
        params: Value,
    ) -> impl std::future::Future<Output = Result<Value, String>> + Send;
}
impl SolanaRpc for reqwest::Client {
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
    let RouteTransaction::Solana {
        transaction,
        lookups,
    } = &api.transaction
    else {
        return Err("Solana swap transaction family mismatch".into());
    };
    encoding::validate(
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
    )?;
    Ok(())
}
async fn fee_and_simulation(
    client: &(impl SolanaRpc + Sync),
    tx: &VersionedTransaction,
    lookups: &[SolanaLookup],
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
    let mut new_accounts = std::collections::BTreeSet::new();
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
            if state.get("value").ok_or("ATA state missing")?.is_null() {
                new_accounts.insert(account.to_string());
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
    let rent = if new_accounts.is_empty() {
        0
    } else {
        let per_account = rpc(
            client,
            "getMinimumBalanceForRentExemption",
            json!([165,{"commitment":"confirmed"}]),
        )
        .await?
        .as_u64()
        .ok_or("Solana rent unavailable")?;
        per_account
            .checked_mul(
                new_accounts
                    .len()
                    .try_into()
                    .map_err(|_| "Too many Solana accounts")?,
            )
            .ok_or("Solana rent overflow")?
    };
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
    let RouteTransaction::Solana {
        transaction,
        lookups,
    } = &api.transaction
    else {
        return Err("Native swap required".into());
    };
    let tx = encoding::validate(
        transaction,
        lookups,
        sender,
        input,
        output,
        amount,
        minimum,
        slippage,
    )?;
    let (fee, rent) = fee_and_simulation(client, &tx, lookups).await?;
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
    crate::operations::reconcile_operation_with(&mut next, |_, hash| async move {
        let response = rpc(
            client,
            "getSignatureStatuses",
            json!([[hash],{"searchTransactionHistory":true}]),
        )
        .await?;
        crate::providers::solana::parse_solana_tx_observation(&json!({"result":response}))
    })
    .await?;
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
                                    if let Some(observation) = observed {
                                        crate::operations::reconcile_operation_with(
                                            &mut next,
                                            |_, _| {
                                                let observation = observation.clone();
                                                async move { Ok(Some(observation)) }
                                            },
                                        )
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
    let (raw, lookups) = transaction(client, &Value::String(raw.clone()), &original.sender).await?;
    api.transaction = RouteTransaction::Solana {
        transaction: raw.clone(),
        lookups: lookups.clone(),
    };
    let mut tx = encoding::validate(
        &raw,
        &lookups,
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
    )?;
    let (fee, rent) = fee_and_simulation(client, &tx, &lookups).await?;
    let (hash, last_valid) = latest(client).await?;
    match &mut tx.message {
        VersionedMessage::Legacy(m) => m.recent_blockhash = hash.clone(),
        VersionedMessage::V0(m) => m.recent_blockhash = hash.clone(),
        _ => return Err("Solana v1 swaps are not supported".into()),
    }
    // The new recent blockhash changes no instruction or signer; price terms remain pinned.
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
    let RouteTransaction::Solana {
        transaction,
        lookups,
    } = &api.transaction
    else {
        return Err("Native transaction missing".into());
    };
    let mut quoted = encoding::decode(transaction)?;
    match &mut quoted.message {
        VersionedMessage::Legacy(m) => {
            m.recent_blockhash = tx.message.recent_blockhash().to_owned()
        }
        VersionedMessage::V0(m) => m.recent_blockhash = tx.message.recent_blockhash().to_owned(),
        _ => return Err("Solana v1 swaps are not supported".into()),
    }
    if quoted.message != tx.message
        || tx.message.recent_blockhash().to_string() != context.blockhash
        || tx.signatures[0].to_string() != expected
        || !tx.signatures[0].verify(key(&original.sender)?.as_ref(), &tx.message.serialize())
    {
        return Err("Signed Solana swap differs from reviewed intent".into());
    }
    let (fee, rent) = fee_and_simulation(client, &tx, lookups).await?;
    if fee.checked_add(rent).ok_or("Native budget overflow")?
        > context
            .fee_lamports
            .checked_add(context.rent_lamports)
            .ok_or("Saved budget overflow")?
    {
        return Err("Solana swap fee increased; review a fresh quote".into());
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
    let result = rpc(client, "sendTransaction", params)
        .await
        .and_then(|value| {
            value
                .as_str()
                .map(String::from)
                .ok_or("Solana broadcast signature missing".into())
        });
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
    let token_params = json!([
        operation.sender,
        {"programId": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},
        {"encoding": "jsonParsed", "commitment": "confirmed"}
    ]);
    let tokens = match rpc(client, "getTokenAccountsByOwner", token_params).await {
        Ok(value) => crate::providers::solana::parse_solana_token_accounts(
            &json!({"result": value}),
            &operation.sender,
        )
        .ok(),
        Err(_) => None,
    };
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
                Some(mint) => {
                    if let Some(accounts) = &tokens {
                        let balance = accounts
                            .iter()
                            .filter(|account| &account.mint == mint)
                            .try_fold(0u128, |sum, account| {
                                sum.checked_add(u128::from(account.amount))
                            })
                            .ok_or("Solana token balance overflow")?;
                        asset.balance = balance.to_string();
                    }
                }
            }
        }
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
