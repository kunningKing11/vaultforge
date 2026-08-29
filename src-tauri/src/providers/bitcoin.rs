use crate::derivation::{
    BITCOIN_BIP84_GAP_LIMIT, BitcoinAccount, BitcoinBranch, BitcoinDerivedAddress, BitcoinKeyOrigin,
};
use crate::providers::http::{http_get_json, http_get_json_with_client, http_post_text};
use crate::registry::network_by_id;
use crate::tx::bitcoin::{BitcoinSignedTransfer, bitcoin_signed_transfer};
use std::collections::{HashSet, VecDeque};
use tokio::task::JoinSet;

const BITCOIN_SCAN_BATCH_SIZE: u32 = 2;
const BITCOIN_SCAN_MAX_INDEX: u32 = 1_000;
const BITCOIN_UTXO_CONCURRENCY: usize = 4;

#[derive(Clone, Debug)]
pub(crate) struct BitcoinAddressSnapshot {
    pub(crate) derived: BitcoinDerivedAddress,
    pub(crate) balance: u128,
    pub(crate) used: bool,
}

#[derive(Clone)]
pub(crate) struct BitcoinAccountSnapshot {
    account: BitcoinAccount,
    addresses: Vec<BitcoinAddressSnapshot>,
}

impl BitcoinAccountSnapshot {
    pub(crate) fn new(account: BitcoinAccount) -> Self {
        Self {
            account,
            addresses: vec![],
        }
    }

    pub(crate) fn account(&self) -> &BitcoinAccount {
        &self.account
    }

    pub(crate) fn total_balance(&self) -> Result<u128, String> {
        self.addresses.iter().try_fold(0u128, |total, item| {
            total
                .checked_add(item.balance)
                .ok_or_else(|| "Bitcoin account balance overflowed".to_string())
        })
    }

    pub(crate) fn next_receive_address(&self) -> Option<&BitcoinDerivedAddress> {
        let next_index = match self
            .addresses
            .iter()
            .filter(|item| item.derived.origin.branch == BitcoinBranch::External && item.used)
            .map(|item| item.derived.origin.index)
            .max()
        {
            Some(index) => index.checked_add(1)?,
            None => 0,
        };
        self.addresses
            .iter()
            .find(|item| {
                item.derived.origin == BitcoinKeyOrigin::external(next_index) && !item.used
            })
            .map(|item| &item.derived)
    }

    fn used_addresses(&self) -> impl Iterator<Item = &BitcoinDerivedAddress> {
        self.addresses
            .iter()
            .filter(|item| item.used)
            .map(|item| &item.derived)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct BitcoinUtxo {
    pub(crate) txid: String,
    pub(crate) vout: u32,
    pub(crate) value: u64,
    pub(crate) confirmed: bool,
    pub(crate) owner: BitcoinDerivedAddress,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BitcoinAddressStats {
    pub(crate) balance: u128,
    pub(crate) used: bool,
}

struct BitcoinBranchScan {
    branch: BitcoinBranch,
    next_index: u32,
    consecutive_unused: u32,
    complete: bool,
    addresses: Vec<BitcoinAddressSnapshot>,
}

impl BitcoinBranchScan {
    fn new(branch: BitcoinBranch) -> Self {
        Self {
            branch,
            next_index: 0,
            consecutive_unused: 0,
            complete: false,
            addresses: vec![],
        }
    }

    fn record(
        &mut self,
        derived: BitcoinDerivedAddress,
        stats: BitcoinAddressStats,
    ) -> Result<(), String> {
        if derived.origin.branch != self.branch {
            return Err("Bitcoin scan result does not match its branch".to_string());
        }
        if stats.used {
            self.consecutive_unused = 0;
        } else {
            self.consecutive_unused = self.consecutive_unused.saturating_add(1);
        }
        self.addresses.push(BitcoinAddressSnapshot {
            derived,
            balance: stats.balance,
            used: stats.used,
        });
        if self.consecutive_unused >= BITCOIN_BIP84_GAP_LIMIT {
            self.complete = true;
        }
        Ok(())
    }
}

fn bitcoin_api_url() -> Result<&'static str, String> {
    network_by_id("bitcoin")
        .ok_or_else(|| "Bitcoin is missing from the network registry".to_string())?
        .api_url()
}

pub(crate) fn parse_bitcoin_address_stats(
    json: &serde_json::Value,
) -> Result<BitcoinAddressStats, String> {
    let chain = json
        .get("chain_stats")
        .ok_or_else(|| "Bitcoin address response is missing chain_stats".to_string())?;
    let mempool = json
        .get("mempool_stats")
        .ok_or_else(|| "Bitcoin address response is missing mempool_stats".to_string())?;
    let funded = required_u128(chain, "funded_txo_sum")?;
    let spent = required_u128(chain, "spent_txo_sum")?;
    let mempool_funded = required_u128(mempool, "funded_txo_sum")?;
    let mempool_spent = required_u128(mempool, "spent_txo_sum")?;
    let total_funded = funded
        .checked_add(mempool_funded)
        .ok_or_else(|| "Bitcoin funded balance overflowed".to_string())?;
    let total_spent = spent
        .checked_add(mempool_spent)
        .ok_or_else(|| "Bitcoin spent balance overflowed".to_string())?;
    let balance = total_funded
        .checked_sub(total_spent)
        .ok_or_else(|| "Bitcoin address response spends more than it funds".to_string())?;
    let chain_tx_count = required_u128(chain, "tx_count")?;
    let mempool_tx_count = required_u128(mempool, "tx_count")?;

    Ok(BitcoinAddressStats {
        balance,
        used: chain_tx_count > 0 || mempool_tx_count > 0,
    })
}

fn required_u128(json: &serde_json::Value, field: &str) -> Result<u128, String> {
    json.get(field)
        .and_then(serde_json::Value::as_u64)
        .map(u128::from)
        .ok_or_else(|| format!("Bitcoin address response is missing {field}"))
}

pub(crate) async fn scan_bitcoin_account(
    client: &reqwest::Client,
    account: &BitcoinAccount,
) -> Result<BitcoinAccountSnapshot, String> {
    let api_url = bitcoin_api_url()?.to_string();
    let mut scans = [
        BitcoinBranchScan::new(BitcoinBranch::External),
        BitcoinBranchScan::new(BitcoinBranch::Change),
    ];

    while scans.iter().any(|scan| !scan.complete) {
        let mut pending = Vec::new();
        for scan in scans.iter_mut().filter(|scan| !scan.complete) {
            if scan.next_index >= BITCOIN_SCAN_MAX_INDEX {
                return Err(format!(
                    "Bitcoin {:?} address scan exceeded its safety limit",
                    scan.branch
                ));
            }
            let end = scan
                .next_index
                .saturating_add(BITCOIN_SCAN_BATCH_SIZE)
                .min(BITCOIN_SCAN_MAX_INDEX);
            for index in scan.next_index..end {
                let origin = match scan.branch {
                    BitcoinBranch::External => BitcoinKeyOrigin::external(index),
                    BitcoinBranch::Change => BitcoinKeyOrigin::change(index),
                };
                pending.push(account.derive_address(origin)?);
            }
            scan.next_index = end;
        }

        let mut results = fetch_bitcoin_address_batch(client, &api_url, pending).await?;
        results.sort_by_key(|(derived, _)| derived.origin);
        for (derived, stats) in results {
            let scan = scans
                .iter_mut()
                .find(|scan| scan.branch == derived.origin.branch)
                .ok_or_else(|| "Bitcoin scan returned an unknown branch".to_string())?;
            if scan.complete {
                continue;
            }
            scan.record(derived, stats)?;
        }
    }

    let mut addresses = scans
        .into_iter()
        .flat_map(|scan| scan.addresses)
        .collect::<Vec<_>>();
    addresses.sort_by_key(|item| item.derived.origin);
    Ok(BitcoinAccountSnapshot {
        account: account.clone(),
        addresses,
    })
}

async fn fetch_bitcoin_address_batch(
    client: &reqwest::Client,
    api_url: &str,
    addresses: Vec<BitcoinDerivedAddress>,
) -> Result<Vec<(BitcoinDerivedAddress, BitcoinAddressStats)>, String> {
    let mut tasks = JoinSet::new();
    for derived in addresses {
        let client = client.clone();
        let url = format!("{api_url}/address/{}", derived.address);
        tasks.spawn(async move {
            let json = http_get_json_with_client(&client, &url).await?;
            let stats = parse_bitcoin_address_stats(&json)?;
            Ok::<_, String>((derived, stats))
        });
    }

    let mut results = Vec::new();
    while let Some(result) = tasks.join_next().await {
        results
            .push(result.map_err(|error| format!("Bitcoin address scan task failed: {error}"))??);
    }
    Ok(results)
}

pub(crate) fn parse_bitcoin_utxos(
    json: &serde_json::Value,
    owner: &BitcoinDerivedAddress,
) -> Result<Vec<BitcoinUtxo>, String> {
    let items = json
        .as_array()
        .ok_or_else(|| "Bitcoin UTXO response is not an array".to_string())?;
    let mut utxos = vec![];
    for item in items {
        let txid = item["txid"]
            .as_str()
            .ok_or_else(|| "Bitcoin UTXO missing txid".to_string())?
            .to_ascii_lowercase();
        if txid.len() != 64 || !txid.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err("Bitcoin UTXO txid is invalid".to_string());
        }
        let vout = item["vout"]
            .as_u64()
            .ok_or_else(|| "Bitcoin UTXO missing vout".to_string())?;
        let value = item["value"]
            .as_u64()
            .ok_or_else(|| "Bitcoin UTXO missing value".to_string())?;
        let confirmed = item["status"]["confirmed"]
            .as_bool()
            .ok_or_else(|| "Bitcoin UTXO missing confirmation status".to_string())?;
        utxos.push(BitcoinUtxo {
            txid,
            vout: u32::try_from(vout).map_err(|_| "Bitcoin UTXO vout is too large".to_string())?,
            value,
            confirmed,
            owner: owner.clone(),
        });
    }
    sort_bitcoin_utxos(&mut utxos);
    Ok(utxos)
}

async fn fetch_bitcoin_account_utxos(
    client: &reqwest::Client,
    account: &BitcoinAccountSnapshot,
) -> Result<Vec<BitcoinUtxo>, String> {
    let api_url = bitcoin_api_url()?.to_string();
    let mut pending = account.used_addresses().cloned().collect::<VecDeque<_>>();
    let mut tasks = JoinSet::new();
    let mut utxos = Vec::new();

    while tasks.len() < BITCOIN_UTXO_CONCURRENCY {
        let Some(owner) = pending.pop_front() else {
            break;
        };
        spawn_bitcoin_utxo_fetch(&mut tasks, client, &api_url, owner);
    }

    while let Some(result) = tasks.join_next().await {
        let mut fetched =
            result.map_err(|error| format!("Bitcoin UTXO task failed: {error}"))??;
        utxos.append(&mut fetched);
        if let Some(owner) = pending.pop_front() {
            spawn_bitcoin_utxo_fetch(&mut tasks, &client, &api_url, owner);
        }
    }

    let mut outpoints = HashSet::new();
    for utxo in &utxos {
        if !outpoints.insert((utxo.txid.clone(), utxo.vout)) {
            return Err("Bitcoin provider returned a duplicate UTXO".to_string());
        }
    }
    sort_bitcoin_utxos(&mut utxos);
    Ok(utxos)
}

fn sort_bitcoin_utxos(utxos: &mut [BitcoinUtxo]) {
    utxos.sort_by(|a, b| {
        b.confirmed
            .cmp(&a.confirmed)
            .then(b.value.cmp(&a.value))
            .then(a.txid.cmp(&b.txid))
            .then(a.vout.cmp(&b.vout))
    });
}

fn spawn_bitcoin_utxo_fetch(
    tasks: &mut JoinSet<Result<Vec<BitcoinUtxo>, String>>,
    client: &reqwest::Client,
    api_url: &str,
    owner: BitcoinDerivedAddress,
) {
    let client = client.clone();
    let url = format!("{api_url}/address/{}/utxo", owner.address);
    tasks.spawn(async move {
        let json = http_get_json_with_client(&client, &url).await?;
        parse_bitcoin_utxos(&json, &owner)
    });
}

pub(crate) async fn fetch_bitcoin_fee_rate(
    client: &reqwest::Client,
) -> Result<u64, String> {
    let json = http_get_json(client, &format!("{}/fee-estimates", bitcoin_api_url()?)).await?;
    parse_bitcoin_fee_rate(&json)
}

pub(crate) fn parse_bitcoin_fee_rate(json: &serde_json::Value) -> Result<u64, String> {
    for target in ["3", "6", "12", "1"] {
        if let Some(rate) = json[target].as_f64()
            && rate.is_finite()
            && rate > 0.0
        {
            return Ok(rate.ceil().max(1.0) as u64);
        }
    }
    Err("Bitcoin fee estimate response missing usable fee rate".to_string())
}

pub(crate) async fn broadcast_bitcoin_transaction(
    client: &reqwest::Client,
    raw_tx_hex: &str,
) -> Result<String, String> {
    http_post_text(client, &format!("{}/tx", bitcoin_api_url()?), raw_tx_hex)
        .await
        .map(|txid| txid.trim().to_string())
}

pub(crate) async fn fetch_bitcoin_tx_status(
    client: &reqwest::Client,
    txid: &str,
) -> Result<Option<String>, String> {
    let url = format!("{}/tx/{txid}/status", bitcoin_api_url()?);
    let json = http_get_json(client, &url).await?;
    if json["confirmed"].as_bool().unwrap_or(false) {
        Ok(Some("confirmed".to_string()))
    } else {
        Ok(None)
    }
}

pub(crate) async fn sign_bitcoin_transfer(
    client: &reqwest::Client,
    mnemonic: &str,
    from: &str,
    to: &str,
    amount_sats: u64,
    account: &BitcoinAccountSnapshot,
) -> Result<BitcoinSignedTransfer, String> {
    if account.account().primary_address()?.address != from {
        return Err("Bitcoin account does not match the wallet receive address".to_string());
    }
    // Address discovery can be stale while the displayed next receive address has since
    // received funds. Refresh it before querying the used addresses for spendable UTXOs.
    let account = scan_bitcoin_account(client, account.account()).await?;
    let utxos = fetch_bitcoin_account_utxos(client, &account).await?;
    let fee_rate = fetch_bitcoin_fee_rate(client).await?;
    let change_destination = account.account().primary_address()?;
    bitcoin_signed_transfer(
        mnemonic,
        from,
        to,
        amount_sats,
        &utxos,
        fee_rate,
        &change_destination,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        BitcoinAccountSnapshot, BitcoinAddressSnapshot, BitcoinAddressStats, BitcoinBranchScan,
    };
    use crate::derivation::{
        BitcoinAccount, BitcoinBranch, BitcoinDerivedAddress, BitcoinKeyOrigin,
    };

    const MNEMONIC: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

    fn derived(index: u32) -> BitcoinDerivedAddress {
        BitcoinDerivedAddress {
            origin: BitcoinKeyOrigin::external(index),
            address: format!("address-{index}"),
        }
    }

    #[test]
    fn bitcoin_gap_limit_resets_after_a_used_address() {
        let mut scan = BitcoinBranchScan::new(BitcoinBranch::External);
        for index in 0..19 {
            scan.record(
                derived(index),
                BitcoinAddressStats {
                    balance: 0,
                    used: false,
                },
            )
            .unwrap();
        }
        assert!(!scan.complete);

        scan.record(
            derived(19),
            BitcoinAddressStats {
                balance: 0,
                used: true,
            },
        )
        .unwrap();
        assert!(!scan.complete);

        for index in 20..40 {
            scan.record(
                derived(index),
                BitcoinAddressStats {
                    balance: 0,
                    used: false,
                },
            )
            .unwrap();
        }
        assert!(scan.complete);
        assert_eq!(scan.addresses.len(), 40);
    }

    #[test]
    fn bitcoin_account_balance_includes_receive_and_change_branches() {
        let account = BitcoinAccount::from_mnemonic(MNEMONIC).unwrap();
        let receive = account.primary_address().unwrap();
        let next_receive = account
            .derive_address(BitcoinKeyOrigin::external(1))
            .unwrap();
        let change = account.derive_address(BitcoinKeyOrigin::change(1)).unwrap();
        let snapshot = BitcoinAccountSnapshot {
            account,
            addresses: vec![
                BitcoinAddressSnapshot {
                    derived: receive,
                    balance: 0,
                    used: true,
                },
                BitcoinAddressSnapshot {
                    derived: next_receive.clone(),
                    balance: 0,
                    used: false,
                },
                BitcoinAddressSnapshot {
                    derived: change,
                    balance: 2_479,
                    used: true,
                },
            ],
        };

        assert_eq!(snapshot.total_balance().unwrap(), 2_479);
        assert_eq!(snapshot.next_receive_address(), Some(&next_receive));
    }
}
