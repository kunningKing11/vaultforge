use blake2::{
    Blake2b, Digest,
    digest::consts::{U4, U20},
};

const BASE32_ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
const CHECKSUM_LEN: usize = 4;
const MAX_ADDRESS_LEN: usize = 115;
const MAX_INT64: u64 = i64::MAX as u64;
const MAX_SUBADDRESS_LEN: usize = 54;

pub(crate) fn filecoin_mainnet_secp256k1_address(
    uncompressed_public_key: &[u8],
) -> Result<String, String> {
    if uncompressed_public_key.len() != 65 || uncompressed_public_key.first() != Some(&0x04) {
        return Err("Filecoin secp256k1 public key must be uncompressed".to_string());
    }
    let payload: [u8; 20] = Blake2b::<U20>::digest(uncompressed_public_key).into();
    encode_filecoin_address(1, &payload)
}

pub(crate) fn validate_address(address: &str) -> Result<(), String> {
    if !(3..=MAX_ADDRESS_LEN).contains(&address.len()) || !address.is_ascii() {
        return Err("Filecoin address has an invalid length or character set".to_string());
    }
    let bytes = address.as_bytes();
    if bytes[0] != b'f' {
        return Err("Filecoin address must use the mainnet f prefix".to_string());
    }
    let protocol = match bytes[1] {
        b'0' => 0,
        b'1' => 1,
        b'2' => 2,
        b'3' => 3,
        b'4' => 4,
        _ => return Err("Filecoin address has an unsupported protocol".to_string()),
    };
    let raw = &address[2..];
    if protocol == 0 {
        return parse_filecoin_decimal(raw, "ID").map(|_| ());
    }

    let (payload, checksum) = if protocol == 4 {
        let (namespace, subaddress) = raw.split_once('f').ok_or_else(|| {
            "Delegated Filecoin address is missing its namespace separator".to_string()
        })?;
        let namespace = parse_filecoin_decimal(namespace, "namespace")?;
        let decoded = decode_base32(subaddress)?;
        let (subaddress, checksum) = split_checksum(&decoded)?;
        if subaddress.len() > MAX_SUBADDRESS_LEN {
            return Err("Delegated Filecoin subaddress is too long".to_string());
        }
        let mut payload = encode_uvarint(namespace);
        payload.extend_from_slice(subaddress);
        (payload, checksum)
    } else {
        let decoded = decode_base32(raw)?;
        let (payload, checksum) = split_checksum(&decoded)?;
        let expected_len = if protocol == 3 { 48 } else { 20 };
        if payload.len() != expected_len {
            return Err("Filecoin address has an invalid payload length".to_string());
        }
        (payload.to_vec(), checksum)
    };

    if filecoin_checksum(protocol, &payload) != checksum {
        return Err("Filecoin address checksum is invalid".to_string());
    }
    Ok(())
}

fn encode_filecoin_address(protocol: u8, payload: &[u8]) -> Result<String, String> {
    let mut data = payload.to_vec();
    data.extend_from_slice(&filecoin_checksum(protocol, payload));
    Ok(format!("f{protocol}{}", encode_base32(&data)))
}

fn parse_filecoin_decimal(raw: &str, label: &str) -> Result<u64, String> {
    if raw.is_empty() || raw.len() > 19 || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("Filecoin {label} must be a decimal u64 below 2^63"));
    }
    let value = raw
        .parse::<u64>()
        .map_err(|_| format!("Filecoin {label} must be a decimal u64 below 2^63"))?;
    if value > MAX_INT64 {
        return Err(format!("Filecoin {label} must be below 2^63"));
    }
    Ok(value)
}

fn split_checksum(decoded: &[u8]) -> Result<(&[u8], [u8; CHECKSUM_LEN]), String> {
    if decoded.len() < CHECKSUM_LEN {
        return Err("Filecoin address is missing its checksum".to_string());
    }
    let payload_len = decoded.len() - CHECKSUM_LEN;
    Ok((
        &decoded[..payload_len],
        decoded[payload_len..]
            .try_into()
            .map_err(|_| "Filecoin address has an invalid checksum".to_string())?,
    ))
}

fn filecoin_checksum(protocol: u8, payload: &[u8]) -> [u8; CHECKSUM_LEN] {
    let mut bytes = Vec::with_capacity(payload.len() + 1);
    bytes.push(protocol);
    bytes.extend_from_slice(payload);
    Blake2b::<U4>::digest(bytes).into()
}

fn encode_uvarint(mut value: u64) -> Vec<u8> {
    let mut encoded = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        encoded.push(byte);
        if value == 0 {
            return encoded;
        }
    }
}

fn encode_base32(data: &[u8]) -> String {
    let mut result = String::new();
    let mut accumulator = 0u32;
    let mut bits = 0usize;
    for byte in data {
        accumulator = (accumulator << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            result.push(BASE32_ALPHABET[((accumulator >> bits) & 0x1f) as usize] as char);
        }
    }
    if bits > 0 {
        result.push(BASE32_ALPHABET[((accumulator << (5 - bits)) & 0x1f) as usize] as char);
    }
    result
}

fn decode_base32(value: &str) -> Result<Vec<u8>, String> {
    if value.is_empty() {
        return Err("Filecoin base32 payload is empty".to_string());
    }
    let mut decoded = Vec::new();
    let mut accumulator = 0u32;
    let mut bits = 0usize;
    for byte in value.bytes() {
        let index = match byte {
            b'a'..=b'z' => byte - b'a',
            b'2'..=b'7' => byte - b'2' + 26,
            _ => return Err("Filecoin payload must use lowercase unpadded base32".to_string()),
        };
        accumulator = (accumulator << 5) | u32::from(index);
        bits += 5;
        while bits >= 8 {
            bits -= 8;
            decoded.push((accumulator >> bits) as u8);
        }
    }
    if bits > 0 && ((accumulator << (8 - bits)) & 0xff) != 0 {
        return Err("Filecoin base32 payload is not canonically encoded".to_string());
    }
    if encode_base32(&decoded) != value {
        return Err("Filecoin base32 payload is not canonically encoded".to_string());
    }
    Ok(decoded)
}
