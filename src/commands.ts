import { pushToast } from "./toasts";
import { formatError, toWei } from "./format";
import { walletApi } from "./walletApi";
import { networkById } from "./networks";
import { appState, addressForNetwork, selectedNetwork } from "./state";
import { render } from "./render";
import type { SessionCommand, WalletSession } from "./types";

const BIP39_WORD_COUNTS = new Set([12, 15, 18, 21, 24]);

export async function setupWizard() {
  const wizard = appState.setupWizard;
  const passphrase = wizard.passphrase;
  const mnemonic = wizard.mnemonic.trim();

  if (!validatePassphraseConfirmation(passphrase, wizard.confirmPassphrase)) return;

  if (wizard.flow === "import") {
    if (!mnemonic) {
      pushToast("Please enter a recovery phrase to import a wallet.", "error");
      return;
    }
    if (!validateRecoveryPhraseWordCount(mnemonic)) return;
    await runCommand("import_wallet", () =>
      walletApi.importWallet({
        name: wizard.name || undefined,
        mnemonic,
        passphrase,
        enabledNetworks: wizard.enabledNetworks,
        autoLockTimeoutSecs: wizard.autoLockTimeoutSecs,
      }),
    );
  } else {
    if (!wizard.acknowledgedBackup) {
      pushToast(
        "Please confirm that you wrote down the recovery phrase before creating the wallet.",
        "error",
      );
      return;
    }
    if (!wizard.generatedMnemonic) {
      wizard.generatedMnemonic = await walletApi.generateMnemonic(wizard.wordCount);
    }
    wizard.mnemonic = "";
    await runCommand("create_wallet", () =>
      walletApi.createWallet({
        name: wizard.name || "Primary Wallet",
        passphrase,
        enabledNetworks: wizard.enabledNetworks,
        autoLockTimeoutSecs: wizard.autoLockTimeoutSecs,
        mnemonic: wizard.generatedMnemonic,
      }),
    );
  }
}

export async function unlockWallet(form: HTMLFormElement) {
  const formData = new FormData(form);
  const ok = await runCommand("unlock_wallet", () =>
    walletApi.unlockWallet({
      passphrase: String(formData.get("passphrase") || ""),
    }),
  );
  if (ok) resetLockedDeleteWallet();
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
  const asset = appState.session?.assets.find(
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

  appState.sendDraft = {
    to: String(formData.get("to") || ""),
    symbol: asset.symbol,
    network,
    token_address: asset.token_address ?? null,
    amount: String(formData.get("amount") || ""),
    note: String(formData.get("note") || ""),
  };
  appState.busy = true;
  render();
  try {
    const decimals = asset.decimals;
    appState.signedTransaction = await walletApi.signTransaction({
      to: appState.sendDraft.to,
      symbol: appState.sendDraft.symbol,
      network: appState.sendDraft.network,
      tokenAddress: appState.sendDraft.token_address,
      amount: toWei(appState.sendDraft.amount || "0", decimals),
      note: appState.sendDraft.note,
    });
    pushToast(successMessage("sign_transaction"), "success");
  } catch (error) {
    pushToast(formatError(error), "error");
  } finally {
    appState.busy = false;
    render();
  }
}

export async function broadcastSignedTransaction() {
  if (!appState.signedTransaction) return;
  if (!window.confirm("Broadcast this signed transaction to the chain RPC?")) return;

  const ok = await runCommand("send_transaction", () =>
    walletApi.sendTransaction({ signed: appState.signedTransaction! }),
  );
  if (ok) {
    appState.signedTransaction = null;
    appState.sendDraft = {
      to: "",
      symbol: "ETH",
      network: "ethereum",
      token_address: null,
      amount: "",
      note: "",
    };
    startPendingTxPolling();
  }
}

function startPendingTxPolling() {
  stopPendingTxPolling();
  appState.pendingTxTimer = window.setInterval(() => {
    void pollPendingTransactions();
  }, 10_000);
}

function stopPendingTxPolling() {
  if (appState.pendingTxTimer !== null) {
    window.clearInterval(appState.pendingTxTimer);
    appState.pendingTxTimer = null;
  }
}

async function pollPendingTransactions() {
  if (!appState.session) return;
  const pending = appState.session.activity.filter(
    (a) => a.status === "pending" && a.hash && a.network,
  );
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
    appState.session = { ...appState.session, activity: [...appState.session.activity] };
    render();
  }
}

export async function swapTokens(form: HTMLFormElement) {
  const formData = new FormData(form);
  const fromSymbol = String(formData.get("fromSymbol") || "ETH");
  const asset = appState.session?.assets.find((a) => a.symbol === fromSymbol);
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
  appState.busy = true;
  render();
  try {
    stopAutoLockTimer();
    await walletApi.lockWallet();
    stopPendingTxPolling();
    appState.session = await walletApi.getWallet();
    appState.currentView = "dashboard";
    pushToast(successMessage("lock_wallet"), "success");
  } catch (error) {
    pushToast(formatError(error), "error");
  } finally {
    syncAutoLockTimerWithSession();
    appState.busy = false;
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
  stopLockedDeleteTimer();
  resetLockedDeleteWallet();
  const ok = await runCommand("clear_wallet", () => walletApi.clearWallet());
  if (ok) {
    stopPendingTxPolling();
    appState.currentView = "dashboard";
    appState.signedTransaction = null;
    appState.sendDraft = {
      to: "",
      symbol: "ETH",
      network: "ethereum",
      token_address: null,
      amount: "",
      note: "",
    };
  }
}

export function showLockedDeleteWallet() {
  stopLockedDeleteTimer();
  appState.lockedDeleteStep = "confirm";
  appState.lockedDeleteRemaining = 10;
  render();
}

export function cancelLockedDeleteWallet() {
  stopLockedDeleteTimer();
  resetLockedDeleteWallet();
  render();
}

function resetLockedDeleteWallet() {
  appState.lockedDeleteStep = "idle";
  appState.lockedDeleteRemaining = 10;
}

export function startLockedDeleteWalletCountdown() {
  stopLockedDeleteTimer();
  appState.lockedDeleteStep = "countdown";
  appState.lockedDeleteRemaining = 10;
  render();

  appState.lockedDeleteTimer = window.setInterval(() => {
    appState.lockedDeleteRemaining -= 1;
    if (appState.lockedDeleteRemaining <= 0) {
      void deleteStoredWallet();
      return;
    }
    render();
  }, 1_000);
}

function stopLockedDeleteTimer() {
  if (appState.lockedDeleteTimer !== null) {
    window.clearInterval(appState.lockedDeleteTimer);
    appState.lockedDeleteTimer = null;
  }
}

function stopAutoLockTimer() {
  if (appState.autoLockTimer !== null) {
    window.clearTimeout(appState.autoLockTimer);
    appState.autoLockTimer = null;
  }
}

function startAutoLockTimer() {
  stopAutoLockTimer();
  const timeout = appState.session?.auto_lock_timeout_secs;
  if (!timeout) return;
  const check = () => {
    const remaining = timeout * 1000 - (Date.now() - appState.lastActivity);
    if (remaining <= 0) {
      void lockWallet();
      return;
    }
    appState.autoLockTimer = window.setTimeout(check, remaining);
  };
  check();
}

function syncAutoLockTimerWithSession() {
  if (!appState.session || appState.session.locked) {
    stopAutoLockTimer();
    return;
  }
  startAutoLockTimer();
}

export async function refreshPrices() {
  await runCommand("refresh_prices", () => walletApi.refreshPrices());
}

async function runCommand(command: SessionCommand, action: () => Promise<WalletSession | null>) {
  appState.busy = true;
  render();
  try {
    const result = await action();
    if (result) {
      appState.session = result;
      if (command === "unlock_wallet") {
        appState.lastActivity = Date.now();
      }
      syncAutoLockTimerWithSession();
    }
    pushToast(successMessage(command), "success");
    return true;
  } catch (error) {
    pushToast(formatError(error), "error");
    return false;
  } finally {
    appState.busy = false;
    render();
  }
}

export async function copyAddress() {
  if (!appState.session?.address) return;
  await copyText(appState.session.address, "Receive address copied.");
}

export async function copyReceiveAddress() {
  const addr = addressForNetwork(selectedNetwork());
  if (!addr) return;
  await copyText(addr, "Receive address copied.");
}

export async function copyQrPayload() {
  if (!appState.qrSvg) {
    pushToast("QR code is still generating.", "error");
    return;
  }
  await copyText(appState.qrSvg, "QR SVG copied.");
}

export async function copyText(value: string, message: string) {
  await navigator.clipboard.writeText(value);
  pushToast(message, "success");
}

function validatePassphraseConfirmation(passphrase: string, confirm: string) {
  if (passphrase !== confirm) {
    pushToast("Passphrases do not match.", "error");
    return false;
  }
  if (passphraseScore(passphrase) < 3) {
    pushToast("Use a stronger passphrase before creating encrypted storage.", "error");
    return false;
  }
  return true;
}

function validateRecoveryPhraseWordCount(mnemonic: string) {
  const wordCount = mnemonic.trim() === "" ? 0 : mnemonic.trim().split(/\s+/).length;
  if (BIP39_WORD_COUNTS.has(wordCount)) return true;

  pushToast("Recovery phrase must contain 12, 15, 18, 21, or 24 words.", "error");
  return false;
}

export function updatePassphraseStrength(input: HTMLInputElement) {
  const meter = (input.closest("form") ?? input.closest(".space-y-4"))?.querySelector<HTMLElement>(
    "[data-passphrase-meter]",
  );
  if (!meter) return;
  const score = passphraseScore(input.value);
  const labels = ["Too weak", "Weak", "Fair", "Strong", "Excellent"];
  meter.dataset.score = String(score);
  meter.querySelector<HTMLElement>("[data-passphrase-label]")!.textContent = labels[score];
}

function passphraseScore(value: string) {
  let score = value.length >= 8 ? 1 : 0;
  if (value.length >= 12) score += 1;
  if (/[A-Z]/.test(value) && /[a-z]/.test(value)) score += 1;
  if (/\d/.test(value)) score += 1;
  if (/[^A-Za-z0-9]/.test(value)) score += 1;
  return Math.min(score, 4);
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
    refresh_prices: "Market prices refreshed.",
  };
  return messages[command] ?? "Updated.";
}
