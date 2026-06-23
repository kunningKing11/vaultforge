import { appState } from "./state";
import { render } from "./render";
import { normalizeNetworkId } from "./networks";
import type { View } from "./types";
import {
  createWallet,
  importWallet,
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
      appState.selectedActivityId = target.closest<HTMLElement>("[data-activity-id]")?.dataset.activityId ?? "";
      render();
    }
    if (action === "copy-value") {
      const value = target.closest<HTMLElement>("[data-copy-value]")?.dataset.copyValue;
      if (value) void copyText(value, "Value copied.");
    }
  });

  document.addEventListener("submit", (event) => {
    event.preventDefault();
    const form = event.target as HTMLFormElement;
    const action = form.dataset.action;

    if (action === "create-wallet") void createWallet(form);
    if (action === "import-wallet") void importWallet(form);
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

    if (target.matches("[data-send-symbol]")) {
      updateRecipientPlaceholder(target.value);
    }
  });

  document.addEventListener("input", (event) => {
    const target = event.target as HTMLInputElement;
    if (target.matches("[data-passphrase-input]")) updatePassphraseStrength(target);
  });

  document.addEventListener("wheel", (event) => {
    const rail = (event.target as HTMLElement).closest<HTMLElement>(".asset-scroll");
    if (!rail || rail.scrollWidth <= rail.clientWidth || Math.abs(event.deltaX) > Math.abs(event.deltaY)) return;

    event.preventDefault();
    rail.scrollLeft += event.deltaY;
  }, { passive: false });
}

async function loadSession() {
  appState.busy = true;
  render();
  try {
    appState.session = await walletApi.getWallet();
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
}

import type { QrResilience } from "./types";
import { updateRecipientPlaceholder } from "./render";
