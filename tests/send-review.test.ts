import { describe, expect, test } from "bun:test";

import { solanaTokenSendDetails } from "../src/react/sendReview";
import type { SignedTransaction } from "../src/types";

const signed: SignedTransaction = {
  tokenAddress: "mint",
  from: "sender",
  to: "recipient",
  symbol: "TOKEN",
  amount: "1000000",
  note: "",
  network: "solana",
  nonce: "hash",
  signedAt: "",
  payloadHash: "sig",
  signature: "sig",
  feeAmount: "5000",
  feeSymbol: "SOL",
  totalDebit: "1000000",
  postBalance: "0",
  decimals: 6,
  fiatValue: 0,
  solanaTokenSend: {
    tokenProgram: "Token-2022",
    mintExtensions: [],
    feeSchedule: { epoch: "1", basisPoints: 100, maximumFee: "10000" },
    sources: [],
    transferFee: "10000",
    recipientAmount: "990000",
    accountRentLamports: "2100000",
  },
};

describe("Solana token send review", () => {
  test("displays net recipient amount and fees without adding the tax to the debit", () => {
    const rows = solanaTokenSendDetails(signed, "TOKEN");
    expect(rows).toContainEqual({ label: "Recipient receives", value: "0.99 TOKEN" });
    expect(rows.find((row) => row.label === "Token transfer fee")?.value).toBe(
      "0.01 TOKEN (1%; capped at 0.01 TOKEN per transfer; included in amount)",
    );
    expect(rows).toContainEqual({ label: "Account rent", value: "0.0021 SOL" });
    expect(rows).toContainEqual({ label: "Total SOL cost", value: "0.002105 SOL" });
  });

  test("keeps legacy and native transfer reviews compatible", () => {
    expect(solanaTokenSendDetails({ ...signed, solanaTokenSend: undefined }, "TOKEN")).toEqual([]);
    expect(solanaTokenSendDetails({ ...signed, network: "ethereum" }, "TOKEN")).toEqual([]);
  });

  test("preserves single base-unit fees and rent in the review", () => {
    const rows = solanaTokenSendDetails(
      {
        ...signed,
        feeAmount: "1",
        decimals: 9,
        solanaTokenSend: {
          ...signed.solanaTokenSend!,
          transferFee: "1",
          recipientAmount: "999999999",
          accountRentLamports: "1",
        },
      },
      "TOKEN",
    );
    expect(rows.find((row) => row.label === "Token transfer fee")?.value).toStartWith(
      "0.000000001 TOKEN",
    );
    expect(rows).toContainEqual({ label: "Account rent", value: "0.000000001 SOL" });
    expect(rows).toContainEqual({ label: "Total SOL cost", value: "0.000000002 SOL" });
  });

  test("shows zero fees and rent for an untaxed token with an existing recipient account", () => {
    const rows = solanaTokenSendDetails(
      {
        ...signed,
        solanaTokenSend: {
          ...signed.solanaTokenSend!,
          feeSchedule: null,
          transferFee: "0",
          recipientAmount: "1000000",
          accountRentLamports: "0",
        },
      },
      "TOKEN",
    );
    expect(rows).toContainEqual({ label: "Recipient receives", value: "1.0 TOKEN" });
    expect(rows).toContainEqual({ label: "Total SOL cost", value: "0.000005 SOL" });
    expect(rows).toContainEqual({
      label: "Token transfer fee",
      value: "0.0 TOKEN (included in amount)",
    });
  });
});
