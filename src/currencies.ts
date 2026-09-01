import type { FiatCurrency } from "./types";

export const fiatCurrencies: ReadonlyArray<{
  code: FiatCurrency;
  label: string;
}> = [
  { code: "USD", label: "US Dollar" },
  { code: "EUR", label: "Euro" },
  { code: "GBP", label: "British Pound" },
  { code: "JPY", label: "Japanese Yen" },
];
