import { afterEach, describe, expect, test } from "bun:test";

import { networkById } from "../src/networks";
import {
  addressForNetwork,
  networkLabel,
  receivePayload,
  selectedActivity,
} from "../src/selectors";
import { appState, walletStateFromSession } from "../src/state";
import type { WalletSession } from "../src/types";

const originalWallet = appState.wallet;
const originalNetworkId = appState.receive.networkId;
const originalActivityId = appState.navigation.selectedActivityId;

function unlockWallet(): void {
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
  };
  appState.wallet = walletStateFromSession(session);
}

afterEach(() => {
  appState.wallet = originalWallet;
  appState.receive.networkId = originalNetworkId;
  appState.navigation.selectedActivityId = originalActivityId;
});

describe("receive selectors", () => {
  test.each([
    ["bitcoin", "bitcoin:bc1qtest"],
    ["ethereum", "ethereum:0x1234@1"],
    ["solana", "solana:SolanaAddress"],
    ["tron", "TronAddress"],
  ] as const)("builds the %s receive payload", (networkId, payload) => {
    unlockWallet();
    appState.receive.networkId = networkId;
    expect(receivePayload()).toBe(payload);
  });

  test("returns no address while the wallet is locked", () => {
    appState.wallet = { status: "locked", name: "Test Wallet" };
    expect(addressForNetwork(networkById("ethereum")!)).toBe("");
  });

  test("includes an EVM chain id when requested", () => {
    expect(networkLabel(networkById("polygon")!, true)).toBe("POL - Chain ID 137");
  });
});

describe("activity selection", () => {
  test("selects the requested activity and otherwise falls back to the first", () => {
    unlockWallet();
    appState.navigation.selectedActivityId = "second";
    expect(selectedActivity()?.id).toBe("second");

    appState.navigation.selectedActivityId = "missing";
    expect(selectedActivity()?.id).toBe("first");
  });
});
