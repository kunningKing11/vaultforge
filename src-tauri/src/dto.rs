use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub(crate) enum FiatCurrency {
    Usd,
    Eur,
    Gbp,
    Jpy,
}

impl FiatCurrency {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Usd => "USD",
            Self::Eur => "EUR",
            Self::Gbp => "GBP",
            Self::Jpy => "JPY",
        }
    }
}

fn default_fiat_currency() -> FiatCurrency {
    FiatCurrency::Usd
}

fn default_usd_exchange_rate() -> f64 {
    1.0
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Wallet {
    pub(crate) name: String,
    pub(crate) mnemonic: String,
    pub(crate) created_at: String,
    pub(crate) addresses: HashMap<String, String>,
    #[serde(alias = "passphrase_hash")]
    pub(crate) wallet_password_hash: String,
    #[serde(default = "default_fiat_currency")]
    pub(crate) fiat_currency: FiatCurrency,
    #[serde(default = "default_usd_exchange_rate")]
    pub(crate) usd_exchange_rate: f64,
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
    pub(crate) addresses: HashMap<String, String>,
    #[serde(alias = "passphrase_hash")]
    pub(crate) wallet_password_hash: String,
    #[serde(default = "default_fiat_currency")]
    pub(crate) fiat_currency: FiatCurrency,
    #[serde(default = "default_usd_exchange_rate")]
    pub(crate) usd_exchange_rate: f64,
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
    pub(crate) addresses: Option<HashMap<String, String>>,
    pub(crate) fiat_currency: Option<FiatCurrency>,
    pub(crate) usd_exchange_rate: Option<f64>,
    pub(crate) assets: Vec<Asset>,
    pub(crate) activity: Vec<Activity>,
    #[serde(default)]
    pub(crate) enabled_networks: Vec<String>,
    #[serde(default)]
    pub(crate) auto_lock_timeout_secs: Option<u64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RefreshWarningKind {
    Balance,
    Value,
}

#[derive(Clone, Serialize)]
pub(crate) struct RefreshWarning {
    pub(crate) kind: RefreshWarningKind,
    pub(crate) subject: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WalletRefreshResult {
    pub(crate) session: WalletSession,
    pub(crate) warnings: Vec<RefreshWarning>,
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

#[cfg(test)]
#[path = "tests/dto.rs"]
mod tests;
