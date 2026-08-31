import { formatError, toWei } from "./format";
import { networkById } from "./networks";
import { render } from "./render";
import { recordWalletActivity, stopAutoLock, syncAutoLock } from "./autoLock";
import { addressForNetwork, selectedNetwork, unlockedWallet } from "./selectors";
import { applyWalletSession, appState, resetOnboarding, resetSendFlow } from "./state";
import { pushToast } from "./toasts";
import type { RefreshWarning, SessionCommand, WalletRefreshResult, WalletSession } from "./types";
import { walletApi } from "./walletApi";
import { walletPasswordStrength } from "./walletPassword";

const BIP39_WORD_COUNTS = new Set([12, 15, 18, 21, 24]);
let lockedDeleteTimer: number | null = null;
let pendingTxTimer: number | null = null;
let portfolioRefreshId = 0;

export async function setupWizard() {
  const wizard = appState.onboarding;
  const walletPassword = wizard.walletPassword;
  const mnemonic = wizard.recoveryPhrase.trim();

  if (wizard.flow === "import") {
    if (!mnemonic) {
      pushToast("Please enter a recovery phrase to import a wallet.", "error");
      return;
    }
    if (!validateRecoveryPhraseWordCount(mnemonic)) return;
    const imported = await runRefreshCommand("import_wallet", () =>
      walletApi.importWallet({
        name: wizard.name || undefined,
        mnemonic,
        walletPassword,
        enabledNetworks: wizard.enabledNetworks,
        autoLockTimeoutSecs: wizard.autoLockTimeoutSecs,
      }),
    );
    if (imported) clearSetupSecrets();
  } else {
    if (!wizard.acknowledgedBackup) {
      pushToast(
        "Please confirm that you wrote down the recovery phrase before creating the wallet.",
        "error",
      );
      return;
    }
    if (!wizard.recoveryPhrase) {
      wizard.recoveryPhrase = await walletApi.generateMnemonic(wizard.wordCount);
    }
    const created = await runRefreshCommand("create_wallet", () =>
      walletApi.createWallet({
        name: wizard.name || "Primary Wallet",
        walletPassword,
        enabledNetworks: wizard.enabledNetworks,
        autoLockTimeoutSecs: wizard.autoLockTimeoutSecs,
        mnemonic: wizard.recoveryPhrase,
      }),
    );
    if (created) clearSetupSecrets();
  }
}

// Drop temporary onboarding secrets after a successful create or import.
function clearSetupSecrets() {
  const wizard = appState.onboarding;
  wizard.walletPassword = "";
  wizard.confirmWalletPassword = "";
  wizard.recoveryPhrase = "";
  wizard.recoveryPhraseVisible = false;
  wizard.acknowledgedBackup = false;
}

export async function unlockWallet(form: HTMLFormElement) {
  const formData = new FormData(form);
  const ok = await runRefreshCommand("unlock_wallet", () =>
    walletApi.unlockWallet({
      walletPassword: String(formData.get("walletPassword") || ""),
    }),
  );
  if (ok) {
    resetLockedDeleteWallet();
    void refreshPortfolioInBackground();
  }
}

export async function signTransaction(form: HTMLFormElement) {
  const formData = new FormData(form);
  const [networkValue, tokenValue] = String(formData.get("asset") || "ethereum:native").split(":");
  const network = networkById(networkValue)?.id;
  if (!network) {
    pushToast("Please select a supported network.", "error");
    return;
  }

  const tokenAddress = tokenValue === "native" ? null : tokenValue;
  const tokenAddressesMatch = (left: string, right: string) =>
    networkById(network)?.kind === "evm"
      ? left.toLowerCase() === right.toLowerCase()
      : left === right;
  const asset = unlockedWallet()?.assets.find(
    (candidate) =>
      candidate.network === network &&
      (tokenAddress === null
        ? candidate.token_address == null
        : candidate.token_address != null &&
          tokenAddressesMatch(candidate.token_address, tokenAddress)),
  );
  if (!asset) {
    pushToast("The selected asset is no longer available.", "error");
    return;
  }

  appState.send.draft = {
    to: String(formData.get("to") || ""),
    symbol: asset.symbol,
    network,
    token_address: asset.token_address ?? null,
    amount: String(formData.get("amount") || ""),
    note: String(formData.get("note") || ""),
  };
  appState.operation.busy = true;
  render();
  try {
    const decimals = asset.decimals;
    const signedTransaction = await walletApi.signTransaction({
      to: appState.send.draft.to,
      symbol: appState.send.draft.symbol,
      network: appState.send.draft.network,
      tokenAddress: appState.send.draft.token_address,
      amount: toWei(appState.send.draft.amount || "0", decimals),
      note: appState.send.draft.note,
    });
    if (appState.wallet.status !== "unlocked") return;
    appState.send.signedTransaction = signedTransaction;
    pushToast(successMessage("sign_transaction"), "success");
  } catch (error) {
    pushToast(formatError(error), "error");
  } finally {
    appState.operation.busy = false;
    render();
  }
}

export async function broadcastSignedTransaction() {
  if (!appState.send.signedTransaction) return;
  if (!window.confirm("Broadcast this signed transaction to the chain RPC?")) return;

  const ok = await runCommand("send_transaction", () =>
    walletApi.sendTransaction({ signed: appState.send.signedTransaction! }),
  );
  if (ok) {
    resetSendFlow();
    startPendingTxPolling();
    render();
  }
}

function startPendingTxPolling() {
  stopPendingTxPolling();
  pendingTxTimer = window.setInterval(() => {
    void pollPendingTransactions();
  }, 10_000);
}

function stopPendingTxPolling() {
  if (pendingTxTimer !== null) {
    window.clearInterval(pendingTxTimer);
    pendingTxTimer = null;
  }
}

async function pollPendingTransactions() {
  const wallet = unlockedWallet();
  if (!wallet) return;
  const pending = wallet.activity.filter((a) => a.status === "pending" && a.hash && a.network);
  if (pending.length === 0) {
    stopPendingTxPolling();
    return;
  }

  let updated = false;
  for (const item of pending) {
    try {
      const status = await walletApi.checkTransactionStatus({
        txHash: item.hash!,
        network: item.network!,
      });
      if (status) {
        item.status = status;
        updated = true;
      }
    } catch {
      // skip errors, retry next poll
    }
  }

  if (updated) {
    wallet.activity = [...wallet.activity];
    render();
  }
}

export async function swapTokens(form: HTMLFormElement) {
  const formData = new FormData(form);
  const fromSymbol = String(formData.get("fromSymbol") || "ETH");
  const asset = unlockedWallet()?.assets.find((a) => a.symbol === fromSymbol);
  const decimals = asset?.decimals ?? 18;
  await runCommand("swap_tokens", () =>
    walletApi.swapTokens({
      fromSymbol,
      toSymbol: String(formData.get("toSymbol") || "USDC"),
      amount: toWei(String(formData.get("amount") || "0"), decimals),
    }),
  );
}

export async function lockWallet() {
  invalidatePortfolioRefresh();
  appState.operation.busy = true;
  appState.dialogs.unlockPasswordVisible = false;
  render();
  try {
    stopAutoLock();
    await walletApi.lockWallet();
    stopPendingTxPolling();
    applyWalletSession(await walletApi.getWallet());
    appState.navigation.currentView = "dashboard";
    pushToast(successMessage("lock_wallet"), "success");
  } catch (error) {
    pushToast(formatError(error), "error");
  } finally {
    syncAutoLock(appState.wallet, () => void lockWallet());
    appState.operation.busy = false;
    render();
  }
}

export async function clearWallet() {
  if (
    !window.confirm(
      "Remove the encrypted local wallet and return to onboarding? This cannot be undone.",
    )
  )
    return;
  await deleteStoredWallet();
}

async function deleteStoredWallet() {
  invalidatePortfolioRefresh();
  stopLockedDeleteTimer();
  resetLockedDeleteWallet();
  const ok = await runCommand("clear_wallet", () => walletApi.clearWallet());
  if (ok) {
    stopPendingTxPolling();
    appState.navigation.currentView = "dashboard";
    resetOnboarding();
    resetSendFlow();
    render();
  }
}

export function showLockedDeleteWallet() {
  stopLockedDeleteTimer();
  appState.dialogs.deleteWallet.step = "confirm";
  appState.dialogs.deleteWallet.secondsRemaining = 10;
  render();
}

export function cancelLockedDeleteWallet() {
  stopLockedDeleteTimer();
  resetLockedDeleteWallet();
  render();
}

function resetLockedDeleteWallet() {
  appState.dialogs.deleteWallet.step = "idle";
  appState.dialogs.deleteWallet.secondsRemaining = 10;
}

export function startLockedDeleteWalletCountdown() {
  stopLockedDeleteTimer();
  appState.dialogs.deleteWallet.step = "countdown";
  appState.dialogs.deleteWallet.secondsRemaining = 10;
  render();

  lockedDeleteTimer = window.setInterval(() => {
    appState.dialogs.deleteWallet.secondsRemaining -= 1;
    if (appState.dialogs.deleteWallet.secondsRemaining <= 0) {
      void deleteStoredWallet();
      return;
    }
    render();
  }, 1_000);
}

function stopLockedDeleteTimer() {
  if (lockedDeleteTimer !== null) {
    window.clearInterval(lockedDeleteTimer);
    lockedDeleteTimer = null;
  }
}

export async function refreshPortfolio() {
  if (appState.portfolio.status === "refreshing") return;
  appState.portfolio.status = "refreshing";
  await runRefreshCommand("refresh_portfolio", () => walletApi.refreshPortfolio());
}

async function refreshPortfolioInBackground() {
  const refreshId = ++portfolioRefreshId;
  appState.portfolio.status = "refreshing";
  render();
  try {
    const result = await walletApi.refreshPortfolio();
    if (refreshId !== portfolioRefreshId || appState.wallet.status === "locked") return;
    applyWalletSession(result.session);
    appState.portfolio.status = result.warnings.length > 0 ? "stale" : "idle";
    syncAutoLock(appState.wallet, () => void lockWallet());
    for (const warning of result.warnings) {
      pushToast(refreshWarningMessage(warning), "warning");
    }
  } catch {
    // The cached portfolio remains visible until the user requests another refresh.
    if (refreshId === portfolioRefreshId) appState.portfolio.status = "stale";
  } finally {
    if (refreshId === portfolioRefreshId) {
      if (appState.portfolio.status === "refreshing") appState.portfolio.status = "idle";
      render();
    }
  }
}

function invalidatePortfolioRefresh() {
  portfolioRefreshId += 1;
  appState.portfolio.status = "idle";
}

async function runCommand(command: SessionCommand, action: () => Promise<WalletSession | null>) {
  appState.operation.busy = true;
  render();
  try {
    const result = await action();
    if (result) {
      applyWalletSession(result);
      if (command === "unlock_wallet") recordWalletActivity();
      syncAutoLock(appState.wallet, () => void lockWallet());
    }
    pushToast(successMessage(command), "success");
    return true;
  } catch (error) {
    pushToast(formatError(error), "error");
    return false;
  } finally {
    appState.operation.busy = false;
    render();
  }
}

async function runRefreshCommand(
  command: SessionCommand,
  action: () => Promise<WalletRefreshResult>,
) {
  appState.operation.busy = true;
  render();
  try {
    const result = await action();
    applyWalletSession(result.session);
    if (["create_wallet", "import_wallet", "unlock_wallet"].includes(command)) {
      recordWalletActivity();
    }
    if (command === "refresh_portfolio") {
      appState.portfolio.status = result.warnings.length > 0 ? "stale" : "idle";
    }
    syncAutoLock(appState.wallet, () => void lockWallet());
    for (const warning of result.warnings) {
      pushToast(refreshWarningMessage(warning), "warning");
    }
    pushToast(successMessage(command), "success");
    return true;
  } catch (error) {
    if (command === "refresh_portfolio") appState.portfolio.status = "stale";
    pushToast(formatError(error), "error");
    return false;
  } finally {
    appState.operation.busy = false;
    render();
  }
}

export async function copyAddress(address: string) {
  await copyText(address, "Address copied.");
}

export async function copyReceiveAddress() {
  const addr = addressForNetwork(selectedNetwork());
  if (!addr) return;
  await copyText(addr, "Receive address copied.");
}

export async function copyQrPayload() {
  if (!appState.receive.qrSvg) {
    pushToast("QR code is still generating.", "error");
    return;
  }
  await copyText(appState.receive.qrSvg, "QR SVG copied.");
}

export async function copyText(value: string, message: string) {
  await navigator.clipboard.writeText(value);
  pushToast(message, "success");
}

function validateRecoveryPhraseWordCount(mnemonic: string) {
  const wordCount = mnemonic.trim() === "" ? 0 : mnemonic.trim().split(/\s+/).length;
  if (BIP39_WORD_COUNTS.has(wordCount)) return true;

  pushToast("Recovery phrase must contain 12, 15, 18, 21, or 24 words.", "error");
  return false;
}

export function updateWalletPasswordStrength(input: HTMLInputElement) {
  const meter = (input.closest("form") ?? input.closest(".space-y-4"))?.querySelector<HTMLElement>(
    "[data-wallet-password-meter]",
  );
  if (!meter) return;
  const { score, label } = walletPasswordStrength(input.value);
  meter.dataset.score = String(score);
  meter.querySelector<HTMLElement>("[data-wallet-password-label]")!.textContent = label;
}

function successMessage(command: string) {
  const messages: Record<string, string> = {
    create_wallet: "Wallet created. Recovery phrase was generated in the Rust backend.",
    import_wallet: "Wallet imported successfully.",
    unlock_wallet: "Wallet unlocked.",
    lock_wallet: "Wallet locked.",
    clear_wallet: "Local wallet cleared.",
    sign_transaction: "Transaction signed locally.",
    send_transaction: "Signed transaction broadcast to the RPC provider.",
    swap_tokens: "Swap completed in the local simulator.",
    refresh_portfolio: "Portfolio refreshed.",
  };
  return messages[command] ?? "Updated.";
}

function refreshWarningMessage({ kind, subject }: RefreshWarning): string {
  if (kind === "balance") {
    return `${subject} balance refresh failed. Try again for an accurate balance.`;
  }

  return `${subject} refresh failed. Try again for an accurate value.`;
}
