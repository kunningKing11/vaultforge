//! Solana token sends: live preflight, exact instruction review and submission checks.
use crate::dto::{
    SignedTransaction, SolanaTokenSendReview, SolanaTokenSendSource, SolanaTransferFeeSchedule,
};
use crate::providers::http::{json_rpc_result, rpc_post};
use crate::solana_tokens::{TokenDescriptor, TokenHolding, inspect_holding, inspect_mint};
use crate::tx::solana::{
    parse_token_send_units as units, token_send_instructions as instructions,
    verify_signed_transfer as verify_signed,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use solana_hash::Hash;
use solana_instruction::Instruction;
use solana_message::Message;
use solana_pubkey::Pubkey;
use std::collections::BTreeSet;
use std::str::FromStr;

pub(crate) trait TokenSendRpc {
    fn token_send_rpc(
        &self,
        method: &str,
        params: Value,
    ) -> impl std::future::Future<Output = Result<Value, String>> + Send;
}
impl TokenSendRpc for reqwest::Client {
    async fn token_send_rpc(&self, method: &str, params: Value) -> Result<Value, String> {
        let network = crate::registry::network_by_id("solana").ok_or("Solana network missing")?;
        let response = rpc_post(
            self,
            network.rpc_url()?,
            &json!({
                "jsonrpc":"2.0", "id":1, "method":method, "params":params
            }),
        )
        .await?;
        Ok(json_rpc_result(&response, "Solana token send")?.clone())
    }
}
fn key(value: &str) -> Result<Pubkey, String> {
    Pubkey::from_str(value).map_err(|_| "Invalid Solana send address".into())
}

async fn descriptor(
    client: &(impl TokenSendRpc + Sync),
    mint: &str,
) -> Result<TokenDescriptor, String> {
    let value = client
        .token_send_rpc(
            "getAccountInfo",
            json!([mint,{"encoding":"base64","commitment":"confirmed"}]),
        )
        .await?;
    let descriptor = inspect_mint(mint, &value["value"])?;
    if let Some(reason) = descriptor.unsupported_reason() {
        return Err(reason);
    }
    Ok(descriptor)
}
async fn epoch(
    client: &(impl TokenSendRpc + Sync),
    descriptor: &TokenDescriptor,
) -> Result<u64, String> {
    if descriptor.transfer_fee.is_none() {
        return Ok(0);
    }
    client
        .token_send_rpc("getEpochInfo", json!([{"commitment":"confirmed"}]))
        .await?["epoch"]
        .as_u64()
        .ok_or_else(|| "Solana fee epoch unavailable".into())
}
fn validate_holding(holding: &TokenHolding, descriptor: &TokenDescriptor) -> Result<(), String> {
    if holding.mint != descriptor.mint
        || holding.program != descriptor.program
        || descriptor
            .required_account_extensions
            .iter()
            .any(|e| !holding.extensions.contains(e))
    {
        return Err("Solana send account program, mint or required extensions changed".into());
    }
    if let Some(reason) = holding.unsupported_reason() {
        return Err(reason);
    }
    Ok(())
}
async fn recipient_rent(
    client: &(impl TokenSendRpc + Sync),
    descriptor: &TokenDescriptor,
    to: &str,
) -> Result<u64, String> {
    let address = descriptor.associated_address(to)?;
    let response = client
        .token_send_rpc(
            "getAccountInfo",
            json!([address,{"encoding":"base64","commitment":"confirmed"}]),
        )
        .await?;
    let value = response
        .get("value")
        .ok_or("Solana recipient account response missing")?;
    if !value.is_null() {
        validate_holding(&inspect_holding(&address, to, value)?, descriptor)?;
        return Ok(0);
    }
    client
        .token_send_rpc(
            "getMinimumBalanceForRentExemption",
            json!([descriptor.account_len()?,{"commitment":"confirmed"}]),
        )
        .await?
        .as_u64()
        .ok_or_else(|| "Solana token account rent unavailable".into())
}
fn review(
    descriptor: &TokenDescriptor,
    epoch: u64,
    sources: Vec<(String, u64)>,
    rent: u64,
) -> Result<SolanaTokenSendReview, String> {
    let mut total = 0u64;
    let mut fees = 0u64;
    let mut seen = BTreeSet::new();
    let sources = sources
        .into_iter()
        .map(|(address, amount)| {
            key(&address)?;
            if amount == 0 || !seen.insert(address.clone()) {
                return Err("Invalid or duplicate Solana send source".into());
            }
            let fee = descriptor.fee(epoch, amount)?;
            total = total
                .checked_add(amount)
                .ok_or("Solana send amount overflow")?;
            fees = fees.checked_add(fee).ok_or("Solana send fee overflow")?;
            Ok(SolanaTokenSendSource {
                address,
                amount: amount.to_string(),
                fee: fee.to_string(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    if sources.is_empty() || sources.len() > 8 || total <= fees {
        return Err(
            "Solana send requires positive net delivery and at most eight source accounts".into(),
        );
    }
    let fee_schedule = descriptor.transfer_fee.as_ref().map(|config| {
        let fee = config.get_epoch_fee(epoch);
        SolanaTransferFeeSchedule {
            epoch: u64::from(fee.epoch).to_string(),
            basis_points: u16::from(fee.transfer_fee_basis_points),
            maximum_fee: u64::from(fee.maximum_fee).to_string(),
        }
    });
    let mut mint_extensions = descriptor
        .extensions
        .iter()
        .copied()
        .map(u16::from)
        .collect::<Vec<_>>();
    mint_extensions.sort_unstable();
    Ok(SolanaTokenSendReview {
        token_program: descriptor.program.id().into(),
        mint_extensions,
        fee_schedule,
        sources,
        transfer_fee: fees.to_string(),
        recipient_amount: (total - fees).to_string(),
        account_rent_lamports: rent.to_string(),
    })
}

#[derive(Debug)]
pub(crate) struct PreparedTokenSend {
    pub(crate) review: SolanaTokenSendReview,
    pub(crate) instructions: Vec<Instruction>,
    pub(crate) blockhash: String,
    pub(crate) fee: u64,
    pub(crate) source_balance: u128,
    pub(crate) decimals: u8,
}
async fn fee_and_balance(
    client: &(impl TokenSendRpc + Sync),
    message: &Message,
    from: &str,
    review: &SolanaTokenSendReview,
) -> Result<u64, String> {
    let fee = client
        .token_send_rpc(
            "getFeeForMessage",
            json!([STANDARD.encode(message.serialize()),{"commitment":"confirmed"}]),
        )
        .await?["value"]
        .as_u64()
        .filter(|v| *v > 0)
        .ok_or("Solana send network fee unavailable")?;
    let balance = client
        .token_send_rpc("getBalance", json!([from,{"commitment":"confirmed"}]))
        .await?["value"]
        .as_u64()
        .ok_or("Solana send SOL balance unavailable")?;
    let required = fee
        .checked_add(units(&review.account_rent_lamports)?)
        .ok_or("Solana send SOL budget overflow")?;
    if balance < required {
        return Err("Insufficient SOL for transaction fee and token account rent".into());
    }
    Ok(fee)
}
async fn check_network(client: &(impl TokenSendRpc + Sync)) -> Result<(), String> {
    if client
        .token_send_rpc("getGenesisHash", json!([]))
        .await?
        .as_str()
        != Some("5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d")
    {
        return Err("Solana token send RPC is not mainnet".into());
    }
    Ok(())
}

pub(crate) async fn prepare(
    client: &(impl TokenSendRpc + Sync),
    from: &str,
    to: &str,
    mint: &str,
    amount: u64,
) -> Result<PreparedTokenSend, String> {
    key(from)?;
    key(to)?;
    if amount == 0 {
        return Err("Solana send amount must be positive".into());
    }
    check_network(client).await?;
    let descriptor = descriptor(client, mint).await?;
    let epoch = epoch(client, &descriptor).await?;
    let response = client.token_send_rpc("getTokenAccountsByOwner", json!([from,{"programId":descriptor.program.id()},{"encoding":"base64","commitment":"confirmed"}])).await?;
    let values = response["value"]
        .as_array()
        .ok_or("Solana send token account list missing")?;
    let mut holdings = Vec::new();
    let mut seen = BTreeSet::new();
    let mut excluded = None;
    for value in values {
        let address = value["pubkey"]
            .as_str()
            .ok_or("Solana send source address missing")?;
        let holding = inspect_holding(address, from, &value["account"])?;
        if !seen.insert(address) || holding.program != descriptor.program {
            return Err("Solana source discovery returned a duplicate or foreign program".into());
        }
        if holding.mint != mint {
            continue;
        }
        match validate_holding(&holding, &descriptor) {
            Ok(()) => holdings.push(holding),
            Err(reason) => excluded = Some(reason),
        }
    }
    let ata = descriptor.associated_address(from)?;
    holdings.sort_by(|a, b| {
        (a.address != ata, std::cmp::Reverse(a.amount), &a.address).cmp(&(
            b.address != ata,
            std::cmp::Reverse(b.amount),
            &b.address,
        ))
    });
    let source_balance = holdings.iter().map(|h| u128::from(h.amount)).sum();
    if source_balance < u128::from(amount) {
        return Err(excluded.unwrap_or_else(|| {
            "Insufficient live token balance across spendable Solana accounts".into()
        }));
    }
    let mut remaining = amount;
    let mut sources = Vec::new();
    for holding in holdings {
        let amount = remaining.min(holding.amount);
        if amount > 0 {
            sources.push((holding.address, amount));
            remaining -= amount;
        }
        if remaining == 0 {
            break;
        }
    }
    let rent = recipient_rent(client, &descriptor, to).await?;
    let review = review(&descriptor, epoch, sources, rent)?;
    let instructions = instructions(from, to, mint, descriptor.decimals, &review)?;
    let blockhash = client
        .token_send_rpc("getLatestBlockhash", json!([{"commitment":"finalized"}]))
        .await?["value"]["blockhash"]
        .as_str()
        .ok_or("Solana send blockhash missing")?
        .to_string();
    let message = Message::new_with_blockhash(
        &instructions,
        Some(&key(from)?),
        &Hash::from_str(&blockhash).map_err(|_| "Invalid Solana send blockhash")?,
    );
    let fee = fee_and_balance(client, &message, from, &review).await?;
    // Last live read before signing: exact sources, recipient and active fee terms.
    revalidate(client, from, to, mint, descriptor.decimals, amount, &review).await?;
    Ok(PreparedTokenSend {
        review,
        instructions,
        blockhash,
        fee,
        source_balance,
        decimals: descriptor.decimals,
    })
}

async fn revalidate(
    client: &(impl TokenSendRpc + Sync),
    from: &str,
    to: &str,
    mint: &str,
    decimals: u8,
    amount: u64,
    saved: &SolanaTokenSendReview,
) -> Result<(), String> {
    let descriptor = descriptor(client, mint).await?;
    if descriptor.decimals != decimals {
        return Err("Solana mint decimals changed; review a fresh transfer".into());
    }
    let epoch = epoch(client, &descriptor).await?;
    if saved.sources.is_empty() || saved.sources.len() > 8 {
        return Err("Invalid saved Solana token sources".into());
    }
    let addresses = saved
        .sources
        .iter()
        .map(|s| s.address.clone())
        .collect::<Vec<_>>();
    let response = client
        .token_send_rpc(
            "getMultipleAccounts",
            json!([addresses,{"encoding":"base64","commitment":"confirmed"}]),
        )
        .await?;
    let values = response["value"]
        .as_array()
        .filter(|v| v.len() == saved.sources.len())
        .ok_or("Solana send source account response mismatch")?;
    let mut sources = Vec::new();
    let mut total = 0u64;
    for (source, value) in saved.sources.iter().zip(values) {
        let holding = inspect_holding(&source.address, from, value)?;
        validate_holding(&holding, &descriptor)?;
        let spend = units(&source.amount)?;
        if holding.amount < spend {
            return Err("Solana source balance changed; review a fresh transfer".into());
        }
        total = total
            .checked_add(spend)
            .ok_or("Solana transfer amount overflow")?;
        sources.push((source.address.clone(), spend));
    }
    if total != amount {
        return Err("Solana source debit differs from the reviewed amount".into());
    }
    let rent = recipient_rent(client, &descriptor, to).await?;
    if &review(&descriptor, epoch, sources, rent)? != saved {
        return Err("Solana token fee, account rent or mint policy changed; edit and review a fresh transfer".into());
    }
    Ok(())
}

pub(crate) async fn validate_submission(
    client: &(impl TokenSendRpc + Sync),
    signed: &SignedTransaction,
) -> Result<(), String> {
    let tx = verify_signed(signed)?;
    check_network(client).await?;
    let review = signed
        .solana_token_send
        .as_ref()
        .ok_or("Solana send review missing")?;
    revalidate(
        client,
        &signed.from,
        &signed.to,
        signed
            .token_address
            .as_deref()
            .ok_or("Solana mint missing")?,
        signed.decimals as u8,
        units(&signed.amount)?,
        review,
    )
    .await?;
    let fee = fee_and_balance(client, &tx.message, &signed.from, review).await?;
    if units(&signed.fee_amount)? != fee {
        return Err("Solana network fee changed; review a fresh transfer".into());
    }
    let simulation = client.token_send_rpc("simulateTransaction", json!([signed.raw_tx,{"encoding":"base64","sigVerify":true,"replaceRecentBlockhash":false,"commitment":"confirmed"}])).await?;
    if simulation["value"].get("err") != Some(&Value::Null) {
        return Err("Solana token send simulation failed; review a fresh transfer".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests/solana_send.rs"]
mod tests;
