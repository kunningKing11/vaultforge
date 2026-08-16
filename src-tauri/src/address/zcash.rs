use sha2::Digest;
use sha2::Sha256;

pub(crate) fn validate_address(address: &str) -> Result<(), String> {
    let bytes = bs58::decode(address)
        .into_vec()
        .map_err(|_| "Recipient must be a valid Zcash transparent address".to_string())?;
    if bytes.len() != 26 {
        return Err("Zcash transparent address must decode to 26 bytes".to_string());
    }
    match bytes[..2] {
        [0x1c, 0xb8] | [0x1c, 0xbd] => {}
        _ => return Err("Only mainnet transparent Zcash recipients are supported".to_string()),
    }
    let hash = Sha256::digest(Sha256::digest(&bytes[..22]));
    if hash[..4] != bytes[22..] {
        return Err("Zcash transparent address checksum invalid".to_string());
    }
    Ok(())
}
