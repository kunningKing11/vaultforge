//! Chain-bound delivery evidence shared by every Solana swap provider.
use crate::operations::{Operation, OperationStep, TransactionStatus};
use crate::swap_routes::RouteTransaction;
use crate::swap_solana::SolanaRpc;
use crate::tx::swap_solana::{self as encoding, key};
use crate::tx::tx_status::{Execution, Finality, TxStatus};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct DeliveryEvidence {
    pub(crate) signature: String,
    pub(crate) slot: u64,
    pub(crate) net_output: String,
}
pub(crate) fn verified(operation: &Operation) -> bool {
    let Some(attempt) = operation.attempts.last() else {
        return false;
    };
    let Some(swap) = operation.swap.as_ref() else {
        return false;
    };
    let Some(evidence) = swap.api.as_ref().and_then(|api| api.delivery.as_ref()) else {
        return false;
    };
    evidence.signature == attempt.tx_hash
        && attempt.tx_status.as_ref().is_some_and(|status| {
            status.execution == Execution::Succeeded && status.slot == Some(evidence.slot)
        })
        && evidence
            .net_output
            .parse::<u128>()
            .ok()
            .zip(swap.minimum_received.parse::<u128>().ok())
            .is_some_and(|(net, min)| min > 0 && net >= min)
}
pub(crate) async fn observe(
    client: &(impl SolanaRpc + Sync),
    operation: &Operation,
    hash: &str,
) -> Result<(Option<TxStatus>, Option<DeliveryEvidence>), String> {
    let response = client
        .swap_rpc(
            "getSignatureStatuses",
            json!([[hash],{"searchTransactionHistory":true}]),
        )
        .await?;
    let Some(status) =
        crate::providers::solana::parse_solana_tx_observation(&json!({"result":response}))?
    else {
        return Ok((None, None));
    };
    if status.execution == Execution::Failed {
        return Ok((Some(status), None));
    }
    let commitment = if status.finality == Finality::Finalized {
        "finalized"
    } else {
        "confirmed"
    };
    let proof=client.swap_rpc("getTransaction",json!([hash,{"encoding":"base64","commitment":commitment,"maxSupportedTransactionVersion":0}])).await?;
    let evidence = verify_delivery(operation, hash, &proof, status.slot)?;
    Ok((Some(status), Some(evidence)))
}
pub(crate) fn verify_delivery(
    operation: &Operation,
    hash: &str,
    proof: &Value,
    slot: Option<u64>,
) -> Result<DeliveryEvidence, String> {
    let attempt = operation
        .attempts
        .iter()
        .find(|a| a.tx_hash == hash)
        .ok_or("Saved Solana swap attempt missing")?;
    let raw = proof["transaction"][0]
        .as_str()
        .ok_or("Solana swap transaction evidence unavailable")?;
    let tx = encoding::decode(raw)?;
    let saved = encoding::decode(
        attempt
            .raw_tx
            .as_deref()
            .ok_or("Saved Solana signed bytes missing")?,
    )?;
    if tx.message != saved.message
        || tx.signatures != saved.signatures
        || tx.signatures.len() != 1
        || tx.signatures[0].to_string() != hash
        || tx.message.static_account_keys().first() != Some(&key(&operation.sender)?)
        || !tx.signatures[0].verify(key(&operation.sender)?.as_ref(), &tx.message.serialize())
        || proof["slot"].as_u64() != slot
        || proof["meta"].get("err") != Some(&Value::Null)
    {
        return Err("Solana chain evidence differs from the saved successful transaction".into());
    }
    let swap = operation.swap.as_ref().ok_or("Solana swap terms missing")?;
    let api = swap.api.as_ref().ok_or("Solana swap provider missing")?;
    let minimum = swap
        .minimum_received
        .parse::<u128>()
        .map_err(|_| "Invalid swap minimum")?;
    let RouteTransaction::Solana { lookups, .. } = &api.transaction else {
        return Err("Solana transaction family mismatch".into());
    };
    let keys = encoding::resolved_keys(&tx, lookups)?;
    let net = if let Some(mint) = &operation.destination_asset.token_address {
        let destination =
            encoding::associated_with_tokens(&operation.sender, mint, api.solana_tokens.as_ref())?;
        let index = keys
            .iter()
            .position(|k| *k == destination)
            .ok_or("Solana receiving account missing")?;
        let program = encoding::token_program(api.solana_tokens.as_ref(), mint)?;
        let read = |field: &str, allow_missing: bool| -> Result<u128, String> {
            let values = proof["meta"][field]
                .as_array()
                .ok_or("Solana token balance evidence missing")?;
            let mut values = values
                .iter()
                .filter(|v| v["accountIndex"].as_u64() == Some(index as u64));
            let Some(value) = values.next() else {
                if allow_missing
                    && (api.solana_tokens.is_none()
                        || proof["meta"]["preBalances"][index].as_u64() == Some(0))
                {
                    return Ok(0);
                }
                return Err("Solana output token balance missing".into());
            };
            if values.next().is_some()
                || value["mint"].as_str() != Some(mint)
                || value["owner"].as_str() != Some(&operation.sender)
                || value["programId"].as_str().is_some_and(|p| p != program)
                || (program == crate::solana_tokens::TOKEN_2022
                    && value["programId"].as_str() != Some(program))
            {
                return Err("Solana output token identity changed".into());
            }
            if let Some(tokens) = &api.solana_tokens {
                if value["uiTokenAmount"]["decimals"].as_u64()
                    != Some(u64::from(tokens.output.decimals))
                {
                    return Err("Solana output decimals changed".into());
                }
            }
            value["uiTokenAmount"]["amount"]
                .as_str()
                .ok_or("Solana output units missing")?
                .parse()
                .map_err(|_| "Invalid Solana output units".into())
        };
        read("postTokenBalances", false)?
            .checked_sub(read("preTokenBalances", true)?)
            .ok_or("Solana output balance decreased")?
    } else {
        let recipient = key(&operation.sender)?;
        let index = keys
            .iter()
            .position(|k| *k == recipient)
            .ok_or("Solana recipient missing")?;
        let before = proof["meta"]["preBalances"][index]
            .as_u64()
            .ok_or("Native pre-balance missing")?;
        let after = proof["meta"]["postBalances"][index]
            .as_u64()
            .ok_or("Native post-balance missing")?;
        let fee = proof["meta"]["fee"].as_u64().ok_or("Native fee missing")?;
        let wsol = encoding::associated(&operation.sender, encoding::NATIVE)?;
        let account = keys
            .iter()
            .position(|k| *k == wsol)
            .ok_or("Native receiving WSOL account missing")?;
        // Unwrapping an existing WSOL ATA also returns its old SOL/rent. That is
        // the wallet's existing property, not output delivered by this swap.
        let old_wsol = proof["meta"]["preBalances"][account]
            .as_u64()
            .ok_or("Native WSOL pre-balance missing")?;
        u128::from(after)
            .checked_add(u128::from(fee))
            .and_then(|v| v.checked_sub(u128::from(before)))
            .and_then(|v| v.checked_sub(u128::from(old_wsol)))
            .ok_or("Solana native delivery is not proved")?
    };
    if minimum == 0 || net < minimum {
        return Err("Solana chain evidence does not prove net minimum delivery".into());
    }
    Ok(DeliveryEvidence {
        signature: hash.into(),
        slot: slot.ok_or("Solana delivery slot missing")?,
        net_output: net.to_string(),
    })
}

/// Used by both unlock recovery and UI sync. Legacy status alone cannot imply delivery.
pub(crate) async fn reconcile(
    operation: &mut Operation,
    client: &(impl SolanaRpc + Sync),
) -> Result<(), String> {
    crate::swap_solana::validate_operation(operation)?;
    let mut recovered_delivery = None;
    if !verified(operation) {
        let snapshot = operation.clone();
        for attempt in &mut operation.attempts {
            if let Some(status) = &attempt.tx_status {
                if status.execution != Execution::Succeeded {
                    continue;
                }
                if status.finality == Finality::Finalized {
                    // A missing delivery marker does not erase established finality.
                    // Fetch the finalized receipt separately; stale status reads cannot downgrade it.
                    let proof=client.swap_rpc("getTransaction",json!([attempt.tx_hash,{"encoding":"base64","commitment":"finalized","maxSupportedTransactionVersion":0}])).await;
                    match proof.and_then(|proof| {
                        verify_delivery(&snapshot, &attempt.tx_hash, &proof, status.slot)
                    }) {
                        Ok(proof) => {
                            recovered_delivery = Some(proof);
                            attempt.error = None;
                        }
                        Err(error) => attempt.error = Some(format!("Status check failed: {error}")),
                    }
                } else {
                    attempt.tx_status = None;
                    attempt.status = TransactionStatus::SubmissionUnknown;
                }
            }
        }
        if operation.step == OperationStep::Completed {
            operation.step = OperationStep::Send;
        }
    }
    let snapshot = operation.clone();
    let evidence = std::sync::Mutex::new(recovered_delivery);
    crate::operations::reconcile_operation_with(operation, |_, hash| {
        let snapshot = &snapshot;
        let evidence = &evidence;
        async move {
            let (status, proof) = observe(client, snapshot, &hash).await?;
            if let Some(proof) = proof {
                *evidence.lock().map_err(|_| "Delivery lock failed")? = Some(proof);
            }
            Ok(status)
        }
    })
    .await?;
    if let Some(api) = operation.swap.as_mut().and_then(|s| s.api.as_mut()) {
        if let Some(proof) = evidence.into_inner().map_err(|_| "Delivery lock failed")? {
            api.delivery = Some(proof);
        } else if operation.attempts.last().is_none_or(|a| {
            a.tx_status
                .as_ref()
                .is_none_or(|s| s.execution != Execution::Succeeded)
        }) {
            api.delivery = None;
        }
    }
    if !verified(operation) && operation.step == OperationStep::Completed {
        operation.step = OperationStep::Send;
    }
    crate::uniswap::update_progress(operation);
    Ok(())
}
