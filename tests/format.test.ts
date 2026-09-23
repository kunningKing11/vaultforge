import { describe, expect, test } from "bun:test";

import {
  cryptoDisplaySymbol,
  escapeHtml,
  formatError,
  formatWei,
  shortAddress,
  toWei,
  usdToFiat,
  weiToNumber,
} from "../src/format";

describe("crypto amount formatting", () => {
  test("converts decimal amounts into integer base units", () => {
    expect(toWei("1.23", 6)).toBe("1230000");
    expect(toWei("0.000001", 6)).toBe("1");
    expect(toWei("0.12345678", 8)).toBe("12345678");
    expect(toWei("0.000000000000000001", 18)).toBe("1");
    expect(toWei("1.2300000", 6)).toBe("1230000");
    expect(toWei("0", 18)).toBe("0");
    expect(() => toWei("1.23456789", 6)).toThrow("6-decimal precision");
    expect(() => toWei("1e-8", 8)).toThrow("valid decimal amount");
  });

  test("formats base units without introducing precision", () => {
    expect(formatWei("1230000", 6)).toBe("1.23");
    expect(formatWei("1000000", 6)).toBe("1.0");
    expect(formatWei("123456789", 6, 2)).toBe("123.45");
    expect(formatWei("123456789", 6, 0)).toBe("123");
    expect(weiToNumber("1230000", 6)).toBe(1.23);
  });

  test("converts USD values using the supplied exchange rate", () => {
    expect(usdToFiat(125, 0.92)).toBe(115);
  });

  test("uses Unicode crypto symbols only when enabled and available", () => {
    expect(cryptoDisplaySymbol("BTC", "₿", true)).toBe("₿");
    expect(cryptoDisplaySymbol("BTC", "₿", false)).toBe("BTC");
    expect(cryptoDisplaySymbol("SOL", null, true)).toBe("SOL");
  });
});

describe("display-safe strings", () => {
  test("shortens addresses and handles missing values", () => {
    expect(shortAddress(null)).toBe("No address");
    expect(shortAddress("0x1234567890abcdef1234567890abcdef12345678")).toBe(
      "0x12345678...12345678",
    );
  });

  test("escapes dynamic HTML content", () => {
    expect(escapeHtml(`<script data-x="1">'&</script>`)).toBe(
      "&lt;script data-x=&quot;1&quot;&gt;&#39;&amp;&lt;/script&gt;",
    );
  });

  test("normalizes unknown errors for display", () => {
    expect(formatError(new Error("offline"))).toBe("Error: Error: offline");
    expect(formatError("offline")).toBe("Error: offline");
  });
});
