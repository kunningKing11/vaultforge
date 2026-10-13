//! Durable native CCTP setup, burn, and mint attempts. No background authorization.
use std::future::Future;
use std::sync::Mutex;

use base64::Engine;
use chrono::Utc;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use solana_keypair::Keypair;
use solana_message::AddressLookupTableAccount;
use solana_signer::Signer;
use solana_transaction::versioned::VersionedTransaction;
use zeroize::Zeroizing;

use crate::cctp_execution::{
    CctpPreparedSource, begin_action, check_snapshot, prepared_view, save_operation,
};
use crate::dto::WalletSession;
use crate::operations::{
    Operation, OperationStep, TransactionAttempt, TransactionStatus, TransactionStep,
    advance_operation, attestation_available,
};
use crate::providers::cctp_native::LiveNative;
use crate::providers::cctp_native::NativePreflight;
use crate::state::{AppState, session_from_state};
use crate::tx::cctp_solana::{
    burn_instruction, compile, compute_budget, key, mint_instruction, setup_instructions, signed,
};
use crate::tx::tx_status::{Execution, Finality};

pub(crate) trait NativeProvider {
    fn observe(
        &self,
        operation: &Operation,
        destination: bool,
        hash: &str,
    ) -> impl Future<Output = Result<Option<crate::tx::tx_status::TxStatus>, String>> + Send;
    fn prepare(
        &self,
        operation: &Operation,
        event: Option<&str>,
        setup: bool,
    ) -> impl Future<Output = Result<NativePreflight, String>> + Send;
    fn expired(
        &self,
        operation: &Operation,
        attempt: &TransactionAttempt,
    ) -> impl Future<Output = Result<bool, String>> + Send;
    fn receipt(
        &self,
        operation: &Operation,
        hash: &str,
    ) -> impl Future<Output = Result<Option<crate::operations::MintEvidence>, String>> + Send;
    fn replay(
        &self,
        operation: &Operation,
    ) -> impl Future<
        Output = Result<
            (
                bool,
                Option<crate::operations::MintEvidence>,
                Option<String>,
            ),
            String,
        >,
    > + Send;
    fn simulate(
        &self,
        operation: &Operation,
        step: TransactionStep,
        raw: &str,
    ) -> impl Future<Output = Result<(), String>> + Send;
    fn preflight_signed(
        &self,
        operation: &Operation,
        attempt: &TransactionAttempt,
    ) -> impl Future<Output = Result<(), String>> + Send;
    fn broadcast(
        &self,
        operation: &Operation,
        step: TransactionStep,
        raw: &str,
    ) -> impl Future<Output = Result<String, String>> + Send;
}
impl NativeProvider for LiveNative<'_> {
    async fn observe(
        &self,
        operation: &Operation,
        destination: bool,
        hash: &str,
    ) -> Result<Option<crate::tx::tx_status::TxStatus>, String> {
        let transfer = operation.cctp_transfer()?;
        let network = if destination {
            &transfer.route.destination.network
        } else {
            &transfer.route.source.network
        };
        if network.kind == "svm" {
            crate::providers::solana::fetch_solana_tx_observation(self.client, network, hash).await
        } else {
            crate::providers::evm::fetch_evm_tx_observation(self.client, network, hash).await
        }
    }
    async fn prepare(
        &self,
        operation: &Operation,
        event: Option<&str>,
        setup: bool,
    ) -> Result<NativePreflight, String> {
        LiveNative::prepare(self, operation, event, setup).await
    }
    async fn expired(
        &self,
        operation: &Operation,
        attempt: &TransactionAttempt,
    ) -> Result<bool, String> {
        LiveNative::expired(self, operation, attempt).await
    }
    async fn receipt(
        &self,
        operation: &Operation,
        hash: &str,
    ) -> Result<Option<crate::operations::MintEvidence>, String> {
        self.mint_receipt(operation, hash).await
    }
    async fn replay(
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
        LiveNative::replay(self, operation).await
    }
    async fn simulate(
        &self,
        operation: &Operation,
        step: TransactionStep,
        raw: &str,
    ) -> Result<(), String> {
        LiveNative::simulate(self, operation, step, raw).await
    }
    async fn broadcast(
        &self,
        operation: &Operation,
        step: TransactionStep,
        raw: &str,
    ) -> Result<String, String> {
        LiveNative::broadcast(self, operation, step, raw).await
    }
    async fn preflight_signed(
        &self,
        operation: &Operation,
        attempt: &TransactionAttempt,
    ) -> Result<(), String> {
        let context = attempt
            .solana_context
            .as_ref()
            .ok_or("Native context missing")?;
        let transfer = operation.cctp_transfer()?;
        let destination = matches!(
            attempt.step,
            TransactionStep::Mint | TransactionStep::DestinationSetup
        );
        if attempt.step == TransactionStep::Mint {
            self.lookup(&transfer, context.lookup.as_ref().ok_or("Lookup missing")?)
                .await?;
            if self.fee_recipient(&transfer).await?.as_str()
                != context
                    .fee_recipient
                    .as_deref()
                    .ok_or("Fee recipient missing")?
            {
                return Err(
                    "Destination fee recipient changed; automatic replacement unavailable".into(),
                );
            }
        }
        let total = context
            .fee_lamports
            .checked_add(context.rent_lamports)
            .ok_or("Native budget overflow")?;
        if self
            .native_balance(&transfer, destination, &context.payer)
            .await?
            < total
        {
            return Err("Insufficient SOL for saved fee and rent".into());
        }
        self.simulate(
            operation,
            attempt.step,
            attempt.raw_tx.as_deref().ok_or("Signed bytes missing")?,
        )
        .await
    }
}

fn signing_keys(wallet: &crate::dto::Wallet, id: &str) -> Result<(Keypair, Keypair), String> {
    let seed = Zeroizing::new(crate::derivation::solana_secret_key_from_mnemonic(
        &wallet.mnemonic,
        &wallet.bip39_passphrase,
    )?);
    let payer = Keypair::new_from_array(*seed);
    let mut mac = Hmac::<Sha256>::new_from_slice(seed.as_ref())
        .map_err(|_| "Event signer initialization failed")?;
    mac.update(b"VaultForge CCTP MessageSent v1\0");
    mac.update(id.as_bytes());
    let event_seed = Zeroizing::new(<[u8; 32]>::from(mac.finalize().into_bytes()));
    Ok((payer, Keypair::new_from_array(*event_seed)))
}

pub(crate) fn validate_native_attempt(
    operation: &Operation,
    attempt: &TransactionAttempt,
) -> Result<(), String> {
    let transfer = operation.cctp_transfer()?;
    let context = attempt
        .solana_context
        .as_ref()
        .ok_or("Native execution context missing")?;
    let destination = matches!(
        attempt.step,
        TransactionStep::Mint | TransactionStep::DestinationSetup
    );
    let payer = if destination {
        transfer.destination_fee_payer()?
    } else {
        transfer.sender()
    };
    if context.payer != payer
        || attempt.nonce_or_sequence.as_deref() != Some(context.blockhash.as_str())
        || attempt.fee.as_deref() != Some(context.fee_lamports.to_string().as_str())
    {
        return Err("Native signer, blockhash or fee context mismatch".into());
    }
    let mut tables = Vec::new();
    let instructions = match attempt.step {
        TransactionStep::DestinationSetup => {
            let (instructions, lookup) = setup_instructions(
                &transfer,
                key(payer)?,
                key(context
                    .fee_recipient
                    .as_deref()
                    .ok_or("Fee recipient missing")?)?,
                context.setup_slot.ok_or("Setup slot missing")?,
                context.create_ata,
            )?;
            if context.lookup.as_ref() != Some(&lookup) {
                return Err("Saved lookup setup differs from intent".into());
            }
            instructions
        }
        TransactionStep::Burn => vec![
            compute_budget()?,
            burn_instruction(
                &transfer,
                key(context
                    .event_account
                    .as_deref()
                    .ok_or("Event account missing")?)?,
            )?,
        ],
        TransactionStep::Mint => {
            if !attestation_available(operation) {
                return Err("Verified bound pair and finalized source required".into());
            }
            let data = operation.cctp.as_ref().ok_or("Message missing")?;
            let lookup = context.lookup.as_ref().ok_or("Lookup table missing")?;
            if data.native_lookup.as_ref() != Some(lookup) {
                return Err("Native mint lookup differs from saved setup".into());
            }
            tables.push(AddressLookupTableAccount {
                key: key(&lookup.address)?,
                addresses: lookup
                    .addresses
                    .iter()
                    .map(|address| key(address))
                    .collect::<Result<Vec<_>, _>>()?,
            });
            vec![
                compute_budget()?,
                mint_instruction(
                    &transfer,
                    key(payer)?,
                    key(context
                        .fee_recipient
                        .as_deref()
                        .ok_or("Fee recipient missing")?)?,
                    &data.message,
                    data.attestation.as_deref().ok_or("Attestation missing")?,
                )?,
            ]
        }
        _ => return Err("Not a native CCTP step".into()),
    };
    let expected = compile(key(payer)?, &instructions, &context.blockhash, &tables)?;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(
            attempt
                .raw_tx
                .as_deref()
                .ok_or("Signed native bytes missing")?,
        )
        .map_err(|_| "Invalid native transaction encoding")?;
    if raw.len() > 1232 {
        return Err("Native transaction exceeds packet limit".into());
    }
    let transaction: VersionedTransaction =
        wincode::deserialize(&raw).map_err(|_| "Malformed native transaction")?;
    if transaction.message != expected
        || transaction
            .signatures
            .first()
            .map(ToString::to_string)
            .as_deref()
            != Some(&attempt.tx_hash)
        || transaction.signatures.len() != usize::from(expected.header().num_required_signatures)
    {
        return Err("Saved native bytes do not match operation intent".into());
    }
    let payload = expected.serialize();
    for (signature, public) in transaction
        .signatures
        .iter()
        .zip(expected.static_account_keys())
    {
        let verifying = ed25519_dalek::VerifyingKey::from_bytes(&public.to_bytes())
            .map_err(|_| "Invalid native signer")?;
        let signature = ed25519_dalek::Signature::from_slice(signature.as_ref())
            .map_err(|_| "Invalid native signature")?;
        verifying
            .verify_strict(&payload, &signature)
            .map_err(|_| "Native signature verification failed")?;
    }
    Ok(())
}

async fn reconcile<P: NativeProvider + Sync>(
    state: &Mutex<AppState>,
    provider: &P,
    generation: u64,
    before: &Operation,
) -> Result<(Operation, bool), String> {
    let mut after = before.clone();
    crate::operations::reconcile_operation_with(&mut after, |network, hash| {
        let destination = network == before.destination_asset.network;
        async move { provider.observe(before, destination, &hash).await }
    })
    .await?;
    let transfer = after.cctp_transfer()?;
    let mut consumed = false;
    if transfer.route.destination.network.kind == "svm"
        && attestation_available(&after)
        && !matches!(
            after.step,
            OperationStep::Completed | OperationStep::Cancelled
        )
    {
        let cached = after
            .cctp
            .as_ref()
            .and_then(|data| data.mint_evidence.clone());
        let own = after
            .attempts
            .iter()
            .rev()
            .find(|a| a.step == TransactionStep::Mint)
            .map(|a| a.tx_hash.clone());
        let proof = if cached.is_some() {
            cached
        } else if let Some(hash) = own {
            provider.receipt(&after, &hash).await?
        } else {
            None
        };
        let (used, proof, cursor) = if proof.is_some() {
            (true, proof, None)
        } else {
            provider.replay(&after).await?
        };
        consumed = used;
        if let Some(cursor) = cursor {
            after
                .cctp
                .as_mut()
                .ok_or("Message missing")?
                .replay_signature = Some(cursor);
        }
        if let Some(proof) = proof {
            if after.step == OperationStep::AwaitingAttestation {
                advance_operation(&mut after, OperationStep::Mint)?;
            }
            if let Some(attempt) = after
                .attempts
                .iter_mut()
                .find(|a| a.step == TransactionStep::Mint && a.tx_hash == proof.transaction_hash)
            {
                attempt.status = TransactionStatus::Confirmed;
                attempt.tx_status = Some(proof.observation.clone());
                attempt.error = None;
            } else {
                after.attempts.push(TransactionAttempt {
                    step: TransactionStep::Mint,
                    status: TransactionStatus::Confirmed,
                    tx_hash: proof.transaction_hash.clone(),
                    raw_tx: None,
                    signature: None,
                    fee: None,
                    nonce_or_sequence: None,
                    error: None,
                    updated_at: Utc::now().to_rfc3339(),
                    tx_status: Some(proof.observation.clone()),
                    evm_context: None,
                    solana_context: None,
                });
            }
            after.cctp.as_mut().ok_or("Message missing")?.mint_evidence = Some(proof);
            advance_operation(&mut after, OperationStep::Completed)?;
        } else if used {
            after.last_error = Some(
                "Message consumed; locating finalized native message and mint evidence".into(),
            );
        }
    }
    if after.step == OperationStep::Completed {
        for index in 0..after.attempts.len() {
            let attempt = &after.attempts[index];
            if attempt.step == TransactionStep::Mint
                && attempt.solana_context.is_some()
                && matches!(
                    attempt.status,
                    TransactionStatus::Signed
                        | TransactionStatus::Submitted
                        | TransactionStatus::SubmissionUnknown
                )
                && provider.expired(&after, attempt).await.unwrap_or(false)
            {
                after.attempts[index].status = TransactionStatus::Superseded;
                after.attempts[index].error=Some("Another verified mint completed; this saved blockhash expired without inclusion".into());
            }
        }
    }
    save_operation(
        &mut *state.lock().map_err(|_| "State lock failed")?,
        generation,
        before,
        &after,
    )?;
    Ok((after, consumed))
}

pub(crate) async fn sync_native<P: NativeProvider + Sync>(
    state: &Mutex<AppState>,
    provider: &P,
    id: &str,
) -> Result<WalletSession, String> {
    let (_action, generation, before) = begin_action(state, id)?;
    let (after, _) = reconcile(state, provider, generation, &before).await?;
    let state = state.lock().map_err(|_| "State lock failed")?;
    check_snapshot(&state, generation, &after)?;
    Ok(session_from_state(&state))
}

pub(crate) async fn prepare_native<P: NativeProvider + Sync>(
    state: &Mutex<AppState>,
    provider: &P,
    id: &str,
    setup: bool,
) -> Result<CctpPreparedSource, String> {
    let (_action, generation, before) = begin_action(state, id)?;
    let (mut operation, consumed) = reconcile(state, provider, generation, &before).await?;
    if consumed
        || matches!(
            operation.step,
            OperationStep::Completed | OperationStep::Cancelled
        )
    {
        return Err("Native step is not actionable; synchronize its evidence".into());
    }
    let mint =
        !setup && (operation.step == OperationStep::Mint || attestation_available(&operation));
    let step = if setup {
        TransactionStep::DestinationSetup
    } else if mint {
        TransactionStep::Mint
    } else {
        TransactionStep::Burn
    };
    if let Some(attempt) = operation
        .attempts
        .iter()
        .rev()
        .find(|a| a.step == step && a.status != TransactionStatus::Superseded)
    {
        let successful = attempt.tx_status.as_ref().is_some_and(|s| {
            s.execution == Execution::Succeeded && s.finality == Finality::Finalized
        });
        if successful {
            return Err("Native step is already finalized".into());
        }
        let failed_final = attempt.tx_status.as_ref().is_some_and(|status| {
            status.execution == Execution::Failed && status.finality == Finality::Finalized
        });
        if step == TransactionStep::Burn
            || (!failed_final && !provider.expired(&operation, attempt).await?)
        {
            if attempt.tx_status.is_some() {
                return Err("Wait for native finality before reviewing another transaction".into());
            }
            return prepared_view(&operation, attempt);
        }
        // Only setup or mint can acquire a new blockhash. A finalized height past
        // expiry plus a history lookup with no signature precedes this decision.
        let mut expired = operation.clone();
        let attempt = expired
            .attempts
            .iter_mut()
            .rev()
            .find(|a| a.step == step)
            .ok_or("Attempt missing")?;
        attempt.status = TransactionStatus::Failed;
        if !failed_final {
            attempt.error = Some("Blockhash expired without inclusion".into());
        }
        save_operation(
            &mut *state.lock().map_err(|_| "State lock failed")?,
            generation,
            &operation,
            &expired,
        )?;
        operation = expired;
    }
    let event_account = {
        let state = state.lock().map_err(|_| "State lock failed")?;
        check_snapshot(&state, generation, &operation)?;
        let (_, event) = signing_keys(state.wallet.as_ref().ok_or("Wallet closed")?, id)?;
        event.pubkey().to_string()
    };
    let preflight = provider
        .prepare(&operation, Some(&event_account), setup)
        .await?;
    if preflight.step != step {
        return Err("Native preflight changed step".into());
    }
    let (hash, raw) = {
        let state = state.lock().map_err(|_| "State lock failed")?;
        check_snapshot(&state, generation, &operation)?;
        let (payer, event) = signing_keys(state.wallet.as_ref().ok_or("Wallet closed")?, id)?;
        if payer.pubkey().to_string() != preflight.context.payer {
            return Err("Native fee payer does not match wallet signer".into());
        }
        let message = compile(
            payer.pubkey(),
            &preflight.instructions,
            &preflight.context.blockhash,
            &preflight.tables,
        )?;
        signed(
            message,
            &payer,
            (step == TransactionStep::Burn).then_some(&event),
        )?
    };
    provider.simulate(&operation, step, &raw).await?;
    let attempt = TransactionAttempt {
        step,
        status: TransactionStatus::Signed,
        tx_hash: hash,
        raw_tx: Some(raw),
        signature: None,
        fee: Some(preflight.context.fee_lamports.to_string()),
        nonce_or_sequence: Some(preflight.context.blockhash.clone()),
        error: None,
        updated_at: Utc::now().to_rfc3339(),
        tx_status: None,
        evm_context: None,
        solana_context: Some(preflight.context.clone()),
    };
    let mut after = operation.clone();
    if setup {
        after
            .cctp
            .get_or_insert_with(Default::default)
            .native_lookup = preflight.context.lookup.clone();
    }
    if mint && after.step == OperationStep::AwaitingAttestation {
        advance_operation(&mut after, OperationStep::Mint)?;
    }
    if step == TransactionStep::Burn && after.step == OperationStep::Approval {
        advance_operation(&mut after, OperationStep::Burn)?;
    }
    validate_native_attempt(&after, &attempt)?;
    after.attempts.push(attempt.clone());
    after.updated_at = Utc::now().to_rfc3339();
    save_operation(
        &mut *state.lock().map_err(|_| "State lock failed")?,
        generation,
        &operation,
        &after,
    )?;
    prepared_view(&after, &attempt)
}

pub(crate) async fn submit_native<P: NativeProvider + Sync>(
    state: &Mutex<AppState>,
    provider: &P,
    id: &str,
    expected_hash: &str,
) -> Result<WalletSession, String> {
    let (_action, generation, before) = begin_action(state, id)?;
    let (operation, consumed) = reconcile(state, provider, generation, &before).await?;
    if operation.step == OperationStep::Completed {
        return Ok(session_from_state(
            &*state.lock().map_err(|_| "State lock failed")?,
        ));
    }
    if consumed {
        return Err("Native message consumed; synchronize destination proof".into());
    }
    let attempt = operation.attempts.last().ok_or("Native attempt missing")?;
    if attempt.tx_hash != expected_hash
        || attempt.tx_status.is_some()
        || !matches!(
            attempt.status,
            TransactionStatus::Signed
                | TransactionStatus::Submitted
                | TransactionStatus::SubmissionUnknown
        )
        || attempt
            .error
            .as_deref()
            .is_some_and(|e| e.starts_with("Status check failed:"))
    {
        return Err(
            "Reviewed native attempt changed, included, failed or could not be reconciled".into(),
        );
    }
    validate_native_attempt(&operation, attempt)?;
    if provider.expired(&operation, attempt).await? {
        return Err("Native blockhash expired; reconcile and explicitly review again".into());
    }
    let raw = attempt.raw_tx.clone().ok_or("Native bytes missing")?;
    let step = attempt.step;
    provider.preflight_signed(&operation, attempt).await?;
    let mut submitting = operation.clone();
    let attempt = submitting.attempts.last_mut().ok_or("Attempt missing")?;
    attempt.status = TransactionStatus::SubmissionUnknown;
    attempt.error = Some("Native submission authorized; reconcile before retry".into());
    save_operation(
        &mut *state.lock().map_err(|_| "State lock failed")?,
        generation,
        &operation,
        &submitting,
    )?;
    let result = provider.broadcast(&submitting, step, &raw).await;
    let mut after = submitting.clone();
    let attempt = after.attempts.last_mut().ok_or("Attempt missing")?;
    let error = match result {
        Ok(hash) if hash == expected_hash => {
            attempt.status = TransactionStatus::Submitted;
            None
        }
        Ok(_) => Some("Unexpected native signature; acceptance uncertain".to_string()),
        Err(error) => Some(error),
    };
    attempt.error = error.clone();
    attempt.updated_at = Utc::now().to_rfc3339();
    after.last_error = error.clone();
    if step == TransactionStep::Burn && after.step == OperationStep::Burn {
        advance_operation(&mut after, OperationStep::AwaitingAttestation)?;
        after.last_error = error.clone();
    }
    save_operation(
        &mut *state.lock().map_err(|_| "State lock failed")?,
        generation,
        &submitting,
        &after,
    )?;
    if let Some(error) = error {
        return Err(format!(
            "Native outcome uncertain; saved signature {expected_hash}: {error}"
        ));
    }
    Ok(session_from_state(
        &*state.lock().map_err(|_| "State lock failed")?,
    ))
}

#[cfg(test)]
#[path = "tests/cctp_native.rs"]
mod tests;
