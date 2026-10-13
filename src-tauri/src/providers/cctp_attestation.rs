//! Circle V2 reads and destination contract verification without submission.

use crate::cctp::CctpTransfer;
use crate::cctp_attestation::{
    AttestationCheck, AttestationProvider, MAX_API_BYTES, MessageResponse,
};
use crate::providers::evm::{fetch_evm_tx_observation, verify_evm_chain};
use crate::providers::http::{json_rpc_result, rpc_post};
use crate::tx::cctp::build_mint_call;
use crate::tx::evm::evm_uint_word;
use crate::tx::tx_status::TxStatus;

pub(crate) struct CircleAttestationProvider<'a> {
    pub(crate) client: &'a reqwest::Client,
}

async fn bounded_json(mut response: reqwest::Response) -> Result<serde_json::Value, String> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_API_BYTES as u64)
    {
        return Err("CCTP response is too large".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("CCTP response read failed: {error}"))?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_API_BYTES {
            return Err("CCTP response is too large".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| "Invalid CCTP API JSON".into())
}

async fn fetch_messages_at(
    client: &reqwest::Client,
    url: &str,
    hash: &str,
) -> Result<MessageResponse, String> {
    // Same retry discipline as RPC and fee reads; the app client also carries
    // a 15s global timeout, so a stalled attestation poll cannot hang.
    let response =
        crate::providers::http::resilient_get(client, url, &[("transactionHash", hash)]).await?;
    let requested = reqwest::Url::parse(url).map_err(|_| "Invalid CCTP API URL")?;
    if response.url().origin() != requested.origin() {
        return Err("CCTP API redirected outside the configured deployment".into());
    }
    match response.status().as_u16() {
        404 => Ok(MessageResponse::Pending),
        429 => {
            let delay = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse().ok())
                .unwrap_or(300);
            Ok(MessageResponse::RateLimited(delay))
        }
        200 => Ok(MessageResponse::Messages(bounded_json(response).await?)),
        _ => Err(format!(
            "CCTP attestation request returned HTTP {}",
            response.status()
        )),
    }
}

pub(crate) async fn simulate_message(
    client: &reqwest::Client,
    transfer: &CctpTransfer,
    message: &[u8],
    attestation: &[u8],
) -> Result<(), String> {
    simulate_message_from(
        client,
        transfer,
        message,
        attestation,
        transfer.destination_caller(),
    )
    .await
}

pub(crate) async fn simulate_message_from(
    client: &reqwest::Client,
    transfer: &CctpTransfer,
    message: &[u8],
    attestation: &[u8],
    caller: &str,
) -> Result<(), String> {
    let call = build_mint_call(transfer, message, attestation)?;
    let network = &transfer.route.destination.network;
    verify_evm_chain(client, network).await?;
    let json = rpc_post(
        client,
        network.rpc_url()?,
        &serde_json::json!({"jsonrpc":"2.0","id":1,"method":"eth_call","params":[{
        "from":caller,"to":call.to,"value":"0x0","data":format!("0x{}",hex::encode(call.data))
    },"latest"]}),
    )
    .await?;
    let result = json_rpc_result(&json, "CCTP destination simulation")?
        .as_str()
        .ok_or("Invalid CCTP simulation result")?;
    if result != format!("0x{}", hex::encode(evm_uint_word(1))) {
        return Err("CCTP destination simulation did not return success".into());
    }
    Ok(())
}

impl AttestationProvider for CircleAttestationProvider<'_> {
    async fn consumed_destination(
        &self,
        operation: &crate::operations::Operation,
    ) -> Result<(bool, Option<crate::operations::MintEvidence>, Option<u64>), String> {
        crate::providers::cctp_execution::LiveCctpProvider {
            client: self.client,
        }
        .replay_page(operation)
        .await
    }

    async fn receipt(
        &self,
        transfer: &CctpTransfer,
        hash: &str,
    ) -> Result<Option<(serde_json::Value, TxStatus)>, String> {
        if transfer.route.source.network.kind == "svm" {
            return crate::providers::cctp_solana::fetch_source_evidence(
                self.client,
                transfer,
                hash,
            )
            .await;
        }
        verify_evm_chain(self.client, &transfer.route.source.network).await?;
        let json = rpc_post(self.client,transfer.route.source.network.rpc_url()?,&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"eth_getTransactionReceipt","params":[hash]})).await?;
        let receipt = json_rpc_result(&json, "CCTP burn receipt")?;
        if receipt.is_null() {
            return Ok(None);
        }
        let observation =
            fetch_evm_tx_observation(self.client, &transfer.route.source.network, hash).await?;
        // decode_burn_receipt checks that the earlier logs and canonical observation
        // refer to exactly the same successful transaction and block.
        Ok(observation.map(|observation| (receipt.clone(), observation)))
    }
    async fn messages(
        &self,
        transfer: &CctpTransfer,
        hash: &str,
    ) -> Result<MessageResponse, String> {
        transfer.route.validate()?;
        fetch_messages_at(
            self.client,
            &format!(
                "{}/v2/messages/{}",
                transfer.route.attestation_host, transfer.route.source.domain
            ),
            hash,
        )
        .await
    }
    async fn verify(
        &self,
        transfer: &CctpTransfer,
        message: &[u8],
        attestation: &[u8],
    ) -> Result<AttestationCheck, String> {
        if transfer.route.destination.network.kind == "svm" {
            crate::providers::cctp_solana::verify_destination_attesters(
                self.client,
                transfer,
                message,
                attestation,
            )
            .await?;
            Ok(AttestationCheck::SolanaAttesters)
        } else {
            simulate_message(self.client, transfer, message, attestation).await?;
            Ok(AttestationCheck::DestinationSimulation)
        }
    }
}

#[cfg(test)]
#[path = "../tests/providers/cctp_attestation.rs"]
mod tests;
