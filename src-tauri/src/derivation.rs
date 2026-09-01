use bech32::{self, Bech32, Hrp, hrp, segwit};
use bip32::{ChildNumber, DerivationPath, XPrv, XPub};
use bip39::{Language, Mnemonic};
use ed25519_dalek::SigningKey as DalekSigningKey;
use hmac::{Hmac, KeyInit, Mac};
use k256::ecdsa::SigningKey;
use rand::RngExt;
use ripemd::Ripemd160;
use ripemd::digest::Digest as RipemdDigest;
use sha2::{Digest as Sha2Digest, Sha256, Sha512};
use sha3::Keccak256;
use sha3::digest::Digest as Sha3Digest;
use std::collections::HashMap;
use zeroize::Zeroize;

use crate::address::filecoin::filecoin_mainnet_secp256k1_address;
use crate::registry::network_by_id;

pub(crate) const BIP39_WORD_COUNTS: [usize; 5] = [12, 15, 18, 21, 24];

#[cfg(test)]
pub(crate) const ALL_NETWORKS: &[&str] = &[
    "bitcoin",
    "ethereum",
    "filecoin",
    "injective",
    "solana",
    "tron",
    "zcash",
];

// TODO: why some are pub(crate) and some are not? Should we make them all pub(crate)?
pub(crate) const BITCOIN_DERIVATION_PATH: &str = "m/84'/0'/0'/0/0";
const BITCOIN_BIP84_ACCOUNT_PATH: &str = "m/84'/0'/0'";
pub(crate) const BITCOIN_BIP84_GAP_LIMIT: u32 = 20;
const EVM_DERIVATION_PATH: &str = "m/44'/60'/0'/0/0";
const FILECOIN_DERIVATION_PATH: &str = "m/44'/461'/0'/0/0";
const INJECTIVE_DERIVATION_PATH: &str = EVM_DERIVATION_PATH;
pub(crate) const SOLANA_DERIVATION_PATH: &[u32] = &[44, 501, 0, 0];
pub(crate) const TRON_DERIVATION_PATH: &str = "m/44'/195'/0'/0/0";
const ZCASH_DERIVATION_PATH: &str = "m/44'/133'/0'/0/0";

struct DerivedWalletKeys {
    bitcoin: [u8; 32],
    evm: [u8; 32],
    filecoin: [u8; 32],
    injective: [u8; 32],
    solana: [u8; 32],
    tron: [u8; 32],
    zcash: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum BitcoinBranch {
    External,
    Change,
}

impl BitcoinBranch {
    fn index(self) -> u32 {
        match self {
            Self::External => 0,
            Self::Change => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct BitcoinKeyOrigin {
    pub(crate) branch: BitcoinBranch,
    pub(crate) index: u32,
}

impl BitcoinKeyOrigin {
    pub(crate) fn external(index: u32) -> Self {
        Self {
            branch: BitcoinBranch::External,
            index,
        }
    }

    pub(crate) fn change(index: u32) -> Self {
        Self {
            branch: BitcoinBranch::Change,
            index,
        }
    }

    pub(crate) fn derivation_path(self) -> String {
        format!(
            "{BITCOIN_BIP84_ACCOUNT_PATH}/{}/{}",
            self.branch.index(),
            self.index
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BitcoinDerivedAddress {
    pub(crate) origin: BitcoinKeyOrigin,
    pub(crate) address: String,
}

#[derive(Clone)]
pub(crate) struct BitcoinAccount {
    account_xpub: XPub,
}

impl BitcoinAccount {
    pub(crate) fn from_mnemonic(mnemonic: &str) -> Result<Self, String> {
        let mut seed = mnemonic_seed(mnemonic)?;
        let result = (|| {
            let path: DerivationPath = BITCOIN_BIP84_ACCOUNT_PATH
                .parse()
                .map_err(|_| "Invalid Bitcoin BIP84 account path".to_string())?;
            let account_xprv = XPrv::derive_from_path(seed.as_slice(), &path)
                .map_err(|_| "Failed to derive Bitcoin BIP84 account".to_string())?;
            Ok(Self {
                account_xpub: XPub::from(&account_xprv),
            })
        })();
        seed.zeroize();
        result
    }

    pub(crate) fn derive_address(
        &self,
        origin: BitcoinKeyOrigin,
    ) -> Result<BitcoinDerivedAddress, String> {
        let branch = ChildNumber::new(origin.branch.index(), false)
            .map_err(|_| "Invalid Bitcoin BIP84 branch".to_string())?;
        let index = ChildNumber::new(origin.index, false)
            .map_err(|_| "Invalid Bitcoin BIP84 address index".to_string())?;
        let branch_xpub = self
            .account_xpub
            .derive_child(branch)
            .map_err(|_| "Failed to derive Bitcoin BIP84 branch".to_string())?;
        let address_xpub = branch_xpub
            .derive_child(index)
            .map_err(|_| "Failed to derive Bitcoin BIP84 address".to_string())?;
        let public_key = address_xpub.public_key().to_encoded_point(true);
        let address = bitcoin_bech32_address_from_public_key(public_key.as_bytes(), false)?;
        Ok(BitcoinDerivedAddress { origin, address })
    }

    pub(crate) fn primary_address(&self) -> Result<BitcoinDerivedAddress, String> {
        self.derive_address(BitcoinKeyOrigin::external(0))
    }
}

pub(crate) fn signing_key_from_mnemonic(mnemonic: &str) -> Result<k256::ecdsa::SigningKey, String> {
    let private_key = secp256k1_private_key_from_mnemonic(mnemonic, EVM_DERIVATION_PATH)?;
    k256::ecdsa::SigningKey::from_bytes((&private_key).into())
        .map_err(|_| "Failed to create signing key".to_string())
}

pub(crate) fn tron_private_key_from_mnemonic(mnemonic: &str) -> Result<[u8; 32], String> {
    secp256k1_private_key_from_mnemonic(mnemonic, TRON_DERIVATION_PATH)
}

pub(crate) fn generate_mnemonic(word_count: u32) -> Result<String, String> {
    let entropy_bytes = match word_count {
        12 => 16,
        15 => 20,
        18 => 24,
        21 => 28,
        24 => 32,
        _ => return Err("Word count must be 12, 15, 18, 21, or 24".to_string()),
    };
    let mut entropy = vec![0u8; entropy_bytes as usize];
    let mut rng = rand::rng();
    rng.fill(entropy.as_mut_slice());
    Mnemonic::from_entropy_in(Language::English, &entropy)
        .map(|mnemonic| mnemonic.to_string())
        .map_err(|_| "Failed to generate recovery phrase".to_string())
}

pub(crate) fn validate_recovery_phrase_word_count(mnemonic: &str) -> Result<(), String> {
    let word_count = mnemonic.split_whitespace().count();
    if BIP39_WORD_COUNTS.contains(&word_count) {
        Ok(())
    } else {
        Err("Recovery phrase must contain 12, 15, 18, 21, or 24 words".to_string())
    }
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
    let mut seed = mnemonic_seed(mnemonic)?;
    let result = (|| {
        let path: DerivationPath = path
            .parse()
            .map_err(|_| format!("Invalid derivation path: {path}"))?;
        let child = XPrv::derive_from_path(seed.as_slice(), &path)
            .map_err(|_| format!("Failed to derive key at {path}"))?;
        let bytes = child.private_key().to_bytes();
        Ok(bytes.into())
    })();
    seed.zeroize();
    result
}

pub(crate) fn solana_secret_key_from_mnemonic(mnemonic: &str) -> Result<[u8; 32], String> {
    type HmacSha512 = Hmac<Sha512>;

    let seed = mnemonic_seed(mnemonic)?;
    let mut mac = HmacSha512::new_from_slice(b"ed25519 seed")
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

        let mut mac = HmacSha512::new_from_slice(&chain_code)
            .map_err(|_| "Failed to derive Solana child key".to_string())?;
        mac.update(&data);
        let result = mac.finalize().into_bytes();
        key.copy_from_slice(&result[..32]);
        chain_code.copy_from_slice(&result[32..]);
    }

    Ok(key)
}

fn derive_wallet_keys(mnemonic: &str) -> Result<DerivedWalletKeys, String> {
    Ok(DerivedWalletKeys {
        bitcoin: secp256k1_private_key_from_mnemonic(mnemonic, BITCOIN_DERIVATION_PATH)?,
        evm: secp256k1_private_key_from_mnemonic(mnemonic, EVM_DERIVATION_PATH)?,
        filecoin: secp256k1_private_key_from_mnemonic(mnemonic, FILECOIN_DERIVATION_PATH)?,
        injective: secp256k1_private_key_from_mnemonic(mnemonic, INJECTIVE_DERIVATION_PATH)?,
        solana: solana_secret_key_from_mnemonic(mnemonic)?,
        tron: secp256k1_private_key_from_mnemonic(mnemonic, TRON_DERIVATION_PATH)?,
        zcash: secp256k1_private_key_from_mnemonic(mnemonic, ZCASH_DERIVATION_PATH)?,
    })
}

pub(crate) fn address_key_for_network(network_id: &str) -> Option<&'static str> {
    network_by_id(network_id).map(|network| network.address_key.as_str())
}

pub(crate) fn derive_addresses_from_mnemonic_filtered(
    mnemonic: &str,
    enabled: &[&str],
) -> Result<HashMap<String, String>, String> {
    let keys = derive_wallet_keys(mnemonic)?;
    let mut addresses = HashMap::new();

    for network_id in enabled {
        if let Some(family) = address_key_for_network(network_id) {
            match family {
                "bitcoin" => {
                    addresses.insert(
                        "bitcoin".to_string(),
                        bitcoin_bech32_address(&keys.bitcoin, false)?,
                    );
                }
                "evm" => {
                    addresses.insert(
                        "evm".to_string(),
                        ethereum_address_from_private_key(&keys.evm)?,
                    );
                }
                "filecoin" => {
                    addresses.insert(
                        "filecoin".to_string(),
                        filecoin_address_from_private_key(&keys.filecoin)?,
                    );
                }
                "injective" => {
                    addresses.insert(
                        "injective".to_string(),
                        bech32_account_address(&keys.injective, "inj")?,
                    );
                }
                "solana" => {
                    addresses.insert(
                        "solana".to_string(),
                        solana_address_from_secret_key(&keys.solana)?,
                    );
                }
                "tron" => {
                    addresses.insert(
                        "tron".to_string(),
                        tron_address_from_private_key(&keys.tron)?,
                    );
                }
                "zcash" => {
                    addresses.insert(
                        "zcash".to_string(),
                        zcash_transparent_address(&keys.zcash, false)?,
                    );
                }
                _ => {}
            }
        }
    }

    Ok(addresses)
}

pub(crate) fn signing_key_from_private_key(private_key: &[u8; 32]) -> Result<SigningKey, String> {
    SigningKey::from_bytes(private_key.into()).map_err(|_| "Invalid private key".to_string())
}

pub(crate) fn ethereum_address_from_private_key(private_key: &[u8; 32]) -> Result<String, String> {
    let signing_key = signing_key_from_private_key(private_key)?;
    let verifying_key = signing_key.verifying_key();
    let public_key = verifying_key.to_sec1_point(false);
    let public_bytes = public_key.as_bytes();
    let hash = <Keccak256 as Sha3Digest>::digest(&public_bytes[1..]);
    Ok(format!("0x{}", hex::encode(&hash[12..])))
}

pub(crate) fn tron_address_from_private_key(private_key: &[u8; 32]) -> Result<String, String> {
    let signing_key = signing_key_from_private_key(private_key)?;
    let verifying_key = signing_key.verifying_key();
    let public_key = verifying_key.to_sec1_point(false);
    let public_bytes = public_key.as_bytes();
    let hash = <Keccak256 as Sha3Digest>::digest(&public_bytes[1..]);

    let mut address = Vec::with_capacity(21);
    address.push(0x41);
    address.extend_from_slice(&hash[12..]);

    Ok(bs58::encode(address).with_check().into_string())
}

pub(crate) fn bitcoin_bech32_address(
    private_key: &[u8; 32],
    is_testnet: bool,
) -> Result<String, String> {
    let signing_key = signing_key_from_private_key(private_key)?;
    let verifying_key = signing_key.verifying_key();
    let encoded = verifying_key.to_sec1_point(true);
    bitcoin_bech32_address_from_public_key(encoded.as_bytes(), is_testnet)
}

fn bitcoin_bech32_address_from_public_key(
    public_key: &[u8],
    is_testnet: bool,
) -> Result<String, String> {
    let hashed = <Ripemd160 as RipemdDigest>::digest(<Sha256 as Sha2Digest>::digest(public_key));
    let hrp = if is_testnet { hrp::TB } else { hrp::BC };
    segwit::encode_v0(hrp, &hashed).map_err(|_| "Failed to encode address".to_string())
}

pub(crate) fn zcash_transparent_address(
    private_key: &[u8; 32],
    is_testnet: bool,
) -> Result<String, String> {
    let signing_key = signing_key_from_private_key(private_key)?;
    let verifying_key = signing_key.verifying_key();
    let encoded = verifying_key.to_sec1_point(true);
    let public_bytes = encoded.as_bytes();
    let payload = <Ripemd160 as RipemdDigest>::digest(<Sha256 as Sha2Digest>::digest(public_bytes));
    let prefix = if is_testnet {
        vec![0x1d, 0x25]
    } else {
        vec![0x1c, 0xb8]
    };
    let mut bytes = prefix;
    bytes.extend(payload);
    Ok(bs58::encode(bytes).with_check().into_string())
}

pub(crate) fn solana_address_from_secret_key(secret_bytes: &[u8; 32]) -> Result<String, String> {
    let secret = DalekSigningKey::from_bytes(secret_bytes);
    let public = secret.verifying_key();
    Ok(bs58::encode(public.as_bytes()).into_string())
}

// TODO: refactor to move imports into here if possible - if no, do opposite for all other chains
pub(crate) fn filecoin_address_from_private_key(private_key: &[u8; 32]) -> Result<String, String> {
    let signing_key = signing_key_from_private_key(private_key)?;
    let verifying_key = signing_key.verifying_key();
    let encoded = verifying_key.to_sec1_point(false);
    filecoin_mainnet_secp256k1_address(encoded.as_bytes())
}

pub(crate) fn bech32_account_address(private_key: &[u8; 32], hrp: &str) -> Result<String, String> {
    let signing_key = signing_key_from_private_key(private_key)?;
    let verifying_key = signing_key.verifying_key();
    let encoded = verifying_key.to_sec1_point(true);
    let public_bytes = encoded.as_bytes();
    let payload = <Ripemd160 as RipemdDigest>::digest(<Sha256 as Sha2Digest>::digest(public_bytes));
    let hrp = Hrp::parse(hrp).map_err(|_| "Invalid bech32 prefix".to_string())?;
    bech32::encode::<Bech32>(hrp, &payload).map_err(|_| "Failed to encode address".to_string())
}

#[cfg(test)]
#[path = "tests/derivation.rs"]
mod tests;
