import { Text } from "@coinbase/cds-web/typography";

import { formatWei } from "../format";
import type { ApiSwap } from "../types";

type TokenFeeTrade = { api?: ApiSwap | null; network: string };
export function swapTokenFeeLabels(trade: TokenFeeTrade): string[] {
  if (trade.network !== "solana" || !trade.api?.solana_tokens) return [];
  return (["input", "output"] as const).flatMap((side) => {
    const token = trade.api!.solana_tokens![side];
    const fee = token.fee_schedule;
    if (!fee) return [];
    return [
      `${side === "input" ? "Input" : "Output"} token transfer fee: ${fee.basisPoints / 100}% (capped at ${formatWei(fee.maximumFee, token.decimals, token.decimals)} tokens per transfer; included in route amounts). Minimum received is the net amount credited to your wallet.`,
    ];
  });
}
export function SwapTokenFees({ trade }: { trade: TokenFeeTrade }) {
  return (
    <>
      {swapTokenFeeLabels(trade).map((label) => (
        <Text key={label} font="caption" color="fgMuted">
          {label}
        </Text>
      ))}
    </>
  );
}
