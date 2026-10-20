use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(crate) struct EvmNonceManager {
    accounts: HashMap<(u64, String), AccountNonces>,
    durable: HashSet<(u64, String, u64)>,
}

#[derive(Default)]
struct AccountNonces {
    next_nonce: Option<u64>,
    reservations: HashMap<u64, ReservationStatus>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReservationStatus {
    AwaitingBroadcast,
    BroadcastAttempted,
    Broadcast,
}

impl EvmNonceManager {
    /// Rebuild durable contract-operation protection, preserving ordinary-send
    /// queues. Ordinary EVM sends are restored per nonce by `restore_send_operations`.
    pub(crate) fn restore_operations(
        &mut self,
        operations: &[crate::operations::Operation],
    ) -> Result<(), String> {
        let mut durable = HashSet::new();
        for operation in operations {
            if !matches!(
                operation.kind,
                crate::operations::OperationKind::Cctp | crate::operations::OperationKind::Swap
            ) {
                continue;
            }
            if operation.kind == crate::operations::OperationKind::Swap {
                crate::uniswap::validate_operation(operation)?;
            }
            for attempt in &operation.attempts {
                if attempt.raw_tx.is_none()
                    || matches!(
                        attempt.status,
                        crate::operations::TransactionStatus::Built
                            | crate::operations::TransactionStatus::Superseded
                    )
                {
                    continue;
                }
                let unresolved = !attempt.tx_status.as_ref().is_some_and(|status| {
                    status.finality == crate::tx::tx_status::Finality::Finalized
                });
                if !unresolved {
                    continue;
                }
                if let Some(context) = &attempt.evm_context {
                    let (expected_chain, expected_sender) =
                        if operation.kind == crate::operations::OperationKind::Swap {
                            let (intent, data) = crate::uniswap::validate_operation(operation)?;
                            (data.chain_id, intent.sender)
                        } else {
                            let transfer = operation.cctp_transfer()?;
                            if matches!(
                                attempt.step,
                                crate::operations::TransactionStep::Mint
                                    | crate::operations::TransactionStep::DestinationSetup
                            ) {
                                (
                                    transfer.route.destination.network.chain_id()?,
                                    transfer.destination_fee_payer()?.to_string(),
                                )
                            } else {
                                (
                                    transfer.route.source.network.chain_id()?,
                                    operation.sender.clone(),
                                )
                            }
                        };
                    if context.chain_id != expected_chain
                        || !context.from.eq_ignore_ascii_case(&expected_sender)
                    {
                        return Err("Persisted EVM nonce context does not match operation".into());
                    }
                    durable.insert((
                        context.chain_id,
                        context.from.to_ascii_lowercase(),
                        context.nonce,
                    ));
                } else if let Some(nonce) = &attempt.nonce_or_sequence {
                    // Legacy signed attempts still protect their known EVM account.
                    let network = if matches!(
                        attempt.step,
                        crate::operations::TransactionStep::Mint
                            | crate::operations::TransactionStep::DestinationSetup
                    ) {
                        &operation.destination_asset.network
                    } else {
                        &operation.source_asset.network
                    };
                    if let Some(config) = crate::registry::network_by_id(network)
                        .filter(|network| network.kind == "evm")
                    {
                        let nonce = nonce.parse().map_err(|_| "Invalid persisted EVM nonce")?;
                        durable.insert((
                            config.chain_id()?,
                            operation.sender.to_ascii_lowercase(),
                            nonce,
                        ));
                    }
                }
            }
        }
        self.durable = durable;
        Ok(())
    }

    /// Restores per-nonce protection for unresolved ordinary EVM sends without
    /// blocking the whole account the way unresolved CCTP operations do.
    pub(crate) fn restore_send_operations(&mut self, operations: &[crate::operations::Operation]) {
        for operation in operations {
            if operation.kind != crate::operations::OperationKind::Send {
                continue;
            }
            let Some(config) = crate::registry::network_by_id(&operation.source_asset.network)
                .filter(|network| network.kind == "evm")
            else {
                continue;
            };
            let Ok(chain_id) = config.chain_id() else {
                continue;
            };
            for attempt in &operation.attempts {
                if attempt.step != crate::operations::TransactionStep::Send {
                    continue;
                }
                let resolved = attempt.tx_status.as_ref().is_some_and(|status| {
                    status.finality == crate::tx::tx_status::Finality::Finalized
                }) || attempt.status == crate::operations::TransactionStatus::Failed;
                if resolved {
                    continue;
                }
                let Some(nonce) = attempt
                    .nonce_or_sequence
                    .as_ref()
                    .and_then(|nonce| nonce.parse::<u64>().ok())
                else {
                    continue;
                };
                let account = self
                    .accounts
                    .entry((chain_id, operation.sender.to_ascii_lowercase()))
                    .or_default();
                let status = if attempt.status == crate::operations::TransactionStatus::Signed {
                    ReservationStatus::AwaitingBroadcast
                } else {
                    ReservationStatus::BroadcastAttempted
                };
                account.reservations.entry(nonce).or_insert(status);
                let next = nonce.saturating_add(1);
                if account.next_nonce.is_none_or(|current| current < next) {
                    account.next_nonce = Some(next);
                }
            }
        }
    }

    pub(crate) fn reserve(
        &mut self,
        chain_id: u64,
        address: &str,
        rpc_pending_nonce: u64,
    ) -> Result<u64, String> {
        let normalized_address = address.to_ascii_lowercase();
        if self
            .durable
            .iter()
            .any(|(chain, owner, _)| *chain == chain_id && owner == &normalized_address)
        {
            return Err("An unresolved operation protects this EVM account nonce".into());
        }
        let account = self
            .accounts
            .entry((chain_id, normalized_address))
            .or_default();

        if account
            .reservations
            .values()
            .any(|status| *status == ReservationStatus::AwaitingBroadcast)
        {
            return Err("An EVM transaction is already awaiting broadcast".to_string());
        }

        let selected_nonce = account
            .next_nonce
            .unwrap_or(rpc_pending_nonce)
            .max(rpc_pending_nonce);
        let next_nonce = selected_nonce
            .checked_add(1)
            .ok_or_else(|| "EVM nonce is too large".to_string())?;

        account
            .reservations
            .insert(selected_nonce, ReservationStatus::AwaitingBroadcast);
        account.next_nonce = Some(next_nonce);

        Ok(selected_nonce)
    }

    pub(crate) fn cancel_unbroadcast(&mut self, chain_id: u64, address: &str, nonce: u64) -> bool {
        let key = (chain_id, address.to_ascii_lowercase());
        let Some(account) = self.accounts.get_mut(&key) else {
            return false;
        };
        if account.reservations.get(&nonce) != Some(&ReservationStatus::AwaitingBroadcast) {
            return false;
        }

        account.reservations.remove(&nonce);
        if nonce.checked_add(1) == account.next_nonce {
            account.next_nonce = Some(nonce);
        }
        true
    }

    pub(crate) fn begin_broadcast(
        &mut self,
        chain_id: u64,
        address: &str,
        nonce: u64,
    ) -> Result<(), String> {
        let key = (chain_id, address.to_ascii_lowercase());
        let account = self
            .accounts
            .get_mut(&key)
            .ok_or_else(|| "EVM nonce reservation was not found".to_string())?;
        let status = account
            .reservations
            .get_mut(&nonce)
            .ok_or_else(|| "EVM nonce reservation was not found".to_string())?;
        match status {
            ReservationStatus::AwaitingBroadcast | ReservationStatus::BroadcastAttempted => {
                *status = ReservationStatus::BroadcastAttempted;
                Ok(())
            }
            ReservationStatus::Broadcast => {
                Err("EVM transaction was already broadcast".to_string())
            }
        }
    }

    pub(crate) fn mark_broadcast(
        &mut self,
        chain_id: u64,
        address: &str,
        nonce: u64,
    ) -> Result<(), String> {
        let key = (chain_id, address.to_ascii_lowercase());
        let account = self
            .accounts
            .get_mut(&key)
            .ok_or_else(|| "EVM nonce reservation was not found".to_string())?;
        let status = account
            .reservations
            .get_mut(&nonce)
            .ok_or_else(|| "EVM nonce reservation was not found".to_string())?;
        if *status != ReservationStatus::BroadcastAttempted {
            return Err("EVM transaction was not submitted".to_string());
        }
        *status = ReservationStatus::Broadcast;
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/tx/evm_nonces.rs"]
mod tests;
