use solana_pubkey::Pubkey;
use std::str::FromStr;

pub(crate) fn validate_address(address: &str) -> Result<(), String> {
    validate_pubkey(address, "recipient")
}

pub(crate) fn validate_pubkey(value: &str, label: &str) -> Result<(), String> {
    Pubkey::from_str(value)
        .map_err(|_| format!("Solana {label} must be a valid 32-byte public key"))?;
    Ok(())
}
