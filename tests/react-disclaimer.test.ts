import { describe, expect, test } from "bun:test";

import { acceptDisclaimer, hasAcceptedDisclaimer } from "../src/react/disclaimer";

describe("first-run disclaimer", () => {
  test("does not treat unavailable storage as an acknowledgment", () => {
    const storage = {
      getItem: (): string | null => {
        throw new Error("Storage unavailable");
      },
      setItem: (): void => {
        throw new Error("Storage unavailable");
      },
    };

    expect(hasAcceptedDisclaimer(storage)).toBe(false);
    expect(acceptDisclaimer(storage)).toBe(false);
  });

  test("requires an acknowledgment and remembers it across visits", () => {
    const values = new Map<string, string>();
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => {
        values.set(key, value);
      },
    };

    expect(hasAcceptedDisclaimer(storage)).toBe(false);
    expect(acceptDisclaimer(storage)).toBe(true);
    expect(hasAcceptedDisclaimer(storage)).toBe(true);
  });
});
