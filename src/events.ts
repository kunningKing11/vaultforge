import { recordWalletActivity, syncAutoLock } from "./autoLock";
import {
  broadcastSignedTransaction,
  cancelDeleteWallet,
  changeFiatCurrency,
  copyAddress,
  copyQrPayload,
  copyReceiveAddress,
  copyText,
  lockWallet,
  refreshPortfolio,
  setupWizard,
  showDeleteWallet,
  signTransaction,
  startDeleteWalletCountdown,
  swapTokens,
  unlockWallet,
  updateWalletPasswordStrength,
} from "./commands";
import { formatError } from "./format";
import { normalizeNetworkId } from "./networks";
import { downloadQrSvg, resetQr } from "./qr";
import { render, updateRecipientPlaceholder } from "./render";
import { installScrollbarBehavior } from "./scrollbars";
import { applyWalletSession, appState, selectSetupFlow } from "./state";
import { applyTheme, ThemeName, themes } from "./theme";
import { pushToast } from "./toasts";
import type { FiatCurrency, QrResilience, View } from "./types";
import { walletApi } from "./walletApi";
import { walletPasswordStrength } from "./walletPassword";

export function bindEvents() {
  document.addEventListener("click", (event) => {
    const target = event.target as HTMLElement;
    const action = target.closest<HTMLElement>("[data-action]")?.dataset.action;
    const view = target.closest<HTMLElement>("[data-view]")?.dataset.view as View | undefined;

    if (view) {
      appState.navigation.currentView = view;
      render();
      return;
    }

    if (!action) return;

    if (action === "lock") void lockWallet();
    if (action === "show-locked-delete-wallet") showDeleteWallet();
    if (action === "cancel-locked-delete-wallet") cancelDeleteWallet();
    if (action === "start-locked-delete-wallet-countdown") startDeleteWalletCountdown();
    if (action === "refresh") void refreshPortfolio();
    if (action === "copy-address") {
      const address = target.closest<HTMLElement>("[data-copy-address]")?.dataset.copyAddress;
      if (address) void copyAddress(address);
    }
    if (action === "copy-receive-address") void copyReceiveAddress();
    if (action === "copy-qr") void copyQrPayload();
    if (action === "download-qr") downloadQrSvg();
    if (action === "broadcast-signed-transaction") void broadcastSignedTransaction();
    if (action === "edit-signed-transaction") {
      appState.send.signedTransaction = null;
      render();
    }
    if (action === "select-activity") {
      appState.navigation.selectedActivityId =
        target.closest<HTMLElement>("[data-activity-id]")?.dataset.activityId ?? "";
      render();
    }
    if (action === "copy-value") {
      const value = target.closest<HTMLElement>("[data-copy-value]")?.dataset.copyValue;
      if (value) void copyText(value, "Value copied.");
    }

    if (action === "setup-prev") wizardPrev();
    if (action === "setup-next") wizardNext();
    if (action === "setup-create") {
      selectSetupFlow("create");
      render();
    }
    if (action === "setup-import") {
      selectSetupFlow("import");
      render();
    }
    if (action === "toggle-wallet-password-visibility") {
      appState.onboarding.walletPasswordVisible = !appState.onboarding.walletPasswordVisible;
      render();
    }
    if (action === "toggle-recovery-phrase") {
      appState.onboarding.recoveryPhraseVisible = !appState.onboarding.recoveryPhraseVisible;
      render();
    }
    if (action === "toggle-unlock-password-visibility") {
      appState.dialogs.unlockPasswordVisible = !appState.dialogs.unlockPasswordVisible;
      render();
    }
    if (action === "setup-wordcount") {
      const wc = Number(target.closest<HTMLElement>("[data-wordcount]")?.dataset.wordcount);
      if (wc) {
        appState.onboarding.wordCount = wc as 12 | 15 | 18 | 21 | 24;
        render();
      }
    }
    if (action === "setup-theme") {
      const id = target.closest<HTMLElement>("[data-theme]")?.dataset.theme;

      if (id && id in themes) {
        appState.onboarding.appearance = id as ThemeName;
        applyTheme(id as ThemeName);
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
      appState.receive.networkId = normalizeNetworkId(target.value);
      resetQr();
      render();
    }

    if (target.matches("[data-receive-resilience]")) {
      appState.receive.qrResilience = target.value as QrResilience;
      resetQr();
      render();
    }

    if (target.matches("[data-send-asset]")) {
      updateRecipientPlaceholder(target.selectedOptions[0]?.dataset.symbol ?? "");
    }

    if (target.matches("[data-fiat-currency]")) {
      void changeFiatCurrency(target.value as FiatCurrency);
    }
  });

  document.addEventListener("input", (event) => {
    const target = event.target as HTMLInputElement;
    if (target.matches("[data-wallet-password-input]")) updateWalletPasswordStrength(target);

    if (target.matches("[data-wizard-field]")) {
      const field = target.dataset.wizardField;
      if (field === "name") appState.onboarding.name = target.value;
      if (field === "walletPassword") appState.onboarding.walletPassword = target.value;
      if (field === "confirmWalletPassword")
        appState.onboarding.confirmWalletPassword = target.value;
      if (field === "mnemonic") appState.onboarding.recoveryPhrase = target.value;
      if (field === "acknowledgedBackup") appState.onboarding.acknowledgedBackup = target.checked;
    }
  });

  document.addEventListener("change", (event) => {
    const target = event.target as HTMLInputElement;

    if (target.matches("[data-wizard-network]")) {
      const id = normalizeNetworkId(target.dataset.wizardNetwork ?? "");
      if (target.checked) {
        if (!appState.onboarding.enabledNetworks.includes(id)) {
          appState.onboarding.enabledNetworks.push(id);
        }
      } else {
        appState.onboarding.enabledNetworks = appState.onboarding.enabledNetworks.filter(
          (network) => network !== id,
        );
      }
    }

    if (target.matches("[data-wizard-autolock]")) {
      const val = target.value;
      appState.onboarding.autoLockTimeoutSecs = val === "0" ? null : Number(val);
    }

    if (target.matches("[data-wizard-currency]")) {
      appState.onboarding.fiatCurrency = target.value as FiatCurrency;
    }

    if (target.matches("[data-wizard-field='customWordCount']")) {
      const val = Number(target.value);
      if (val) {
        appState.onboarding.wordCount = val as 12 | 15 | 18 | 21 | 24;
        render();
      }
    }
  });

  document.addEventListener("keydown", (event) => {
    const target = event.target as HTMLTextAreaElement;
    if (!target.matches('[data-wizard-field="mnemonic"]')) return;

    if (event.key === "Enter") event.preventDefault();
    if (
      event.key === " " &&
      target.selectionStart === target.selectionEnd &&
      target.value[target.selectionStart - 1] === " "
    ) {
      event.preventDefault();
    }
  });
}

async function wizardNext() {
  const wizard = appState.onboarding;
  const walletPassword = wizard.walletPassword;

  if (wizard.step === 2) {
    if (walletPassword.length < 8) {
      pushToast("Wallet password must be at least 8 characters.", "error");
      return;
    }

    if (walletPassword !== wizard.confirmWalletPassword) {
      pushToast("Wallet passwords do not match.", "error");
      return;
    }

    if (!walletPasswordStrength(walletPassword).meetsPolicy) {
      pushToast("Use a stronger wallet password to continue.", "error");
      return false;
    }
  }

  if (wizard.step === 5 && wizard.enabledNetworks.length === 0) {
    pushToast("Enable at least one network.", "error");
    return;
  }

  if (wizard.step < 6) {
    wizard.step++;

    if (wizard.step === 6) wizard.recoveryPhraseVisible = false;

    if (wizard.step === 6 && wizard.flow === "create" && !wizard.recoveryPhrase) {
      try {
        wizard.recoveryPhrase = await walletApi.generateMnemonic(wizard.wordCount);
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
  if (appState.onboarding.step > 1) {
    appState.onboarding.step--;
    render();
  }
}

const MINIMUM_SPLASH_DURATION_MS = 650;

async function loadSession() {
  const splashStartedAt = performance.now();
  appState.operation.busy = true;
  render();
  try {
    applyWalletSession(await walletApi.getWallet());
    recordWalletActivity();
    syncAutoLock(appState.wallet, () => void lockWallet());
  } catch (error) {
    pushToast(formatError(error), "error");
  } finally {
    const remainingSplashTime = MINIMUM_SPLASH_DURATION_MS - (performance.now() - splashStartedAt);
    if (remainingSplashTime > 0) {
      await new Promise<void>((resolve) => window.setTimeout(resolve, remainingSplashTime));
    }
    appState.operation.busy = false;
    render();
  }
}

export async function boot() {
  await loadSession();
  bindEvents();
  installScrollbarBehavior();
  syncAutoLock(appState.wallet, () => void lockWallet());

  document.addEventListener("click", () => {
    recordWalletActivity();
  });
  document.addEventListener("keydown", () => {
    recordWalletActivity();
  });
}
