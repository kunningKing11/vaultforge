//! Additional swap-provider terms. Chain-specific transactions are explicit.
use crate::operations::RouteNode;
use crate::tx::evm::EvmCall;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RouteProvider {
    Jupiter,
    Lifi,
    Relay,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "family", rename_all = "snake_case")]
pub(crate) enum RouteTransaction {
    Evm {
        to: String,
        data: String,
        value: String,
    },
    Solana {
        transaction: String,
        lookups: Vec<crate::operations::SolanaLookup>,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct ApiSwap {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) solana_tokens: Option<crate::solana_swap_tokens::SwapTokens>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) delivery: Option<crate::solana_delivery::DeliveryEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) jupiter: Option<crate::jupiter::JupiterTerms>,
    pub(crate) provider: RouteProvider,
    pub(crate) request_id: String,
    pub(crate) route: Vec<RouteNode>,
    pub(crate) transaction: RouteTransaction,
}
impl ApiSwap {
    pub(crate) fn evm_call(&self) -> Result<EvmCall, String> {
        match &self.transaction {
            RouteTransaction::Evm { to, data, value } => Ok(EvmCall {
                to: to.clone(),
                data: hex::decode(
                    data.strip_prefix("0x")
                        .ok_or("Saved LI.FI data prefix missing")?,
                )
                .map_err(|_| "Invalid saved LI.FI calldata")?,
                value: value
                    .parse()
                    .map_err(|_| "Invalid saved LI.FI native value")?,
            }),
            _ => Err("Solana routes require native execution".into()),
        }
    }
}

pub(crate) fn native(network: &str) -> &'static str {
    if network == "solana" {
        crate::tx::swap_solana::NATIVE
    } else {
        crate::providers::relay::NATIVE
    }
}
pub(crate) fn token_identity<'a>(network: &str, token: &'a str) -> Option<&'a str> {
    if (network == "solana"
        && (token == native(network) || token == crate::providers::relay::NATIVE))
        || (network != "solana" && token.eq_ignore_ascii_case(native(network)))
    {
        None
    } else {
        Some(token)
    }
}
pub(crate) fn is_solana(operation: &crate::operations::Operation) -> bool {
    operation.source_asset.network == "solana"
        && operation.swap.as_ref().is_some_and(|s| s.api.is_some())
}
pub(crate) fn validate_evm(
    operation: &crate::operations::Operation,
) -> Result<(crate::uniswap::SwapIntent, &crate::operations::SwapData), String> {
    let terms = operation.swap.as_ref().ok_or("Swap terms missing")?;
    let api = terms.api.as_ref().ok_or("API swap terms missing")?;
    let config = crate::registry::network_by_id(&operation.source_asset.network)
        .filter(|n| n.kind == "evm")
        .ok_or("Unsupported API swap source")?;
    if api.provider != RouteProvider::Lifi
        || operation.destination_asset.network != config.id
        || operation.sender != operation.recipient
        || terms.chain_id != config.chain_id
        || terms.relay.is_some()
        || !terms.router.eq_ignore_ascii_case(crate::tx::lifi::DIAMOND)
        || !terms.quoter.is_empty()
        || terms.pool_fee.is_some()
    {
        return Err("Unsupported LI.FI operation or changed network identity".into());
    }
    let token_in = operation
        .source_asset
        .token_address
        .as_deref()
        .unwrap_or(native(&config.id));
    let token_out = operation
        .destination_asset
        .token_address
        .as_deref()
        .unwrap_or(native(&config.id));
    let amount = operation
        .amount_base_units
        .parse()
        .map_err(|_| "Invalid LI.FI swap amount")?;
    let minimum = terms
        .minimum_received
        .parse()
        .map_err(|_| "Invalid LI.FI minimum")?;
    let expected = terms
        .quoted_output
        .parse()
        .map_err(|_| "Invalid LI.FI expected output")?;
    if minimum < crate::providers::uniswap::minimum_output(expected, terms.slippage_bps)? {
        return Err("Saved LI.FI slippage terms changed".into());
    }
    crate::tx::lifi::verify_call(
        &api.evm_call()?,
        &operation.sender,
        token_in,
        token_out,
        amount,
        minimum,
    )?;
    Ok((
        crate::uniswap::SwapIntent {
            api: Some(api.clone()),
            relay: None,
            network: config.clone(),
            sender: operation.sender.clone(),
            token_in: token_in.into(),
            token_out: token_out.into(),
            amount,
        },
        terms,
    ))
}

fn string<'a>(value: &'a serde_json::Value, name: &str) -> Result<&'a str, String> {
    value
        .as_str()
        .ok_or_else(|| format!("Swap response is missing {name}"))
}
fn units(value: &serde_json::Value) -> Result<u128, String> {
    crate::tx::relay::integer(value)
}
fn same_asset(network: &str, a: &str, b: &str) -> bool {
    crate::dto::AssetIdentity::new(network, token_identity(network, a))
        == crate::dto::AssetIdentity::new(network, token_identity(network, b))
}

/// Bind provider metadata to the wallet selection before interpreting its transaction.
fn validate_lifi_response(
    body: &serde_json::Value,
    config: &crate::registry::NetworkConfig,
    sender: &str,
    from: &crate::dto::Asset,
    to: &crate::dto::Asset,
    input_amount: u128,
    bps: u16,
) -> Result<(u128, u128), String> {
    let network = config.id.as_str();
    let chain = if network == "solana" {
        1151111081099710
    } else {
        config.chain_id()?
    };
    let input = from.token_address.as_deref().unwrap_or(native(network));
    let output = to.token_address.as_deref().unwrap_or(native(network));
    let action = &body["action"];
    if action["fromChainId"].as_u64() != Some(chain)
        || action["toChainId"].as_u64() != Some(chain)
        || !same_asset(
            network,
            string(&action["fromToken"]["address"], "input asset")?,
            input,
        )
        || !same_asset(
            network,
            string(&action["toToken"]["address"], "output asset")?,
            output,
        )
        || units(&action["fromAmount"])? != input_amount
        || !same_account(network, string(&action["fromAddress"], "sender")?, sender)
        || !same_account(network, string(&action["toAddress"], "recipient")?, sender)
        || action["fromToken"]["decimals"].as_u64() != Some(u64::from(from.decimals))
        || action["toToken"]["decimals"].as_u64() != Some(u64::from(to.decimals))
    {
        return Err("LI.FI quote identity does not match the selected trade".into());
    }
    let estimate = &body["estimate"];
    if units(&estimate["fromAmount"])? != input_amount {
        return Err("LI.FI input amount changed".into());
    }
    let expected = units(&estimate["toAmount"])?;
    let minimum = units(&estimate["toAmountMin"])?;
    if minimum == 0
        || minimum > expected
        || minimum < crate::providers::uniswap::minimum_output(expected, bps)?
    {
        return Err("LI.FI minimum output does not enforce the selected slippage".into());
    }
    Ok((expected, minimum))
}

/// Only executable quotes enter the comparison/cache. Provider amounts alone
/// never authorize arbitrary calldata or a foreign Solana program.
pub(crate) async fn quote(
    state: &std::sync::Mutex<crate::state::AppState>,
    client: &reqwest::Client,
    provider: RouteProvider,
    network: &str,
    input: &str,
    output: &str,
    amount: &str,
    bps: u16,
) -> Result<crate::uniswap::UniswapQuote, String> {
    use crate::tx::evm::evm_address_word;
    let (generation, config, sender, from, to) = {
        let guard = state.lock().map_err(|_| "State lock failed")?;
        if guard.locked {
            return Err("Unlock the wallet to request a swap".into());
        }
        let wallet = guard.wallet.as_ref().ok_or("Wallet missing")?;
        let config = crate::registry::network_by_id(network)
            .filter(|n| n.kind == "evm" || n.kind == "svm")
            .ok_or("Unsupported API swap network")?
            .clone();
        if !wallet.enabled_networks.iter().any(|n| n == network) {
            return Err("Enable the selected swap network".into());
        }
        let sender = wallet
            .addresses
            .get(&config.address_key)
            .ok_or("Wallet address missing")?
            .clone();
        let from =
            crate::assets::cached_asset(&wallet.assets, network, token_identity(network, input))
                .ok_or("Select an input asset from the wallet")?;
        let to =
            crate::assets::cached_asset(&wallet.assets, network, token_identity(network, output))
                .ok_or("Select an output asset from the wallet")?;
        (guard.wallet_generation, config, sender, from, to)
    };
    let input = token_identity(network, input).unwrap_or(native(network));
    let output = token_identity(network, output).unwrap_or(native(network));
    let input_amount = units(&serde_json::Value::String(amount.into()))?;
    if input_amount == 0 || same_asset(network, input, output) {
        return Err("Select different assets and a positive input amount".into());
    }
    crate::providers::uniswap::minimum_output(1, bps)?;
    let solana_tokens = if network == "solana" {
        let tokens = crate::solana_swap_tokens::load(client, input, output).await?;
        if u32::from(tokens.input.decimals) != from.decimals
            || u32::from(tokens.output.decimals) != to.decimals
        {
            return Err(
                "Live Solana mint decimals differ from selected assets; refresh balances".into(),
            );
        }
        Some(tokens)
    } else {
        None
    };
    let mut jupiter = None;
    let (id, expected, minimum, transaction, tool) = match provider {
        RouteProvider::Jupiter => {
            if network != "solana" {
                return Err("Jupiter requires same-chain Solana".into());
            }
            let request = crate::providers::jupiter::OrderRequest {
                taker: sender.clone(),
                input_mint: input.into(),
                output_mint: output.into(),
                amount_base_units: amount.into(),
                slippage_bps: bps,
            };
            let order = crate::providers::jupiter::get_order(client, &request).await?;
            let lookups =
                crate::swap_solana::order_lookups(client, &order.transaction_base64).await?;
            let validated = crate::tx::jupiter::validate_order_with_tokens(
                &order,
                &request,
                &lookups,
                solana_tokens.as_ref(),
            )?;
            let last_valid_height = crate::jupiter::last_valid_height(&order)?;
            let deadline = crate::jupiter::deadline(&order)?;
            let result = (
                order.request_id.clone(),
                units(&serde_json::Value::String(
                    order.quoted_output_base_units.clone(),
                ))?,
                units(&serde_json::Value::String(
                    order.minimum_output_base_units.clone(),
                ))?,
                RouteTransaction::Solana {
                    transaction: order.transaction_base64.clone(),
                    lookups,
                },
                "Jupiter (Metis)".into(),
            );
            jupiter = Some(crate::jupiter::JupiterTerms {
                order,
                message_hash: validated.message_hash,
                last_valid_height,
                deadline,
            });
            result
        }
        RouteProvider::Lifi => {
            let body =
                crate::providers::lifi::quote(client, &config, &sender, input, output, amount, bps)
                    .await?;
            let chain = if network == "solana" {
                1151111081099710
            } else {
                config.chain_id()?
            };
            let (expected, minimum) =
                validate_lifi_response(&body, &config, &sender, &from, &to, input_amount, bps)?;
            let estimate = &body["estimate"];
            let transaction = if network == "solana" {
                let (raw, lookups) = crate::swap_solana::transaction(
                    client,
                    &body["transactionRequest"]["data"],
                    &sender,
                )
                .await?;
                RouteTransaction::Solana {
                    transaction: raw,
                    lookups,
                }
            } else {
                let request = &body["transactionRequest"];
                if request["chainId"].as_u64() != Some(chain)
                    || evm_address_word(string(&request["from"], "transaction sender")?)?
                        != evm_address_word(&sender)?
                {
                    return Err("LI.FI transaction network or sender changed".into());
                }
                let value = string(&request["value"], "native value")?;
                let value = if let Some(hex) = value.strip_prefix("0x") {
                    u128::from_str_radix(hex, 16).map_err(|_| "LI.FI native value overflow")?
                } else {
                    units(&request["value"])?
                };
                if token_identity(network, input).is_some()
                    && !string(&estimate["approvalAddress"], "approval spender")?
                        .eq_ignore_ascii_case(crate::tx::lifi::DIAMOND)
                {
                    return Err("LI.FI route uses an unsupported approval spender".into());
                }
                RouteTransaction::Evm {
                    to: string(&request["to"], "execution contract")?.into(),
                    data: string(&request["data"], "calldata")?.into(),
                    value: value.to_string(),
                }
            };
            (
                string(&body["id"], "quote ID")?.into(),
                expected,
                minimum,
                transaction,
                body["tool"].as_str().unwrap_or("LI.FI").to_owned(),
            )
        }
        RouteProvider::Relay => {
            if network != "solana" {
                return Err("Use the existing verified Relay EVM executor".into());
            }
            let mut request = client.post("https://api.relay.link/quote/v2");
            if let Ok(key) = std::env::var("VAULTFORGE_RELAY_API_KEY") {
                request = request.header("x-api-key", key);
            }
            let body = serde_json::json!({
                "user": sender, "recipient": sender,
                "originChainId": 792703809u64, "destinationChainId": 792703809u64,
                "originCurrency": input, "destinationCurrency": output,
                "amount": amount, "tradeType": "EXACT_INPUT",
                "slippageTolerance": bps.to_string(), "usePermit": false,
                "refundTo": sender, "includedSwapSources": ["jupiter"],
                "disableSwapProviderPreference": true, "useV1Transaction": false
            });
            let response = request
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("Relay Solana quote unavailable: {e}"))?;
            if matches!(response.status().as_u16(), 401 | 403) {
                return Err("Relay requires VAULTFORGE_RELAY_API_KEY for Solana quotes".into());
            }
            let body: serde_json::Value = response
                .error_for_status()
                .map_err(|_| "No Relay Solana route available")?
                .json()
                .await
                .map_err(|_| "Invalid Relay Solana response")?;
            let details = &body["details"];
            if string(&details["sender"], "sender")? != sender
                || string(&details["recipient"], "recipient")? != sender
            {
                return Err("Relay Solana wallet identity changed".into());
            }
            for (value, token, decimals) in [
                (&details["currencyIn"], input, from.decimals),
                (&details["currencyOut"], output, to.decimals),
            ] {
                if value["currency"]["chainId"].as_u64() != Some(792703809)
                    || !same_asset(
                        network,
                        string(&value["currency"]["address"], "currency")?,
                        token,
                    )
                    || value["currency"]["decimals"].as_u64() != Some(u64::from(decimals))
                {
                    return Err("Relay Solana asset identity changed".into());
                }
            }
            if units(&details["currencyIn"]["amount"])? != input_amount {
                return Err("Relay Solana input changed".into());
            }
            let expected = units(&details["currencyOut"]["amount"])?;
            let minimum = units(&details["currencyOut"]["minimumAmount"])?;
            let steps = body["steps"]
                .as_array()
                .filter(|s| s.len() == 1)
                .ok_or("Relay Solana route requires unsupported additional steps")?;
            if steps[0]["kind"] != "transaction" {
                return Err("Unexpected Relay Solana signature step".into());
            }
            let items = steps[0]["items"]
                .as_array()
                .filter(|s| s.len() == 1)
                .ok_or("Unsupported multi-transaction Solana route")?;
            let data = &items[0]["data"];
            let (raw, lookups) = crate::swap_solana::transaction(client, data, &sender).await?;
            (
                string(&steps[0]["requestId"], "request ID")?.into(),
                expected,
                minimum,
                RouteTransaction::Solana {
                    transaction: raw,
                    lookups,
                },
                "Relay".into(),
            )
        }
    };
    if minimum == 0
        || minimum > expected
        || minimum < crate::providers::uniswap::minimum_output(expected, bps)?
    {
        return Err("Provider minimum output does not enforce the selected slippage".into());
    }
    let route = vec![
        RouteNode {
            network: network.into(),
            token_address: token_identity(network, input).map(Into::into),
            symbol: from.symbol.clone(),
            pool_fee: None,
            via: Some(tool.clone()),
        },
        RouteNode {
            network: network.into(),
            token_address: token_identity(network, output).map(Into::into),
            symbol: to.symbol.clone(),
            pool_fee: None,
            via: None,
        },
    ];
    let deadline = jupiter
        .as_ref()
        .map(|terms| terms.deadline)
        .unwrap_or_else(|| crate::uniswap::now() + 1200);
    let api = ApiSwap {
        solana_tokens,
        delivery: None,
        jupiter,
        provider,
        request_id: id,
        route: route.clone(),
        transaction,
    };
    let (approval_required, fee) = if network == "solana" {
        (
            false,
            u128::from(
                crate::swap_solana::quote_fee(
                    client,
                    &api,
                    &sender,
                    input,
                    output,
                    input_amount,
                    minimum,
                    bps,
                )
                .await?,
            ),
        )
    } else {
        crate::providers::lifi::verify_deployment(client, &config).await?;
        let call = api.evm_call()?;
        crate::tx::lifi::verify_call(&call, &sender, input, output, input_amount, minimum)?;
        let allowed = if token_identity(network, input).is_none() {
            true
        } else {
            crate::providers::evm::fetch_evm_allowance(
                client,
                &config,
                input,
                &sender,
                crate::tx::lifi::DIAMOND,
            )
            .await?
            .covers(input_amount)
        };
        let call = if allowed {
            call
        } else {
            crate::tx::uniswap::build_approval_call(input, crate::tx::lifi::DIAMOND, input_amount)?
        };
        let fee = crate::providers::evm::estimate_with_fee_headroom(
            crate::providers::evm::estimate_evm_call(client, &config, &sender, &call).await?,
        )?
        .max_native_fee;
        (!allowed, fee)
    };
    let quote = crate::uniswap::UniswapQuote {
        api: Some(api),
        id: crate::activity::random_hex(16),
        network: network.into(),
        destination_network: network.into(),
        relay: None,
        route,
        sender,
        token_in: input.into(),
        token_out: output.into(),
        amount_base_units: input_amount.to_string(),
        quoted_output: expected.to_string(),
        minimum_received: minimum.to_string(),
        slippage_bps: bps,
        pool_fee: None,
        deadline,
        approval_required,
        approval_max_fee: approval_required.then(|| fee.to_string()),
        swap_max_fee: (!approval_required).then(|| fee.to_string()),
        input_symbol: from.symbol,
        output_symbol: to.symbol,
        input_decimals: from.decimals,
        output_decimals: to.decimals,
    };
    let mut guard = state.lock().map_err(|_| "State lock failed")?;
    if !guard.can_commit_refresh(generation) {
        return Err("Wallet changed while requesting swap quotes".into());
    }
    guard
        .uniswap_quotes
        .retain(|_, q| q.deadline > crate::uniswap::now());
    if guard.uniswap_quotes.len() >= 16 {
        if let Some(id) = guard
            .uniswap_quotes
            .values()
            .min_by_key(|q| q.deadline)
            .map(|q| q.id.clone())
        {
            guard.uniswap_quotes.remove(&id);
        }
    }
    guard.uniswap_quotes.insert(quote.id.clone(), quote.clone());
    Ok(quote)
}

fn same_account(network: &str, a: &str, b: &str) -> bool {
    if network == "solana" {
        a == b
    } else {
        a.eq_ignore_ascii_case(b)
    }
}

pub(crate) async fn solana_max(
    state: &std::sync::Mutex<crate::state::AppState>,
    client: &reqwest::Client,
    output: &str,
    bps: u16,
) -> Result<crate::uniswap::UniswapComparison, String> {
    let sender = {
        let guard = state.lock().map_err(|_| "State lock failed")?;
        guard
            .wallet
            .as_ref()
            .filter(|_| !guard.locked)
            .and_then(|w| w.addresses.get("solana"))
            .ok_or("Unlock the Solana wallet")?
            .clone()
    };
    let balance = crate::providers::solana::fetch_solana_native_balance(client, &sender).await?;
    let mut spend = balance / 2;
    if spend == 0 {
        return Err("No SOL is available to swap".into());
    }
    for _ in 0..4 {
        let comparison = Box::pin(crate::uniswap::compare_quotes(
            state,
            client,
            "solana",
            "solana",
            native("solana"),
            output,
            &spend.to_string(),
            bps,
            false,
        ))
        .await?;
        let best = comparison
            .quotes
            .first()
            .ok_or("No Solana Max route available")?;
        let fee = best
            .swap_max_fee
            .as_ref()
            .ok_or("Solana swap fee unavailable")?
            .parse::<u128>()
            .map_err(|_| "Invalid Solana swap fee")?;
        let next = balance
            .checked_sub(fee)
            .filter(|amount| *amount > 0)
            .ok_or("SOL balance cannot cover swap fees and rent")?;
        if next == spend {
            return Ok(comparison);
        }
        spend = next;
    }
    Err("Solana swap fees changed while calculating Max; retry or enter an amount".into())
}

#[cfg(test)]
#[path = "tests/swap_routes.rs"]
mod tests;
