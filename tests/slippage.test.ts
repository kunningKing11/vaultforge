import { describe, expect, test } from "bun:test";

import { slippageBasisPoints } from "../src/react/slippage";

describe("Slippage percentage", () => {
  test("converts presets and custom decimals into exact basis points", () => {
    for (const [percent, bps] of [
      ["0", 0],
      ["0.1", 10],
      ["0.2", 20],
      ["0.5", 50],
      ["1", 100],
      [" 1.23 ", 123],
      ["99.99", 9999],
    ] as const) {
      expect(slippageBasisPoints(percent)).toBe(bps);
    }
  });
  test("rejects missing fractional excessive and unsupported input", () => {
    for (const percent of [
      "",
      "-1",
      "100",
      "100.01",
      "0.001",
      "NaN",
      "Infinity",
      "1e2",
      "0x10",
      "1,5",
      "1.",
    ]) {
      expect(() => slippageBasisPoints(percent)).toThrow();
    }
  });
});
