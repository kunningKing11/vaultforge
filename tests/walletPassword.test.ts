import { describe, expect, test } from "bun:test";

import { walletPasswordScore, walletPasswordStrength } from "../src/walletPassword";

describe("wallet password strength", () => {
  test.each([
    ["", 0],
    ["password", 1],
    ["longpassword", 2],
    ["LongPassword", 3],
    ["LongPassword1", 4],
    ["LongPassword1!", 4],
  ] as const)("scores %s", (password, score) => {
    expect(walletPasswordScore(password)).toBe(score);
  });

  test("applies the frontend password policy threshold", () => {
    expect(walletPasswordStrength("password")).toEqual({
      score: 1,
      label: "Weak",
      meetsPolicy: false,
    });
    expect(walletPasswordStrength("LongPassword")).toEqual({
      score: 3,
      label: "Strong",
      meetsPolicy: true,
    });
  });
});
