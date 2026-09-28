use std::collections::HashMap;

#[derive(Default)]
pub(crate) struct EvmNonceManager {
    accounts: HashMap<(u64, String), AccountNonces>,
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
    pub(crate) fn cancel_all_unbroadcast(&mut self) {
        let mut reservations: Vec<_> = self
            .accounts
            .iter()
            .flat_map(|((chain_id, address), account)| {
                account
                    .reservations
                    .iter()
                    .filter(|(_, status)| **status == ReservationStatus::AwaitingBroadcast)
                    .map(|(nonce, _)| (*chain_id, address.clone(), *nonce))
                    .collect::<Vec<_>>()
            })
            .collect();
        reservations.sort_unstable_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
                .then_with(|| right.2.cmp(&left.2))
        });

        for (chain_id, address, nonce) in reservations {
            self.cancel_unbroadcast(chain_id, &address, nonce);
        }
    }

    pub(crate) fn reserve(
        &mut self,
        chain_id: u64,
        address: &str,
        rpc_pending_nonce: u64,
    ) -> Result<u64, String> {
        let normalized_address = address.to_ascii_lowercase();
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
