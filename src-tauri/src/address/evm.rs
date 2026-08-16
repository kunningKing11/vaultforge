use sha3::Keccak256;
use sha3::digest::Digest;

pub(crate) fn validate_address(address: &str) -> Result<(), String> {
    let hex_part = address
        .strip_prefix("0x")
        .ok_or_else(|| "EVM addresses must be 0x-prefixed".to_string())?;
    if hex_part.len() != 40 || !hex_part.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("Recipient must be a valid 0x-prefixed 40-hex-char EVM address".to_string());
    }

    let has_lower = hex_part.chars().any(|c| c.is_ascii_lowercase());
    let has_upper = hex_part.chars().any(|c| c.is_ascii_uppercase());
    if has_lower && has_upper {
        let hex_lower = hex_part.to_lowercase();
        let hash = Keccak256::digest(hex_lower.as_bytes());
        let hash_hex = hex::encode(hash);
        for (i, c) in hex_part.chars().enumerate() {
            if c.is_ascii_digit() {
                continue;
            }
            let nibble = u8::from_str_radix(&hash_hex[i..i + 1], 16).unwrap_or(0);
            if (nibble >= 8) != c.is_ascii_uppercase() {
                return Err("EIP-55 checksum validation failed".to_string());
            }
        }
    }

    Ok(())
}
