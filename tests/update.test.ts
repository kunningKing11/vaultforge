import { expect, test } from "bun:test";

import { updateSize } from "../src/update";

test("falls back to the base target when the installer entry is absent", () => {
  expect(updateSize({ platforms: { "linux-x86_64": { size: 100 } } }, "linux-x86_64", "deb")).toBe(
    100,
  );
});

test("reads the size for the exact OS, architecture, and installer", () => {
  const manifest = {
    platforms: {
      "linux-aarch64-deb": { size: 200 },
      "linux-x86_64": { size: 300 },
      "linux-x86_64-deb": { size: 400 },
      "linux-x86_64-rpm": { size: 500 },
    },
  };
  expect(updateSize(manifest, "linux-x86_64", "deb")).toBe(400);
  expect(updateSize(manifest, "linux-aarch64", "deb")).toBe(200);
  expect(updateSize(manifest, "linux-x86_64", "rpm")).toBe(500);
});

test("returns unknown for old, missing, or invalid size metadata", () => {
  expect(updateSize({}, "linux-x86_64", "deb")).toBeNull();
  expect(updateSize({ platforms: { "linux-x86_64-deb": {} } }, "linux-x86_64", "deb")).toBeNull();
  expect(
    updateSize(
      { platforms: { "linux-x86_64-deb": { size: -1 }, "linux-x86_64": { size: 300 } } },
      "linux-x86_64",
      "deb",
    ),
  ).toBeNull();
});
