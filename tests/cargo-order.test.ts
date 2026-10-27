import { expect, test } from "bun:test";

import { cargoDependencyOrderErrors } from "../scripts/lint-cargo-order.mjs";

test("accepts sorted dependency tables with comments and multiline values", () => {
  expect(
    cargoDependencyOrderErrors(`
[package]
z = "metadata"
a = "metadata"
[dependencies]
# This comment belongs to alpha.
alpha = { version = "1", features = [
  "z",
  "a",
] }
zeta = "1"
[target.'cfg(unix)'.dev-dependencies]
alpha = "1"
zeta = "1"
`),
  ).toEqual([]);
});

test("reports out-of-order root and target dependency names", () => {
  expect(
    cargoDependencyOrderErrors(`
[build-dependencies]
zeta = "1"
alpha = "1"
[workspace.dependencies]
zeta = "1"
alpha = "1"
[target.'cfg(windows)'.dependencies]
zeta = "1"
alpha = "1"
`),
  ).toEqual([
    "build-dependencies: alpha should sort before zeta",
    "workspace.dependencies: alpha should sort before zeta",
    "target.cfg(windows).dependencies: alpha should sort before zeta",
  ]);
});
