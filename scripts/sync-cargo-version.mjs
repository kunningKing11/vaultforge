import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const packagePath = resolve(root, "package.json");
const cargoPath = resolve(root, "src-tauri", "Cargo.toml");

const packageJson = JSON.parse(await readFile(packagePath, "utf8"));
const version = packageJson.version;

if (typeof version !== "string" || !/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
  throw new Error(`package.json has an invalid semver version: ${String(version)}`);
}

const cargoToml = await readFile(cargoPath, "utf8");
const nextCargoToml = cargoToml.replace(
  /(^\[package\][\s\S]*?^version\s*=\s*")[^"]+("\s*$)/m,
  `$1${version}$2`,
);

if (nextCargoToml === cargoToml && !cargoToml.includes(`version = "${version}"`)) {
  throw new Error("Could not update src-tauri/Cargo.toml package version");
}

if (nextCargoToml !== cargoToml) {
  await writeFile(cargoPath, nextCargoToml);
  console.log(`Synced src-tauri/Cargo.toml package version to ${version}`);
}
