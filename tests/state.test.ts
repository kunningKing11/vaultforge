import { describe, expect, test } from "bun:test";

import { appState, emptySendDraft, resetOnboarding, walletStateFromSession } from "../src/state";
import type { FiatCurrency, WalletSession } from "../src/types";

function session(overrides: Partial<WalletSession> = {}): WalletSession {
  return {
    has_wallet: true,
    locked: false,
    wallet_name: "Test Wallet",
    addresses: { evm: "0x1234" },
    fiat_currency: "EUR",
    usd_exchange_rate: 0.92,
    assets: [],
    activity: [],
    enabled_networks: ["ethereum"],
    auto_lock_timeout_secs: 300,
    ...overrides,
  };
}

describe("wallet session transitions", () => {
  test("represents a missing wallet explicitly", () => {
    expect(walletStateFromSession(session({ has_wallet: false }))).toEqual({ status: "missing" });
  });

  test("keeps only the locked wallet name", () => {
    expect(walletStateFromSession(session({ locked: true, wallet_name: null }))).toEqual({
      status: "locked",
      name: "Wallet",
    });
  });

  test("normalizes an unlocked session and filters unknown networks", () => {
    expect(
      walletStateFromSession(
        session({
          enabled_networks: ["ethereum", "not-a-network"],
          addresses: null,
          fiat_currency: null,
          usd_exchange_rate: null,
        }),
      ),
    ).toEqual({
      status: "unlocked",
      name: "Test Wallet",
      addresses: {},
      fiatCurrency: "USD",
      usdExchangeRate: 1,
      assets: [],
      activity: [],
      enabledNetworks: ["ethereum"],
      autoLockTimeoutSecs: 300,
    });
  });
});

test("creates a fresh default send draft", () => {
  expect(emptySendDraft()).toEqual({
    to: "",
    symbol: "ETH",
    network: "ethereum",
    token_address: null,
    amount: "",
    note: "",
  });
});

test("resets the onboarding display currency to USD", () => {
  appState.onboarding.fiatCurrency = "JPY";
  resetOnboarding();
  expect(appState.onboarding.fiatCurrency as FiatCurrency).toBe("USD");
});
