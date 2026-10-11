use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Execution {
    #[serde(alias = "Succeeded")]
    Succeeded,
    #[serde(alias = "Failed")]
    Failed,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Finality {
    #[serde(alias = "Included")]
    Included,
    #[serde(alias = "Finalized")]
    Finalized,
}

impl Default for Finality {
    fn default() -> Self {
        Self::Finalized
    }
}

impl Finality {
    pub(crate) fn satisfies(&self, required: &Self) -> bool {
        self == &Self::Finalized || required == &Self::Included
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) struct TxStatus {
    pub(crate) execution: Execution,
    pub(crate) finality: Finality,
    pub(crate) block_height: Option<u64>,
    pub(crate) block_hash: Option<String>,
    pub(crate) confirmations: Option<u64>,
    // Solana slots are not block heights.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) slot: Option<u64>,
}

impl TxStatus {
    pub(crate) fn legacy_label(&self) -> &'static str {
        match self.execution {
            Execution::Succeeded => "confirmed",
            Execution::Failed => "failed",
        }
    }
    pub(crate) fn new(execution: Execution, finality: Finality) -> Self {
        Self {
            execution,
            finality,
            block_height: None,
            block_hash: None,
            confirmations: None,
            slot: None,
        }
    }
}

/// Preserves the old display labels during the typed-observation migration, not finality guarantees.
pub(crate) fn legacy_tx_status(observation: Option<TxStatus>) -> Option<String> {
    observation.map(|status| status.legacy_label().to_string())
}
