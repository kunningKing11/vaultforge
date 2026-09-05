import { isTauri } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";

import { formatError } from "./format";
import { render } from "./render";
import { appState } from "./state";
import { pushToast } from "./toasts";

export async function checkForUpdates() {
  if (!isTauri()) {
    pushToast("Update checks are available in the desktop app.", "info");
    return;
  }

  if (appState.operation.busy) return;

  appState.operation.busy = true;
  render();

  try {
    const update = await check();
    if (!update) {
      pushToast("VaultForge is up to date.", "info");
      return;
    }

    const notes = update.body ? `\n\nRelease notes:\n${update.body}` : "";
    if (
      !window.confirm(
        `VaultForge ${update.version} is available.${notes}\n\nInstall and restart now?`,
      )
    ) {
      return;
    }

    pushToast(`Downloading VaultForge ${update.version}…`, "info");
    await update.downloadAndInstall((event) => {
      if (event.event === "Finished") {
        pushToast("Update downloaded. Installing…", "info");
      }
    });
    await relaunch();
  } catch (error) {
    pushToast(formatError(error), "error");
  } finally {
    appState.operation.busy = false;
    render();
  }
}
