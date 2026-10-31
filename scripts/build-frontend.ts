import { cp, rm } from "node:fs/promises";

export async function buildFrontend(output: string): Promise<void> {
  await rm(output, { recursive: true, force: true });
  const build = Bun.spawn(
    [process.execPath, "build", "index.html", "--production", "--outdir", output],
    {
      env: { ...process.env, NODE_ENV: "production" },
      stdin: "inherit",
      stdout: "inherit",
      stderr: "inherit",
    },
  );
  if ((await build.exited) !== 0) throw new Error("Frontend build failed");
  await cp("public", output, { recursive: true });
}

if (import.meta.main) await buildFrontend("dist");
