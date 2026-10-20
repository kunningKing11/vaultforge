/** Convert a percentage to basis points without floating-point rounding. */
export function slippageBasisPoints(percent: string): number {
  const match = /^(\d+)(?:\.(\d{1,2}))?$/.exec(percent.trim());
  if (!match) throw new Error("Enter slippage as a percentage with up to two decimal places.");
  const bps = BigInt(match[1]) * 100n + BigInt((match[2] ?? "").padEnd(2, "0"));
  if (bps >= 10000n) throw new Error("Slippage must be at least 0% and less than 100%.");
  return Number(bps);
}
