import { describe, expect, test } from "bun:test";

import {
  hasValidRecoveryPhraseWordCount,
  sanitizeRecoveryPhraseInput,
} from "../src/recoveryPhrase";

describe("recovery phrase input", () => {
  test("converts uppercase letters to lowercase", () => {
    expect(sanitizeRecoveryPhraseInput("AbAnDoN ABOUT")).toBe("abandon about");
  });

  test("drops nonletter characters", () => {
    expect(sanitizeRecoveryPhraseInput("ab4andon! @about.2")).toBe("abandon about");
  });

  test("normalizes pasted whitespace to single spaces", () => {
    expect(sanitizeRecoveryPhraseInput("  ABANDON  \n\tAbout   ")).toBe("abandon about ");
  });
});

describe("recovery phrase word counts", () => {
  test.each([12, 15, 18, 21, 24])("accepts %i words", (wordCount) => {
    expect(hasValidRecoveryPhraseWordCount(Array(wordCount).fill("word").join(" "))).toBeTrue();
  });

  test.each([0, 1, 11, 13, 23, 25])("rejects %i words", (wordCount) => {
    expect(hasValidRecoveryPhraseWordCount(Array(wordCount).fill("word").join(" "))).toBeFalse();
  });

  test("normalizes surrounding and repeated whitespace", () => {
    const phrase = `  ${Array(12).fill("word").join("  \n\t")}  `;
    expect(hasValidRecoveryPhraseWordCount(phrase)).toBeTrue();
  });
});
