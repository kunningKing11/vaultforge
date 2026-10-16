import { expect, test } from "bun:test";

import { fitSidebarAddress } from "../src/react/SidebarAddress";

const address = "0x1234567890123456789012345678901234567890";
const measure = (text: string) => text.length * 8;

test("fits narrow widths by removing whole prefix characters", () => {
  const text = fitSidebarAddress(address, 131.5, measure);
  expect(measure(text)).toBeLessThanOrEqual(131.5);
  expect(text).toBe("0x123...34567890");
  expect(text.endsWith("...")).toBe(false);
});

test("keeps the normal prefix and suffix when enough space exists", () => {
  expect(fitSidebarAddress(address, 300, measure)).toBe("0x12345678...34567890");
});

test("preserves short addresses that fit", () => {
  expect(fitSidebarAddress("short", 100, measure)).toBe("short");
});
