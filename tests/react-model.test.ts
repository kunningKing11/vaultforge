import { describe, expect, test } from "bun:test";

import { networkById } from "../src/networks";
import {
  applyTransactionObservations,
  applyWalletSession,
  assetKey,
  createInitialAppState,
  emptySendDraft,
  findAsset,
  isPendingTransactionActivity,
  receivePayload,
  sendAddressPlaceholder,
  sendDraftForNetwork,
  sortAssetsByValue,
  sortAssetsForSelection,
  sortNetworksForSelection,
  transactionStatusLabel,
  walletStateFromSession,
} from "../src/react/model";
import type { Activity, Asset, TxStatus, WalletSession } from "../src/types";

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

const ethereumAsset: Asset = {
  network: "ethereum",
  symbol: "ETH",
  name: "Ether",
  balance: "0",
  decimals: 18,
  price_usd: 0,
  change_24h: 0,
  token_address: null,
};

describe("React wallet session model", () => {
  test("address placeholders follow chain families rather than token tickers", () => {
    for (const id of ["ethereum", "base", "polygon", "avalanche_c"] as const) {
      expect(sendAddressPlaceholder(id)).toBe("0x...");
    }
    expect(sendAddressPlaceholder("bitcoin")).toBe("bc1... / 1... / 3...");
    expect(sendAddressPlaceholder("solana")).toBe("Solana address (base58)");
    expect(sendAddressPlaceholder("tron")).toBe("T...");
    expect(sendAddressPlaceholder("xrpl")).toBe("r...");
    expect(sendAddressPlaceholder("filecoin")).toBe("f1... / f3...");
    expect(sendAddressPlaceholder("injective")).toBe("inj1...");
    expect(sendAddressPlaceholder("zcash")).toBe("t1... / t3...");
  });
  test("applies observations and missing receipts without treating RPC errors as disappearance", () => {
    const item: Activity = {
      id: "send",
      kind: "send",
      title: "Sent",
      subtitle: "",
      status: "pending",
      timestamp: "now",
      hash: "0xabc",
      network: "ethereum",
    };
    const included: TxStatus = {
      execution: "succeeded",
      finality: "included",
      block_height: 42,
      block_hash: "0xblock",
      confirmations: null,
    };
    const updated = applyTransactionObservations(
      [item],
      [item],
      new Map([[item.id, included]]),
      new Set(),
    );
    expect(updated[0]?.tx_status).toEqual(included);
    expect(updated[0]?.status).toBe("included");
    const changed = { ...included, block_height: 43, block_hash: "0xreplacement" };
    const reIncluded = applyTransactionObservations(
      updated,
      updated,
      new Map([[item.id, changed]]),
      new Set(),
    );
    expect(reIncluded[0]?.tx_status).toEqual(changed);
    expect(reIncluded[0]?.status).toBe("included");
    expect(applyTransactionObservations(updated, updated, new Map(), new Set())).toEqual(updated);
    const disappeared = applyTransactionObservations(
      updated,
      updated,
      new Map(),
      new Set([item.id]),
    );
    expect(disappeared[0]?.status).toBe("pending");
    expect(disappeared[0]?.tx_status).toBeNull();
  });
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
  test("backend evidence overrides stale labels and observations during session merge", () => {
    const included: TxStatus = {
      execution: "succeeded",
      finality: "included",
      block_height: 42,
      block_hash: "0xnew",
      confirmations: 1,
    };
    const item: Activity = {
      id: "send",
      kind: "send",
      title: "Sent",
      subtitle: "",
      status: "confirmed",
      timestamp: "now",
      hash: "0xabc",
      network: "ethereum",
      tx_status: { ...included, finality: "finalized", block_hash: "0xold" },
    };
    const state = applyWalletSession(createInitialAppState(), {
      ...unlockedSession,
      activity: [item],
    });
    const refreshed = applyWalletSession(state, {
      ...unlockedSession,
      activity: [{ ...item, status: "confirmed", tx_status: included }],
    });
    if (refreshed.wallet.status !== "unlocked") throw new Error("Expected unlocked wallet");
    expect(refreshed.wallet.activity[0]?.status).toBe("included");
    expect(refreshed.wallet.activity[0]?.tx_status).toEqual(included);
  });
  test("bridge activity is excluded from single-transaction polling", () => {
    const activity = {
      id: "bridge",
      kind: "cctp",
      title: "Waiting for attestation",
      subtitle: "",
      amount: "1000000",
      status: "pending",
      timestamp: "2026-10-08T00:00:00Z",
      hash: "0xburn",
      network: "ethereum" as const,
    };
    expect(isPendingTransactionActivity(activity)).toBe(false);
    expect(isPendingTransactionActivity({ ...activity, kind: "send" })).toBe(true);
    expect(isPendingTransactionActivity({ ...activity, kind: "send", status: "confirmed" })).toBe(
      true,
    );
    expect(isPendingTransactionActivity({ ...activity, kind: "send", hash: "" })).toBe(false);
  });

  test("bridge activity retains backend progress instead of a polled leg result", () => {
    for (const status of ["confirmed", "failed"]) {
      const activity = {
        id: "bridge",
        kind: "cctp",
        title: "Waiting for attestation",
        subtitle: "",
        amount: "1000000",
        status: "pending",
        timestamp: "2026-10-08T00:00:00Z",
        hash: "0xburn",
        network: "ethereum" as const,
      };
      const state = applyWalletSession(createInitialAppState(), {
        ...unlockedSession,
        activity: [{ ...activity, status }],
      });
      const refreshed = applyWalletSession(state, { ...unlockedSession, activity: [activity] });
      if (refreshed.wallet.status !== "unlocked") throw new Error("Expected unlocked wallet");
      expect(refreshed.wallet.activity[0]?.status).toBe("pending");
    }
  });
  test("builds an XRP Ledger receive payload from derived classic address", () => {
    const wallet = walletStateFromSession(unlockedSession);
    const xrpl = networkById("xrpl");
    if (!xrpl) throw new Error("XRP Ledger network is missing");

    expect(receivePayload(wallet, xrpl)).toBe("rExample");
  });

  test("changing send network selects its native asset and clears incompatible fields", () => {
    const native = { ...ethereumAsset, network: "base" as const };
    const token = { ...native, symbol: "USDC", token_address: "0xabc" };
    const draft = { ...emptySendDraft(), to: "old recipient", amount: "1", destinationTag: 5 };
    expect(sendDraftForNetwork(draft, "base", [ethereumAsset, token, native])).toEqual({
      ...draft,
      network: "base",
      symbol: "ETH",
      token_address: null,
      to: "",
      amount: "",
      destinationTag: null,
    });
  });

  test("changing to a network without assets never retains another network token", () => {
    const draft = { ...emptySendDraft(), symbol: "USDC", token_address: "0xabc", amount: "1" };
    const next = sendDraftForNetwork(draft, "solana", [ethereumAsset]);
    expect(next.network).toBe("solana");
    expect(next.symbol).toBe("SOL");
    expect(next.token_address).toBeNull();
    expect(next.amount).toBe("");
  });

  test("falls back to an applicable token when no native asset is available", () => {
    const token = {
      ...ethereumAsset,
      network: "base" as const,
      symbol: "USDC",
      token_address: "0xabc",
    };
    expect(
      sendDraftForNetwork(emptySendDraft(), "base", [ethereumAsset, token]).token_address,
    ).toBe("0xabc");
  });

  test("included successes and failures keep polling while finalized observations stop", () => {
    const item: Activity = {
      id: "send",
      kind: "send",
      title: "Sent",
      subtitle: "",
      status: "confirmed",
      timestamp: "now",
      hash: "0xabc",
      network: "ethereum",
    };
    for (const execution of ["succeeded", "failed"] as const) {
      const status: TxStatus = {
        execution,
        finality: "included",
        block_height: 42,
        block_hash: null,
        confirmations: null,
      };
      expect(isPendingTransactionActivity({ ...item, tx_status: status })).toBe(true);
      expect(
        isPendingTransactionActivity({ ...item, tx_status: { ...status, finality: "finalized" } }),
      ).toBe(false);
      expect(isPendingTransactionActivity({ ...item, kind: "cctp", tx_status: status })).toBe(
        false,
      );
      expect(transactionStatusLabel(status)).toBe(
        execution === "failed" ? "failed (included)" : "included",
      );
    }
    expect(isPendingTransactionActivity(item)).toBe(true);
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

  test("poll results cannot overwrite changed identities newer observations or finalized records", () => {
    const status: TxStatus = {
      execution: "succeeded",
      finality: "included",
      block_height: 42,
      block_hash: "0xnew",
      confirmations: 1,
    };
    const item: Activity = {
      id: "send",
      kind: "send",
      title: "Sent",
      subtitle: "",
      status: "pending",
      timestamp: "now",
      hash: "0xabc",
      network: "ethereum",
    };
    for (const current of [
      { ...item, hash: "0xother" },
      { ...item, network: "base" as const },
      { ...item, kind: "cctp" },
      { ...item, tx_status: status },
      { ...item, tx_status: { ...status, finality: "finalized" as const } },
    ]) {
      expect(
        applyTransactionObservations([current], [item], new Map([[item.id, status]]), new Set()),
      ).toEqual([current]);
    }
  });

  test("preserves an applicable send asset and entered details during session refresh", () => {
    const token = { ...ethereumAsset, symbol: "USDC", token_address: "0xabc" };
    const state = createInitialAppState();
    state.send.draft = {
      ...emptySendDraft(),
      symbol: "USDC",
      token_address: "0xABC",
      amount: "2",
      to: "recipient",
    };
    const refreshed = applyWalletSession(state, {
      ...unlockedSession,
      assets: [ethereumAsset, token],
    });
    expect(refreshed.send.draft).toEqual(state.send.draft);
  });

  test("replaces a disabled send network when a session is ingested", () => {
    const state = createInitialAppState();
    state.send.draft = { ...emptySendDraft(), amount: "1", to: "old recipient" };
    const next = applyWalletSession(state, {
      ...unlockedSession,
      enabled_networks: ["base"],
      assets: [{ ...ethereumAsset, network: "base" }],
    });
    expect(next.send.draft.network).toBe("base");
    expect(next.send.draft.amount).toBe("");
    expect(next.send.draft.to).toBe("");
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

  test("sorts networks alphabetically when none have funds", () => {
    expect(
      sortNetworksForSelection(["bitcoin", "ethereum", "avalanche_c", "base"], []).map(
        (network) => network.id,
      ),
    ).toEqual(["avalanche_c", "base", "bitcoin", "ethereum"]);
  });

  test("sorts networks by funds then displayed name including token-only balances", () => {
    const ids = ["bitcoin", "ethereum", "avalanche_c", "base"] as const;
    const assets: Asset[] = [
      { ...ethereumAsset, network: "bitcoin", symbol: "BTC", balance: "1" },
      { ...ethereumAsset, network: "avalanche_c", symbol: "AVAX", balance: "0" },
      {
        ...ethereumAsset,
        network: "avalanche_c",
        symbol: "USDC",
        balance: "1",
        token_address: "0xa",
      },
      { ...ethereumAsset, network: "base", balance: "0" },
    ];
    expect(sortNetworksForSelection(ids, assets).map((network) => network.id)).toEqual([
      "avalanche_c",
      "bitcoin",
      "base",
      "ethereum",
    ]);
    expect(ids).toEqual(["bitcoin", "ethereum", "avalanche_c", "base"]);
  });

  test("sorts selection assets alphabetically within funded and unfunded groups", () => {
    const assets: Asset[] = [
      { ...ethereumAsset, symbol: "ZED", balance: "1", price_usd: 0, token_address: "0xz" },
      { ...ethereumAsset, symbol: "AAA", balance: "0", token_address: "0xa" },
      { ...ethereumAsset, symbol: "bbb", balance: "900719925474099300000", token_address: "0xb" },
      { ...ethereumAsset, symbol: "CCC", balance: "000", token_address: "0xc" },
      { ...ethereumAsset, symbol: "DDD", balance: "0", token_address: "0xd" },
    ];
    const original = [...assets];
    expect(sortAssetsForSelection(assets).map((asset) => asset.symbol)).toEqual([
      "bbb",
      "ZED",
      "AAA",
      "CCC",
      "DDD",
    ]);
    expect(assets).toEqual(original);
  });

  test("sorts selection ties by name and identity regardless of input order", () => {
    const first = { ...ethereumAsset, symbol: "USDC", name: "Alpha", token_address: "0xa" };
    const second = { ...first, token_address: "0xb" };
    const third = { ...first, name: "Beta", token_address: "0xc" };
    expect(sortAssetsForSelection([third, second, first])).toEqual([first, second, third]);
    expect(sortAssetsForSelection([second, first, third])).toEqual([first, second, third]);
  });

  test("uses backend legacy status rather than preserving a frontend terminal override", () => {
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
    expect(refreshed.wallet.activity[0]?.status).toBe("pending");

    const differentTransaction = applyWalletSession(state, {
      ...unlockedSession,
      activity: [{ ...activity, hash: "0xdef" }],
    });
    expect(differentTransaction.wallet.status).toBe("unlocked");
    if (differentTransaction.wallet.status !== "unlocked") return;
    expect(differentTransaction.wallet.activity[0]?.status).toBe("pending");
  });
});
