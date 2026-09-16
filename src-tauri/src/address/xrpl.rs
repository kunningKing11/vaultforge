use xrpl::core::addresscodec::is_valid_classic_address;

pub(crate) fn validate_address(address: &str) -> Result<(), String> {
    if is_valid_classic_address(address) {
        Ok(())
    } else {
        Err("Recipient must be a valid XRP Ledger classic address".to_string())
    }
}

#[cfg(test)]
#[path = "../tests/address/xrpl.rs"]
mod tests;
