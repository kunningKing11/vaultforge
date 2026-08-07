import { appState } from "./state";
import { render } from "./render";
import { normalizeNetworkId } from "./networks";
import type { View } from "./types";
import {
  setupWizard,
  unlockWallet,
  signTransaction,
  broadcastSignedTransaction,
  swapTokens,
  lockWallet,
  clearWallet,
  showLockedDeleteWallet,
  cancelLockedDeleteWallet,
  startLockedDeleteWalletCountdown,
  refreshPrices,
  copyAddress,
  copyReceiveAddress,
  copyQrPayload,
  copyText,
  updatePassphraseStrength,
} from "./commands";
import { downloadQrSvg } from "./qr";
import { walletApi } from "./walletApi";
import { formatError } from "./format";
import { pushToast } from "./toasts";
import { installScrollbarBehavior } from "./scrollbars";

export function bindEvents() {
  document.addEventListener("click", (event) => {
    const target = event.target as HTMLElement;
    const action = target.closest<HTMLElement>("[data-action]")?.dataset.action;
    const view = target.closest<HTMLElement>("[data-view]")?.dataset.view as View | undefined;

    if (view) {
      appState.currentView = view;
      render();
      return;
    }

    if (!action) return;

    if (action === "lock") void lockWallet();
    if (action === "clear-wallet") void clearWallet();
    if (action === "show-locked-delete-wallet") showLockedDeleteWallet();
    if (action === "cancel-locked-delete-wallet") cancelLockedDeleteWallet();
    if (action === "start-locked-delete-wallet-countdown") startLockedDeleteWalletCountdown();
    if (action === "refresh") void refreshPrices();
    if (action === "copy-address") void copyAddress();
    if (action === "copy-receive-address") void copyReceiveAddress();
    if (action === "copy-qr") void copyQrPayload();
    if (action === "download-qr") downloadQrSvg();
    if (action === "broadcast-signed-transaction") void broadcastSignedTransaction();
    if (action === "edit-signed-transaction") {
      appState.signedTransaction = null;
      render();
    }
    if (action === "select-activity") {
      appState.selectedActivityId =
        target.closest<HTMLElement>("[data-activity-id]")?.dataset.activityId ?? "";
      render();
    }
    if (action === "copy-value") {
      const value = target.closest<HTMLElement>("[data-copy-value]")?.dataset.copyValue;
      if (value) void copyText(value, "Value copied.");
    }

    if (action === "setup-next") wizardNext();
    if (action === "setup-prev") wizardPrev();
    if (action === "setup-create") {
      appState.setupWizard.flow = "create";
      appState.setupWizard.step = 2;
      render();
    }
    if (action === "setup-import") {
      appState.setupWizard.flow = "import";
      appState.setupWizard.step = 2;
      render();
    }
    if (action === "setup-wordcount") {
      const wc = Number(target.closest<HTMLElement>("[data-wordcount]")?.dataset.wordcount);
      if (wc) {
        appState.setupWizard.wordCount = wc as 12 | 15 | 18 | 21 | 24;
        render();
      }
    }
  });

  document.addEventListener("submit", (event) => {
    event.preventDefault();
    const form = event.target as HTMLFormElement;
    const action = form.dataset.action;

    if (action === "wallet-setup") void setupWizard();
    if (action === "unlock-wallet") void unlockWallet(form);
    if (action === "sign-transaction") void signTransaction(form);
    if (action === "swap-tokens") void swapTokens(form);
  });

  document.addEventListener("change", (event) => {
    const target = event.target as HTMLSelectElement;

    if (target.matches("[data-receive-network-id]")) {
      appState.receiveNetworkId = normalizeNetworkId(target.value);
      appState.qrSvg = "";
      appState.qrKey = "";
      render();
    }

    if (target.matches("[data-receive-resilience]")) {
      appState.qrResilience = target.value as QrResilience;
      appState.qrSvg = "";
      appState.qrKey = "";
      render();
    }

    if (target.matches("[data-send-asset]")) {
      updateRecipientPlaceholder(target.selectedOptions[0]?.dataset.symbol ?? "");
    }
  });

  document.addEventListener("input", (event) => {
    const target = event.target as HTMLInputElement;
    if (target.matches("[data-passphrase-input]")) updatePassphraseStrength(target);

    if (target.matches("[data-wizard-field]")) {
      const field = target.dataset.wizardField;
      if (field === "name") appState.setupWizard.name = target.value;
      if (field === "passphrase") appState.setupWizard.passphrase = target.value;
      if (field === "confirmPassphrase") appState.setupWizard.confirmPassphrase = target.value;
      if (field === "mnemonic") appState.setupWizard.mnemonic = target.value;
      if (field === "acknowledgedBackup") appState.setupWizard.acknowledgedBackup = target.checked;
    }
  });

  document.addEventListener("change", (event) => {
    const target = event.target as HTMLInputElement;

    if (target.matches("[data-wizard-network]")) {
      const id = target.dataset.wizardNetwork!;
      if (target.checked) {
        if (!appState.setupWizard.enabledNetworks.includes(id)) {
          appState.setupWizard.enabledNetworks.push(id);
        }
      } else {
        appState.setupWizard.enabledNetworks = appState.setupWizard.enabledNetworks.filter(
          (n) => n !== id,
        );
      }
    }

    if (target.matches("[data-wizard-autolock]")) {
      const val = target.value;
      appState.setupWizard.autoLockTimeoutSecs = val === "0" ? null : Number(val);
    }

    if (target.matches("[data-wizard-field='customWordCount']")) {
      const val = Number(target.value);
      if (val) {
        appState.setupWizard.wordCount = val as 12 | 15 | 18 | 21 | 24;
        render();
      }
    }
  });
}

async function wizardNext() {
  const wizard = appState.setupWizard;
  if (wizard.step === 2) {
    if (!wizard.passphrase || wizard.passphrase.length < 8) {
      pushToast("Passphrase must be at least 8 characters.", "error");
      return;
    }
    if (wizard.passphrase !== wizard.confirmPassphrase) {
      pushToast("Passphrases do not match.", "error");
      return;
    }
  }
  if (wizard.step === 4 && wizard.enabledNetworks.length === 0) {
    pushToast("Enable at least one network.", "error");
    return;
  }
  if (wizard.step < 5) {
    wizard.step++;
    if (wizard.step === 5 && wizard.flow === "create" && !wizard.generatedMnemonic) {
      try {
        wizard.generatedMnemonic = await walletApi.generateMnemonic(wizard.wordCount);
      } catch {
        pushToast("Failed to generate recovery phrase.", "error");
        wizard.step--;
        return;
      }
    }
    render();
  } else {
    void setupWizard();
  }
}

function wizardPrev() {
  if (appState.setupWizard.step > 1) {
    appState.setupWizard.step--;
    render();
  }
}

function startAutoLockTimer() {
  stopAutoLockTimer();
  const timeout = appState.session?.auto_lock_timeout_secs;
  if (!timeout) return;
  appState.autoLockTimer = window.setInterval(() => {
    if (Date.now() - appState.lastActivity > timeout * 1000) {
      void lockWallet();
    }
  }, 30_000);
}

function syncAutoLockTimerWithSession() {
  if (!appState.session || appState.session.locked) {
    stopAutoLockTimer();
    return;
  }
  startAutoLockTimer();
}

function stopAutoLockTimer() {
  if (appState.autoLockTimer !== null) {
    window.clearInterval(appState.autoLockTimer);
    appState.autoLockTimer = null;
  }
}

async function loadSession() {
  appState.busy = true;
  render();
  try {
    appState.session = await walletApi.getWallet();
    syncAutoLockTimerWithSession();
  } catch (error) {
    pushToast(formatError(error), "error");
  } finally {
    appState.busy = false;
    render();
  }
}

export async function boot() {
  await loadSession();
  bindEvents();
  installScrollbarBehavior();
  syncAutoLockTimerWithSession();

  document.addEventListener("click", () => {
    appState.lastActivity = Date.now();
  });
  document.addEventListener("keydown", () => {
    appState.lastActivity = Date.now();
  });
}

import type { QrResilience } from "./types";
import { updateRecipientPlaceholder } from "./render";
