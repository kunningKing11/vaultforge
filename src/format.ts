export function toWei(amount: string, decimals: number): string {
  const parts = amount.split(".");
  const intPart = parts[0] || "0";
  const fracPart = (parts[1] || "").padEnd(decimals, "0").slice(0, decimals);
  const combined = intPart + fracPart;
  return combined.replace(/^0+/, "") || "0";
}

export function formatWei(wei: string, decimals: number, displayDecimals = 6): string {
  const padded = wei.padStart(decimals + 1, "0");
  const intPart = padded.slice(0, padded.length - decimals) || "0";
  const fracPart = padded.slice(padded.length - decimals).replace(/0+$/, "") || "0";
  if (displayDecimals === 0) return intPart;
  return `${intPart}.${fracPart.slice(0, displayDecimals)}`;
}

export function weiToNumber(wei: string, decimals: number): number {
  return Number(formatWei(wei, decimals, decimals));
}

export function money(value: number) {
  return new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 2 }).format(value);
}

export function shortAddress(address: string | null) {
  if (!address) return "No address";
  return `${address.slice(0, 10)}...${address.slice(-8)}`;
}

export function escapeHtml(value: string) {
  return value.replace(/[&<>'"]/g, (char) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", "'": "&#39;", '"': "&quot;" })[char] ?? char);
}

export function formatError(error: unknown) {
  return `Error: ${String(error)}`;
}
