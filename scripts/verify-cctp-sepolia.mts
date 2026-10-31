import assert from "node:assert/strict";

// Read-only proof for the wallet's pinned Ethereum Sepolia -> Arbitrum Sepolia
// route. Run after the UI restart test; no wallet file, password or key is read.
const SOURCE_RPC = "https://ethereum-sepolia-rpc.publicnode.com";
const DESTINATION_RPC = "https://sepolia-rollup.arbitrum.io/rpc";
const MESSENGER = "0x8fe6b999dc680ccfdd5bf7eb0974218be2542daa";
const TRANSMITTER = "0xe737e5cebeeba77efe34d4aa090756590b1ce275";
const SOURCE_USDC = "0x1c7d4b196cb0c7b01d743fbc6116a902379c7238";
const DESTINATION_USDC = "0x75faf114eafb1bdbe2f0316df893fd58ce46aa4d";
const MESSAGE_SENT = "0x8c5261668696ce22758910d05bab8f186d6eb247ceac2af2e82c7dc17669b036";
const BURN = "0x0c8c1cbdc5190613ebd485511d4e2812cfa45eecb79d845893331fedad5130a5";
const MESSAGE_RECEIVED = "0xff48c13eda96b1cceacc6b9edeedc9e9db9d6226afbc30146b720c19d3addb1c";
const MINT = "0x50c55e915134d457debfa58eb6f4342956f8b0616d51a89a3659360178e1ab63";

type JsonObject = Record<string, unknown>;
export type RpcRead = (url: string, method: string, params: unknown[]) => Promise<unknown>;
const object = (value: unknown): JsonObject => {
  assert(value && typeof value === "object" && !Array.isArray(value), "Expected JSON object");
  return value as JsonObject;
};
const hex = (value: unknown): string => {
  assert(typeof value === "string" && /^0x(?:[a-f\d]{2})*$/iu.test(value), "Invalid hex bytes");
  return value.toLowerCase();
};
const quantity = (value: unknown): bigint => {
  assert(typeof value === "string" && /^0x[a-f\d]+$/iu.test(value), "Invalid RPC quantity");
  return BigInt(value);
};
const word = (value: bigint): string => value.toString(16).padStart(64, "0");
const addressWord = (address: string): string => address.slice(2).padStart(64, "0");
const slice = (bytes: string, offset: number, length: number): string =>
  bytes.slice(2 + offset * 2, 2 + (offset + length) * 2);
const uint = (bytes: string, offset: number, length = 32): bigint =>
  BigInt(`0x${slice(bytes, offset, length)}`);
const equal = (actual: unknown, expected: string, label: string) =>
  assert.equal(hex(actual), expected.toLowerCase(), label);
const rpcRead: RpcRead = async (url, method, params) => {
  const response = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
    signal: AbortSignal.timeout(15000),
  });
  assert(response.ok, `${method}: HTTP ${response.status}`);
  const json = object(await response.json());
  assert(!json.error, `${method}: ${JSON.stringify(json.error)}`);
  assert("result" in json, `${method}: missing result`);
  return json.result;
};
function event(receipt: JsonObject, contract: string, topic: string): JsonObject {
  assert(Array.isArray(receipt.logs), "Receipt logs missing");
  const matches = receipt.logs
    .map(object)
    .filter(
      (log) =>
        hex(log.address) === contract && Array.isArray(log.topics) && hex(log.topics[0]) === topic,
    );
  assert.equal(matches.length, 1, "Unique matching event required");
  assert.notEqual(matches[0].removed, true, "Removed log");
  return matches[0];
}
async function finalized(url: string, hash: string, read: RpcRead): Promise<JsonObject> {
  const receipt = object(await read(url, "eth_getTransactionReceipt", [hash]));
  equal(receipt.transactionHash, hash, "Receipt hash");
  assert.equal(quantity(receipt.status), 1n, "Execution failed");
  const head = object(await read(url, "eth_getBlockByNumber", ["finalized", false]));
  assert(quantity(head.number) >= quantity(receipt.blockNumber), "Receipt is not finalized yet");
  const block = object(await read(url, "eth_getBlockByNumber", [receipt.blockNumber, false]));
  equal(block.hash, hex(receipt.blockHash), "Receipt is not on canonical chain");
  assert.equal(quantity(block.number), quantity(receipt.blockNumber));
  return receipt;
}

export async function verifyCctpSepolia(
  burnHash: string,
  mintHash: string,
  read: RpcRead = rpcRead,
  fetchAttestation: (hash: string) => Promise<unknown> = async (hash) => {
    const response = await fetch(
      `https://iris-api-sandbox.circle.com/v2/messages/0?transactionHash=${encodeURIComponent(hash)}`,
      { signal: AbortSignal.timeout(15000) },
    );
    assert(response.ok, `Circle attestation HTTP ${response.status}`);
    return response.json();
  },
) {
  assert(
    /^0x[a-f\d]{64}$/iu.test(burnHash) && /^0x[a-f\d]{64}$/iu.test(mintHash),
    "Pass real 32-byte transaction hashes",
  );
  assert.equal(quantity(await read(SOURCE_RPC, "eth_chainId", [])), 11155111n);
  assert.equal(quantity(await read(DESTINATION_RPC, "eth_chainId", [])), 421614n);
  const source = await finalized(SOURCE_RPC, burnHash, read);
  const destination = await finalized(DESTINATION_RPC, mintHash, read);
  const sent = hex(event(source, TRANSMITTER, MESSAGE_SENT).data);
  assert.equal(uint(sent, 0), 32n);
  assert.equal(uint(sent, 32), 376n);
  assert.equal(sent.length, 2 + 448 * 2);
  assert.equal(slice(sent, 440, 8), "00".repeat(8));
  const emitted = `0x${slice(sent, 64, 376)}`;
  assert.equal(uint(emitted, 0, 4), 1n);
  assert.equal(uint(emitted, 4, 4), 0n);
  assert.equal(uint(emitted, 8, 4), 3n);
  assert.equal(slice(emitted, 44, 32), addressWord(MESSENGER));
  assert.equal(slice(emitted, 76, 32), addressWord(MESSENGER));
  assert.equal(uint(emitted, 140, 4), 2000n);
  assert.equal(uint(emitted, 148, 4), 1n);
  assert.equal(slice(emitted, 152, 32), addressWord(SOURCE_USDC));
  assert.equal(uint(emitted, 280), 0n);
  const api = object(await fetchAttestation(burnHash));
  equal(api.sourceTxHash, burnHash, "Circle source hash");
  assert(Array.isArray(api.messages), "Circle messages missing");
  const matching = api.messages.map(object).filter((entry) => {
    if (entry.status !== "complete") return false;
    const message = hex(entry.message);
    if (message.length !== 2 + 376 * 2) return false;
    return [
      [0, 12],
      [44, 100],
      [148, 164],
    ].every(
      ([offset, length]) => slice(message, offset, length) === slice(emitted, offset, length),
    );
  });
  assert.equal(matching.length, 1, "Unique bound Circle attestation required");
  const message = hex(matching[0].message);
  const attestation = hex(matching[0].attestation);
  assert(attestation.length > 2 && ((attestation.length - 2) / 2) % 65 === 0);
  const nonce = slice(message, 12, 32);
  assert.notEqual(nonce, "00".repeat(32));
  assert(uint(message, 144, 4) >= 2000n);
  assert.equal(uint(message, 312), 0n);
  const amount = uint(message, 216);
  const recipient = slice(message, 184, 32);
  const sender = slice(message, 248, 32);
  assert.equal(slice(message, 108, 32), sender, "Destination caller is source wallet");
  const deposit = event(source, MESSENGER, BURN);
  assert(Array.isArray(deposit.topics) && deposit.topics.length === 4);
  equal(deposit.topics[1], `0x${addressWord(SOURCE_USDC)}`, "Burn token");
  equal(deposit.topics[2], `0x${sender}`, "Depositor");
  equal(deposit.topics[3], `0x${word(2000n)}`, "Burn finality");
  const depositData = hex(deposit.data);
  assert.equal(
    depositData,
    `0x${word(amount)}${recipient}${word(3n)}${addressWord(MESSENGER)}${sender}${word(0n)}${word(224n)}${word(0n)}`,
  );
  const received = event(destination, TRANSMITTER, MESSAGE_RECEIVED);
  assert(Array.isArray(received.topics) && received.topics.length === 4);
  equal(received.topics[1], `0x${sender}`, "Mint caller");
  equal(received.topics[2], `0x${nonce}`, "Received nonce");
  equal(received.topics[3], `0x${word(uint(message, 144, 4))}`, "Executed finality");
  equal(
    received.data,
    `0x${word(0n)}${addressWord(MESSENGER)}${word(96n)}${word(228n)}${slice(message, 148, 228)}${"00".repeat(28)}`,
    "Received message body",
  );
  const minted = event(destination, MESSENGER, MINT);
  assert(Array.isArray(minted.topics) && minted.topics.length === 3);
  equal(minted.topics[1], `0x${recipient}`, "Mint recipient");
  equal(minted.topics[2], `0x${addressWord(DESTINATION_USDC)}`, "Mint token");
  equal(minted.data, `0x${word(amount)}${word(0n)}`, "Net receipt and protocol fee");
  assert.equal(
    quantity(
      await read(DESTINATION_RPC, "eth_call", [
        { to: TRANSMITTER, data: `0xfeb61724${nonce}` },
        "finalized",
      ]),
    ),
    1n,
  );
  return {
    environment: "Ethereum Sepolia -> Arbitrum Sepolia",
    burn_hash: burnHash.toLowerCase(),
    mint_hash: mintHash.toLowerCase(),
    source_block: quantity(source.blockNumber).toString(),
    destination_block: quantity(destination.blockNumber).toString(),
    nonce: `0x${nonce}`,
    recipient: `0x${recipient.slice(24)}`,
    amount_base_units: amount.toString(),
    protocol_fee_base_units: "0",
    source_finalized: true,
    destination_finalized: true,
    bound_circle_attestation: true,
    message,
    attestation,
    restart_proof:
      "Record the same UI operation ID and burn hash before and after restarting; this read-only chain check cannot prove an application restart.",
  };
}

if (import.meta.main) {
  const args = Bun.argv.slice(2);
  const burn = args[args.indexOf("--burn") + 1];
  const mint = args[args.indexOf("--mint") + 1];
  if (!args.includes("--burn") || !args.includes("--mint") || !burn || !mint) {
    console.error(
      "Usage: bun scripts/verify-cctp-sepolia.mts --burn 0xSOURCE_HASH --mint 0xDESTINATION_HASH",
    );
    process.exitCode = 2;
  } else {
    try {
      console.log(JSON.stringify(await verifyCctpSepolia(burn, mint), null, 2));
    } catch (error) {
      console.error(error instanceof Error ? error.message : error);
      process.exitCode = 1;
    }
  }
}
