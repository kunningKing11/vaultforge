import { invoke } from "@tauri-apps/api/core";
import QRCode from "qrcode";
import "./styles.css";

type Asset = {
  symbol: string;
  name: string;
  balance: number;
  price_usd: number;
  change_24h: number;
  network: string;
};

type Activity = {
  id: string;
  kind: string;
  title: string;
  subtitle: string;
  status: string;
  timestamp: string;
  hash: string;
  amount?: string;
  from?: string | null;
  to?: string | null;
  network?: string | null;
  payload_hash?: string | null;
  signature?: string | null;
  fee?: string | null;
};

type WalletSession = {
  has_wallet: boolean;
  locked: boolean;
  wallet_name: string | null;
  address: string | null;
  network: string;
  assets: Asset[];
  activity: Activity[];
};

type SignedTransaction = {
  from: string;
  to: string;
  symbol: string;
  amount: number;
  note: string;
  network: string;
  nonce: string;
  signedAt: string;
  payloadHash: string;
  signature: string;
  feeAmount: number;
  feeSymbol: string;
  totalDebit: number;
  postBalance: number;
  fiatValue: number;
};

type SendDraft = {
  to: string;
  symbol: string;
  amount: string;
  note: string;
};

type SessionCommand = "create_wallet" | "import_wallet" | "unlock_wallet" | "send_transaction" | "swap_tokens" | "set_network" | "clear_wallet" | "refresh_prices";

type View = "dashboard" | "send" | "receive" | "swap" | "assets" | "activity" | "security" | "settings";

type EvmReceiveNetworkInput = {
  kind: "evm";
  id: string;
  name: string;
  chainId: number;
  ticker: string;
  vm_type?: "EVM";
  isL2?: boolean;
  isTestNet?: boolean;
};

type EvmReceiveNetwork = Omit<Required<EvmReceiveNetworkInput>, "vm_type"> & {
  vm_type: "EVM";
  isL2: boolean;
  isTestNet: boolean;
};

type BitcoinReceiveNetworkInput = {
  kind: "bitcoin";
  id: string;
  name: string;
  ticker: "BTC";
  isTestNet?: boolean;
};

type BitcoinReceiveNetwork = Omit<Required<BitcoinReceiveNetworkInput>, "vm_type"> & {
  vm_type: "FVM",
  isL2: boolean,
  isTestNet: boolean,
}

type LightningReceiveNetworkInput = {
  kind: "lightning";
  id: string;
  name: string;
  ticker: "BTC";
  isTestNet?: boolean;
};

type LightningReceiveNetwork = Omit<Required<LightningReceiveNetworkInput>, "vm_type"> & {
  vm_type: "FVM",
  isL2: boolean,
  isTestNet: boolean,
}

type SolanaReceiveNetworkInput = {
  kind: "solana";
  id: string;
  name: string;
  ticker: "SOL";
  isTestNet?: boolean;
};

type SolanaReceiveNetwork = Omit<Required<SolanaReceiveNetworkInput>, "vm_type"> & {
  vm_type: "FVM",
  isL2: boolean,
  isTestNet: boolean,
}

type ZcashReceiveNetworkInput = {
  kind: "zcash";
  id: string;
  name: string;
  ticker: "ZEC";
  isTestNet?: boolean;
}

type ZcashReceiveNetwork = Omit<Required<ZcashReceiveNetworkInput>, "vm_type"> & {
  vm_type: "FVM",
  isL2: boolean,
  isTestNet: boolean,
}

type FilecoinReceiveNetworkInput = {
  kind: "filecoin";
  id: string;
  name: string;
  ticker: "FIL";
  isTestNet?: boolean;
}

type FilecoinReceiveNetwork = Omit<Required<FilecoinReceiveNetworkInput>, "vm_type"> & {
  vm_type: "FVM",
  isL2: boolean,
  isTestNet: boolean,
}

type ReceiveNetworkInput =
  | EvmReceiveNetworkInput
  | BitcoinReceiveNetworkInput
  | LightningReceiveNetworkInput
  | SolanaReceiveNetworkInput
  | ZcashReceiveNetworkInput
  | FilecoinReceiveNetworkInput;
type ReceiveNetwork = EvmReceiveNetwork | BitcoinReceiveNetwork | LightningReceiveNetwork | SolanaReceiveNetwork | ZcashReceiveNetwork | FilecoinReceiveNetwork;

type QrResilience = "L" | "M" | "Q" | "H";

type Toast = {
  id: number;
  message: string;
  tone: "info" | "success" | "warning" | "error";
  createdAt: number;
  duration: number;
  exiting: boolean;
};

const rawReceiveNetworks: ReceiveNetworkInput[] = [
  { kind: "evm", id: "ethereum", name: "Ethereum", chainId: 1, ticker: "ETH", vm_type: "EVM" },
  { kind: "evm", id: "monad", name: "Monad", chainId: 167004, ticker: "MONAD", vm_type: "EVM" },
  { kind: "evm", id: "polygon", name: "Polygon", chainId: 137, ticker: "MATIC", vm_type: "EVM", isL2: true },
  { kind: "evm", id: "arbitrum_one", name: "Arbitrum One", chainId: 42161, ticker: "ETH", vm_type: "EVM", isL2: true },
  { kind: "evm", id: "base", name: "Base", chainId: 8453, ticker: "ETH", vm_type: "EVM", isL2: true },
  { kind: "evm", id: "optimism", name: "Optimism", chainId: 10, ticker: "ETH", vm_type: "EVM", isL2: true },
  { kind: "evm", id: "avalanche_c", name: "Avalanche C-Chain", chainId: 43114, ticker: "AVAX", vm_type: "EVM" },
  { kind: "bitcoin", id: "bitcoin", name: "Bitcoin", ticker: "BTC" },
  { kind: "solana", id: "solana", name: "Solana", ticker: "SOL" },
  { kind: "zcash", id: "zcash", name: "Zcash", ticker: "ZEC" },
  { kind: "filecoin", id: "filecoin", name: "Filecoin", ticker: "FIL" },
];

export const receiveNetworks: ReceiveNetwork[] = rawReceiveNetworks.map(normalizeReceiveNetwork);

function normalizeReceiveNetwork(network: ReceiveNetworkInput): ReceiveNetwork {
  if (network.kind === "evm") {
    return {
      ...network,
      vm_type: network.vm_type ?? "EVM",
      isL2: network.isL2 ?? false,
      isTestNet: network.isTestNet ?? false,
    };
  }

  return {
    ...network,
    vm_type: "FVM",
    isL2: false,
    isTestNet: network.isTestNet ?? false,
  };
}

const qrResilienceOptions: Array<{ value: QrResilience; label: string; detail: string }> = [
  { value: "L", label: "Low", detail: "~7% recovery" },
  { value: "M", label: "Medium", detail: "~15% recovery" },
  { value: "Q", label: "Quartile", detail: "~25% recovery" },
  { value: "H", label: "High", detail: "~30% recovery" },
];

const walletApi = {
  getWallet: () => invoke<WalletSession>("get_wallet"),
  refreshPrices: () => invoke<WalletSession>("refresh_prices"),
  createWallet: (args: { name: string; passphrase: string }) => invoke<WalletSession>("create_wallet", args),
  importWallet: (args: { mnemonic: string; passphrase: string }) => invoke<WalletSession>("import_wallet", args),
  unlockWallet: (args: { passphrase: string }) => invoke<WalletSession>("unlock_wallet", args),
  lockWallet: () => invoke<null>("lock_wallet"),
  clearWallet: () => invoke<WalletSession>("clear_wallet"),
  signTransaction: (args: { to: string; symbol: string; amount: number; note: string }) => invoke<SignedTransaction>("sign_transaction", args),
  sendTransaction: (args: { signed: SignedTransaction }) => invoke<WalletSession>("send_transaction", args),
  swapTokens: (args: { fromSymbol: string; toSymbol: string; amount: number }) => invoke<WalletSession>("swap_tokens", args),
  setNetwork: (args: { network: string }) => invoke<WalletSession>("set_network", args),
};

const app = document.querySelector<HTMLDivElement>("#app");

let session: WalletSession | null = null;
let currentView: View = "dashboard";
let receiveNetworkId = "ethereum";
let qrResilience: QrResilience = "M";
let qrSvg = "";
let qrKey = "";
let qrGeneratingKey = "";
let signedTransaction: SignedTransaction | null = null;
let sendDraft: SendDraft = { to: "", symbol: "ETH", amount: "", note: "" };
let selectedActivityId = "";
let busy = false;
let toastId = 0;
let toasts: Toast[] = [];

const enteredToasts = new Set<number>();

if (!app) {
  throw new Error("App root not found");
}

const appRoot = app;
const toastRoot = document.createElement("div");
toastRoot.className = "toast-stack";
document.body.appendChild(toastRoot);

void boot();

async function boot() {
  await loadSession();
  bindEvents();
}

async function loadSession() {
  busy = true;
  render();
  try {
    session = await walletApi.getWallet();
  } catch (error) {
    pushToast(formatError(error), "error");
  } finally {
    busy = false;
    render();
  }
}

function bindEvents() {
  document.addEventListener("click", (event) => {
    const target = event.target as HTMLElement;
    const action = target.closest<HTMLElement>("[data-action]")?.dataset.action;
    const view = target.closest<HTMLElement>("[data-view]")?.dataset.view as View | undefined;

    if (view) {
      currentView = view;
      render();
      return;
    }

    if (!action) return;

    if (action === "lock") void lockWallet();
    if (action === "clear-wallet") void clearWallet();
    if (action === "refresh") void refreshPrices();
    if (action === "copy-address") void copyAddress();
    if (action === "copy-receive-address") void copyReceiveAddress();
    if (action === "copy-qr") void copyQrPayload();
    if (action === "download-qr") downloadQrSvg();
    if (action === "broadcast-signed-transaction") void broadcastSignedTransaction();
    if (action === "edit-signed-transaction") {
      signedTransaction = null;
      render();
    }
    if (action === "select-activity") {
      selectedActivityId = target.closest<HTMLElement>("[data-activity-id]")?.dataset.activityId ?? "";
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
    if (action === "set-network") void setNetwork(form);
  });

  document.addEventListener("change", (event) => {
    const target = event.target as HTMLSelectElement;

    if (target.matches("[data-receive-network]")) {
      receiveNetworkId = target.value;
      resetQr();
      render();
    }

    if (target.matches("[data-receive-resilience]")) {
      qrResilience = target.value as QrResilience;
      resetQr();
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

async function createWallet(form: HTMLFormElement) {
  const formData = new FormData(form);
  const passphrase = String(formData.get("passphrase") || "");
  if (!validatePassphraseConfirmation(form, passphrase)) return;

  await runCommand("create_wallet", () => walletApi.createWallet({
    name: String(formData.get("name") || "Primary Wallet"),
    passphrase,
  }));
}

async function importWallet(form: HTMLFormElement) {
  const formData = new FormData(form);
  const passphrase = String(formData.get("passphrase") || "");
  if (!validatePassphraseConfirmation(form, passphrase)) return;

  await runCommand("import_wallet", () => walletApi.importWallet({
    mnemonic: String(formData.get("mnemonic") || ""),
    passphrase,
  }));
}

async function unlockWallet(form: HTMLFormElement) {
  const formData = new FormData(form);
  await runCommand("unlock_wallet", () => walletApi.unlockWallet({
    passphrase: String(formData.get("passphrase") || ""),
  }));
}

async function signTransaction(form: HTMLFormElement) {
  const formData = new FormData(form);
  sendDraft = {
    to: String(formData.get("to") || ""),
    symbol: String(formData.get("symbol") || "ETH"),
    amount: String(formData.get("amount") || ""),
    note: String(formData.get("note") || ""),
  };
  busy = true;
  render();
  try {
    signedTransaction = await walletApi.signTransaction({
      to: sendDraft.to,
      symbol: sendDraft.symbol,
      amount: Number(sendDraft.amount || 0),
      note: sendDraft.note,
    });
    pushToast(successMessage("sign_transaction"), "success");
  } catch (error) {
    pushToast(formatError(error), "error");
  } finally {
    busy = false;
    render();
  }
}

async function broadcastSignedTransaction() {
  if (!signedTransaction) return;
  if (!window.confirm("Broadcast this signed transaction to the local simulator?")) return;
  const signed = signedTransaction;

  const ok = await runCommand("send_transaction", () => walletApi.sendTransaction({ signed }));
  if (ok) {
    signedTransaction = null;
    sendDraft = { to: "", symbol: "ETH", amount: "", note: "" };
    render();
  }
}

async function swapTokens(form: HTMLFormElement) {
  const formData = new FormData(form);
  await runCommand("swap_tokens", () => walletApi.swapTokens({
    fromSymbol: String(formData.get("fromSymbol") || "ETH"),
    toSymbol: String(formData.get("toSymbol") || "USDC"),
    amount: Number(formData.get("amount") || 0),
  }));
}

async function setNetwork(form: HTMLFormElement) {
  const formData = new FormData(form);
  await runCommand("set_network", () => walletApi.setNetwork({
    network: String(formData.get("network") || "Ethereum"),
  }));
}

async function lockWallet() {
  busy = true;
  render();
  try {
    await walletApi.lockWallet();
    session = await walletApi.getWallet();
    currentView = "dashboard";
    pushToast(successMessage("lock_wallet"), "success");
  } catch (error) {
    pushToast(formatError(error), "error");
  } finally {
    busy = false;
    render();
  }
}

async function clearWallet() {
  if (!window.confirm("Remove the encrypted local wallet and return to onboarding? This cannot be undone.")) return;
  const ok = await runCommand("clear_wallet", () => walletApi.clearWallet());
  if (ok) {
    currentView = "dashboard";
    signedTransaction = null;
    sendDraft = { to: "", symbol: "ETH", amount: "", note: "" };
    render();
  }
}

async function refreshPrices() {
  await runCommand("refresh_prices", () => walletApi.refreshPrices());
}

async function runCommand(command: SessionCommand, action: () => Promise<WalletSession | null>) {
  busy = true;
  render();
  try {
    const result = await action();
    if (result) session = result;
    pushToast(successMessage(command), "success");
    return true;
  } catch (error) {
    pushToast(formatError(error), "error");
    return false;
  } finally {
    busy = false;
    render();
  }
}

async function copyAddress() {
  if (!session?.address) return;
  await copyText(session!.address, "Receive address copied.");
}

async function copyReceiveAddress() {
  const address = receiveAddress(selectedReceiveNetwork());
  if (!address) return;
  await copyText(address, "Receive address copied.");
}

async function copyQrPayload() {
  if (!qrSvg) {
    pushToast("QR code is still generating.", "error");
    return;
  }

  await copyText(qrSvg, "QR SVG copied.");
}

async function copyText(value: string, message: string) {
  await navigator.clipboard.writeText(value);
  pushToast(message, "success");
}

function validatePassphraseConfirmation(form: HTMLFormElement, passphrase: string) {
  const confirm = String(new FormData(form).get("confirmPassphrase") || "");
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

function updatePassphraseStrength(input: HTMLInputElement) {
  const meter = input.closest("form")?.querySelector<HTMLElement>("[data-passphrase-meter]");
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

function downloadQrSvg() {
  if (!qrSvg || !session?.address) return;

  const network = selectedReceiveNetwork();
  const blob = new Blob([qrSvg], { type: "image/svg+xml;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = `vaultforge-${network.id}-${shortAddress(session!.address).replace(/[^a-zA-Z0-9]/g, "")}-qr.svg`;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
  pushToast("QR code downloaded.", "success");
}

function render() {
  appRoot.innerHTML = `
    <main class="noise min-h-screen px-4 py-5 text-slate-100 sm:px-6 lg:px-8">
      ${busy ? loadingBar() : ""}
      ${renderBody()}
    </main>
  `;
  void ensureReceiveQr();
}

function renderBody() {
  if (!session && busy) return splash();
  if (!session?.has_wallet) return onboarding();
  if (session?.locked) return lockedWallet();
  return walletShell();
}

function splash() {
  return `
    <section class="mx-auto flex min-h-[80vh] max-w-5xl items-center justify-center">
      <div class="glass rounded-[2rem] p-10 text-center">
        <p class="text-sm uppercase tracking-[0.4em] text-acid">VaultForge</p>
        <h1 class="mt-4 text-4xl font-black">Loading wallet core</h1>
      </div>
    </section>
  `;
}

function onboarding() {
  return `
    <section class="mx-auto grid min-h-[88vh] max-w-7xl items-center gap-8 lg:grid-cols-[1fr_0.95fr]">
      <div class="space-y-8">
        <div class="inline-flex rounded-full border border-acid/30 bg-acid/10 px-4 py-2 text-sm font-bold text-acid">Desktop self-custody wallet</div>
        <div>
          <h1 class="max-w-3xl text-5xl font-black tracking-tight text-white sm:text-7xl">Control crypto from a local-first command center.</h1>
          <p class="mt-6 max-w-2xl text-lg leading-8 text-slate-300">VaultForge combines a TypeScript interface, TailwindCSS system, and Rust-powered Tauri backend for portfolio management, transfers, swaps, activity tracking, and wallet locking.</p>
        </div>
        <div class="grid gap-4 sm:grid-cols-3">
          ${featureCard("Rust core", "Wallet state and validations run behind Tauri commands.")}
          ${featureCard("Fast UI", "Vite, TypeScript, and Tailwind power the frontend.")}
          ${featureCard("Local first", "No hosted service is required for this simulated wallet foundation.")}
        </div>
      </div>
      <div class="glass rounded-[2rem] p-6 sm:p-8">
        <div class="mb-6 flex items-center justify-between">
          <div>
            <p class="text-sm uppercase tracking-[0.3em] text-slate-500">Start</p>
            <h2 class="text-2xl font-black">Create wallet</h2>
          </div>
          <span class="rounded-full bg-acid px-3 py-1 text-xs font-black text-ink">NEW</span>
        </div>
        <form data-action="create-wallet" class="space-y-4">
          <label class="block space-y-2"><span class="text-sm font-bold text-slate-300">Wallet name</span><input class="field" name="name" placeholder="Primary Vault" required /></label>
          <label class="block space-y-2"><span class="text-sm font-bold text-slate-300">Passphrase</span><input class="field" name="passphrase" type="password" minlength="8" placeholder="Minimum 8 characters" data-passphrase-input required /></label>
          ${passphraseMeter()}
          <label class="block space-y-2"><span class="text-sm font-bold text-slate-300">Confirm passphrase</span><input class="field" name="confirmPassphrase" type="password" minlength="8" required /></label>
          <button class="btn-primary w-full" type="submit">Generate wallet</button>
        </form>
        <div class="my-7 h-px bg-white/10"></div>
        <form data-action="import-wallet" class="space-y-4">
          <h3 class="font-black">Import existing wallet</h3>
          <label class="block space-y-2"><span class="text-sm font-bold text-slate-300">Recovery phrase</span><textarea class="field min-h-28" name="mnemonic" placeholder="12 or 24 word phrase" required></textarea></label>
          <label class="block space-y-2"><span class="text-sm font-bold text-slate-300">New local passphrase</span><input class="field" name="passphrase" type="password" minlength="8" data-passphrase-input required /></label>
          ${passphraseMeter()}
          <label class="block space-y-2"><span class="text-sm font-bold text-slate-300">Confirm passphrase</span><input class="field" name="confirmPassphrase" type="password" minlength="8" required /></label>
          <button class="btn-secondary w-full" type="submit">Import wallet</button>
        </form>
      </div>
    </section>
  `;
}

function lockedWallet() {
  return `
    <section class="mx-auto flex min-h-[88vh] max-w-xl items-center justify-center">
      <div class="glass w-full rounded-[2rem] p-8 text-center">
        <div class="mx-auto mb-5 flex h-16 w-16 items-center justify-center rounded-2xl bg-acid/15 text-3xl">#</div>
        <p class="text-sm uppercase tracking-[0.3em] text-slate-500">Wallet locked</p>
        <h1 class="mt-2 text-3xl font-black">Unlock VaultForge</h1>
        <p class="mt-3 text-slate-400">Your wallet session is locked locally. Enter your passphrase to restore dashboard access.</p>
        <form data-action="unlock-wallet" class="mt-7 space-y-4 text-left">
          <label class="block space-y-2"><span class="text-sm font-bold text-slate-300">Passphrase</span><input class="field" name="passphrase" type="password" required /></label>
          <button class="btn-primary w-full" type="submit">Unlock wallet</button>
        </form>
      </div>
    </section>
  `;
}

function walletShell() {
  if (!session) return "";
  return `
    <div class="mx-auto grid max-w-[1500px] gap-5 pb-24 lg:grid-cols-[280px_1fr] lg:pb-0">
      <aside class="glass hidden rounded-[2rem] p-5 lg:sticky lg:top-5 lg:block lg:h-[calc(100vh-2.5rem)]">
        <div class="mb-8 flex items-center gap-3">
          <div class="flex h-12 w-12 items-center justify-center rounded-2xl bg-acid text-xl font-black text-ink">VF</div>
          <div><p class="font-black">${escapeHtml(session.wallet_name ?? "VaultForge")}</p><p class="text-sm text-slate-500">${escapeHtml(session.network)}</p></div>
        </div>
        <nav class="space-y-2">
          ${navButton("dashboard", "Dashboard")}
          ${navButton("send", "Send")}
          ${navButton("receive", "Receive")}
          ${navButton("swap", "Swap")}
          ${navButton("assets", "Assets")}
          ${navButton("activity", "Activity")}
          ${navButton("settings", "Settings")}
        </nav>
        <div class="mt-8 rounded-2xl border border-white/10 bg-white/[0.03] p-4">
          <p class="text-xs uppercase tracking-[0.25em] text-slate-500">Address</p>
          <p class="mt-2 break-all font-mono text-sm text-slate-300">${escapeHtml(shortAddress(session.address))}</p>
          <button class="btn-secondary mt-4 w-full text-sm" data-action="copy-address" type="button">Copy</button>
        </div>
      </aside>
      <section class="space-y-5">
        ${topBar()}
        ${renderView()}
      </section>
      ${mobileNav()}
    </div>
  `;
}

function topBar() {
  if (!session) return "";
  return `
    <header class="glass flex flex-col gap-4 rounded-[2rem] p-5 sm:flex-row sm:items-center sm:justify-between">
      <div>
        <p class="text-sm uppercase tracking-[0.3em] text-slate-500">Overview</p>
        <h1 class="mt-1 text-4xl font-black">${escapeHtml(session.wallet_name ?? "VaultForge")}</h1>
      </div>
      <div class="flex flex-wrap gap-3">
        <button class="btn-secondary" data-action="refresh" type="button">Refresh</button>
        <button class="btn-secondary" data-action="lock" type="button">Lock</button>
        <button class="btn-primary" data-view="send" type="button">Send funds</button>
      </div>
    </header>
  `;
}

function renderView() {
  if (currentView === "send") return sendView();
  if (currentView === "receive") return receiveView();
  if (currentView === "swap") return swapView();
  if (currentView === "assets") return assetsView();
  if (currentView === "activity") return activityView();
  if (currentView === "settings") return settingsView();
  return dashboardView();
}

function dashboardView() {
  if (!session) return "";
  const topAssets = [...session.assets]
    .sort((left, right) => assetValue(right) - assetValue(left))
    .map(assetCard)
    .join("");
  const recent = session.activity.slice(0, 5).map(activityRow).join("") || emptyState("No recent activity", "Sign, send, swap, or change networks to build a local activity timeline.");
  const change = portfolioChange();
  return `
    <div class="grid gap-5 xl:grid-cols-[1.35fr_0.75fr]">
      <div class="min-w-0 space-y-5">
        <section class="glass min-w-0 overflow-hidden rounded-[2rem] p-6">
          <div class="flex flex-col gap-6 lg:flex-row lg:items-end lg:justify-between">
            <div>
              <p class="text-sm uppercase tracking-[0.3em] text-acid">Portfolio</p>
              <h2 class="mt-3 max-w-2xl text-4xl font-black tracking-tight">Multi-asset wallet with transaction controls.</h2>
            </div>
            <div class="grid gap-3 sm:grid-cols-1">
              <div class="rounded-2xl border border-acid/30 bg-acid/10 p-4 text-right">
                <p class="text-sm text-slate-400">Weighted 24h</p>
                <p class="text-3xl font-black ${change >= 0 ? "text-emerald-300" : "text-rose-300"}">${change >= 0 ? "+" : ""}${change.toFixed(2)}%</p>
              </div>
            </div>
          </div>
          <div class="asset-scroll mt-6">${topAssets || emptyState("No assets", "Create or import a wallet to populate simulated balances.")}</div>
        </section>
        <section class="glass rounded-[2rem] p-6">
          <div class="mb-5 flex items-center justify-between"><h2 class="text-xl font-black">Recent activity</h2><button class="text-sm font-bold text-acid" data-view="activity">View all</button></div>
          <div class="space-y-3">${recent}</div>
        </section>
      </div>
      <aside class="space-y-5">
        ${quickActions()}
        ${networkCard()}
      </aside>
    </div>
  `;
}

function sendView() {
  if (signedTransaction) return signedTransactionView(signedTransaction);
  const selectedSymbol = sendDraft.symbol || "ETH";

  return `
    <section class="glass max-w-3xl rounded-[2rem] p-6">
      <p class="text-sm uppercase tracking-[0.3em] text-slate-500">Transfer</p>
      <h2 class="mt-2 text-3xl font-black">Send crypto</h2>
      <p class="mt-3 text-sm leading-6 text-slate-400">Transactions are signed locally before broadcast to the simulator. Review the signature before funds leave your simulated balance.</p>
      <form data-action="sign-transaction" class="mt-6 grid gap-4">
        <label class="space-y-2"><span class="text-sm font-bold text-slate-300">Recipient address</span><input class="field" name="to" data-recipient-address placeholder="${addressPlaceholder(selectedSymbol)}" value="${escapeHtml(sendDraft.to)}" required /></label>
        <div class="grid gap-4 sm:grid-cols-2">
          <label class="space-y-2"><span class="text-sm font-bold text-slate-300">Asset</span>${assetSelect("symbol", selectedSymbol, "data-send-symbol")}</label>
          <label class="space-y-2"><span class="text-sm font-bold text-slate-300">Amount</span><input class="field" name="amount" type="number" min="0.000001" step="0.000001" value="${escapeHtml(sendDraft.amount)}" required /></label>
        </div>
        <label class="space-y-2"><span class="text-sm font-bold text-slate-300">Note</span><input class="field" name="note" placeholder="Optional transaction memo" value="${escapeHtml(sendDraft.note)}" /></label>
        <button class="btn-primary justify-self-start" type="submit">Sign transaction</button>
      </form>
    </section>
  `;
}

function signedTransactionView(signed: SignedTransaction) {
  return `
    <section class="glass max-w-4xl rounded-[2rem] p-6">
      <div class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
        <div>
          <p class="text-sm uppercase tracking-[0.3em] text-acid">Signed transfer</p>
          <h2 class="mt-2 text-3xl font-black">Review signature</h2>
          <p class="mt-3 max-w-2xl text-sm leading-6 text-slate-400">The backend signed this simulated transaction locally. Broadcast only if the details match your intent.</p>
        </div>
        <span class="rounded-full bg-acid/15 px-3 py-1 text-xs font-black uppercase tracking-[0.2em] text-acid">Ready</span>
      </div>
      <div class="mt-6 grid gap-4 sm:grid-cols-2">
        ${signedDetail("From", shortAddress(signed.from))}
        ${signedDetail("To", shortAddress(signed.to))}
        ${signedDetail("Amount", `${signed.amount.toFixed(6)} ${signed.symbol}`)}
        ${signedDetail("Network fee", `${signed.feeAmount.toFixed(6)} ${signed.feeSymbol}`)}
        ${signedDetail("Total debit", `${signed.totalDebit.toFixed(6)} ${signed.symbol}`)}
        ${signedDetail("Post-send balance", `${signed.postBalance.toFixed(6)} ${signed.symbol}`)}
        ${signedDetail("Estimated value", money(signed.fiatValue))}
        ${signedDetail("Network", signed.network)}
        ${signedDetail("Nonce", signed.nonce)}
        ${signedDetail("Signed", new Date(signed.signedAt).toLocaleString())}
      </div>
      <div class="mt-4 space-y-4">
        ${signedDetail("Payload hash", signed.payloadHash, true)}
        ${signedDetail("Signature", signed.signature, true)}
      </div>
      <div class="mt-6 flex flex-col gap-3 sm:flex-row">
        <button class="btn-primary" data-action="broadcast-signed-transaction" type="button">Broadcast signed transaction</button>
        <button class="btn-secondary" data-action="edit-signed-transaction" type="button">Edit transaction</button>
      </div>
    </section>
  `;
}

function signedDetail(label: string, value: string, mono = false) {
  return `
    <div class="rounded-2xl border border-white/10 bg-white/[0.035] p-4">
      <p class="text-xs uppercase tracking-[0.22em] text-slate-500">${escapeHtml(label)}</p>
      <p class="mt-2 ${mono ? "break-all font-mono text-xs" : "text-sm font-bold"} text-slate-200">${escapeHtml(value)}</p>
    </div>
  `;
}

function receiveView() {
  const network = selectedReceiveNetwork();
  const address = receiveAddress(network);
  const payload = receivePayload();
  const qrContent = payload ? qrSvg || `<span class="text-sm font-bold text-slate-500">Generating QR...</span>` : `<span class="text-sm font-bold text-slate-500">Receive is not available for this network yet.</span>`;
  const qrActionsDisabled = payload ? "" : "disabled";
  return `
    <section class="glass max-w-3xl rounded-[2rem] p-6">
      <p class="text-sm uppercase tracking-[0.3em] text-slate-500">Receive</p>
      <h2 class="mt-2 text-3xl font-black">Deposit address</h2>
      <div class="mt-6 grid gap-4 sm:grid-cols-2">
        <label class="space-y-2"><span class="text-sm font-bold text-slate-300">Receive network</span>${receiveNetworkSelect()}</label>
        <label class="space-y-2"><span class="text-sm font-bold text-slate-300">QR resilience</span>${qrResilienceSelect()}</label>
      </div>
      <div class="mt-6 rounded-3xl border border-dashed border-acid/40 bg-acid/10 p-6 text-center">
        <div class="qr-code mx-auto flex h-56 w-56 items-center justify-center rounded-2xl bg-white p-4 shadow-glow">${qrContent}</div>
        <div class="mt-4 flex flex-col justify-center gap-3 sm:flex-row">
          <button class="btn-secondary" data-action="copy-qr" type="button" ${qrActionsDisabled}>${iconCopy()} Copy SVG</button>
          <button class="btn-secondary" data-action="download-qr" type="button" ${qrActionsDisabled}>${iconDownload()} Download SVG</button>
        </div>
        <div class="mt-5 rounded-2xl border border-white/10 bg-white/[0.04] p-4 text-left">
          <div class="flex flex-col gap-1 sm:flex-row sm:items-center sm:justify-between">
            <p class="font-black">${escapeHtml(network.name)} receive URI</p>
            <span class="text-sm text-slate-400">${escapeHtml(receiveNetworkDetail(network))}</span>
          </div>
          <p class="mt-3 break-all font-mono text-xs text-slate-400">${escapeHtml(payload)}</p>
        </div>
        <p class="mt-5 break-all font-mono text-sm text-slate-200">${escapeHtml(address)}</p>
        <button class="btn-primary mt-5" data-action="copy-receive-address" type="button">Copy address</button>
      </div>
    </section>
  `;
}

function swapView() {
  return `
    <section class="glass max-w-3xl rounded-[2rem] p-6">
      <p class="text-sm uppercase tracking-[0.3em] text-slate-500">Exchange</p>
      <h2 class="mt-2 text-3xl font-black">Swap assets</h2>
      <form data-action="swap-tokens" class="mt-6 grid gap-4">
        <div class="grid gap-4 sm:grid-cols-2">
          <label class="space-y-2"><span class="text-sm font-bold text-slate-300">From</span>${assetSelect("fromSymbol")}</label>
          <label class="space-y-2"><span class="text-sm font-bold text-slate-300">To</span>${assetSelect("toSymbol", "USDC")}</label>
        </div>
        <label class="space-y-2"><span class="text-sm font-bold text-slate-300">Amount</span><input class="field" name="amount" type="number" min="0.000001" step="0.000001" required /></label>
        <button class="btn-primary justify-self-start" type="submit">Execute simulated swap</button>
      </form>
    </section>
  `;
}

function assetsView() {
  const assets = session?.assets ?? [];
  return `
    <section class="glass rounded-[2rem] p-6">
      <div class="mb-5 flex items-center justify-between"><h2 class="text-2xl font-black">Assets</h2><span class="text-sm text-slate-500">${assets.length} tracked</span></div>
      <div class="grid gap-4 lg:grid-cols-2">${assets.map(assetCard).join("") || emptyState("No assets tracked", "Unlock or create a wallet to view simulated asset balances.")}</div>
    </section>
  `;
}

function activityView() {
  const selected = selectedActivity();
  return `
    <div class="grid gap-5 xl:grid-cols-[1fr_0.85fr]">
      <section class="glass rounded-[2rem] p-6">
        <h2 class="text-2xl font-black">Activity</h2>
        <div class="mt-5 space-y-3">${session?.activity.map(activityRow).join("") || emptyState("No activity yet", "Your signed sends, swaps, and network changes will appear here.")}</div>
      </section>
      ${activityDetails(selected)}
    </div>
  `;
}

function settingsView() {
  return `
    <div class="grid gap-5 xl:grid-cols-[0.95fr_1fr]">
      <section class="glass rounded-[2rem] p-6">
        <p class="text-sm uppercase tracking-[0.3em] text-slate-500">Preferences</p>
        <h2 class="mt-2 text-3xl font-black">Wallet settings</h2>
        <form data-action="set-network" class="mt-6 space-y-4">
          <label class="space-y-2"><span class="text-sm font-bold text-slate-300">Active network</span>
            <select class="field" name="network">
              ${["Ethereum", "Polygon", "Arbitrum", "Base", "Optimism"].map((network) => `<option ${session?.network === network ? "selected" : ""}>${network}</option>`).join("")}
            </select>
          </label>
          <button class="btn-primary" type="submit">Save network</button>
        </form>
        <div class="mt-6 rounded-2xl border border-amber-400/25 bg-amber-400/10 p-4 text-sm text-amber-100">This build simulates balances and transactions. Connect audited chain clients and hardware-backed signing before using real funds.</div>
      </section>
      <section class="glass rounded-[2rem] p-6">
        <p class="text-sm uppercase tracking-[0.3em] text-acid">Security center</p>
        <h2 class="mt-2 text-3xl font-black">Local protection</h2>
        <div class="mt-6 grid gap-3 sm:grid-cols-2">
          ${securityTile("Storage", "AES-GCM encrypted")}
          ${securityTile("Key derivation", "Argon2 passphrase key")}
          ${securityTile("Mode", "Simulated signing")}
          ${securityTile("Lock state", session?.locked ? "Locked" : "Unlocked")}
        </div>
        <div class="mt-6 rounded-2xl border border-rose-400/25 bg-rose-400/10 p-4">
          <h3 class="font-black text-rose-100">Danger zone</h3>
          <p class="mt-2 text-sm leading-6 text-rose-100/80">Remove the encrypted local wallet file and return this app to onboarding.</p>
          <button class="btn-danger mt-4" data-action="clear-wallet" type="button">Clear local wallet</button>
        </div>
      </section>
    </div>
  `;
}

function securityTile(label: string, value: string) {
  return `<div class="rounded-2xl border border-white/10 bg-white/[0.035] p-4"><p class="text-xs uppercase tracking-[0.22em] text-slate-500">${escapeHtml(label)}</p><p class="mt-2 font-black text-slate-100">${escapeHtml(value)}</p></div>`;
}

function quickActions() {
  return `
    <section class="glass rounded-[2rem] p-5">
      <h2 class="text-xl font-black">Quick actions</h2>
      <div class="mt-4 grid gap-3">
        <button class="btn-primary w-full" data-view="send" type="button">Send</button>
        <button class="btn-secondary w-full" data-view="receive" type="button">Receive</button>
        <button class="btn-secondary w-full" data-view="swap" type="button">Swap</button>
      </div>
    </section>
  `;
}

function networkCard() {
  return `
    <section class="glass rounded-[2rem] p-5">
      <h2 class="text-xl font-black">Network health</h2>
      <div class="mt-4 space-y-3 text-sm text-slate-300">
        <div class="flex justify-between"><span>RPC latency</span><strong class="text-acid">42 ms</strong></div>
        <div class="flex justify-between"><span>Gas estimate</span><strong>18 gwei</strong></div>
        <div class="flex justify-between"><span>Protections</span><strong>Enabled</strong></div>
      </div>
    </section>
  `;
}

function featureCard(title: string, body: string) {
  return `<div class="rounded-2xl border border-white/10 bg-white/[0.04] p-5"><h3 class="font-black">${title}</h3><p class="mt-2 text-sm leading-6 text-slate-400">${body}</p></div>`;
}

function passphraseMeter() {
  return `<div class="passphrase-meter" data-passphrase-meter data-score="0"><div class="passphrase-meter-track"><div></div></div><p class="mt-2 text-xs font-bold text-slate-500">Strength: <span data-passphrase-label>Too weak</span></p></div>`;
}

function navButton(view: View, label: string) {
  return `<button class="nav-item ${currentView === view ? "active" : ""}" data-view="${view}" type="button">${label}</button>`;
}

function mobileNav() {
  return `
    <nav class="mobile-nav glass">
      ${mobileNavButton("dashboard", "Home")}
      ${mobileNavButton("send", "Send")}
      ${mobileNavButton("receive", "Receive")}
      ${mobileNavButton("activity", "Activity")}
      ${mobileNavButton("settings", "Secure")}
    </nav>
  `;
}

function mobileNavButton(view: View, label: string) {
  return `<button class="mobile-nav-item ${currentView === view ? "active" : ""}" data-view="${view}" type="button">${label}</button>`;
}

function assetCard(asset: Asset) {
  const value = assetValue(asset);
  const positive = asset.change_24h >= 0;
  const total = session ? session.assets.reduce((s, a) => s + assetValue(a), 0) : 0;
  const allocation = total ? (value / total) * 100 : 0;
  return `
    <article class="asset-card rounded-3xl border border-white/10 bg-white/[0.04] p-5">
      <div class="flex items-start justify-between gap-4">
        <div class="asset-card-header"><p class="truncate text-lg font-black">${escapeHtml(asset.symbol)}</p><p class="truncate text-sm text-slate-500">${escapeHtml(asset.name)}</p></div>
        <span class="asset-change rounded-full ${positive ? "bg-emerald-400/10 text-emerald-300" : "bg-rose-400/10 text-rose-300"} px-3 py-1 text-xs font-bold">${positive ? "+" : ""}${asset.change_24h.toFixed(2)}%</span>
      </div>
      <p class="asset-value mt-5 text-2xl font-black">${money(value)}</p>
      <p class="mt-1 text-sm text-slate-400">${asset.balance.toFixed(asset.symbol === "USDC" ? 2 : 5)} ${escapeHtml(asset.symbol)}</p>
      <div class="mt-4">
        <div class="flex justify-between text-xs font-bold text-slate-500"><span>Allocation</span><span>${allocation.toFixed(1)}%</span></div>
        <div class="mt-2 h-2 overflow-hidden rounded-full bg-slate-900"><div class="h-full rounded-full bg-acid" style="width: ${Math.max(2, allocation).toFixed(1)}%"></div></div>
      </div>
    </article>
  `;
}

function assetValue(asset: Asset) {
  return asset.balance * asset.price_usd;
}

function portfolioChange() {
  const assets = session?.assets ?? [];
  const total = assets.reduce((t, a) => t + assetValue(a), 0);
  if (!total) return 0;
  return assets.reduce((acc, asset) => acc + asset.change_24h * (assetValue(asset) / total), 0);
}

function emptyState(title: string, body: string) {
  return `<div class="rounded-3xl border border-dashed border-white/10 bg-white/[0.025] p-6 text-center"><p class="font-black text-slate-200">${escapeHtml(title)}</p><p class="mt-2 text-sm leading-6 text-slate-500">${escapeHtml(body)}</p></div>`;
}

function activityRow(item: Activity) {
  return `
    <article class="flex cursor-pointer flex-col gap-3 rounded-2xl border ${selectedActivityId === item.id ? "border-acid/50 bg-acid/10" : "border-white/10 bg-white/[0.035]"} p-4 sm:flex-row sm:items-center sm:justify-between" data-action="select-activity" data-activity-id="${escapeHtml(item.id)}">
      <div><p class="font-black">${escapeHtml(item.title)}</p><p class="mt-1 text-sm text-slate-500">${escapeHtml(item.subtitle)} - ${new Date(item.timestamp).toLocaleString()}</p></div>
      <div class="text-left sm:text-right"><p class="font-mono font-bold">${escapeHtml(item.amount ?? "")}</p><p class="text-xs uppercase tracking-[0.2em] text-acid">${escapeHtml(item.status)}</p></div>
    </article>
  `;
}

function selectedActivity() {
  const activity = session?.activity ?? [];
  return activity.find((item) => item.id === selectedActivityId) ?? activity[0] ?? null;
}

function activityDetails(item: Activity | null) {
  if (!item) {
    return `<section class="glass rounded-[2rem] p-6"><p class="text-sm text-slate-400">No activity selected.</p></section>`;
  }

  return `
    <section class="glass rounded-[2rem] p-6">
      <p class="text-sm uppercase tracking-[0.3em] text-slate-500">Activity details</p>
      <h2 class="mt-2 text-2xl font-black">${escapeHtml(item.title)}</h2>
      <div class="mt-5 space-y-3">
        ${detailRow("Status", item.status)}
        ${detailRow("Amount", item.amount ?? "n/a")}
        ${detailRow("Fee", item.fee ?? "n/a")}
        ${detailRow("Network", item.network ?? session?.network ?? "n/a")}
        ${detailRow("Timestamp", new Date(item.timestamp).toLocaleString())}
        ${copyableDetailRow("Transaction hash", item.hash)}
        ${item.from ? copyableDetailRow("From", item.from) : ""}
        ${item.to ? copyableDetailRow("To", item.to) : ""}
        ${item.payload_hash ? copyableDetailRow("Payload hash", item.payload_hash) : ""}
        ${item.signature ? copyableDetailRow("Signature", item.signature) : ""}
      </div>
    </section>
  `;
}

function detailRow(label: string, value: string) {
  return `<div class="rounded-2xl border border-white/10 bg-white/[0.035] p-4"><p class="text-xs uppercase tracking-[0.22em] text-slate-500">${escapeHtml(label)}</p><p class="mt-2 break-all text-sm font-bold text-slate-200">${escapeHtml(value)}</p></div>`;
}

function copyableDetailRow(label: string, value: string) {
  return `<div class="rounded-2xl border border-white/10 bg-white/[0.035] p-4"><div class="flex items-start justify-between gap-3"><div class="min-w-0"><p class="text-xs uppercase tracking-[0.22em] text-slate-500">${escapeHtml(label)}</p><p class="mt-2 break-all font-mono text-xs text-slate-200">${escapeHtml(value)}</p></div><button class="btn-secondary shrink-0 text-xs" data-action="copy-value" data-copy-value="${escapeHtml(value)}" type="button">Copy</button></div></div>`;
}

function assetSelect(name: string, selected = "ETH", attributes = "") {
  return `<select class="field" name="${name}" ${attributes}>${session?.assets.map((asset) => `<option value="${asset.symbol}" ${asset.symbol === selected ? "selected" : ""}>${asset.symbol} - ${asset.name}</option>`).join("") ?? ""}</select>`;
}

function updateRecipientPlaceholder(symbol: string) {
  const input = document.querySelector<HTMLInputElement>("[data-recipient-address]");
  if (input) input.placeholder = addressPlaceholder(symbol);
}

function addressPlaceholder(symbol: string) {
  const placeholders: Record<string, string> = {
    BTC: "bc1... / 1... / 3...",
    SOL: "Solana address",
  };
  return placeholders[symbol] ?? "0x...";
}

function iconCopy() {
  return `<svg aria-hidden="true" class="h-4 w-4" viewBox="0 0 24 24" fill="none"><path d="M8 8.5C8 7.12 9.12 6 10.5 6h6C17.88 6 19 7.12 19 8.5v8c0 1.38-1.12 2.5-2.5 2.5h-6A2.5 2.5 0 0 1 8 16.5v-8Z" stroke="currentColor" stroke-width="1.8"/><path d="M5 14.5v-8C5 5.12 6.12 4 7.5 4h6" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/></svg>`;
}

function iconDownload() {
  return `<svg aria-hidden="true" class="h-4 w-4" viewBox="0 0 24 24" fill="none"><path d="M12 4v10m0 0 4-4m-4 4-4-4" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/><path d="M5 16.5V18a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-1.5" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/></svg>`;
}

function receiveNetworkSelect() {
  return `<select class="field" data-receive-network>${receiveNetworks.map((network) => `<option value="${network.id}" ${network.id === receiveNetworkId ? "selected" : ""}>${network.name} - ${receiveNetworkDetail(network)}</option>`).join("")}</select>`;
}

function qrResilienceSelect() {
  return `<select class="field" data-receive-resilience>${qrResilienceOptions.map((option) => `<option value="${option.value}" ${option.value === qrResilience ? "selected" : ""}>${option.label} (${option.value}) - ${option.detail}</option>`).join("")}</select>`;
}

function selectedReceiveNetwork() {
  return receiveNetworks.find((network) => network.id === receiveNetworkId) ?? receiveNetworks[0];
}

function receiveNetworkDetail(network: ReceiveNetwork) {
  if (network.kind === "evm") return `Chain ID ${network.chainId} - ${network.ticker}`;
  if (network.kind === "bitcoin") return network.ticker + " - " + (network.isTestNet ? "Testnet" : "Mainnet");
  return `${network.ticker} - Simulated`;
}

function receivePayload() {
  const network = selectedReceiveNetwork();
  const address = receiveAddress(network);
  if (!address) return "";
  if (network.kind === "evm") return `ethereum:${address}@${network.chainId}`;
  if (network.kind === "bitcoin") return `bitcoin:${address}`;
  if (network.kind === "solana") return `solana:${address}`;
  return "";
}

function receiveAddress(network: ReceiveNetwork) {
  const address = session?.address ?? "";
  if (!address) return "";
  const seed = address.replace(/[^a-fA-F0-9]/g, "");
  if (network.kind === "bitcoin") return `bc1q${seed.toLowerCase().padEnd(38, "0").slice(0, 38)}`;
  if (network.kind === "solana") return seed.padEnd(44, "7").slice(0, 44);
  return address;
}

async function ensureReceiveQr() {
  if (currentView !== "receive" || !session?.address || session?.locked) return;

  const payload = receivePayload();
  if (!payload) {
    if (qrSvg || qrKey || qrGeneratingKey) {
      resetQr();
      render();
    }
    return;
  }

  const nextQrKey = `${payload}:${qrResilience}`;
  if ((qrKey === nextQrKey && qrSvg) || qrGeneratingKey === nextQrKey) return;

  qrGeneratingKey = nextQrKey;
  try {
    const svg = await QRCode.toString(payload, {
      type: "svg",
      margin: 2,
      errorCorrectionLevel: qrResilience,
      color: {
        dark: "#071013",
        light: "#ffffff",
      },
    });

    if (qrGeneratingKey === nextQrKey) {
      qrKey = nextQrKey;
      qrSvg = svg;
      render();
    }
  } catch (error) {
    pushToast(formatError(error), "error");
    resetQr();
    render();
  } finally {
    if (qrGeneratingKey === nextQrKey) qrGeneratingKey = "";
  }
}

function resetQr() {
  qrSvg = "";
  qrKey = "";
}

function loadingBar() {
  return `<div class="fixed left-0 top-0 z-50 h-1 w-full overflow-hidden bg-slate-900"><div class="h-full w-1/2 animate-pulse bg-acid"></div></div>`;
}

function pushToast(message: string, tone: Toast["tone"]) {
  const toast: Toast = {
    id: ++toastId,
    message,
    tone,
    createdAt: Date.now(),
    duration: 4_500,
    exiting: false,
  };

  toasts = [...toasts, toast];
  renderToasts();
  window.setTimeout(() => dismissToast(toast.id), toast.duration);
}

function dismissToast(id: number) {
  const toast = toasts.find((item) => item.id === id);
  if (!toast || toast.exiting) return;

  toasts = toasts.map((item) => (item.id === id ? { ...item, exiting: true } : item));
  renderToasts();
  window.setTimeout(() => {
    toasts = toasts.filter((item) => item.id !== id);
    enteredToasts.delete(id);
    renderToasts();
  }, 240);
}

function renderToasts() {
  const previousTops = new Map<number, number>();
  toastRoot.querySelectorAll<HTMLElement>("[data-toast-id]").forEach((element) => {
    previousTops.set(Number(element.dataset.toastId), element.getBoundingClientRect().top);
  });

  toastRoot.innerHTML = toasts.map(toastHtml).join("");

  toastRoot.querySelectorAll<HTMLElement>("[data-toast-id]").forEach((element) => {
    const id = Number(element.dataset.toastId);
    const previousTop = previousTops.get(id);
    const nextTop = element.getBoundingClientRect().top;

    if (previousTop !== undefined && previousTop !== nextTop && !element.classList.contains("toast-exit")) {
      element.animate([{ transform: `translateY(${previousTop - nextTop}px)` }, { transform: "translateY(0)" }], {
        duration: 260,
        easing: "cubic-bezier(.2, .9, .2, 1)",
      });
    }

    enteredToasts.add(id);
  });
}

function toastHtml(toast: Toast) {
  const elapsed = Date.now() - toast.createdAt;
  const entryClass = enteredToasts.has(toast.id) || toast.exiting ? "" : "toast-enter";
  const exitClass = toast.exiting ? "toast-exit" : "";
  const toneClass = toast.tone === "error" ? "toast-error" : "toast-success";

  return `
    <article class="toast-card ${toneClass} ${entryClass} ${exitClass}" data-toast-id="${toast.id}">
      <div class="flex items-start gap-3">
        <div class="toast-dot"></div>
        <p class="text-sm font-bold leading-6">${escapeHtml(toast.message)}</p>
      </div>
      <div class="toast-track"><div class="toast-progress" style="animation-duration: ${toast.duration}ms; animation-delay: -${Math.min(elapsed, toast.duration)}ms;"></div></div>
    </article>
  `;
}

function successMessage(command: string) {
  const messages: Record<string, string> = {
    create_wallet: "Wallet created. Recovery phrase was generated in the Rust backend.",
    import_wallet: "Wallet imported successfully.",
    unlock_wallet: "Wallet unlocked.",
    lock_wallet: "Wallet locked.",
    clear_wallet: "Local wallet cleared.",
    sign_transaction: "Transaction signed locally.",
    send_transaction: "Signed transaction broadcast to the local simulator.",
    swap_tokens: "Swap completed in the local simulator.",
    set_network: "Network updated.",
    refresh_prices: "Market prices refreshed.",
  };
  return messages[command] ?? "Updated.";
}

function formatError(error: unknown) {
  return `Error: ${String(error)}`;
}

function money(value: number) {
  return new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 2 }).format(value);
}

function shortAddress(address: string | null) {
  if (!address) return "No address";
  return `${address.slice(0, 10)}...${address.slice(-8)}`;
}

function escapeHtml(value: string) {
  return value.replace(/[&<>'"]/g, (char) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", "'": "&#39;", '"': "&quot;" })[char] ?? char);
}
