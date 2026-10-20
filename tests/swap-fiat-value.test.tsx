import { describe, expect, test } from "bun:test";

import { money } from "../src/format";
import { swapFiatValue } from "../src/react/SwapFiatValue";
import type { Asset } from "../src/types";

const asset: Asset = {
  network: "base",
  token_address: null,
  symbol: "ETH",
  name: "Ether",
  balance: "0",
  decimals: 18,
  price_usd: 2000,
  change_24h: 0,
};
describe("Swap wallet-currency values", () => {
  test("converts base units using the selected currency and exchange rate", () => {
    expect(swapFiatValue("1000000000000000000", asset, "EUR", 0.8)).toBe(`≈ ${money(1600, "EUR")}`);
    expect(swapFiatValue("1000000000000000", asset, "GBP", 0.75)).toBe(`≈ ${money(1.5, "GBP")}`);
    expect(swapFiatValue("0", asset, "JPY", 150)).toBe(`≈ ${money(0, "JPY")}`);
  });
  test("does not invent a value for missing or invalid prices", () => {
    expect(swapFiatValue("100", undefined, "USD", 1)).toBe("Price unavailable");
    for (const price of [0, -1, Infinity, NaN])
      expect(swapFiatValue("100", { ...asset, price_usd: price }, "USD", 1)).toBe(
        "Price unavailable",
      );
    for (const rate of [0, -1, Infinity, NaN])
      expect(swapFiatValue("100", asset, "USD", rate)).toBe("Price unavailable");
    expect(swapFiatValue("invalid", asset, "USD", 1)).toBe("Price unavailable");
  });
});
