export const walletPasswordStrengthLabels = [
  "Very weak",
  "Weak",
  "Fair",
  "Strong",
  "Excellent",
] as const;

export function walletPasswordScore(value: string) {
  let score = value.length >= 8 ? 1 : 0;
  if (value.length >= 12) score += 1;
  if (/[A-Z]/.test(value) && /[a-z]/.test(value)) score += 1;
  if (/\d/.test(value)) score += 1;
  if (/[^A-Za-z0-9]/.test(value)) score += 1;
  return Math.min(score, 4);
}

export function walletPasswordStrength(value: string) {
  const score = walletPasswordScore(value);
  return {
    score,
    label: walletPasswordStrengthLabels[score],
    meetsPolicy: score >= 3,
  };
}
