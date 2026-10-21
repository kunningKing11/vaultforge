use serde_json::Value;

use crate::providers::evm::{
    EvmCallEstimate, estimate_evm_call, estimate_with_fee_headroom, verify_evm_chain,
};
use crate::registry::NetworkConfig;
use crate::tx::evm::{EvmCall, evm_address_word};
use crate::tx::uniswap::decode_deadline_swap;
use crate::uniswap_config::deployment_for_network;

pub(crate) const DEFAULT_SLIPPAGE_BPS: u16 = 50;
const MAX_QUOTE_LIFETIME_SECS: u64 = 1200;

/// The user's requested trade, independent of the API's response fields.
pub(crate) struct UniswapQuoteRequest<'a> {
    pub(crate) network_id: &'a str,
    pub(crate) token_in: &'a str,
    pub(crate) token_out: &'a str,
    pub(crate) sender: &'a str,
    pub(crate) amount: u128,
}

/// Only verification can construct this type. Its call cannot be changed by callers.
pub(crate) struct VerifiedUniswapQuote {
    network_id: String,
    sender: String,
    call: EvmCall,
    quoted_output: u128,
    minimum_received: u128,
    deadline: u64,
}

impl VerifiedUniswapQuote {
    pub(crate) fn call(&self) -> &EvmCall {
        &self.call
    }

    pub(crate) fn quoted_output(&self) -> u128 {
        self.quoted_output
    }

    pub(crate) fn minimum_received(&self) -> u128 {
        self.minimum_received
    }

    pub(crate) fn deadline(&self) -> u64 {
        self.deadline
    }
}

/// Calculates the minimum output in base units, rounding down.
/// Multiplication overflow is an error, never a wrapped or floating-point amount.
pub(crate) fn minimum_output(quoted_output: u128, slippage_bps: u16) -> Result<u128, String> {
    let retained_bps = 10_000_u16
        .checked_sub(slippage_bps)
        .ok_or("Swap slippage must be between 0 and 10,000 basis points")?;
    quoted_output
        .checked_mul(u128::from(retained_bps))
        .map(|scaled| scaled / 10_000)
        .ok_or_else(|| "Swap minimum output calculation overflows base units".into())
}

/// Checks separate quote and transaction responses against the requested trade.
/// Accepts only the configured SwapRouter02 format, not Universal Router or UniswapX.
/// `now` is a Unix timestamp supplied by the backend; verification never signs or sends.
pub(crate) fn verify_routing_quote(
    request: &UniswapQuoteRequest<'_>,
    quote_response: &Value,
    swap_response: &Value,
    slippage_bps: Option<u16>,
    now: u64,
) -> Result<VerifiedUniswapQuote, String> {
    let deployment = deployment_for_network(request.network_id)?;
    let token_in = nonzero_address(request.token_in)?;
    let token_out = nonzero_address(request.token_out)?;
    let sender = nonzero_address(request.sender)?;
    if token_in == token_out || request.amount == 0 {
        return Err("Swap requires different tokens and a positive input amount".into());
    }
    let quote = &quote_response["quote"];
    if quote_response["routing"].as_str() != Some("CLASSIC")
        || quote["chainId"].as_u64() != Some(deployment.chain_id)
        || nonzero_address(json_string(&quote["input"]["token"], "input token")?)? != token_in
        || nonzero_address(json_string(&quote["output"]["token"], "output token")?)? != token_out
        || positive_amount(&quote["input"]["amount"])? != request.amount
    {
        return Err("Uniswap quote does not match the requested network, tokens, or input".into());
    }
    let quoted_output = positive_amount(&quote["output"]["amount"])?;
    let swap = &swap_response["swap"];
    if swap["chainId"].as_u64() != Some(deployment.chain_id)
        || nonzero_address(json_string(&swap["from"], "transaction sender")?)? != sender
    {
        return Err("Swap transaction network or sender does not match the wallet".into());
    }
    let router = json_string(&swap["to"], "router")?;
    if nonzero_address(router)? != nonzero_address(deployment.router)? {
        return Err("Swap transaction targets an unsupported router".into());
    }
    // Both decimal and hex zero are common transaction-response representations.
    let value = json_string(&swap["value"], "native value")?;
    if value != "0" && value != "0x0" {
        return Err("ERC-20 swaps must send zero native coin".into());
    }
    let data = json_string(&swap["data"], "calldata")?
        .strip_prefix("0x")
        .filter(|data| data.len() == 840)
        .ok_or("Swap calldata must be a single deadline-wrapped call")?;
    let data = hex::decode(data).map_err(|_| "Invalid swap calldata hex")?;
    verify_encoded_swap(
        request,
        quoted_output,
        EvmCall {
            to: router.into(),
            value: 0,
            data,
        },
        slippage_bps,
        now,
    )
}

/// Applies the same intent checks to locally built calls from on-chain quotes.
pub(crate) fn verify_encoded_swap(
    request: &UniswapQuoteRequest<'_>,
    quoted_output: u128,
    call: EvmCall,
    slippage_bps: Option<u16>,
    now: u64,
) -> Result<VerifiedUniswapQuote, String> {
    let deployment = deployment_for_network(request.network_id)?;
    let token_in = nonzero_address(request.token_in)?;
    let token_out = nonzero_address(request.token_out)?;
    let sender = nonzero_address(request.sender)?;
    if token_in == token_out || request.amount == 0 || quoted_output == 0 {
        return Err("Swap requires different tokens and positive amounts".into());
    }
    if nonzero_address(&call.to)? != nonzero_address(deployment.router)? || call.value != 0 {
        return Err("Swap must target the configured router with zero native value".into());
    }
    let minimum_received =
        minimum_output(quoted_output, slippage_bps.unwrap_or(DEFAULT_SLIPPAGE_BPS))?;
    let decoded = decode_deadline_swap(&call.data)?;
    check_deadline(decoded.deadline, now)?;
    if decoded.token_in != token_in
        || decoded.token_out != token_out
        || decoded.recipient != sender
        || decoded.amount != request.amount
        || decoded.minimum_received != minimum_received
    {
        return Err(
            "Swap instructions do not match the tokens, recipient, amount, or minimum output"
                .into(),
        );
    }
    Ok(VerifiedUniswapQuote {
        network_id: deployment.network_id.into(),
        sender: request.sender.into(),
        call,
        quoted_output,
        minimum_received,
        deadline: decoded.deadline,
    })
}

/// Estimates the checked call using live RPC data and the existing fee headroom.
/// API gas estimates are not used. Approval fees must be estimated separately.
pub(crate) async fn estimate_routing_quote_fee(
    client: &reqwest::Client,
    network: &NetworkConfig,
    quote: &VerifiedUniswapQuote,
    now: u64,
) -> Result<EvmCallEstimate, String> {
    let deployment = deployment_for_network(&quote.network_id)?;
    if network.id != deployment.network_id
        || network.kind != "evm"
        || network.chain_id != Some(deployment.chain_id)
    {
        return Err("Swap fee provider does not match the verified quote network".into());
    }
    check_deadline(quote.deadline, now)?;
    verify_evm_chain(client, network).await?;
    let estimate = estimate_evm_call(client, network, &quote.sender, quote.call()).await?;
    estimate_with_fee_headroom(estimate)
}

fn check_deadline(deadline: u64, now: u64) -> Result<(), String> {
    if deadline <= now || deadline - now > MAX_QUOTE_LIFETIME_SECS {
        return Err("Swap expiry must be in the future and no more than 20 minutes away".into());
    }
    Ok(())
}

fn json_string<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    value
        .as_str()
        .ok_or_else(|| format!("Uniswap response has missing or invalid {field}"))
}

fn nonzero_address(address: &str) -> Result<[u8; 32], String> {
    let word = evm_address_word(address)?;
    if word == [0; 32] {
        return Err("Swap addresses must be nonzero".into());
    }
    Ok(word)
}

fn positive_amount(value: &Value) -> Result<u128, String> {
    let value = json_string(value, "token amount")?;
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("Uniswap token amounts must be decimal integer strings".into());
    }
    value
        .parse::<u128>()
        .ok()
        .filter(|amount| *amount > 0)
        .ok_or_else(|| "Uniswap token amount is zero or exceeds supported range".into())
}

pub(crate) async fn fetch_routing_quote(
    client: &reqwest::Client,
    api_key: &str,
    network_id: &str,
    token_in: &str,
    token_out: &str,
    sender: &str,
    amount: u128,
) -> Result<serde_json::Value, String> {
    let deployment = deployment_for_network(network_id)?;

    let body = serde_json::json!({
        "type": "EXACT_INPUT",
        "amount": amount.to_string(),
        "tokenInChainId": deployment.chain_id,
        "tokenOutChainId": deployment.chain_id,
        "tokenIn": token_in,
        "tokenOut": token_out,
        "swapper": sender,
        "slippageTolerance": 0.5
    });

    let response = client
        .post("https://trade-api.gateway.uniswap.org/v1/quote")
        .header("x-api-key", api_key)
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("Uniswap quote request failed: {error}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "Uniswap quote request returned HTTP {}",
            response.status()
        ));
    }

    let json = response
        .json::<serde_json::Value>()
        .await
        .map_err(|error| format!("Invalid Uniswap quote response: {error}"))?;

    Ok(json)
}

#[cfg(test)]
#[path = "../tests/providers/uniswap.rs"]
mod tests;

/// Reads pool prices directly from the pinned QuoterV2. No API key or foreign
/// router calldata is needed for this single-pool SwapRouter02 execution path.
pub(crate) struct LiveSwapProvider<'a> {
    pub(crate) client: &'a reqwest::Client,
}

impl crate::uniswap::SwapProvider for LiveSwapProvider<'_> {
    async fn route_progress(
        &self,
        intent: &crate::uniswap::SwapIntent,
        hash: &str,
    ) -> Result<crate::providers::relay::RelayData, String> {
        crate::providers::relay::destination_progress(self.client, intent, hash).await
    }
    async fn quote(
        &self,
        intent: &crate::uniswap::SwapIntent,
        fee: Option<u32>,
    ) -> Result<crate::uniswap::PoolQuote, String> {
        use crate::providers::http::{json_rpc_result, rpc_post_network};
        use crate::tx::uniswap::build_pool_quote_call;
        use crate::uniswap::{POOL_FEES, PoolQuote};

        verify_evm_chain(self.client, &intent.network).await?;
        let deployment = deployment_for_network(&intent.network.id)?;
        let fees = match fee {
            Some(fee) if POOL_FEES.contains(&fee) => vec![fee],
            Some(_) => return Err("Unsupported swap pool fee".into()),
            None => POOL_FEES.to_vec(),
        };
        let mut best: Option<PoolQuote> = None;
        for pool_fee in fees {
            let call = build_pool_quote_call(
                &intent.token_in,
                &intent.token_out,
                deployment.quoter_v2,
                intent.amount,
                pool_fee,
            )?;
            let json = rpc_post_network(self.client, &intent.network.id, intent.network.rpc_url()?, &serde_json::json!({
                "jsonrpc":"2.0", "id":1, "method":"eth_call",
                "params":[{"to":call.to, "data":format!("0x{}",hex::encode(call.data))}, "latest"],
            })).await?;
            if let Some(error) = json.get("error") {
                // Absent pools and insufficient pool liquidity revert. Transport,
                // rate limits, and other provider failures are not a missing pool.
                let message = error["message"].as_str().unwrap_or("").to_ascii_lowercase();
                if error["code"].as_i64() == Some(3) || message.contains("execution reverted") {
                    continue;
                }
                return Err(format!("Pool quote RPC error: {error}"));
            }
            let result = json_rpc_result(&json, "Uniswap pool quote")?
                .as_str()
                .ok_or("Invalid pool quote result")?;
            let amount_out = parse_pool_quote(result)?;
            if best
                .as_ref()
                .is_none_or(|current| amount_out > current.amount_out)
            {
                best = Some(PoolQuote {
                    pool_fee,
                    amount_out,
                });
            }
        }
        best.ok_or_else(|| "No supported V3 pool can quote this token pair and amount".into())
    }

    async fn allowance(&self, intent: &crate::uniswap::SwapIntent) -> Result<bool, String> {
        if intent
            .token_in
            .eq_ignore_ascii_case(crate::providers::relay::NATIVE)
        {
            return Ok(true);
        }
        let router = if intent.api.is_some() {
            crate::tx::lifi::DIAMOND
        } else if intent.relay.is_some() {
            crate::providers::relay::DEPOSITORY
        } else {
            deployment_for_network(&intent.network.id)?.router
        };
        Ok(crate::providers::evm::fetch_evm_allowance(
            self.client,
            &intent.network,
            &intent.token_in,
            &intent.sender,
            router,
        )
        .await?
        .covers(intent.amount))
    }

    async fn balances(
        &self,
        intent: &crate::uniswap::SwapIntent,
    ) -> Result<(u128, u128, u128), String> {
        use crate::providers::evm::{
            fetch_evm_native_balance, fetch_evm_token_balance_for_contract,
        };
        let native = fetch_evm_native_balance(self.client, &intent.network, &intent.sender).await?;
        let input = if intent
            .token_in
            .eq_ignore_ascii_case(crate::providers::relay::NATIVE)
        {
            native
        } else {
            fetch_evm_token_balance_for_contract(
                self.client,
                &intent.network,
                &intent.token_in,
                &intent.sender,
            )
            .await?
        };
        let destination = intent
            .relay
            .as_ref()
            .and_then(|r| crate::registry::network_by_id(&r.destination_network))
            .unwrap_or(&intent.network);
        let output = if intent
            .token_out
            .eq_ignore_ascii_case(crate::providers::relay::NATIVE)
        {
            fetch_evm_native_balance(self.client, destination, &intent.sender).await?
        } else {
            fetch_evm_token_balance_for_contract(
                self.client,
                destination,
                &intent.token_out,
                &intent.sender,
            )
            .await?
        };
        Ok((native, input, output))
    }

    async fn estimate(
        &self,
        intent: &crate::uniswap::SwapIntent,
        call: &EvmCall,
    ) -> Result<EvmCallEstimate, String> {
        if intent.api.is_some() {
            crate::providers::lifi::verify_deployment(self.client, &intent.network).await?;
        }
        verify_evm_chain(self.client, &intent.network).await?;
        estimate_with_fee_headroom(
            estimate_evm_call(self.client, &intent.network, &intent.sender, call).await?,
        )
    }

    async fn nonce(&self, intent: &crate::uniswap::SwapIntent) -> Result<u64, String> {
        crate::providers::evm::fetch_evm_nonce(self.client, &intent.network, &intent.sender).await
    }

    async fn observe(
        &self,
        intent: &crate::uniswap::SwapIntent,
        hash: &str,
    ) -> Result<Option<crate::tx::tx_status::TxStatus>, String> {
        crate::providers::evm::fetch_evm_tx_observation(self.client, &intent.network, hash).await
    }

    async fn observe_inclusion(
        &self,
        intent: &crate::uniswap::SwapIntent,
        hash: &str,
    ) -> Result<Option<crate::tx::tx_status::TxStatus>, String> {
        crate::providers::evm::fetch_evm_tx_inclusion(self.client, &intent.network, hash).await
    }

    async fn broadcast(
        &self,
        intent: &crate::uniswap::SwapIntent,
        raw: &str,
    ) -> Result<String, String> {
        crate::providers::evm::broadcast_evm_tx(self.client, &intent.network, raw).await
    }
}

fn parse_pool_quote(result: &str) -> Result<u128, String> {
    let hex = result
        .strip_prefix("0x")
        .filter(|value| value.len() == 256)
        .ok_or("Invalid QuoterV2 response length")?;
    let bytes = hex::decode(hex).map_err(|_| "Invalid QuoterV2 response hex")?;
    // amountOut(uint256), sqrtPriceX96After(uint160), initializedTicksCrossed(uint32), gasEstimate(uint256).
    if bytes[..16].iter().any(|b| *b != 0)
        || bytes[32..44].iter().any(|b| *b != 0)
        || bytes[64..92].iter().any(|b| *b != 0)
    {
        return Err("Pool quote amount or metadata exceeds supported range".into());
    }
    let output = u128::from_be_bytes(
        bytes[16..32]
            .try_into()
            .map_err(|_| "Invalid pool quote amount")?,
    );
    if output == 0 {
        return Err("Pool quote output must be positive".into());
    }
    Ok(output)
}
