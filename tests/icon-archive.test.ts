import { expect, test } from "bun:test";

import { strToU8, zipSync } from "fflate";

import { extractIconArchiveMember } from "../scripts/icon-archive";

test("extracts only the requested archive member", () => {
  const archive = zipSync({
    "Token Logo/USDC Token.svg": strToU8("icon"),
    "other.txt": strToU8("other"),
  });
  expect(
    new TextDecoder().decode(extractIconArchiveMember(archive, "Token Logo/USDC Token.svg")),
  ).toBe("icon");
});

test("rejects a missing archive member", () => {
  expect(() =>
    extractIconArchiveMember(zipSync({ "other.txt": strToU8("other") }), "icon.svg"),
  ).toThrow();
});
