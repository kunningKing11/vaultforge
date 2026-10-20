import type { UniswapPreparedStep, UniswapQuote } from "../types";

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
  return quote.relay ? "Relay" : "Uniswap V3";
}
