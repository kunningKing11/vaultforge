//! Native RPC preparation, lookup validation, and exact transaction simulation.
use base64::Engine;
use solana_instruction::Instruction;
use solana_message::AddressLookupTableAccount;

use crate::cctp::CctpTransfer;
use crate::operations::{Operation, SolanaAttemptContext, SolanaLookup, TransactionStep};
use crate::providers::cctp_solana::{account_data, verify_mainnet};
use crate::providers::http::{json_rpc_result, rpc_post};
use crate::tx::cctp_solana::{
    compile, discriminator, key, mint_instruction, pda, setup_instructions,
};

pub(crate) struct NativePreflight {
    pub(crate) step: TransactionStep,
    pub(crate) instructions: Vec<Instruction>,
    pub(crate) context: SolanaAttemptContext,
    pub(crate) tables: Vec<AddressLookupTableAccount>,
}
pub(crate) struct LiveNative<'a> {
    pub(crate) client: &'a reqwest::Client,
}

impl LiveNative<'_> {
    pub(crate) async fn rpc(
        &self,
        transfer: &CctpTransfer,
        destination: bool,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let network = if destination {
            &transfer.route.destination.network
        } else {
            &transfer.route.source.network
        };
        let json = rpc_post(
            self.client,
            network.rpc_url()?,
            &serde_json::json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}),
        )
        .await?;
        json_rpc_result(&json, method).cloned()
    }
    async fn account(
        &self,
        transfer: &CctpTransfer,
        destination: bool,
        address: &str,
    ) -> Result<serde_json::Value, String> {
        self.rpc(
            transfer,
            destination,
            "getAccountInfo",
            serde_json::json!([address,{"encoding":"base64","commitment":"finalized"}]),
        )
        .await
    }
    async fn rent(
        &self,
        transfer: &CctpTransfer,
        destination: bool,
        size: u64,
    ) -> Result<u64, String> {
        self.rpc(
            transfer,
            destination,
            "getMinimumBalanceForRentExemption",
            serde_json::json!([size,{"commitment":"confirmed"}]),
        )
        .await?
        .as_u64()
        .ok_or("Invalid rent value".into())
    }
    pub(crate) async fn native_balance(
        &self,
        transfer: &CctpTransfer,
        destination: bool,
        payer: &str,
    ) -> Result<u64, String> {
        self.rpc(
            transfer,
            destination,
            "getBalance",
            serde_json::json!([payer,{"commitment":"confirmed"}]),
        )
        .await?["value"]
            .as_u64()
            .ok_or("Invalid native balance".into())
    }
    pub(crate) async fn fee_recipient(&self, transfer: &CctpTransfer) -> Result<String, String> {
        let address = pda(&key(crate::cctp::SOLANA_MESSENGER)?, &[b"token_messenger"]);
        let response = self.account(transfer, true, &address.to_string()).await?;
        let data = account_data(&response["value"], crate::cctp::SOLANA_MESSENGER)?;
        if data.len() != 177
            || data[..8] != discriminator("account:TokenMessenger")
            || data[104..108] != 1u32.to_le_bytes()
        {
            return Err("Invalid native TokenMessenger state".into());
        }
        Ok(solana_pubkey::Pubkey::new_from_array(
            data[109..141]
                .try_into()
                .map_err(|_| "Fee recipient missing")?,
        )
        .to_string())
    }
    pub(crate) async fn lookup(
        &self,
        transfer: &CctpTransfer,
        lookup: &SolanaLookup,
    ) -> Result<AddressLookupTableAccount, String> {
        let response = self.account(transfer, true, &lookup.address).await?;
        let bytes = account_data(
            &response["value"],
            "AddressLookupTab1e1111111111111111111111111",
        )?;
        let table =
            solana_address_lookup_table_interface::state::AddressLookupTable::deserialize(&bytes)
                .map_err(|_| "Invalid CCTP lookup table")?;
        let expected = lookup
            .addresses
            .iter()
            .map(|a| key(a))
            .collect::<Result<Vec<_>, _>>()?;
        if table.meta.deactivation_slot != u64::MAX
            || table.meta.last_extended_slot
                >= response["context"]["slot"]
                    .as_u64()
                    .ok_or("Lookup slot missing")?
            || table.addresses.as_ref() != expected
            || table.meta.authority.is_some()
        {
            return Err("Lookup table is inactive, changed, or belongs to another payer".into());
        }
        Ok(AddressLookupTableAccount {
            key: key(&lookup.address)?,
            addresses: expected,
        })
    }
    pub(crate) async fn destination_budget(&self, transfer: &CctpTransfer) -> Result<u64, String> {
        verify_mainnet(self.client, transfer, true).await?;
        // Provisional mint gas budget plus nonce-account rent. Setup has already
        // finalized before an EVM source is actionable; mint is estimated again.
        let rent = self
            .rent(transfer, true, 9)
            .await?
            .checked_add(10000)
            .ok_or("Destination budget overflow")?;
        if self
            .native_balance(transfer, true, transfer.destination_fee_payer()?)
            .await?
            < rent
        {
            return Err("Insufficient SOL for destination gas and account setup".into());
        }
        Ok(rent)
    }
    pub(crate) async fn prepare(
        &self,
        operation: &Operation,
        event: Option<&str>,
        setup: bool,
    ) -> Result<NativePreflight, String> {
        let transfer = operation.cctp_transfer()?;
        let destination = setup
            || operation.step == crate::operations::OperationStep::Mint
            || crate::operations::attestation_available(operation);
        verify_mainnet(self.client, &transfer, destination).await?;
        let payer = if destination {
            transfer.destination_fee_payer()?
        } else {
            transfer.sender()
        };
        let payer_key = key(payer)?;
        let block = self
            .rpc(
                &transfer,
                destination,
                "getLatestBlockhash",
                serde_json::json!([{"commitment":"confirmed"}]),
            )
            .await?;
        let blockhash = block["value"]["blockhash"]
            .as_str()
            .ok_or("Recent blockhash missing")?
            .to_string();
        let last_valid_height = block["value"]["lastValidBlockHeight"]
            .as_u64()
            .ok_or("Blockhash expiry missing")?;
        let mut context = SolanaAttemptContext {
            payer: payer.into(),
            blockhash,
            last_valid_height,
            fee_lamports: 0,
            rent_lamports: 0,
            event_account: None,
            fee_recipient: None,
            lookup: None,
            setup_slot: None,
            create_ata: false,
            provisional_destination_fee: None,
        };
        let mut tables = Vec::new();
        let (step, instructions) = if setup {
            let fee_recipient = self.fee_recipient(&transfer).await?;
            let ata = crate::tx::solana::solana_associated_token_address(
                transfer.recipient(),
                transfer.route.destination.usdc,
            )?;
            let account = self.account(&transfer, true, &ata).await?;
            let missing = account["value"].is_null();
            if !missing {
                validate_token_account(
                    &account["value"],
                    transfer.route.destination.usdc,
                    transfer.recipient(),
                    0,
                )?;
            }
            let slot = block["context"]["slot"]
                .as_u64()
                .ok_or("Setup slot missing")?;
            let (instructions, lookup) =
                setup_instructions(&transfer, payer_key, key(&fee_recipient)?, slot, missing)?;
            context.rent_lamports = self
                .rent(&transfer, true, 56 + 32 * lookup.addresses.len() as u64)
                .await?;
            if missing {
                context.rent_lamports = context
                    .rent_lamports
                    .checked_add(self.rent(&transfer, true, 165).await?)
                    .ok_or("Setup rent overflow")?;
            }
            context.lookup = Some(lookup);
            context.fee_recipient = Some(fee_recipient);
            context.setup_slot = Some(slot);
            context.create_ata = missing;
            (TransactionStep::DestinationSetup, instructions)
        } else if destination {
            if !crate::operations::attestation_available(operation) {
                return Err("Finalized source and verified attestation are required".into());
            }
            let data = operation.cctp.as_ref().ok_or("CCTP data missing")?;
            let attestation = data.attestation.as_deref().ok_or("Attestation missing")?;
            crate::providers::cctp_solana::verify_destination_attesters(
                self.client,
                &transfer,
                &data.message,
                attestation,
            )
            .await?;
            let fee_recipient = self.fee_recipient(&transfer).await?;
            let lookup = data
                .native_lookup
                .as_ref()
                .ok_or("Destination lookup setup is required")?;
            tables.push(self.lookup(&transfer, lookup).await?);
            let ata = crate::tx::solana::solana_associated_token_address(
                transfer.recipient(),
                transfer.route.destination.usdc,
            )?;
            validate_token_account(
                &self.account(&transfer, true, &ata).await?["value"],
                transfer.route.destination.usdc,
                transfer.recipient(),
                0,
            )?;
            context.rent_lamports = self.rent(&transfer, true, 9).await?;
            context.lookup = Some(lookup.clone());
            context.fee_recipient = Some(fee_recipient.clone());
            (
                TransactionStep::Mint,
                vec![
                    crate::tx::cctp_solana::compute_budget()?,
                    mint_instruction(
                        &transfer,
                        payer_key,
                        key(&fee_recipient)?,
                        &data.message,
                        attestation,
                    )?,
                ],
            )
        } else {
            let url = format!(
                "{}/v2/burn/USDC/fees/{}/{}",
                transfer.route.attestation_host,
                transfer.route.source.domain,
                transfer.route.destination.domain
            );
            crate::providers::cctp::fetch_standard_fee_at(self.client, &url).await?;
            let ata = crate::tx::solana::solana_associated_token_address(
                transfer.sender(),
                transfer.route.source.usdc,
            )?;
            validate_token_account(
                &self.account(&transfer, false, &ata).await?["value"],
                transfer.route.source.usdc,
                transfer.sender(),
                u64::try_from(transfer.amount()).map_err(|_| "Amount overflow")?,
            )?;
            let destination = &transfer.route.destination;
            crate::providers::evm::verify_evm_chain(self.client, &destination.network).await?;
            let fees =
                crate::providers::evm::fetch_evm_fee_estimate(self.client, &destination.network)
                    .await?;
            let budget = crate::providers::evm::evm_max_native_fee(
                crate::cctp::MINT_GAS_BUDGET,
                fees.max_fee_per_gas,
            )?;
            context.provisional_destination_fee = Some(budget.to_string());
            if crate::providers::evm::fetch_evm_native_balance(
                self.client,
                &destination.network,
                transfer.destination_fee_payer()?,
            )
            .await?
                < budget
            {
                return Err("Insufficient destination native gas funds".into());
            }
            let event = event.ok_or("MessageSent signer missing")?;
            context.event_account = Some(event.into());
            context.rent_lamports = self.rent(&transfer, false, 428).await?;
            (
                TransactionStep::Burn,
                vec![
                    crate::tx::cctp_solana::compute_budget()?,
                    crate::tx::cctp_solana::burn_instruction(&transfer, key(event)?)?,
                ],
            )
        };
        let message = compile(payer_key, &instructions, &context.blockhash, &tables)?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(message.serialize());
        context.fee_lamports = self
            .rpc(
                &transfer,
                destination,
                "getFeeForMessage",
                serde_json::json!([encoded,{"commitment":"confirmed"}]),
            )
            .await?["value"]
            .as_u64()
            .ok_or("Native fee unavailable or blockhash expired")?;
        let total = context
            .fee_lamports
            .checked_add(context.rent_lamports)
            .ok_or("Native budget overflow")?;
        if self.native_balance(&transfer, destination, payer).await? < total {
            return Err("Insufficient SOL for transaction fee and rent".into());
        }
        Ok(NativePreflight {
            step,
            instructions,
            context,
            tables,
        })
    }
    pub(crate) async fn simulate(
        &self,
        operation: &Operation,
        step: TransactionStep,
        raw: &str,
    ) -> Result<(), String> {
        let transfer = operation.cctp_transfer()?;
        let destination = matches!(
            step,
            TransactionStep::Mint | TransactionStep::DestinationSetup
        );
        verify_mainnet(self.client, &transfer, destination).await?;
        let json=self.rpc(&transfer,destination,"simulateTransaction",serde_json::json!([raw,{"encoding":"base64","sigVerify":true,"replaceRecentBlockhash":false,"commitment":"confirmed"}])).await?;
        if json["value"].get("err") != Some(&serde_json::Value::Null)
            || json["value"]["unitsConsumed"]
                .as_u64()
                .is_none_or(|v| v > 500000)
        {
            return Err(format!(
                "Native CCTP simulation failed: {}",
                json["value"]["err"]
            ));
        }
        Ok(())
    }
    pub(crate) async fn broadcast(
        &self,
        operation: &Operation,
        step: TransactionStep,
        raw: &str,
    ) -> Result<String, String> {
        let transfer = operation.cctp_transfer()?;
        let destination = matches!(
            step,
            TransactionStep::Mint | TransactionStep::DestinationSetup
        );
        let network = if destination {
            &transfer.route.destination.network
        } else {
            &transfer.route.source.network
        };
        let response=self.client.post(network.rpc_url()?).json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"sendTransaction","params":[raw,{"encoding":"base64","skipPreflight":false,"preflightCommitment":"confirmed","maxRetries":0}]})).send().await.map_err(|e|format!("Native submission response unavailable: {e}"))?;
        if !response.status().is_success() {
            return Err(format!("Native broadcast HTTP {}", response.status()));
        }
        let json = response
            .json()
            .await
            .map_err(|_| "Invalid native broadcast response")?;
        json_rpc_result(&json, "Native CCTP broadcast")?
            .as_str()
            .map(str::to_string)
            .ok_or("Native signature missing".into())
    }
}

pub(crate) fn validate_token_account(
    value: &serde_json::Value,
    mint: &str,
    owner: &str,
    amount: u64,
) -> Result<(), String> {
    let bytes = account_data(value, &spl_token_interface::ID.to_string())?;
    if bytes.len() != 165
        || bytes[..32] != key(mint)?.to_bytes()
        || bytes[32..64] != key(owner)?.to_bytes()
        || bytes[108] != 1
        || u64::from_le_bytes(
            bytes[64..72]
                .try_into()
                .map_err(|_| "Token amount missing")?,
        ) < amount
    {
        return Err("Invalid, frozen, or underfunded native USDC token account".into());
    }
    Ok(())
}

impl LiveNative<'_> {
    pub(crate) async fn expired(
        &self,
        operation: &Operation,
        attempt: &crate::operations::TransactionAttempt,
    ) -> Result<bool, String> {
        let transfer = operation.cctp_transfer()?;
        let destination = matches!(
            attempt.step,
            TransactionStep::Mint | TransactionStep::DestinationSetup
        );
        let context = attempt
            .solana_context
            .as_ref()
            .ok_or("Native context missing")?;
        let height = self
            .rpc(
                &transfer,
                destination,
                "getBlockHeight",
                serde_json::json!([{"commitment":"finalized"}]),
            )
            .await?
            .as_u64()
            .ok_or("Finalized height missing")?;
        let status = self
            .rpc(
                &transfer,
                destination,
                "getSignatureStatuses",
                serde_json::json!([[attempt.tx_hash],{"searchTransactionHistory":true}]),
            )
            .await?;
        Ok(height > context.last_valid_height
            && status["value"]
                .as_array()
                .is_some_and(|values| values.len() == 1 && values[0].is_null()))
    }
    async fn finalized_transaction(
        &self,
        operation: &Operation,
        hash: &str,
    ) -> Result<Option<(serde_json::Value, crate::tx::tx_status::TxStatus)>, String> {
        let transfer = operation.cctp_transfer()?;
        verify_mainnet(self.client, &transfer, true).await?;
        let Some(status) = crate::providers::solana::fetch_solana_tx_observation(
            self.client,
            &transfer.route.destination.network,
            hash,
        )
        .await?
        else {
            return Ok(None);
        };
        if status.execution != crate::tx::tx_status::Execution::Succeeded
            || status.finality != crate::tx::tx_status::Finality::Finalized
        {
            return Ok(None);
        }
        let transaction=self.rpc(&transfer,true,"getTransaction",serde_json::json!([hash,{"encoding":"json","commitment":"finalized","maxSupportedTransactionVersion":0}])).await?;
        if transaction.is_null() {
            return Ok(None);
        }
        Ok(Some((transaction, status)))
    }
    pub(crate) async fn mint_receipt(
        &self,
        operation: &Operation,
        hash: &str,
    ) -> Result<Option<crate::operations::MintEvidence>, String> {
        let Some((transaction, status)) = self.finalized_transaction(operation, hash).await? else {
            return Ok(None);
        };
        decode_mint_transaction(operation, hash, &transaction, &status).map(Some)
    }
    pub(crate) async fn replay(
        &self,
        operation: &Operation,
    ) -> Result<
        (
            bool,
            Option<crate::operations::MintEvidence>,
            Option<String>,
        ),
        String,
    > {
        let transfer = operation.cctp_transfer()?;
        verify_mainnet(self.client, &transfer, true).await?;
        let data = operation.cctp.as_ref().ok_or("Message missing")?;
        let nonce = crate::tx::cctp_solana::used_nonce(&data.message)?.to_string();
        let account = self.account(&transfer, true, &nonce).await?;
        if account["value"].is_null() {
            return Ok((false, None, None));
        }
        let bytes = account_data(&account["value"], crate::cctp::SOLANA_TRANSMITTER)?;
        if bytes.len() != 9 || bytes[..8] != discriminator("account:UsedNonce") || bytes[8] != 1 {
            return Err("Invalid destination nonce account".into());
        }
        let mut config = serde_json::json!({"commitment":"finalized","limit":20});
        if let Some(before) = &data.replay_signature {
            config["before"] = before.clone().into();
        }
        let result = self
            .rpc(
                &transfer,
                true,
                "getSignaturesForAddress",
                serde_json::json!([nonce, config]),
            )
            .await?;
        let signatures = result.as_array().ok_or("Invalid replay signatures")?;
        for entry in signatures {
            if entry.get("err") == Some(&serde_json::Value::Null) {
                let hash = entry["signature"]
                    .as_str()
                    .ok_or("Replay signature missing")?;
                let Some((transaction, status)) =
                    self.finalized_transaction(operation, hash).await?
                else {
                    // A finalized signature page whose receipt is temporarily
                    // unavailable must be retried instead of skipped.
                    return Ok((true, None, data.replay_signature.clone()));
                };
                if has_nonce_event(&transaction, &data.message[12..44])? {
                    let proof = decode_mint_transaction(operation, hash, &transaction, &status)?;
                    return Ok((true, Some(proof), None));
                }
                // A complete unrelated transaction may mention the nonce PDA;
                // it is not mint evidence and must not block later pages.
            }
        }
        let cursor = signatures
            .last()
            .and_then(|entry| entry["signature"].as_str())
            .map(str::to_string)
            .or_else(|| data.replay_signature.clone());
        Ok((true, None, cursor))
    }
}

fn has_nonce_event(transaction: &serde_json::Value, nonce: &[u8]) -> Result<bool, String> {
    let keys = crate::providers::cctp_solana::keys(transaction)?;
    let Some(groups) = transaction["meta"]["innerInstructions"].as_array() else {
        return Ok(false);
    };
    for group in groups {
        for instruction in group["instructions"].as_array().ok_or("Invalid CPI list")? {
            let program = instruction["programIdIndex"]
                .as_u64()
                .and_then(|index| keys.get(index as usize))
                .ok_or("CPI program missing")?;
            if program == crate::cctp::SOLANA_TRANSMITTER {
                let bytes = crate::providers::cctp_solana::instruction_data(instruction)?;
                if bytes.get(8..16) == Some(discriminator("event:MessageReceived").as_slice())
                    && bytes.get(52..84) == Some(nonce)
                {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

pub(crate) fn decode_mint_transaction(
    operation: &Operation,
    hash: &str,
    transaction: &serde_json::Value,
    status: &crate::tx::tx_status::TxStatus,
) -> Result<crate::operations::MintEvidence, String> {
    use crate::providers::cctp_solana::{instruction_data, instruction_key, keys};
    use sha3::{Digest, Keccak256};
    let transfer = operation.cctp_transfer()?;
    let data = operation.cctp.as_ref().ok_or("Message missing")?;
    let message = &data.message;
    if message.len() != 376
        || status.execution != crate::tx::tx_status::Execution::Succeeded
        || status.finality != crate::tx::tx_status::Finality::Finalized
        || status.slot.is_none()
        || status.slot != transaction["slot"].as_u64()
        || transaction["meta"].get("err") != Some(&serde_json::Value::Null)
        || transaction["transaction"]["signatures"][0] != hash
    {
        return Err("Finalized successful native mint transaction required".into());
    }
    let keys = keys(transaction)?;
    let mut received = 0;
    let mut minted = 0;
    let event_tag = [0x1d, 0x9a, 0xcb, 0x51, 0x2e, 0xa5, 0x45, 0xe4];
    for group in transaction["meta"]["innerInstructions"]
        .as_array()
        .ok_or("Native mint CPIs missing")?
    {
        for ix in group["instructions"]
            .as_array()
            .ok_or("Native CPI list missing")?
        {
            let program = ix["programIdIndex"]
                .as_u64()
                .and_then(|index| keys.get(index as usize))
                .ok_or("CPI program missing")?;
            if program != crate::cctp::SOLANA_TRANSMITTER
                && program != crate::cctp::SOLANA_MESSENGER
            {
                continue;
            }
            let bytes = instruction_data(ix)?;
            if !bytes.starts_with(&event_tag) {
                continue;
            }
            let program_key = key(program)?;
            let authority = pda(&program_key, &[b"__event_authority"]).to_string();
            if instruction_key(ix, 0, &keys)? != authority
                || ix["stackHeight"].as_u64().is_none_or(|height| height < 2)
            {
                return Err("Untrusted native CCTP event CPI".into());
            }
            if bytes.get(8..16) == Some(discriminator("event:MessageReceived").as_slice())
                && program == crate::cctp::SOLANA_TRANSMITTER
            {
                if bytes.len() != 352 {
                    return Err("Invalid native message event length".into());
                }
                let caller = &bytes[16..48];
                if message[108..140] != [0; 32] && caller != &message[108..140] {
                    return Err("Native destination caller mismatch".into());
                }
                let mut expected = Vec::new();
                expected.extend_from_slice(&transfer.route.source.domain.to_le_bytes());
                expected.extend_from_slice(&message[12..44]);
                expected.extend_from_slice(&message[44..76]);
                expected.extend_from_slice(
                    &u32::from_be_bytes(
                        message[144..148]
                            .try_into()
                            .map_err(|_| "Invalid finality")?,
                    )
                    .to_le_bytes(),
                );
                expected.extend_from_slice(&228u32.to_le_bytes());
                expected.extend_from_slice(&message[148..]);
                if bytes[48..] != expected {
                    return Err("Native received message mismatch".into());
                }
                received += 1;
            }
            if bytes.get(8..16) == Some(discriminator("event:MintAndWithdraw").as_slice())
                && program == crate::cctp::SOLANA_MESSENGER
            {
                let mut expected = transfer.mint_recipient_word()?.to_vec();
                expected.extend_from_slice(
                    &u64::try_from(transfer.amount())
                        .map_err(|_| "Amount overflow")?
                        .to_le_bytes(),
                );
                expected.extend_from_slice(&key(transfer.route.destination.usdc)?.to_bytes());
                expected.extend_from_slice(&0u64.to_le_bytes());
                if bytes[16..] != expected {
                    return Err("Native mint amount, recipient, token or fee mismatch".into());
                }
                minted += 1;
            }
        }
    }
    if received != 1 || minted != 1 {
        return Err("Unique native message and mint events required".into());
    }
    Ok(crate::operations::MintEvidence {
        transaction_hash: hash.into(),
        message_hash: hex::encode(Keccak256::digest(message)),
        observation: status.clone(),
    })
}

#[cfg(test)]
#[path = "../tests/providers/cctp_native.rs"]
mod tests;
