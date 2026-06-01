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
  amount: string;
  status: string;
  timestamp: string;
  hash: string;
};

type WalletSession = {
  has_wallet: boolean;
  locked: boolean;
  wallet_name: string | null;
  address: string | null;
  network: string;
  fiat_balance: number;
  risk_score: number;
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
};

type View = "dashboard" | "send" | "receive" | "swap" | "assets" | "activity" | "settings";

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

type BitcoinReceiveNetwork = {
  kind: "bitcoin";
  id: string;
  name: string;
  network: "mainnet" | "testnet";
  ticker: "BTC";
  isTestNet: boolean;
};

type LightningReceiveNetwork = {
  kind: "lightning";
  id: string;
  name: string;
  ticker: "BTC";
  isTestNet: boolean;
};

type ReceiveNetworkInput = EvmReceiveNetworkInput | BitcoinReceiveNetwork | LightningReceiveNetwork;
type ReceiveNetwork = EvmReceiveNetwork | BitcoinReceiveNetwork | LightningReceiveNetwork;

type QrResilience = "L" | "M" | "Q" | "H";

type Toast = {
  id: number;
  message: string;
  tone: "success" | "warning" | "error";
  createdAt: number;
  duration: number;
  exiting: boolean;
};

const rawReceiveNetworks: ReceiveNetworkInput[] = [
  { kind: "evm", id: "ethereum", name: "Ethereum", chainId: 1, ticker: "ETH", vm_type: "EVM" },
  { kind: "evm", id: "polygon", name: "Polygon", chainId: 137, ticker: "MATIC", vm_type: "EVM", isL2: true },
  { kind: "evm", id: "arbitrum_one", name: "Arbitrum One", chainId: 42161, ticker: "ETH", vm_type: "EVM", isL2: true },
  { kind: "evm", id: "base", name: "Base", chainId: 8453, ticker: "ETH", vm_type: "EVM", isL2: true },
  { kind: "evm", id: "optimism", name: "Optimism", chainId: 10, ticker: "ETH", vm_type: "EVM", isL2: true },
  { kind: "evm", id: "avalanche_c", name: "Avalanche C-Chain", chainId: 43114, ticker: "AVAX", vm_type: "EVM" },
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

  return network;
}

const qrResilienceOptions: Array<{ value: QrResilience; label: string; detail: string }> = [
  { value: "L", label: "Low", detail: "~7% recovery" },
  { value: "M", label: "Medium", detail: "~15% recovery" },
  { value: "Q", label: "Quartile", detail: "~25% recovery" },
  { value: "H", label: "High", detail: "~30% recovery" },
];

const app = document.querySelector<HTMLDivElement>("#app");

let session: WalletSession | null = null;
let currentView: View = "dashboard";
let receiveNetworkId = "ethereum";
let qrResilience: QrResilience = "M";
let qrSvg = "";
let qrKey = "";
let qrGeneratingKey = "";
let signedTransaction: SignedTransaction | null = null;
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
    session = await invoke<WalletSession>("get_wallet");
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

    if (action === "lock") void runCommand("lock_wallet");
    if (action === "refresh") void loadSession();
    if (action === "copy-address") void copyAddress();
    if (action === "copy-qr") void copyQrPayload();
    if (action === "download-qr") downloadQrSvg();
    if (action === "broadcast-signed-transaction") void broadcastSignedTransaction();
    if (action === "edit-signed-transaction") {
      signedTransaction = null;
      render();
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

  document.addEventListener("wheel", (event) => {
    const rail = (event.target as HTMLElement).closest<HTMLElement>(".asset-scroll");
    if (!rail || rail.scrollWidth <= rail.clientWidth || Math.abs(event.deltaX) > Math.abs(event.deltaY)) return;

    event.preventDefault();
    rail.scrollLeft += event.deltaY;
  }, { passive: false });
}

async function createWallet(form: HTMLFormElement) {
  const formData = new FormData(form);
  await runCommand("create_wallet", {
    name: String(formData.get("name") || "Primary Wallet"),
    passphrase: String(formData.get("passphrase") || ""),
  });
}

async function importWallet(form: HTMLFormElement) {
  const formData = new FormData(form);
  await runCommand("import_wallet", {
    mnemonic: String(formData.get("mnemonic") || ""),
    passphrase: String(formData.get("passphrase") || ""),
  });
}

async function unlockWallet(form: HTMLFormElement) {
  const formData = new FormData(form);
  await runCommand("unlock_wallet", {
    passphrase: String(formData.get("passphrase") || ""),
  });
}

async function signTransaction(form: HTMLFormElement) {
  const formData = new FormData(form);
  busy = true;
  render();
  try {
    signedTransaction = await invoke<SignedTransaction>("sign_transaction", {
      to: String(formData.get("to") || ""),
      symbol: String(formData.get("symbol") || "ETH"),
      amount: Number(formData.get("amount") || 0),
      note: String(formData.get("note") || ""),
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

  const ok = await runCommand("send_transaction", { signed: signedTransaction });
  if (ok) {
    signedTransaction = null;
    render();
  }
}

async function swapTokens(form: HTMLFormElement) {
  const formData = new FormData(form);
  await runCommand("swap_tokens", {
    fromSymbol: String(formData.get("fromSymbol") || "ETH"),
    toSymbol: String(formData.get("toSymbol") || "USDC"),
    amount: Number(formData.get("amount") || 0),
  });
}

async function setNetwork(form: HTMLFormElement) {
  const formData = new FormData(form);
  await runCommand("set_network", {
    network: String(formData.get("network") || "Ethereum"),
  });
}

async function runCommand(command: string, args?: Record<string, unknown>) {
  busy = true;
  render();
  try {
    const result = await invoke<WalletSession | null>(command, args);
    if (result) session = result;
    if (command === "lock_wallet") {
      session = await invoke<WalletSession>("get_wallet");
      currentView = "dashboard";
    }
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
  await navigator.clipboard.writeText(session.address);
  pushToast("Receive address copied.", "success");
}

async function copyQrPayload() {
  if (!qrSvg) {
    pushToast("QR code is still generating.", "error");
    return;
  }

  await navigator.clipboard.writeText(qrSvg);
  pushToast("QR SVG copied.", "success");
}

function downloadQrSvg() {
  if (!qrSvg || !session?.address) return;

  const network = selectedReceiveNetwork();
  const blob = new Blob([qrSvg], { type: "image/svg+xml;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = `vaultforge-${network.id}-${shortAddress(session.address).replace(/[^a-zA-Z0-9]/g, "")}-qr.svg`;
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
  if (session.locked) return lockedWallet();
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
          <label class="block space-y-2"><span class="text-sm font-bold text-slate-300">Passphrase</span><input class="field" name="passphrase" type="password" minlength="8" placeholder="Minimum 8 characters" required /></label>
          <button class="btn-primary w-full" type="submit">Generate wallet</button>
        </form>
        <div class="my-7 h-px bg-white/10"></div>
        <form data-action="import-wallet" class="space-y-4">
          <h3 class="font-black">Import existing wallet</h3>
          <label class="block space-y-2"><span class="text-sm font-bold text-slate-300">Recovery phrase</span><textarea class="field min-h-28" name="mnemonic" placeholder="12 or 24 word phrase" required></textarea></label>
          <label class="block space-y-2"><span class="text-sm font-bold text-slate-300">New local passphrase</span><input class="field" name="passphrase" type="password" minlength="8" required /></label>
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
    <div class="mx-auto grid max-w-[1500px] gap-5 lg:grid-cols-[280px_1fr]">
      <aside class="glass rounded-[2rem] p-5 lg:sticky lg:top-5 lg:h-[calc(100vh-2.5rem)]">
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
    </div>
  `;
}

function topBar() {
  if (!session) return "";
  return `
    <header class="glass flex flex-col gap-4 rounded-[2rem] p-5 sm:flex-row sm:items-center sm:justify-between">
      <div>
        <p class="text-sm uppercase tracking-[0.3em] text-slate-500">Total balance</p>
        <h1 class="mt-1 text-4xl font-black">${money(session.fiat_balance)}</h1>
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
  const recent = session.activity.slice(0, 5).map(activityRow).join("");
  return `
    <div class="grid gap-5 xl:grid-cols-[1.35fr_0.75fr]">
      <div class="min-w-0 space-y-5">
        <section class="glass min-w-0 overflow-hidden rounded-[2rem] p-6">
          <div class="flex flex-col gap-6 lg:flex-row lg:items-end lg:justify-between">
            <div>
              <p class="text-sm uppercase tracking-[0.3em] text-acid">Portfolio</p>
              <h2 class="mt-3 max-w-2xl text-4xl font-black tracking-tight">Multi-asset wallet with transaction controls.</h2>
            </div>
            <div class="rounded-2xl border border-acid/30 bg-acid/10 p-4 text-right">
              <p class="text-sm text-slate-400">Security score</p>
              <p class="text-3xl font-black text-acid">${session.risk_score}/100</p>
            </div>
          </div>
          <div class="asset-scroll mt-6">${topAssets}</div>
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

  return `
    <section class="glass max-w-3xl rounded-[2rem] p-6">
      <p class="text-sm uppercase tracking-[0.3em] text-slate-500">Transfer</p>
      <h2 class="mt-2 text-3xl font-black">Send crypto</h2>
      <p class="mt-3 text-sm leading-6 text-slate-400">Transactions are signed locally before broadcast to the simulator. Review the signature before funds leave your simulated balance.</p>
      <form data-action="sign-transaction" class="mt-6 grid gap-4">
        <label class="space-y-2"><span class="text-sm font-bold text-slate-300">Recipient address</span><input class="field" name="to" data-recipient-address placeholder="${addressPlaceholder("ETH")}" required /></label>
        <div class="grid gap-4 sm:grid-cols-2">
          <label class="space-y-2"><span class="text-sm font-bold text-slate-300">Asset</span>${assetSelect("symbol", "ETH", "data-send-symbol")}</label>
          <label class="space-y-2"><span class="text-sm font-bold text-slate-300">Amount</span><input class="field" name="amount" type="number" min="0.000001" step="0.000001" required /></label>
        </div>
        <label class="space-y-2"><span class="text-sm font-bold text-slate-300">Note</span><input class="field" name="note" placeholder="Optional transaction memo" /></label>
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
  const address = session?.address ?? "";
  const network = selectedReceiveNetwork();
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
        <button class="btn-primary mt-5" data-action="copy-address" type="button">Copy address</button>
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
  return `
    <section class="glass rounded-[2rem] p-6">
      <div class="mb-5 flex items-center justify-between"><h2 class="text-2xl font-black">Assets</h2><span class="text-sm text-slate-500">${session?.assets.length ?? 0} tracked</span></div>
      <div class="grid gap-4 lg:grid-cols-2">${session?.assets.map(assetCard).join("") ?? ""}</div>
    </section>
  `;
}

function activityView() {
  return `
    <section class="glass rounded-[2rem] p-6">
      <h2 class="text-2xl font-black">Activity</h2>
      <div class="mt-5 space-y-3">${session?.activity.map(activityRow).join("") ?? ""}</div>
    </section>
  `;
}

function settingsView() {
  return `
    <section class="glass max-w-3xl rounded-[2rem] p-6">
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
  `;
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

function navButton(view: View, label: string) {
  return `<button class="nav-item ${currentView === view ? "active" : ""}" data-view="${view}" type="button">${label}</button>`;
}

function assetCard(asset: Asset) {
  const value = assetValue(asset);
  const positive = asset.change_24h >= 0;
  return `
    <article class="asset-card rounded-3xl border border-white/10 bg-white/[0.04] p-5">
      <div class="flex items-start justify-between gap-4">
        <div class="asset-card-header"><p class="truncate text-lg font-black">${escapeHtml(asset.symbol)}</p><p class="truncate text-sm text-slate-500">${escapeHtml(asset.name)}</p></div>
        <span class="asset-change rounded-full ${positive ? "bg-emerald-400/10 text-emerald-300" : "bg-rose-400/10 text-rose-300"} px-3 py-1 text-xs font-bold">${positive ? "+" : ""}${asset.change_24h.toFixed(2)}%</span>
      </div>
      <p class="asset-value mt-5 text-2xl font-black">${money(value)}</p>
      <p class="mt-1 text-sm text-slate-400">${asset.balance.toFixed(asset.symbol === "USDC" ? 2 : 5)} ${escapeHtml(asset.symbol)}</p>
    </article>
  `;
}

function assetValue(asset: Asset) {
  return asset.balance * asset.price_usd;
}

function activityRow(item: Activity) {
  return `
    <article class="flex flex-col gap-3 rounded-2xl border border-white/10 bg-white/[0.035] p-4 sm:flex-row sm:items-center sm:justify-between">
      <div><p class="font-black">${escapeHtml(item.title)}</p><p class="mt-1 text-sm text-slate-500">${escapeHtml(item.subtitle)} - ${new Date(item.timestamp).toLocaleString()}</p></div>
      <div class="text-left sm:text-right"><p class="font-mono font-bold">${escapeHtml(item.amount)}</p><p class="text-xs uppercase tracking-[0.2em] text-acid">${escapeHtml(item.status)}</p></div>
    </article>
  `;
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
  return `${network.ticker} - Coming soon`;
}

function receivePayload() {
  const address = session?.address;
  if (!address) return "";

  const network = selectedReceiveNetwork();
  if (network.kind === "evm") return `ethereum:${address}@${network.chainId}`;
  return "";
}

async function ensureReceiveQr() {
  if (currentView !== "receive" || !session?.address || session.locked) return;

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
    sign_transaction: "Transaction signed locally.",
    send_transaction: "Signed transaction broadcast to the local simulator.",
    swap_tokens: "Swap completed in the local simulator.",
    set_network: "Network updated.",
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
