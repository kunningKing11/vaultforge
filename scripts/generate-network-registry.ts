import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";

import { getNetworkRegistry } from "../src/networks";

const output = process.argv[2];
if (!output) throw new Error("Expected an output path for the normalized network registry");

const outputPath = resolve(output);
mkdirSync(dirname(outputPath), { recursive: true });
writeFileSync(outputPath, `${JSON.stringify(getNetworkRegistry(), null, 2)}\n`);
