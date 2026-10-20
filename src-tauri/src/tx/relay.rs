//! Canonical Relay v1 EIP-712 order hashing and EVM deposit construction.
//! Schema: @relay-protocol/settlement-sdk 0.0.149 (MIT), dist/order/index.js.
use crate::operations::RelayData;
use crate::registry::NetworkConfig;
use crate::tx::evm::{EvmCall, evm_address_word};
use serde_json::Value;
use sha3::{Digest, Keccak256};

pub(crate) const NATIVE: &str = "0x0000000000000000000000000000000000000000";
// Independently fetched from Relay GET /chains; never accepted from a quote itself.
pub(crate) const DEPOSITORY: &str = "0x4cd00e387622c35bddb9b4c962c136462338bc31";
pub(crate) const SOLVER: &str = "0xf70da97812cb96acdf810712aa562db8dfa3dbef";

pub(crate) fn protocol_chain(network: &str) -> Result<&'static str, String> {
    match network {
        "bnb" => Ok("bnb"), "ethereum" => Ok("ethereum"), "base" => Ok("base"), "optimism" => Ok("optimism"),
        "arbitrum_one" => Ok("arbitrum"), "polygon" => Ok("polygon"),
        "avalanche_c" => Ok("avalanche"), "monad" => Ok("monad"),
        _ => Err("No working swap route: this network is not supported by the wallet's cross-chain executor".into()),
    }
}
pub(crate) fn network_for_protocol(chain: &str) -> Result<&'static NetworkConfig, String> {
    crate::registry::evm_networks()
        .find(|n| protocol_chain(&n.id).ok() == Some(chain))
        .ok_or_else(|| "Unsupported chain in Relay order".into())
}
pub(crate) fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    value
        .as_str()
        .ok_or_else(|| format!("Relay response is missing {field}"))
}
pub(crate) fn array(value: &Value) -> Result<&Vec<Value>, String> {
    value
        .as_array()
        .filter(|v| v.len() <= 16)
        .ok_or_else(|| "Unsupported Relay array".into())
}
pub(crate) fn integer(value: &Value) -> Result<u128, String> {
    let text = value
        .as_str()
        .ok_or("Relay amount is not a base-unit string")?;
    if text.is_empty() || !text.bytes().all(|c| c.is_ascii_digit()) {
        return Err("Invalid Relay amount".into());
    }
    text.parse().map_err(|_| "Relay amount is too large".into())
}
pub(crate) fn bytes(value: &str) -> Result<Vec<u8>, String> {
    hex::decode(value.strip_prefix("0x").ok_or("Relay hex prefix missing")?)
        .map_err(|_| "Invalid Relay hex".into())
}
fn hash(value: impl AsRef<[u8]>) -> [u8; 32] {
    Keccak256::digest(value.as_ref()).into()
}
fn word(value: &Value) -> Result<[u8; 32], String> {
    let text = value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_u64().map(|v| v.to_string()))
        .ok_or("Invalid Relay integer")?;
    let mut out = [0u8; 32];
    if text.starts_with("0x") {
        let data = bytes(&text)?;
        if data.len() > 32 {
            return Err("Relay integer exceeds uint256".into());
        }
        out[32 - data.len()..].copy_from_slice(&data);
    } else {
        if text.is_empty() || !text.bytes().all(|c| c.is_ascii_digit()) {
            return Err("Invalid Relay integer".into());
        }
        for digit in text.bytes() {
            let mut carry = u16::from(digit - b'0');
            for byte in out.iter_mut().rev() {
                let n = u16::from(*byte) * 10 + carry;
                *byte = n as u8;
                carry = n >> 8;
            }
            if carry != 0 {
                return Err("Relay integer exceeds uint256".into());
            }
        }
    }
    Ok(out)
}
// EIP-712 schema pinned to @relay-protocol/settlement-sdk 0.0.149 Order v1.
fn fields(name: &str) -> &'static [(&'static str, &'static str)] {
    match name {
        "Order" => &[
            ("version", "string"),
            ("solverChainId", "string"),
            ("solver", "address"),
            ("salt", "uint256"),
            ("inputs", "Input[]"),
            ("output", "Output"),
            ("fees", "Fee[]"),
        ],
        "Input" => &[("payment", "InputPayment"), ("refunds", "InputRefund[]")],
        "InputPayment" => &[
            ("chainId", "string"),
            ("currency", "bytes"),
            ("amount", "uint256"),
            ("weight", "uint256"),
        ],
        "InputRefund" => &[
            ("chainId", "string"),
            ("recipient", "bytes"),
            ("currency", "bytes"),
            ("minimumAmount", "uint256"),
            ("deadline", "uint32"),
            ("extraData", "bytes"),
        ],
        "Output" => &[
            ("chainId", "string"),
            ("payments", "OutputPayment[]"),
            ("deadline", "uint32"),
            ("calls", "bytes[]"),
            ("extraData", "bytes"),
        ],
        "OutputPayment" => &[
            ("recipient", "bytes"),
            ("currency", "bytes"),
            ("minimumAmount", "uint256"),
            ("expectedAmount", "uint256"),
        ],
        "Fee" => &[
            ("recipientChainId", "string"),
            ("recipient", "bytes"),
            ("currencyChainId", "string"),
            ("currency", "bytes"),
            ("amount", "uint256"),
        ],
        _ => &[],
    }
}
fn definition(name: &str) -> String {
    format!(
        "{}({})",
        name,
        fields(name)
            .iter()
            .map(|(n, t)| format!("{t} {n}"))
            .collect::<Vec<_>>()
            .join(",")
    )
}
fn struct_hash(name: &str, value: &Value) -> Result<[u8; 32], String> {
    fn dependencies(name: &str, found: &mut std::collections::BTreeSet<String>) {
        for (_, typ) in fields(name) {
            let base = typ.trim_end_matches("[]");
            if !fields(base).is_empty() && found.insert(base.into()) {
                dependencies(base, found);
            }
        }
    }
    let mut deps = std::collections::BTreeSet::new();
    dependencies(name, &mut deps);
    let mut schema = definition(name);
    for dep in deps {
        schema.push_str(&definition(&dep));
    }
    let mut encoded = hash(schema).to_vec();
    for (field, typ) in fields(name) {
        encoded.extend(typed_hash(typ, &value[field])?);
    }
    Ok(hash(encoded))
}
fn typed_hash(typ: &str, value: &Value) -> Result<[u8; 32], String> {
    if let Some(base) = typ.strip_suffix("[]") {
        let mut encoded = Vec::new();
        for item in array(value)? {
            encoded.extend(typed_hash(base, item)?);
        }
        return Ok(hash(encoded));
    }
    match typ {
        "string" => Ok(hash(string(value, "string")?)),
        "bytes" => Ok(hash(bytes(string(value, "bytes")?)?)),
        "address" => evm_address_word(string(value, "address")?),
        "uint256" => word(value),
        "uint32" => {
            let n = word(value)?;
            if n[..28] != [0; 28] {
                return Err("Relay deadline exceeds uint32".into());
            }
            Ok(n)
        }
        name => struct_hash(name, value),
    }
}
pub(crate) fn order_id(order: &Value) -> Result<String, String> {
    Ok(format!("0x{}", hex::encode(struct_hash("Order", order)?)))
}

/// Bind the on-chain deposit to the wallet's original trade; remote calldata is never signed.
pub(crate) fn deposit_call(
    relay: &RelayData,
    source: &str,
    token_in: &str,
    token_out: &str,
    sender: &str,
    amount: u128,
    minimum: u128,
    expected: u128,
) -> Result<EvmCall, String> {
    let order = &relay.order;
    let inputs = array(&order["inputs"])?;
    let outputs = array(&order["output"]["payments"])?;
    let output = &order["output"];
    if order["version"] != "v1"
        || order["solverChainId"] != "base"
        || !string(&order["solver"], "solver")?.eq_ignore_ascii_case(SOLVER)
        || inputs.len() != 1
        || outputs.len() != 1
        || !array(&order["fees"])?.is_empty()
        || !array(&output["calls"])?.is_empty()
        || order_id(order)? != relay.order_id.to_ascii_lowercase()
        || output["chainId"] != protocol_chain(&relay.destination_network)?
        || outputs[0]["recipient"]
            .as_str()
            .map(evm_address_word)
            .transpose()?
            != Some(evm_address_word(sender)?)
        || outputs[0]["currency"]
            .as_str()
            .map(evm_address_word)
            .transpose()?
            != Some(evm_address_word(token_out)?)
        || integer(&outputs[0]["minimumAmount"])? != minimum
        || integer(&outputs[0]["expectedAmount"])? != expected
    {
        return Err(
            "Relay order does not match the reviewed recipient, output, or destination".into(),
        );
    }
    let payment = &inputs[0]["payment"];
    if payment["chainId"] != protocol_chain(source)?
        || evm_address_word(string(&payment["currency"], "input currency")?)?
            != evm_address_word(token_in)?
        || integer(&payment["amount"])? != amount
        || integer(&payment["weight"])? != 1
    {
        return Err("Relay order does not match the input token or amount".into());
    }
    // All encoded addresses must be from a known EVM family; a source-chain refund
    // to this wallet must exist. No foreign refund owner is accepted.
    let mut source_refund = false;
    for refund in array(&inputs[0]["refunds"])? {
        network_for_protocol(string(&refund["chainId"], "refund chain")?)?;
        if evm_address_word(string(&refund["recipient"], "refund recipient")?)?
            != evm_address_word(sender)?
        {
            return Err("Unexpected Relay refund owner".into());
        }
        evm_address_word(string(&refund["currency"], "refund currency")?)?;
        source_refund |= refund["chainId"] == protocol_chain(source)?;
    }
    if !source_refund {
        return Err("Relay source refund instructions missing".into());
    }
    let id = bytes(&relay.order_id)?;
    if id.len() != 32 {
        return Err("Invalid Relay order ID".into());
    }
    let native = evm_address_word(token_in)? == [0; 32];
    let signature = if native {
        "depositNative(address,bytes32)"
    } else {
        "depositErc20(address,address,uint256,bytes32)"
    };
    let mut data = hash(signature)[..4].to_vec();
    data.extend(evm_address_word(sender)?);
    if !native {
        data.extend(evm_address_word(token_in)?);
        data.extend(word(&Value::String(amount.to_string()))?);
    }
    data.extend(id);
    Ok(EvmCall {
        to: DEPOSITORY.into(),
        value: if native { amount } else { 0 },
        data,
    })
}
