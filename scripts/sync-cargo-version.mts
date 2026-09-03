import { readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

type PackageJson = {
  version?: unknown;
};

const semverPattern: RegExp = /^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/;

function isVersion(value: unknown): value is string {
  return typeof value === "string" && semverPattern.test(value);
}

const root: string = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const packagePath: string = resolve(root, "package.json");
const cargoPath: string = resolve(root, "src-tauri", "Cargo.toml");

const packageJson: PackageJson = JSON.parse(await readFile(packagePath, "utf8")) as PackageJson;
const version: unknown = packageJson.version;

if (!isVersion(version)) {
  throw new Error(`package.json has an invalid semver version: ${String(version)}`);
}

const cargoToml: string = await readFile(cargoPath, "utf8");
const nextCargoToml: string = cargoToml.replace(
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
