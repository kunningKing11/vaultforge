//! Read-only preparation. Durable authorization, signing, and broadcast are task 8.

use crate::cctp::{BURN_GAS_BUDGET, CctpTransfer, MINT_GAS_BUDGET, STANDARD_FINALITY_THRESHOLD};
use crate::providers::evm::{
    EvmCallEstimate, estimate_evm_call, estimate_with_fee_headroom, evm_max_native_fee,
    fetch_evm_allowance, fetch_evm_fee_estimate, fetch_evm_native_balance,
    fetch_evm_token_balance_for_contract, verify_evm_chain,
};
use crate::tx::cctp::{build_approval_call, build_burn_call, build_mint_call};
use crate::tx::evm::EvmCall;

#[derive(Debug)]
pub(crate) struct PreparedCctpCall {
    pub(crate) call: EvmCall,
    pub(crate) estimate: EvmCallEstimate,
}

#[derive(Debug)]
pub(crate) enum CctpSourcePreparation {
    Approval {
        approval: PreparedCctpCall,
        provisional_burn_fee: u128,
    },
    Burn(PreparedCctpCall),
}

#[derive(Debug)]
pub(crate) struct CctpPreflight {
    pub(crate) source: CctpSourcePreparation,
    pub(crate) source_native_budget: u128,
    /// Uses a provisional gas budget until the attested message is available.
    pub(crate) provisional_destination_fee: u128,
    pub(crate) protocol_fee: u128,
    pub(crate) expected_receive_amount: u128,
}

/// The pinned route has no Standard fee switch. Permit only a live zero fee;
/// a missing, duplicate, malformed, or newly nonzero fee fails closed. No floats
/// are converted to crypto amounts, and no unsupported fee-switch call is made.
fn validate_standard_fee(json: &serde_json::Value) -> Result<(), String> {
    let entries = json.as_array().ok_or("Invalid CCTP fee response")?;
    let mut standard = entries.iter().filter(|entry| {
        entry["finalityThreshold"].as_u64() == Some(STANDARD_FINALITY_THRESHOLD as u64)
    });
    let entry = standard.next().ok_or("CCTP Standard fee is missing")?;
    if standard.next().is_some() {
        return Err("Duplicate CCTP Standard fee".into());
    }
    let fee = entry["minimumFee"]
        .as_number()
        .ok_or("Invalid CCTP Standard fee")?
        .to_string();
    if fee != "0" && fee != "0.0" {
        return Err("CCTP route permits only zero Standard protocol fee".into());
    }
    Ok(())
}

pub(crate) async fn fetch_standard_fee_at(
    client: &reqwest::Client,
    url: &str,
) -> Result<(), String> {
    // A single dropped connection must not fail the transfer; retry transport
    // errors, then fail closed on any missing or nonzero fee.
    let response = crate::providers::http::resilient_get(client, url, &[]).await?;
    if !response.status().is_success() {
        return Err(format!(
            "CCTP fee request returned HTTP {}",
            response.status()
        ));
    }
    let json = response
        .json()
        .await
        .map_err(|error| format!("Invalid CCTP fee response: {error}"))?;
    validate_standard_fee(&json)
}

fn require_gas(balance: u128, fee: u128, context: &str) -> Result<(), String> {
    if balance < fee {
        return Err(format!("Insufficient native gas funds for CCTP {context}"));
    }
    Ok(())
}

pub(crate) async fn prepare_cctp_source(
    client: &reqwest::Client,
    transfer: &CctpTransfer,
) -> Result<CctpPreflight, String> {
    transfer.route.validate()?;
    if transfer.route.source.network.kind != "evm" {
        return Err("Solana CCTP source execution preparation is unavailable; native attestation reads are supported".into());
    }

    let route = &transfer.route;
    let fee_url = format!(
        "{}/v2/burn/USDC/fees/{}/{}",
        route.attestation_host, route.source.domain, route.destination.domain
    );
    fetch_standard_fee_at(client, &fee_url).await?;
    prepare_source_after_fee_check(client, transfer).await
}

/// Also called again after approval, so burn signing always uses fresh reads and
/// real calldata estimation rather than the initial provisional budget.
async fn prepare_source_after_fee_check(
    client: &reqwest::Client,
    transfer: &CctpTransfer,
) -> Result<CctpPreflight, String> {
    transfer.route.validate()?;
    let source = &transfer.route.source;
    let destination = &transfer.route.destination;
    verify_evm_chain(client, &source.network).await?;
    if destination.network.kind == "evm" {
        verify_evm_chain(client, &destination.network).await?;
    }
    let usdc = fetch_evm_token_balance_for_contract(
        client,
        &source.network,
        source.usdc,
        transfer.sender(),
    )
    .await?;
    if usdc < transfer.amount() {
        return Err("Insufficient USDC balance for CCTP burn".into());
    }
    let allowance = fetch_evm_allowance(
        client,
        &source.network,
        source.usdc,
        transfer.sender(),
        source.token_messenger,
    )
    .await?;
    let source_balance =
        fetch_evm_native_balance(client, &source.network, transfer.sender()).await?;
    let provisional_destination_fee = if destination.network.kind == "svm" {
        u128::from(
            crate::providers::cctp_native::LiveNative { client }
                .destination_budget(transfer)
                .await?,
        )
    } else {
        let balance = fetch_evm_native_balance(
            client,
            &destination.network,
            transfer.destination_fee_payer()?,
        )
        .await?;
        let fees = fetch_evm_fee_estimate(client, &destination.network).await?;
        let budget = evm_max_native_fee(
            MINT_GAS_BUDGET,
            crate::providers::evm::with_fee_headroom(fees.max_fee_per_gas)?,
        )?;
        require_gas(balance, budget, "provisional destination mint budget")?;
        budget
    };
    let (source_preparation, source_native_budget) = if allowance.covers(transfer.amount()) {
        let call = build_burn_call(transfer)?;
        let raw = estimate_evm_call(client, &source.network, transfer.sender(), &call).await?;
        let estimate = estimate_with_fee_headroom(raw)?;
        let fee = estimate.max_native_fee;
        (
            CctpSourcePreparation::Burn(PreparedCctpCall { call, estimate }),
            fee,
        )
    } else {
        let call = build_approval_call(transfer)?;
        let raw = estimate_evm_call(client, &source.network, transfer.sender(), &call).await?;
        let estimate = estimate_with_fee_headroom(raw)?;
        let provisional_burn_fee =
            evm_max_native_fee(BURN_GAS_BUDGET, estimate.fees.max_fee_per_gas)?;
        let total = estimate
            .max_native_fee
            .checked_add(provisional_burn_fee)
            .ok_or("CCTP source gas budget overflows base units")?;
        (
            CctpSourcePreparation::Approval {
                approval: PreparedCctpCall { call, estimate },
                provisional_burn_fee,
            },
            total,
        )
    };
    require_gas(
        source_balance,
        source_native_budget,
        "source transaction budget",
    )?;
    Ok(CctpPreflight {
        source: source_preparation,
        source_native_budget,
        provisional_destination_fee,
        protocol_fee: 0,
        expected_receive_amount: transfer.amount(),
    })
}

/// Estimate only the real message/attestation calldata. Message binding, source
/// finality, replay checks, and authorization must precede signing in tasks 9–10.
pub(crate) async fn prepare_cctp_mint(
    client: &reqwest::Client,
    transfer: &CctpTransfer,
    message: &[u8],
    attestation: &[u8],
) -> Result<PreparedCctpCall, String> {
    let call = build_mint_call(transfer, message, attestation)?;
    let network = &transfer.route.destination.network;
    verify_evm_chain(client, network).await?;
    crate::providers::cctp_attestation::simulate_message_from(
        client,
        transfer,
        message,
        attestation,
        transfer.destination_fee_payer()?,
    )
    .await?;
    let estimate = estimate_with_fee_headroom(
        estimate_evm_call(client, network, transfer.destination_fee_payer()?, &call).await?,
    )?;
    let balance =
        fetch_evm_native_balance(client, network, transfer.destination_fee_payer()?).await?;
    require_gas(balance, estimate.max_native_fee, "destination mint")?;
    Ok(PreparedCctpCall { call, estimate })
}

#[cfg(test)]
#[path = "../tests/providers/cctp.rs"]
pub(crate) mod tests;
