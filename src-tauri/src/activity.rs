use crate::dto::Activity;
use chrono::Utc;
use rand::Rng;
use sha2::{Digest, Sha256};

pub(crate) fn activity(kind: &str, title: &str, subtitle: &str, amount: &str) -> Activity {
    Activity {
        id: random_hex(8),
        kind: kind.to_string(),
        title: title.to_string(),
        subtitle: subtitle.to_string(),
        amount: amount.to_string(),
        status: "confirmed".to_string(),
        timestamp: Utc::now().to_rfc3339(),
        hash: format!("0x{}", random_hex(32)),
        from: None,
        to: None,
        network: None,
        payload_hash: None,
        signature: None,
        fee: None,
    }
}

pub(crate) fn hash_secret(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}

pub(crate) fn random_hex(bytes: usize) -> String {
    let mut rng = rand::rng();
    let data: Vec<u8> = (0..bytes).map(|_| rng.random()).collect();
    hex::encode(data)
}

pub(crate) fn short_address(address: &str) -> String {
    if address.len() <= 14 {
        return address.to_string();
    }
    format!("{}...{}", &address[..8], &address[address.len() - 6..])
}
