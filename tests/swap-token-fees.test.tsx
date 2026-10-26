import { describe, expect, test } from "bun:test";

import { ThemeProvider } from "@coinbase/cds-web/system/ThemeProvider";
import { defaultTheme } from "@coinbase/cds-web/themes/defaultTheme";
import { renderToStaticMarkup } from "react-dom/server";

import { swapTokenFeeLabels, SwapTokenFees } from "../src/react/SwapTokenFees";
import type { ApiSwap, SolanaSwapToken } from "../src/types";

const token: SolanaSwapToken = {
  mint: "mint",
  program: "Token-2022",
  decimals: 9,
  extensions: [1],
  fee_schedule: { epoch: "4", basisPoints: 25, maximumFee: "1" },
  account_len: 182,
};
function trade(provider: ApiSwap["provider"]) {
  return {
    network: "solana",
    api: {
      provider,
      solana_tokens: { input: token, output: { ...token, mint: "other", fee_schedule: null } },
      request_id: "quote",
      route: [],
      transaction: { family: "solana", transaction: "", lookups: [] },
    } as ApiSwap,
  };
}

describe("Solana swap token fees", () => {
  test("discloses transfer taxes separately for all three providers without subtracting output twice", () => {
    for (const provider of ["jupiter", "relay", "lifi"] as const) {
      expect(swapTokenFeeLabels(trade(provider))).toEqual([
        "Input token transfer fee: 0.25% (capped at 0.000000001 tokens per transfer; included in route amounts). Minimum received is the net amount credited to your wallet.",
      ]);
    }
  });
  test("keeps classic and legacy quotes compatible", () => {
    expect(swapTokenFeeLabels({ network: "solana", api: null })).toEqual([]);
    expect(swapTokenFeeLabels({ ...trade("relay"), network: "ethereum" })).toEqual([]);
  });
  test("renders fee disclosures through CDS text", () => {
    const html = renderToStaticMarkup(
      <ThemeProvider theme={defaultTheme} activeColorScheme="dark">
        <SwapTokenFees trade={trade("jupiter")} />
      </ThemeProvider>,
    );
    expect(html).toContain("Input token transfer fee: 0.25%");
    expect(html).toContain("Minimum received is the net amount");
  });
});
