use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Wallet {
    pub(crate) name: String,
    pub(crate) mnemonic: String,
    pub(crate) created_at: String,
    pub(crate) address: String,
    pub(crate) addresses: HashMap<String, String>,
    #[serde(alias = "passphrase_hash")]
    pub(crate) wallet_password_hash: String,
    pub(crate) assets: Vec<Asset>,
    pub(crate) activity: Vec<Activity>,
    #[serde(default = "default_enabled_networks")]
    pub(crate) enabled_networks: Vec<String>,
    #[serde(default)]
    pub(crate) auto_lock_timeout_secs: Option<u64>,
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct WalletPayload {
    pub(crate) wallet_name: String,
    pub(crate) mnemonic: String,
    pub(crate) created_at: String,
    pub(crate) address: String,
    pub(crate) addresses: HashMap<String, String>,
    #[serde(alias = "passphrase_hash")]
    pub(crate) wallet_password_hash: String,
    pub(crate) assets: Vec<Asset>,
    pub(crate) activity: Vec<Activity>,
    #[serde(default = "default_enabled_networks")]
    pub(crate) enabled_networks: Vec<String>,
    #[serde(default)]
    pub(crate) auto_lock_timeout_secs: Option<u64>,
}

fn default_enabled_networks() -> Vec<String> {
    vec![
        "bitcoin".into(),
        "ethereum".into(),
        "filecoin".into(),
        "injective".into(),
        "solana".into(),
        "tron".into(),
        "zcash".into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::default_enabled_networks;
    use crate::derivation::derive_addresses_from_mnemonic_filtered;

    #[test]
    fn default_enabled_networks_derives_evm_address() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let enabled_networks = default_enabled_networks();
        let enabled: Vec<&str> = enabled_networks.iter().map(String::as_str).collect();
        let addresses = derive_addresses_from_mnemonic_filtered(mnemonic, &enabled).unwrap();

        assert!(addresses.contains_key("evm"));
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Asset {
    pub(crate) symbol: String,
    pub(crate) name: String,
    pub(crate) balance: String,
    pub(crate) decimals: u32,
    pub(crate) price_usd: f64,
    pub(crate) change_24h: f64,
    pub(crate) network: String,
    #[serde(default)]
    pub(crate) token_address: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Activity {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) amount: String,
    pub(crate) status: String,
    pub(crate) timestamp: String,
    pub(crate) hash: String,
    pub(crate) from: Option<String>,
    pub(crate) to: Option<String>,
    pub(crate) network: Option<String>,
    pub(crate) payload_hash: Option<String>,
    pub(crate) signature: Option<String>,
    pub(crate) fee: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct WalletSession {
    pub(crate) has_wallet: bool,
    pub(crate) locked: bool,
    pub(crate) wallet_name: Option<String>,
    pub(crate) address: Option<String>,
    pub(crate) addresses: Option<HashMap<String, String>>,
    pub(crate) assets: Vec<Asset>,
    pub(crate) activity: Vec<Activity>,
    #[serde(default)]
    pub(crate) enabled_networks: Vec<String>,
    #[serde(default)]
    pub(crate) auto_lock_timeout_secs: Option<u64>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SignedTransaction {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) symbol: String,
    pub(crate) amount: String,
    pub(crate) note: String,
    pub(crate) network: String,
    pub(crate) nonce: String,
    pub(crate) signed_at: String,
    pub(crate) payload_hash: String,
    pub(crate) signature: String,
    pub(crate) fee_amount: String,
    pub(crate) fee_symbol: String,
    pub(crate) total_debit: String,
    pub(crate) post_balance: String,
    pub(crate) decimals: u32,
    pub(crate) fiat_value: f64,
    pub(crate) raw_tx: Option<String>,
    pub(crate) tx_hash: Option<String>,
}
