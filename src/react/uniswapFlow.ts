import type { UniswapOperation, UniswapPreparedStep, UniswapQuote } from "../types";

/** A changed next step needs a new fee review, even if its price terms match. */
export function canSubmitFromQuote(quote: UniswapQuote, prepared: UniswapPreparedStep): boolean {
  const budget = prepared.step === "approval" ? quote.approval_max_fee : quote.swap_max_fee;
  if (prepared.network !== quote.network || budget === null) return false;
  try {
    return (
      BigInt(prepared.max_native_fee) > 0n && BigInt(prepared.max_native_fee) <= BigInt(budget)
    );
  } catch {
    return false;
  }
}

/** Provider preference survives refreshes; a new pair starts with the best output. */
export function selectComparedQuote(
  quotes: UniswapQuote[],
  preferred: string | null,
): UniswapQuote | null {
  return quotes.find((quote) => quoteProvider(quote) === preferred) ?? quotes[0] ?? null;
}

export function quoteProvider(quote: UniswapQuote): string {
  if (quote.api?.provider === "jupiter") return "Jupiter";
  return quote.api?.provider === "lifi"
    ? "LI.FI"
    : quote.relay || quote.api?.provider === "relay"
      ? "Relay"
      : "Uniswap V3";
}

/** Native identifiers belong to their chain family, not a token ticker. */
export function nativeSwapToken(network: string): string {
  return network === "solana"
    ? "11111111111111111111111111111111"
    : "0x0000000000000000000000000000000000000000";
}

/** Prioritize an active swap; keep completed receipts polling in the background. */
export function selectSwapToSync(
  operations: Pick<UniswapOperation, "id" | "needs_sync" | "terminal">[],
  cursor: number,
): string | null {
  const pending = operations.filter((operation) => operation.needs_sync);
  const active = pending.filter((operation) => !operation.terminal);
  const candidates = active.length ? active : pending;
  return candidates[cursor % candidates.length]?.id ?? null;
}
