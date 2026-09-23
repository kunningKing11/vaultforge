import { describe, expect, test } from "bun:test";

import { networkById } from "../src/networks";
import {
  addressForNetwork,
  networkLabel,
  receivePayload,
  selectedActivity,
  walletStateFromSession,
} from "../src/react/model";
import type { WalletSession } from "../src/types";

function unlockWallet() {
  const session: WalletSession = {
    has_wallet: true,
    locked: false,
    wallet_name: "Test Wallet",
    addresses: {
      bitcoin: "bc1qtest",
      evm: "0x1234",
      solana: "SolanaAddress",
      tron: "TronAddress",
    },
    fiat_currency: "USD",
    usd_exchange_rate: 1,
    assets: [],
    activity: [
      {
        id: "first",
        kind: "receive",
        title: "Received",
        subtitle: "Bitcoin",
        status: "confirmed",
        timestamp: "2026-01-01T00:00:00Z",
        hash: "hash-1",
      },
      {
        id: "second",
        kind: "send",
        title: "Sent",
        subtitle: "Ethereum",
        status: "pending",
        timestamp: "2026-01-02T00:00:00Z",
        hash: "hash-2",
      },
    ],
    enabled_networks: ["bitcoin", "ethereum", "solana", "tron"],
    auto_lock_timeout_secs: null,
    use_crypto_symbols: false,
  };
  const wallet = walletStateFromSession(session);
  if (wallet.status !== "unlocked") throw new Error("Expected unlocked test wallet");
  return wallet;
}

describe("receive selectors", () => {
  test.each([
    ["bitcoin", "bitcoin:bc1qtest"],
    ["ethereum", "ethereum:0x1234@1"],
    ["solana", "solana:SolanaAddress"],
    ["tron", "TronAddress"],
  ] as const)("builds the %s receive payload", (networkId, payload) => {
    const network = networkById(networkId);
    if (!network) throw new Error(`Missing ${networkId} network`);
    expect(receivePayload(unlockWallet(), network)).toBe(payload);
  });

  test("returns no address while the wallet is locked", () => {
    const network = networkById("ethereum");
    if (!network) throw new Error("Missing Ethereum network");
    expect(addressForNetwork({ status: "locked", name: "Test Wallet" }, network)).toBe("");
  });

  test("includes an EVM chain id when requested", () => {
    const network = networkById("polygon");
    if (!network) throw new Error("Missing Polygon network");
    expect(networkLabel(network, false, true)).toBe("POL - Chain ID 137");
  });

  test("uses configured crypto symbols when the wallet preference is enabled", () => {
    const bitcoin = networkById("bitcoin");
    const solana = networkById("solana");
    if (!bitcoin || !solana) throw new Error("Missing display-symbol test network");
    expect(networkLabel(bitcoin, true)).toBe("₿");
    expect(networkLabel(solana, true)).toBe("SOL");
  });
});

describe("activity selection", () => {
  test("selects the requested activity and otherwise falls back to the first", () => {
    const wallet = unlockWallet();
    expect(selectedActivity(wallet, "second")?.id).toBe("second");
    expect(selectedActivity(wallet, "missing")?.id).toBe("first");
  });
});
