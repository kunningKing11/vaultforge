//! Relay exact-input routes. Only locally verified EVM protocol-v2 deposits execute.
use serde_json::{Value, json};
use sha3::{Digest, Keccak256};

pub(crate) use crate::operations::{RelayData, RouteNode};
use crate::registry::{NetworkConfig, network_by_id};
use crate::tx::evm::evm_address_word;
pub(crate) use crate::tx::relay::{DEPOSITORY, NATIVE, deposit_call, integer, protocol_chain};
#[cfg(test)]
use crate::tx::relay::{SOLVER, order_id};
use crate::tx::relay::{array, bytes, string};

pub(crate) async fn quote(
    client: &reqwest::Client,
    source: &NetworkConfig,
    destination: &NetworkConfig,
    token_in: &str,
    token_out: &str,
    sender: &str,
    amount: u128,
    slippage: u16,
) -> Result<(RelayData, u128, u128), String> {
    protocol_chain(&source.id)?;
    protocol_chain(&destination.id)?;
    let mut request = client.post("https://api.relay.link/quote/v2");
    if let Ok(key) = std::env::var("VAULTFORGE_RELAY_API_KEY") {
        request = request.header("x-api-key", key);
    }
    let response=request.json(&json!({
        "user":sender,"recipient":sender,"refundTo":sender,"originChainId":source.chain_id()?,
        "destinationChainId":destination.chain_id()?,"originCurrency":token_in,"destinationCurrency":token_out,
        "amount":amount.to_string(),"tradeType":"EXACT_INPUT","slippageTolerance":slippage.to_string(),
        "includeProtocolData":true,"explicitDeposit":true,"usePermit":false,"forceSolverExecution":true,"ttl":1200
    })).send().await.map_err(|e| format!("Cross-chain quote service unavailable: {e}"))?;
    if matches!(response.status().as_u16(), 401 | 403) {
        return Err("Cross-chain routing requires a Relay API key. Set VAULTFORGE_RELAY_API_KEY when starting VaultForge.".into());
    }
    if response.status().as_u16() == 429 {
        return Err("The swap route service is busy. Wait a moment and refresh the quote.".into());
    }
    if response.status().is_server_error() {
        return Err(
            "The swap route service is temporarily unavailable. Refresh the quote to try again."
                .into(),
        );
    }
    if !response.status().is_success() {
        return Err(format!(
            "No working swap route was found for this pair and amount (route service HTTP {}). Try another amount or pair.",
            response.status().as_u16()
        ));
    }
    let body: Value = response
        .json()
        .await
        .map_err(|_| "Invalid cross-chain quote response")?;
    let deadline = body["protocol"]["v2"]["orderData"]["output"]["deadline"]
        .as_u64()
        .ok_or("Relay expiry missing")?;
    if deadline <= crate::uniswap::now() + 60 {
        return Err("Route expires too soon; refresh the quote".into());
    }
    parse_quote(
        &body,
        source,
        destination,
        token_in,
        token_out,
        sender,
        amount,
        slippage,
    )
}

pub(crate) fn parse_quote(
    body: &Value,
    source: &NetworkConfig,
    destination: &NetworkConfig,
    token_in: &str,
    token_out: &str,
    sender: &str,
    amount: u128,
    slippage: u16,
) -> Result<(RelayData, u128, u128), String> {
    let protocol = &body["protocol"]["v2"];
    if protocol["hubType"] != "onchain"
        || !string(&protocol["paymentDetails"]["depository"], "depository")?
            .eq_ignore_ascii_case(DEPOSITORY)
    {
        return Err("No working route: unsupported Relay settlement contract".into());
    }
    let steps = array(&body["steps"])?;
    let deposits = steps
        .iter()
        .filter(|s| s["id"] == "deposit")
        .collect::<Vec<_>>();
    if steps.iter().any(|s| s["kind"] != "transaction")
        || deposits.len() != 1
        || steps
            .iter()
            .any(|s| s["id"] != "deposit" && s["id"] != "approve")
    {
        return Err("No working route: unsupported signature or transaction step".into());
    }
    let order = protocol["orderData"].clone();
    let out = &order["output"]["payments"][0];
    let expected = integer(&out["expectedAmount"])?;
    let minimum = integer(&out["minimumAmount"])?;
    if expected == 0
        || minimum == 0
        || minimum > expected
        || minimum < crate::providers::uniswap::minimum_output(expected, slippage)?
    {
        return Err("Route exceeds the selected slippage".into());
    }
    let mut route = Vec::new();
    for (side, network) in [("origin", source), ("destination", destination)] {
        for field in ["inputCurrency", "outputCurrency"] {
            let currency = &body["details"]["route"][side][field]["currency"];
            if currency.is_null() {
                continue;
            }
            if currency["chainId"].as_u64() != network.chain_id {
                return Err("Route preview network mismatch".into());
            }
            let address = string(&currency["address"], "route token")?;
            evm_address_word(address)?;
            let node = RouteNode {
                network: network.id.clone(),
                token_address: (!address.eq_ignore_ascii_case(NATIVE)).then(|| address.into()),
                symbol: string(&currency["symbol"], "route symbol")?
                    .chars()
                    .take(24)
                    .collect(),
                pool_fee: None,
                via: Some(
                    if side == "destination"
                        && route
                            .last()
                            .is_some_and(|n: &RouteNode| n.network != network.id)
                    {
                        "Relay bridge".into()
                    } else {
                        body["details"]["route"][side]["includedSwapSources"]
                            .as_array()
                            .map(|values| {
                                values
                                    .iter()
                                    .filter_map(Value::as_str)
                                    .take(6)
                                    .collect::<Vec<_>>()
                                    .join(" + ")
                            })
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| {
                                body["details"]["route"][side]["router"]
                                    .as_str()
                                    .map(|s| s.chars().take(40).collect())
                                    .unwrap_or_else(|| "Swap".into())
                            })
                    },
                ),
            };
            if !route.last().is_some_and(|n: &RouteNode| {
                n.network == node.network
                    && n.token_address
                        .as_deref()
                        .unwrap_or(NATIVE)
                        .eq_ignore_ascii_case(address)
            }) {
                route.push(node);
            }
        }
    }
    if route.first().is_none_or(|n| {
        n.network != source.id
            || !n
                .token_address
                .as_deref()
                .unwrap_or(NATIVE)
                .eq_ignore_ascii_case(token_in)
    }) || route.last().is_none_or(|n| {
        n.network != destination.id
            || !n
                .token_address
                .as_deref()
                .unwrap_or(NATIVE)
                .eq_ignore_ascii_case(token_out)
    }) {
        return Err("Route preview does not match the selected assets".into());
    }
    let relay = RelayData {
        destination_network: destination.id.clone(),
        request_id: string(&deposits[0]["requestId"], "request ID")?.into(),
        order_id: string(&protocol["orderId"], "order ID")?.into(),
        order,
        route,
        destination_hash: None,
        destination_status: None,
        status: None,
    };
    if bytes(&relay.request_id)?.len() != 32 {
        return Err("Invalid Relay request ID".into());
    }
    let call = deposit_call(
        &relay, &source.id, token_in, token_out, sender, amount, minimum, expected,
    )?;
    let items = array(&deposits[0]["items"])?;
    if items.len() != 1 {
        return Err("Unsupported Relay deposit steps".into());
    }
    let tx = &items[0]["data"];
    if tx["chainId"].as_u64() != source.chain_id
        || evm_address_word(string(&tx["from"], "payer")?)? != evm_address_word(sender)?
        || evm_address_word(string(&tx["to"], "target")?)? != evm_address_word(&call.to)?
        || bytes(string(&tx["data"], "calldata")?)? != call.data
        || integer(&tx["value"])? != call.value
    {
        return Err("Relay deposit does not match the verified order".into());
    }
    Ok((relay, expected, minimum))
}

/// A provider success label is not chain finality; callers verify the destination receipt.
pub(crate) async fn status(
    client: &reqwest::Client,
    relay: &RelayData,
    source: &str,
    hash: &str,
) -> Result<(String, Option<String>), String> {
    let mut request = client.get("https://api.relay.link/intents/status/v3");
    if let Ok(key) = std::env::var("VAULTFORGE_RELAY_API_KEY") {
        request = request.header("x-api-key", key);
    }
    let body: Value = request
        .query(&[("requestId", &relay.request_id)])
        .send()
        .await
        .map_err(|e| format!("Route status unavailable: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Route status unavailable: {e}"))?
        .json()
        .await
        .map_err(|_| "Invalid route status")?;
    let state = string(&body["status"], "status")?;
    if !matches!(
        state,
        "waiting"
            | "depositing"
            | "pending"
            | "submitted"
            | "success"
            | "delayed"
            | "refund"
            | "failure"
    ) {
        return Err("Unknown route status; funds remain pending".into());
    }
    if matches!(state, "waiting") {
        return Ok((state.into(), None));
    }
    if body["originChainId"].as_u64() != network_by_id(source).and_then(|n| n.chain_id)
        || body["destinationChainId"].as_u64()
            != network_by_id(&relay.destination_network).and_then(|n| n.chain_id)
        || !array(&body["inTxHashes"])?
            .iter()
            .any(|v| v.as_str().is_some_and(|h| h.eq_ignore_ascii_case(hash)))
    {
        return Err("Route status does not match the saved source transaction".into());
    }
    let destination = if state == "success" {
        let hashes = array(&body["txHashes"])?;
        if hashes.len() != 1 {
            return Err("Destination transaction proof unavailable".into());
        }
        let h = string(&hashes[0], "destination hash")?;
        if bytes(h)?.len() != 32 {
            return Err("Invalid destination hash".into());
        }
        Some(h.into())
    } else {
        None
    };
    Ok((state.into(), destination))
}

#[cfg(test)]
#[path = "../tests/providers/relay.rs"]
mod tests;

pub(crate) async fn destination_progress(
    client: &reqwest::Client,
    intent: &crate::uniswap::SwapIntent,
    source_hash: &str,
) -> Result<RelayData, String> {
    use crate::providers::http::{json_rpc_result, rpc_post_network};
    use crate::tx::tx_status::{Execution, Finality};
    let mut relay = intent.relay.clone().ok_or("Route missing")?;
    if relay
        .destination_status
        .as_ref()
        .is_some_and(|s| s.finality == Finality::Finalized && s.execution == Execution::Succeeded)
    {
        return Ok(relay);
    }
    let (status, hash) = status(client, &relay, &intent.network.id, source_hash).await?;
    relay.status = Some(status);
    if let Some(hash) = hash {
        let network = network_by_id(&relay.destination_network).ok_or("Destination missing")?;
        crate::providers::evm::verify_evm_chain(client, network).await?;
        let observation =
            crate::providers::evm::fetch_evm_tx_observation(client, network, &hash).await?;
        if let Some(observation) = observation {
            if observation.execution != Execution::Succeeded {
                return Err(
                    "Route service reported success but destination transaction failed".into(),
                );
            }
            let response=rpc_post_network(client,&network.id,network.rpc_url()?,&json!({"jsonrpc":"2.0","id":1,"method":"eth_getTransactionReceipt","params":[hash]})).await?;
            let receipt = json_rpc_result(&response, "Relay settlement receipt")?;
            verify_destination_receipt(intent, &relay, &hash, &observation, receipt)?;
            relay.destination_hash = Some(hash);
            relay.destination_status = Some(observation);
        }
    }
    Ok(relay)
}

fn verify_destination_receipt(
    intent: &crate::uniswap::SwapIntent,
    relay: &RelayData,
    hash: &str,
    observation: &crate::tx::tx_status::TxStatus,
    receipt: &Value,
) -> Result<(), String> {
    if observation.execution != crate::tx::tx_status::Execution::Succeeded
        || observation.block_hash.is_none()
        || receipt["status"] != "0x1"
        || receipt["transactionHash"]
            .as_str()
            .is_none_or(|h| !h.eq_ignore_ascii_case(&hash))
        || receipt["blockHash"].as_str() != observation.block_hash.as_deref()
    {
        return Err("Destination receipt changed; check progress again".into());
    }
    let transfer = format!(
        "0x{}",
        hex::encode(Keccak256::digest("Transfer(address,address,uint256)"))
    );
    let recipient = format!("0x{}", hex::encode(evm_address_word(&intent.sender)?));
    let mut received = 0u128;
    let mut spent = 0u128;
    let logs = receipt["logs"]
        .as_array()
        .ok_or("Destination transfer logs missing")?;
    for log in logs {
        if log["removed"].as_bool() == Some(true) {
            continue;
        }
        if log["address"]
            .as_str()
            .is_none_or(|a| !a.eq_ignore_ascii_case(&intent.token_out))
        {
            continue;
        }
        let topics = log["topics"]
            .as_array()
            .ok_or("Malformed destination transfer log")?;
        if topics.len() != 3 || topics[0].as_str() != Some(&transfer) {
            continue;
        }
        let incoming = topics[2]
            .as_str()
            .is_some_and(|t| t.eq_ignore_ascii_case(&recipient));
        let outgoing = topics[1]
            .as_str()
            .is_some_and(|t| t.eq_ignore_ascii_case(&recipient));
        if !incoming && !outgoing {
            continue;
        }
        if log
            .get("transactionHash")
            .is_some_and(|h| h.as_str().is_none_or(|h| !h.eq_ignore_ascii_case(hash)))
            || log
                .get("blockHash")
                .is_some_and(|h| h.as_str() != observation.block_hash.as_deref())
        {
            return Err("Destination transfer log identity mismatch".into());
        }
        let data = bytes(string(&log["data"], "transfer amount")?)?;
        if data.len() != 32 || data[..16] != [0; 16] {
            return Err("Invalid destination transfer amount".into());
        }
        let amount = u128::from_be_bytes(
            data[16..]
                .try_into()
                .map_err(|_| "Transfer amount missing")?,
        );
        if incoming {
            received = received
                .checked_add(amount)
                .ok_or("Destination amount overflow")?;
        }
        if outgoing {
            spent = spent
                .checked_add(amount)
                .ok_or("Destination amount overflow")?;
        }
    }
    let minimum = integer(&relay.order["output"]["payments"][0]["minimumAmount"])?;
    if received.checked_sub(spent).is_none_or(|net| net < minimum) {
        return Err(
            "Destination receipt does not prove the agreed token amount reached your wallet".into(),
        );
    }
    Ok(())
}
