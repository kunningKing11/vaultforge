import { readFile } from "node:fs/promises";

type Table = Record<string, unknown>;

function isTable(value: unknown): value is Table {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Find dependency names that do not follow their predecessor alphabetically. */
export function cargoDependencyOrderErrors(source: string): string[] {
  const manifest = Bun.TOML.parse(source) as Table;
  const errors: string[] = [];
  const checkTables = (table: Table, prefix: string) => {
    for (const kind of ["dependencies", "dev-dependencies", "build-dependencies"]) {
      const dependencies = table[kind];
      if (!isTable(dependencies)) continue;
      const names = Object.keys(dependencies);
      for (let index = 1; index < names.length; index++) {
        if (names[index - 1] > names[index]) {
          errors.push(`${prefix}${kind}: ${names[index]} should sort before ${names[index - 1]}`);
        }
      }
    }
  };

  checkTables(manifest, "");
  if (isTable(manifest.workspace)) checkTables(manifest.workspace, "workspace.");
  if (isTable(manifest.target)) {
    for (const [target, tables] of Object.entries(manifest.target)) {
      if (isTable(tables)) checkTables(tables, `target.${target}.`);
    }
  }
  return errors;
}

if (import.meta.main) {
  const source = await readFile(new URL("../src-tauri/Cargo.toml", import.meta.url), "utf8");
  const errors = cargoDependencyOrderErrors(source);
  if (errors.length) {
    console.error(`Cargo dependency order is incorrect:\n${errors.join("\n")}`);
    process.exitCode = 1;
  } else {
    console.log("Cargo dependency order passed.");
  }
}
