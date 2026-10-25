import { Text } from "@coinbase/cds-web/typography";

import type { Asset, UniswapQuote } from "../types";
import { findAsset } from "./model";
import { swapFiatValue } from "./SwapFiatValue";
import { nativeSwapToken } from "./uniswapFlow";

type FeeTrade = Pick<
  UniswapQuote,
  "api" | "amount_base_units" | "input_decimals" | "network" | "token_in"
>;

function record(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

export function platformFeeDisplay(trade: FeeTrade, assets: readonly Asset[]): string | null {
  if (trade.api?.provider !== "jupiter") return null;
  const raw = record(trade.api.jupiter?.order.raw_response);
  const platform = record(raw?.platformFee);
  const bps = platform?.feeBps ?? (raw?.feeBps === 0 ? 0 : undefined);
  if (
    typeof bps !== "number" ||
    !Number.isInteger(bps) ||
    bps < 0 ||
    bps >= 10_000 ||
    raw?.feeBps !== bps
  )
    return "Jupiter platform fee unavailable";

  let estimate = "USD estimate unavailable";
  if (
    /^\d+$/.test(trade.amount_base_units) &&
    Number.isInteger(trade.input_decimals) &&
    trade.input_decimals >= 0 &&
    trade.input_decimals <= 255
  ) {
    const input = findAsset(assets, {
      network: trade.network,
      token_address: trade.token_in === nativeSwapToken(trade.network) ? null : trade.token_in,
    });
    // Keep fractional base units until display: bps introduces four decimal places.
    const scaledFee = (BigInt(trade.amount_base_units) * BigInt(bps)).toString();
    const value = swapFiatValue(
      scaledFee,
      input ? { ...input, decimals: trade.input_decimals + 4 } : undefined,
      "USD",
      1,
    );
    if (value !== "Price unavailable") estimate = `${value} USD of initial input`;
  }
  return `Jupiter platform fee: ${bps / 100}% (${estimate}; included)`;
}

export function SwapPlatformFee({ trade, assets }: { trade: FeeTrade; assets: readonly Asset[] }) {
  const display = platformFeeDisplay(trade, assets);
  if (display === null) return null;
  return (
    <Text font="caption" color="fgMuted">
      {display}
    </Text>
  );
}
