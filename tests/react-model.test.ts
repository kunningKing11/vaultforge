import { describe, expect, test } from "bun:test";

import { networkById } from "../src/networks";
import {
  applyWalletSession,
  assetKey,
  createInitialAppState,
  findAsset,
  receivePayload,
  sortAssetsByValue,
  walletStateFromSession,
} from "../src/react/model";
import type { Asset, WalletSession } from "../src/types";

const unlockedSession: WalletSession = {
  has_wallet: true,
  locked: false,
  wallet_name: "Vault",
  addresses: { evm: "0x1111111111111111111111111111111111111111", xrpl: "rExample" },
  fiat_currency: "USD",
  usd_exchange_rate: 1,
  assets: [],
  activity: [],
  enabled_networks: ["ethereum", "xrpl"],
  auto_lock_timeout_secs: null,
  use_crypto_symbols: false,
};

describe("React wallet session model", () => {
  test("asset keys separate networks and native assets while preserving non-EVM case", () => {
    expect(assetKey({ network: "ethereum", token_address: "0xABC" })).toBe(
      assetKey({ network: "ethereum", token_address: "0xabc" }),
    );
    expect(assetKey({ network: "ethereum", token_address: "0xabc" })).not.toBe(
      assetKey({ network: "base", token_address: "0xabc" }),
    );
    expect(assetKey({ network: "ethereum", token_address: null })).not.toBe(
      assetKey({ network: "base", token_address: null }),
    );
    expect(assetKey({ network: "ethereum", token_address: null })).not.toBe(
      assetKey({ network: "ethereum", token_address: "native" }),
    );
    for (const network of ["solana", "tron"] as const) {
      expect(assetKey({ network, token_address: "AbC" })).not.toBe(
        assetKey({ network, token_address: "abc" }),
      );
    }
  });

  test("builds an XRP Ledger receive payload from derived classic address", () => {
    const wallet = walletStateFromSession(unlockedSession);
    const xrpl = networkById("xrpl");
    if (!xrpl) throw new Error("XRP Ledger network is missing");

    expect(receivePayload(wallet, xrpl)).toBe("rExample");
  });

  test("keeps a polled terminal status when a portfolio refresh returns cached pending activity", () => {
    const activity = {
      id: "payment-1",
      kind: "send",
      title: "Sent ETH",
      subtitle: "Ethereum",
      status: "pending",
      timestamp: "2026-09-23T00:00:00Z",
      hash: "0xabc",
      network: "ethereum" as const,
    };
    const state = applyWalletSession(createInitialAppState(), {
      ...unlockedSession,
      activity: [{ ...activity, status: "confirmed" }],
    });
    const refreshed = applyWalletSession(state, {
      ...unlockedSession,
      activity: [activity],
    });
    expect(refreshed.wallet.status).toBe("unlocked");
    if (refreshed.wallet.status !== "unlocked") return;
    expect(refreshed.wallet.activity[0]?.status).toBe("confirmed");

    const differentTransaction = applyWalletSession(state, {
      ...unlockedSession,
      activity: [{ ...activity, hash: "0xdef" }],
    });
    expect(differentTransaction.wallet.status).toBe("unlocked");
    if (differentTransaction.wallet.status !== "unlocked") return;
    expect(differentTransaction.wallet.activity[0]?.status).toBe("pending");
  });

  test("keeps locked sessions free of addresses and provider data", () => {
    expect(
      walletStateFromSession({
        ...unlockedSession,
        locked: true,
      }),
    ).toEqual({ status: "locked", name: "Vault" });
  });

  test("moves receive selection to an enabled network after session ingest", () => {
    const state = createInitialAppState();
    state.receive.networkId = "solana";

    const next = applyWalletSession(state, unlockedSession);
    expect(next.wallet.status).toBe("unlocked");
    expect(next.receive.networkId).toBe("ethereum");
  });

  test("selects same-ticker assets by network and identifier instead of symbol", () => {
    const native: Asset = {
      network: "ethereum",
      symbol: "ETH",
      name: "Ether",
      balance: "0",
      decimals: 18,
      price_usd: 0,
      change_24h: 0,
      token_address: null,
    };
    const token = { ...native, token_address: "0xABC", decimals: 6 };
    const otherToken = { ...token, token_address: "0xDEF", decimals: 8 };
    const otherChain = { ...token, network: "base" as const };
    const assets = [token, otherToken, otherChain, native];
    expect(findAsset(assets, { network: "ethereum", token_address: null })).toBe(native);
    expect(findAsset(assets, { network: "ethereum", token_address: "0xabc" })).toBe(token);
    expect(findAsset(assets, { network: "ethereum", token_address: "0xdef" })).toBe(otherToken);
    expect(findAsset(assets, { network: "base", token_address: "0xabc" })).toBe(otherChain);
    expect(findAsset(assets, { network: "ethereum", token_address: "0x456" })).toBeUndefined();
    token.symbol = "renamed";
    expect(findAsset(assets, { network: "ethereum", token_address: "0xabc" })).toBe(token);
  });

  test("sorts asset displays by fiat value with stable identifier ties", () => {
    const assets = sortAssetsByValue([
      {
        symbol: "USDC",
        name: "USD Coin",
        balance: "1000000",
        decimals: 6,
        price_usd: 1,
        change_24h: 0,
        network: "base",
        token_address: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
      },
      {
        symbol: "BTC",
        name: "Bitcoin",
        balance: "200000000",
        decimals: 8,
        price_usd: 100,
        change_24h: 0,
        network: "bitcoin",
        token_address: null,
      },
      {
        symbol: "ETH",
        name: "Ethereum",
        balance: "1000000000000000000",
        decimals: 18,
        price_usd: 100,
        change_24h: 0,
        network: "ethereum",
        token_address: null,
      },
    ]);

    expect(assets.map((asset) => asset.symbol)).toEqual(["BTC", "ETH", "USDC"]);
  });
});
