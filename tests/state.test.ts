import { describe, expect, test } from "bun:test";

import { createSetupWizardState, emptySendDraft, walletStateFromSession } from "../src/react/model";
import type { WalletSession } from "../src/types";

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
    use_crypto_symbols: true,
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
      useCryptoSymbols: true,
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
    destinationTag: null,
  });
});

test("creates onboarding defaults without cross-session mutable state", () => {
  const onboarding = createSetupWizardState();
  expect(onboarding.fiatCurrency).toBe("USD");
  expect(onboarding.useCryptoSymbols).toBeFalse();
});
