import { formatWei } from "../format";
import type { SignedTransaction } from "../types";

/** Reviewed integer amounts come from Rust; the UI only formats them. */
export function solanaTokenSendDetails(
  signed: SignedTransaction,
  symbol: string,
): { label: string; value: string }[] {
  const review = signed.solanaTokenSend;
  if (signed.network !== "solana" || !review) return [];
  const schedule = review.feeSchedule;
  const feeTerms = schedule
    ? ` (${schedule.basisPoints / 100}%; capped at ${formatWei(schedule.maximumFee, signed.decimals, signed.decimals)} ${symbol} per transfer; included in amount)`
    : " (included in amount)";
  return [
    {
      label: "Recipient receives",
      value: `${formatWei(review.recipientAmount, signed.decimals, signed.decimals)} ${symbol}`,
    },
    {
      label: "Token transfer fee",
      value: `${formatWei(review.transferFee, signed.decimals, signed.decimals)} ${symbol}${feeTerms}`,
    },
    { label: "Account rent", value: `${formatWei(review.accountRentLamports, 9, 9)} SOL` },
    {
      label: "Total SOL cost",
      value: `${formatWei((BigInt(signed.feeAmount) + BigInt(review.accountRentLamports)).toString(), 9, 9)} SOL`,
    },
  ];
}
