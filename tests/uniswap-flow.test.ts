import { describe, expect, test } from "bun:test";

import {
  canSubmitFromQuote,
  nativeSwapToken,
  quoteProvider,
  selectComparedQuote,
  selectSwapToSync,
} from "../src/react/uniswapFlow";
import type { UniswapPreparedStep, UniswapQuote } from "../src/types";

const quote: UniswapQuote = {
  destination_network: "base",
  relay: null,
  route: [],
  id: "quote",
  network: "base",
  token_in: "0xinput",
  token_out: "0xoutput",
  sender: "0xwallet",
  amount_base_units: "1000000",
  quoted_output: "2000000",
  minimum_received: "1990000",
  slippage_bps: 50,
  pool_fee: 500,
  deadline: 1800000000,
  approval_required: true,
  approval_max_fee: "42000",
  swap_max_fee: null,
  input_symbol: "USDC",
  output_symbol: "OUT",
  input_decimals: 6,
  output_decimals: 6,
};
const prepared: UniswapPreparedStep = {
  operation_id: "operation",
  step: "approval",
  tx_hash: "0xhash",
  network: "base",
  max_native_fee: "42000",
};

describe("Additional swap providers", () => {
  test("labels LI.FI and native Relay quotes and retains their selection", () => {
    const lifi = {
      ...quote,
      id: "lifi",
      api: { provider: "lifi" } as NonNullable<UniswapQuote["api"]>,
    };
    const relay = {
      ...quote,
      id: "svm-relay",
      api: { provider: "relay" } as NonNullable<UniswapQuote["api"]>,
    };
    expect(quoteProvider(lifi)).toBe("LI.FI");
    expect(quoteProvider(relay)).toBe("Relay");
    expect(selectComparedQuote([relay, lifi], "LI.FI")?.id).toBe("lifi");
    expect(selectComparedQuote([lifi], "Relay")?.id).toBe("lifi");
  });
  test("uses family-specific native asset identifiers", () => {
    expect(nativeSwapToken("base")).toBe("0x0000000000000000000000000000000000000000");
    expect(nativeSwapToken("solana")).toBe("11111111111111111111111111111111");
  });
});

describe("Swap route selection", () => {
  const relay = { ...quote, id: "relay", relay: {} as NonNullable<UniswapQuote["relay"]> };
  test("defaults to the highest ranked output and handles no routes", () => {
    expect(selectComparedQuote([quote, relay], null)?.id).toBe("quote");
    expect(selectComparedQuote([relay, quote], null)?.id).toBe("relay");
    expect(selectComparedQuote([], null)).toBeNull();
  });
  test("falls back to the available route when the preferred provider fails", () => {
    expect(selectComparedQuote([relay], "Uniswap V3")?.id).toBe("relay");
    expect(selectComparedQuote([quote], "Relay")?.id).toBe("quote");
  });
  test("retains an explicitly selected provider across refreshed quote ids", () => {
    const refreshed = { ...quote, id: "new quote" };
    expect(selectComparedQuote([relay, refreshed], "Uniswap V3")?.id).toBe("new quote");
    expect(selectComparedQuote([refreshed, relay], "Relay")?.id).toBe("relay");
  });
});

describe("Swap status polling", () => {
  test("continues finality polling for completed swaps without blocking an active swap", () => {
    const done = { id: "done", terminal: true, needs_sync: true };
    const active = { id: "active", terminal: false, needs_sync: true };
    expect(selectSwapToSync([done], 0)).toBe("done");
    expect(selectSwapToSync([done, active], 0)).toBe("active");
    expect(selectSwapToSync([done, active], 1)).toBe("active");
  });
  test("rotates background receipts and stops after finality", () => {
    const first = { id: "first", terminal: true, needs_sync: true };
    const second = { ...first, id: "second" };
    expect(selectSwapToSync([first, second], 0)).toBe("first");
    expect(selectSwapToSync([first, second], 1)).toBe("second");
    expect(selectSwapToSync([{ ...first, needs_sync: false }], 0)).toBeNull();
    expect(selectSwapToSync([], 0)).toBeNull();
  });
});

describe("Uniswap fee consent", () => {
  test("allows the reviewed step within its exact integer budget", () => {
    expect(canSubmitFromQuote(quote, prepared)).toBe(true);
    expect(canSubmitFromQuote(quote, { ...prepared, max_native_fee: "41999" })).toBe(true);
    const large = "100000000000000000000001";
    expect(
      canSubmitFromQuote(
        { ...quote, approval_max_fee: large },
        { ...prepared, max_native_fee: large },
      ),
    ).toBe(true);
    expect(
      canSubmitFromQuote(
        { ...quote, approval_max_fee: large },
        { ...prepared, max_native_fee: "100000000000000000000002" },
      ),
    ).toBe(false);
  });
  test("requires a new fee review when approval becomes unnecessary", () => {
    expect(canSubmitFromQuote(quote, { ...prepared, step: "swap" })).toBe(false);
    expect(
      canSubmitFromQuote(
        { ...quote, approval_required: false, swap_max_fee: "42000" },
        { ...prepared, step: "swap" },
      ),
    ).toBe(true);
  });
  test("requires review for a different network or an unquoted approval", () => {
    expect(canSubmitFromQuote(quote, { ...prepared, network: "ethereum" })).toBe(false);
    expect(canSubmitFromQuote({ ...quote, approval_max_fee: null }, prepared)).toBe(false);
  });
  test("stops automatic submission for invalid zero or excessive fees", () => {
    for (const fee of ["0", "-1", "invalid", "42001"]) {
      expect(canSubmitFromQuote(quote, { ...prepared, max_native_fee: fee })).toBe(false);
    }
  });
});
