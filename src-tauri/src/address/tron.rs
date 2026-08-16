pub(crate) fn validate_address(address: &str) -> Result<(), String> {
    let bytes = bs58::decode(address)
        .with_check(None)
        .into_vec()
        .map_err(|_| "Recipient must be a valid Tron base58check address".to_string())?;
    if bytes.len() != 21 || bytes[0] != 0x41 {
        return Err("Recipient must be a Tron mainnet address".to_string());
    }
    Ok(())
}
