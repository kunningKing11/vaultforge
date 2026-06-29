import { spawnSync } from "node:child_process";

const input = "src-tauri/icons/icon.svg";
const outputs = [
  ["512x512", "src-tauri/icons/icon.png"],
  ["256x256", "src-tauri/icons/icon.ico"],
  ["512x512", "src-tauri/icons/icon.icns"],
];

function run(command, args) {
  return spawnSync(command, args, { stdio: "inherit" });
}

function convert(args) {
  let result = run("magick", args);
  if (result.error?.code === "ENOENT") {
    result = run("convert", args);
  }

  if (result.error) {
    throw result.error;
  }

  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

for (const [size, output] of outputs) {
  convert([input, "-background", "none", "-resize", size, output]);
}
