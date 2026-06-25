use bech32::{self, ToBase32, Variant};
use bip32::{DerivationPath, XPrv};
use bip39::{Language, Mnemonic};
use ed25519_dalek::{PublicKey as DalekPublicKey, SecretKey as DalekSecretKey};
use hmac::{Hmac, Mac};
use k256::ecdsa::SigningKey;
use rand::Rng;
use ripemd::Ripemd160;
use sha2::{Digest, Sha256, Sha512};
use sha3::Keccak256;
use std::collections::HashMap;

const EVM_DERIVATION_PATH: &str = "m/44'/60'/0'/0/0";
pub(crate) const BITCOIN_DERIVATION_PATH: &str = "m/84'/0'/0'/0/0";
const ZCASH_DERIVATION_PATH: &str = "m/44'/133'/0'/0/0";
const SOLANA_DERIVATION_PATH: &[u32] = &[44, 501, 0, 0];
const FILECOIN_DERIVATION_PATH: &str = "m/44'/461'/0'/0/0";
const INJECTIVE_DERIVATION_PATH: &str = EVM_DERIVATION_PATH;

pub(crate) fn signing_key_from_mnemonic(mnemonic: &str) -> Result<k256::ecdsa::SigningKey, String> {
    let private_key = secp256k1_private_key_from_mnemonic(mnemonic, EVM_DERIVATION_PATH)?;
    k256::ecdsa::SigningKey::from_bytes((&private_key).into())
        .map_err(|_| "Failed to create signing key".to_string())
}

pub(crate) fn generate_mnemonic() -> Result<String, String> {
    let mut entropy = [0u8; 16];
    let mut rng = rand::thread_rng();
    rng.fill(&mut entropy);
    Mnemonic::from_entropy_in(Language::English, &entropy)
        .map(|mnemonic| mnemonic.to_string())
        .map_err(|_| "Failed to generate recovery phrase".to_string())
}

pub(crate) fn address_from_seed(seed: &str) -> String {
    format!("0x{}", &crate::activity::hash_secret(seed)[..40])
}

fn mnemonic_seed(mnemonic: &str) -> Result<[u8; 64], String> {
    let mnemonic = Mnemonic::parse_in_normalized(Language::English, mnemonic)
        .map_err(|_| "Invalid recovery phrase".to_string())?;
    Ok(mnemonic.to_seed(""))
}

pub(crate) fn secp256k1_private_key_from_mnemonic(
    mnemonic: &str,
    path: &str,
) -> Result<[u8; 32], String> {
    let seed = mnemonic_seed(mnemonic)?;
    let path: DerivationPath = path
        .parse()
        .map_err(|_| format!("Invalid derivation path: {path}"))?;
    let child = XPrv::derive_from_path(seed, &path)
        .map_err(|_| format!("Failed to derive key at {path}"))?;
    let bytes = child.private_key().to_bytes();
    Ok(bytes.into())
}

fn solana_secret_key_from_mnemonic(mnemonic: &str) -> Result<[u8; 32], String> {
    type HmacSha512 = Hmac<Sha512>;

    let seed = mnemonic_seed(mnemonic)?;
    let mut mac = <HmacSha512 as Mac>::new_from_slice(b"ed25519 seed")
        .map_err(|_| "Failed to initialize Solana derivation".to_string())?;
    mac.update(&seed);
    let result = mac.finalize().into_bytes();
    let mut key = [0u8; 32];
    let mut chain_code = [0u8; 32];
    key.copy_from_slice(&result[..32]);
    chain_code.copy_from_slice(&result[32..]);

    for index in SOLANA_DERIVATION_PATH {
        let hardened = index | 0x8000_0000;
        let mut data = Vec::with_capacity(37);
        data.push(0);
        data.extend_from_slice(&key);
        data.extend_from_slice(&hardened.to_be_bytes());

        let mut mac = <HmacSha512 as Mac>::new_from_slice(&chain_code)
            .map_err(|_| "Failed to derive Solana child key".to_string())?;
        mac.update(&data);
        let result = mac.finalize().into_bytes();
        key.copy_from_slice(&result[..32]);
        chain_code.copy_from_slice(&result[32..]);
    }

    Ok(key)
}

pub(crate) fn derive_addresses_from_mnemonic(
    mnemonic: &str,
) -> Result<HashMap<String, String>, String> {
    let evm_private_key = secp256k1_private_key_from_mnemonic(mnemonic, EVM_DERIVATION_PATH)?;
    let bitcoin_private_key =
        secp256k1_private_key_from_mnemonic(mnemonic, BITCOIN_DERIVATION_PATH)?;
    let zcash_private_key = secp256k1_private_key_from_mnemonic(mnemonic, ZCASH_DERIVATION_PATH)?;
    let solana_secret_key = solana_secret_key_from_mnemonic(mnemonic)?;
    let filecoin_private_key =
        secp256k1_private_key_from_mnemonic(mnemonic, FILECOIN_DERIVATION_PATH)?;
    let injective_private_key =
        secp256k1_private_key_from_mnemonic(mnemonic, INJECTIVE_DERIVATION_PATH)?;

    let evm_address = ethereum_address_from_private_key(&evm_private_key)?;
    let bitcoin_address = bitcoin_bech32_address(&bitcoin_private_key, false)?;
    let zcash_address = zcash_transparent_address(&zcash_private_key, false)?;
    let solana_address = solana_address_from_secret_key(&solana_secret_key)?;
    let filecoin_address = filecoin_address_from_private_key(&filecoin_private_key)?;
    let injective_address = bech32_account_address(&injective_private_key, "inj")?;

    let mut addresses = HashMap::new();
    addresses.insert("evm".to_string(), evm_address);
    addresses.insert("bitcoin".to_string(), bitcoin_address);
    addresses.insert("zcash".to_string(), zcash_address);
    addresses.insert("solana".to_string(), solana_address);
    addresses.insert("filecoin".to_string(), filecoin_address);
    addresses.insert("injective".to_string(), injective_address);
    Ok(addresses)
}

pub(crate) fn signing_key_from_private_key(private_key: &[u8; 32]) -> Result<SigningKey, String> {
    SigningKey::from_bytes(private_key.into()).map_err(|_| "Invalid private key".to_string())
}

pub(crate) fn ethereum_address_from_private_key(private_key: &[u8; 32]) -> Result<String, String> {
    let signing_key = signing_key_from_private_key(private_key)?;
    let verifying_key = signing_key.verifying_key();
    let public_key = verifying_key.to_encoded_point(false);
    let public_bytes = public_key.as_bytes();
    let hash = Keccak256::digest(&public_bytes[1..]);
    Ok(format!("0x{}", hex::encode(&hash[12..])))
}

pub(crate) fn bitcoin_bech32_address(
    private_key: &[u8; 32],
    is_testnet: bool,
) -> Result<String, String> {
    let signing_key = signing_key_from_private_key(private_key)?;
    let verifying_key = signing_key.verifying_key();
    let encoded = verifying_key.to_encoded_point(true);
    let public_bytes = encoded.as_bytes();
    let hashed = Ripemd160::digest(Sha256::digest(public_bytes));
    let hrp = if is_testnet { "tb" } else { "bc" };
    let mut bech32_data = vec![bech32::u5::try_from_u8(0).map_err(|_| "Failed to encode address")?];
    bech32_data.extend(hashed.to_base32());
    bech32::encode(hrp, bech32_data, Variant::Bech32)
        .map_err(|_| "Failed to encode address".to_string())
}

pub(crate) fn zcash_transparent_address(
    private_key: &[u8; 32],
    is_testnet: bool,
) -> Result<String, String> {
    let signing_key = signing_key_from_private_key(private_key)?;
    let verifying_key = signing_key.verifying_key();
    let encoded = verifying_key.to_encoded_point(true);
    let public_bytes = encoded.as_bytes();
    let payload = Ripemd160::digest(Sha256::digest(public_bytes));
    let prefix = if is_testnet {
        vec![0x1d, 0x25]
    } else {
        vec![0x1c, 0xb8]
    };
    let mut bytes = prefix;
    bytes.extend(payload);
    Ok(bs58::encode(bytes).with_check().into_string())
}

fn solana_address_from_secret_key(secret_bytes: &[u8; 32]) -> Result<String, String> {
    let secret = DalekSecretKey::from_bytes(secret_bytes)
        .map_err(|_| "Failed to derive Solana key".to_string())?;
    let public = DalekPublicKey::from(&secret);
    Ok(bs58::encode(public.as_bytes()).into_string())
}

pub(crate) fn filecoin_address_from_private_key(private_key: &[u8; 32]) -> Result<String, String> {
    let signing_key = signing_key_from_private_key(private_key)?;
    let verifying_key = signing_key.verifying_key();
    let encoded = verifying_key.to_encoded_point(true);
    let public_bytes = encoded.as_bytes();
    let payload = Ripemd160::digest(Sha256::digest(public_bytes));
    let mut bytes = vec![0x01];
    bytes.extend(payload);
    Ok(format!(
        "f1{}",
        bs58::encode(bytes).with_check().into_string()
    ))
}

pub(crate) fn bech32_account_address(private_key: &[u8; 32], hrp: &str) -> Result<String, String> {
    let signing_key = signing_key_from_private_key(private_key)?;
    let verifying_key = signing_key.verifying_key();
    let encoded = verifying_key.to_encoded_point(true);
    let public_bytes = encoded.as_bytes();
    let payload = Ripemd160::digest(Sha256::digest(public_bytes));
    let bech32_data = payload.to_base32();
    bech32::encode(hrp, bech32_data, Variant::Bech32)
        .map_err(|_| "Failed to encode address".to_string())
}
