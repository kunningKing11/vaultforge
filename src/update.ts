import { invoke } from "@tauri-apps/api/core";

export function updateSize(
  manifest: Record<string, unknown>,
  target: string,
  installer: string | null,
): number | null {
  const platforms = manifest.platforms;
  if (!platforms || typeof platforms !== "object" || Array.isArray(platforms)) return null;
  const entries = platforms as Record<string, unknown>;
  const entry = (installer ? entries[`${target}-${installer}`] : undefined) ?? entries[target];
  if (!entry || typeof entry !== "object" || Array.isArray(entry)) return null;
  const size = (entry as Record<string, unknown>).size;
  return typeof size === "number" && Number.isSafeInteger(size) && size > 0 ? size : null;
}

export async function updateDownloadSize(
  manifest: Record<string, unknown>,
): Promise<number | null> {
  try {
    const target = await invoke<[string, string | null] | null>("updater_target");
    return target ? updateSize(manifest, ...target) : null;
  } catch {
    return null;
  }
}
