const BIP39_WORD_COUNTS = new Set([12, 15, 18, 21, 24]);

export function hasValidRecoveryPhraseWordCount(mnemonic: string): boolean {
  const normalized = mnemonic.trim();
  const wordCount = normalized === "" ? 0 : normalized.split(/\s+/).length;
  return BIP39_WORD_COUNTS.has(wordCount);
}
