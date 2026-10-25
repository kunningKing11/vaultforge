import { describe, expect, test } from "bun:test";

import { ThemeProvider } from "@coinbase/cds-web/system/ThemeProvider";
import { defaultTheme } from "@coinbase/cds-web/themes/defaultTheme";
import { renderToStaticMarkup } from "react-dom/server";

import { money } from "../src/format";
import { platformFeeDisplay, SwapPlatformFee } from "../src/react/SwapPlatformFee";
import type { ApiSwap, Asset } from "../src/types";

const input: Asset = {
  network: "solana",
  token_address: "input-mint",
  symbol: "IN",
  name: "Input",
  decimals: 6,
  balance: "0",
  price_usd: 2,
  change_24h: 0,
};
function trade(bps = 20) {
  return {
    api: {
      provider: "jupiter",
      jupiter: {
        order: {
          raw_response: {
            feeBps: bps,
            platformFee: { feeBps: bps, amount: "999999999", feeMint: "output-mint" },
          },
        },
      },
    } as ApiSwap,
    network: "solana" as const,
    token_in: input.token_address!,
    amount_base_units: "50000000",
    input_decimals: 6,
  };
}

describe("Jupiter platform fee display", () => {
  test("calculates USD directly from initial input regardless of the collection mint", () => {
    expect(platformFeeDisplay(trade(), [input])).toBe(
      `Jupiter platform fee: 0.2% (≈ ${money(0.2, "USD")} USD of initial input; included)`,
    );
    expect(platformFeeDisplay({ ...trade(), amount_base_units: "100000000" }, [input])).toContain(
      money(0.4, "USD"),
    );
  });
  test("keeps fractional base units until the USD display calculation", () => {
    const tiny = { ...trade(1), amount_base_units: "1", input_decimals: 0 };
    expect(platformFeeDisplay(tiny, [{ ...input, decimals: 0, price_usd: 10000 }])).toContain(
      money(1, "USD"),
    );
  });
  test("renders the percentage and USD amount together", () => {
    const html = renderToStaticMarkup(
      <ThemeProvider theme={defaultTheme} activeColorScheme="dark">
        <SwapPlatformFee trade={trade()} assets={[input]} />
      </ThemeProvider>,
    );
    expect(html).toContain("Jupiter platform fee: 0.2%");
    expect(html).toContain(money(0.2, "USD"));
    expect(html).toContain("USD of initial input; included");
  });
  test("shows missing rates and prices explicitly instead of inventing zero fees", () => {
    expect(
      platformFeeDisplay({ ...trade(), api: { provider: "jupiter" } as ApiSwap }, [input]),
    ).toBe("Jupiter platform fee unavailable");
    expect(platformFeeDisplay(trade(), [])).toContain("0.2% (USD estimate unavailable");
    for (const price of [0, -1, NaN, Infinity])
      expect(platformFeeDisplay(trade(), [{ ...input, price_usd: price }])).toContain(
        "USD estimate unavailable",
      );
    expect(platformFeeDisplay({ ...trade(), amount_base_units: "bad" }, [input])).toContain(
      "USD estimate unavailable",
    );
  });
  test("supports native SOL and does not substitute another asset with the same ticker", () => {
    const native = { ...input, token_address: null, decimals: 9, price_usd: 150 };
    const selected = {
      ...trade(2),
      token_in: "11111111111111111111111111111111",
      amount_base_units: "2000000000",
      input_decimals: 9,
    };
    expect(platformFeeDisplay(selected, [native])).toContain(money(0.06, "USD"));
    expect(platformFeeDisplay(trade(), [{ ...input, token_address: "another-mint" }])).toContain(
      "USD estimate unavailable",
    );
    expect(platformFeeDisplay(trade(), [{ ...input, network: "base" }])).toContain(
      "USD estimate unavailable",
    );
  });
  test("uses percent units including zero and leaves other providers unchanged", () => {
    for (const [bps, percent] of [
      [0, "0"],
      [1, "0.01"],
      [20, "0.2"],
      [100, "1"],
    ] as const)
      expect(platformFeeDisplay(trade(bps), [input])).toContain(`fee: ${percent}%`);
    for (const provider of ["lifi", "relay"] as const)
      expect(platformFeeDisplay({ ...trade(), api: { provider } as ApiSwap }, [input])).toBeNull();
  });
});
