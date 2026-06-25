use crate::dto::Wallet;
use sha2::{Digest, Sha256};
use sha3::Keccak256;

pub(crate) fn validate_passphrase(passphrase: &str) -> Result<(), String> {
    if passphrase.chars().count() < 8 {
        return Err("Passphrase must be at least 8 characters".to_string());
    }
    Ok(())
}

pub(crate) fn clean_name(name: String) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        "Primary Wallet".to_string()
    } else {
        trimmed.chars().take(48).collect()
    }
}

pub(crate) fn validate_transfer(
    wallet: &Wallet,
    to: &str,
    symbol: &str,
    amount_wei: &str,
) -> Result<(), String> {
    let to = to.trim();

    let asset = wallet
        .assets
        .iter()
        .find(|asset| asset.symbol == symbol)
        .ok_or_else(|| "Asset not found".to_string())?;

    let amount: u128 = amount_wei
        .parse()
        .map_err(|_| "Invalid amount".to_string())?;
    let balance: u128 = asset
        .balance
        .parse()
        .map_err(|_| "Invalid stored balance".to_string())?;
    if amount == 0 {
        return Err("Amount must be greater than zero".to_string());
    }
    if balance < amount {
        return Err(format!("Insufficient {} balance", symbol));
    }
    validate_address_for_symbol(to, symbol)?;

    Ok(())
}

pub(crate) fn validate_address_for_symbol(address: &str, symbol: &str) -> Result<(), String> {
    match symbol {
        "BTC" => validate_bitcoin_address(address),
        "SOL" => validate_solana_address(address),
        "ZEC" => validate_zcash_address(address),
        "FIL" => validate_filecoin_address(address),
        "INJ" => validate_injective_address(address),
        _ => validate_evm_address(address),
    }
}

pub(crate) fn validate_evm_address(address: &str) -> Result<(), String> {
    let hex_part = address.strip_prefix("0x").unwrap_or(address);
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
            let should_be_upper = nibble >= 8;
            if should_be_upper != c.is_ascii_uppercase() {
                return Err("EIP-55 checksum validation failed".to_string());
            }
        }
    }

    Ok(())
}

pub(crate) fn validate_bitcoin_address(address: &str) -> Result<(), String> {
    if address.starts_with("bc1") || address.starts_with("tb1") {
        bech32::decode(address)
            .map_err(|_| "Recipient must be a valid Bitcoin bech32 address".to_string())?;
        return Ok(());
    }
    if address.starts_with('1')
        || address.starts_with('3')
        || address.starts_with('2')
        || address.starts_with('m')
        || address.starts_with('n')
    {
        bs58::decode(address)
            .with_check(None)
            .into_vec()
            .map_err(|_| "Recipient must be a valid Bitcoin base58 address".to_string())?;
        return Ok(());
    }
    Err("Recipient must be a valid Bitcoin address (bc1, 1, or 3)".to_string())
}

pub(crate) fn validate_solana_address(address: &str) -> Result<(), String> {
    let bytes = bs58::decode(address)
        .into_vec()
        .map_err(|_| "Recipient must be a valid base58 Solana address".to_string())?;
    if bytes.len() != 32 {
        return Err("Solana address must decode to 32 bytes".to_string());
    }
    Ok(())
}

pub(crate) fn validate_zcash_address(address: &str) -> Result<(), String> {
    if address.starts_with("zs1") || address.starts_with("ztestsapling") {
        return Err("Zcash shielded addresses are not yet supported".to_string());
    }
    if address.starts_with("t1") || address.starts_with("t3") || address.starts_with("tm") {
        let bytes = bs58::decode(address)
            .into_vec()
            .map_err(|_| "Recipient must be a valid Zcash transparent address".to_string())?;
        if bytes.len() != 26 {
            return Err("Zcash transparent address must decode to 26 bytes".to_string());
        }
        let payload = &bytes[..22];
        let checksum = &bytes[22..];
        let hash = Sha256::digest(Sha256::digest(payload));
        if &hash[..4] != checksum {
            return Err("Zcash transparent address checksum invalid".to_string());
        }
        return Ok(());
    }
    Err("Recipient must be a valid Zcash address (t1 or tm)".to_string())
}

pub(crate) fn validate_filecoin_address(address: &str) -> Result<(), String> {
    if !address.starts_with('f') && !address.starts_with('t') {
        return Err("Filecoin address must start with f or t".to_string());
    }
    if address.len() < 3 {
        return Err("Filecoin address too short".to_string());
    }
    let protocol = address.chars().nth(1).unwrap_or(' ');
    match protocol {
        '0' => {
            if !address[2..].chars().all(|c| c.is_ascii_digit()) {
                return Err("Filecoin ID address must contain only digits after f0".to_string());
            }
            Ok(())
        }
        '1' => {
            let bytes = bs58::decode(&address[2..])
                .with_check(Some(0x01))
                .into_vec()
                .map_err(|_| "Invalid Filecoin f1 address".to_string())?;
            if bytes.len() != 21 {
                return Err(
                    "Filecoin f1 address must decode to 21 bytes (1 prefix + 20 payload)"
                        .to_string(),
                );
            }
            if bytes[0] != 1 {
                return Err("Filecoin f1 address has wrong protocol byte".to_string());
            }
            Ok(())
        }
        '3' => {
            let bytes = bs58::decode(&address[2..])
                .with_check(Some(0x03))
                .into_vec()
                .map_err(|_| "Invalid Filecoin f3 address".to_string())?;
            if bytes.len() != 48 {
                return Err("Filecoin f3 (BLS) address must decode to 48 bytes".to_string());
            }
            Ok(())
        }
        '4' => {
            if address.starts_with("f410") || address.starts_with("t410") {
                bech32::decode(address)
                    .map_err(|_| "Invalid Filecoin f4 (delegated) address".to_string())?;
                Ok(())
            } else {
                Err("Filecoin f4 address must start with f410".to_string())
            }
        }
        _ => Err("Unknown Filecoin address protocol".to_string()),
    }
}

pub(crate) fn validate_injective_address(address: &str) -> Result<(), String> {
    if !address.starts_with("inj1") {
        return Err("Injective address must start with inj1".to_string());
    }
    bech32::decode(address)
        .map_err(|_| "Recipient must be a valid Injective bech32 address".to_string())?;
    Ok(())
}
