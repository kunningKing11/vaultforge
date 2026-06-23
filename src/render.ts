import { appRoot } from "./main";
import { escapeHtml, formatWei, money, shortAddress, weiToNumber } from "./format";
import { DEFAULT_NETWORK_ID, networkDisplayName, networks, normalizeNetworkId } from "./networks";
import { pushToast } from "./toasts";
import type { Activity, Asset, Network, NetworkId, QrResilience, SendDraft, SignedTransaction, WalletSession } from "./types";
import {
  appState,
  selectedNetwork,
  networkDetail,
  addressForNetwork,
  receivePayload,
  selectedActivity,
} from "./state";
import { ensureReceiveQr } from "./qr";

const qrResilienceOptions: Array<{ value: QrResilience; label: string; detail: string }> = [
  { value: "L", label: "Low", detail: "~7% recovery" },
  { value: "M", label: "Medium", detail: "~15% recovery" },
  { value: "Q", label: "Quartile", detail: "~25% recovery" },
  { value: "H", label: "High", detail: "~30% recovery" },
];

export function render() {
  appRoot.innerHTML = `
    <main class="noise min-h-screen px-4 py-5 text-slate-100 sm:px-6 lg:px-8">
      ${appState.busy ? loadingBar() : ""}
      ${renderBody()}
      ${lockedDeleteWalletModal()}
    </main>
  `;
  void ensureReceiveQr().then((needsRender) => {
    if (needsRender) render();
  });
}

function renderBody() {
  if (!appState.session && appState.busy) return splash();
  if (!appState.session?.has_wallet) return onboarding();
  if (appState.session?.locked) return lockedWallet();
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
          ${featureCard("Blazing fast Rust core", "Wallet state and validations run behind Tauri commands.")}
          ${featureCard("Fast UI", "Vite, TypeScript, and Tailwind power the frontend.")}
          ${featureCard("Local-first & maximum security", "Runs fully locally on your machine.")}
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
        <div class="mt-7 border-t border-rose-400/20 pt-5 text-left">
          <button class="btn-danger w-full" data-action="show-locked-delete-wallet" type="button">Delete stored wallet</button>
          <p class="mt-3 text-sm leading-6 text-rose-200/80">Only use this if you need to remove the encrypted local wallet from this device and your seed is backed up.</p>
        </div>
      </div>
    </section>
  `;
}

function lockedDeleteWalletModal() {
  if (!appState.session?.locked || appState.lockedDeleteStep === "idle") return "";

  if (appState.lockedDeleteStep === "confirm") {
    return `
      <div class="destructive-modal fixed inset-0 z-[70] flex items-center justify-center bg-slate-950/75 p-4 backdrop-blur-md">
        <section class="w-full max-w-2xl rounded-[2rem] border border-white/10 bg-slate-950/95 p-6 text-left text-slate-100 shadow-[0_30px_120px_rgba(2,6,23,0.72)] sm:p-8">
          <p class="text-xs font-black uppercase tracking-[0.3em] text-rose-300">Destructive Action</p>
          <h2 class="mt-3 text-3xl font-black text-white">Are you sure you want to do this?</h2>
          <p class="mt-4 text-sm leading-6 text-slate-300">Deleting stored wallet files is destructive. You could lose all your funds if your wallet seed is not backed up.</p>
          <div class="mt-6 rounded-2xl border border-rose-400/25 bg-rose-400/10 p-4 text-sm font-bold leading-6 text-rose-100">Only continue if you have verified your recovery phrase is backed up and usable.</div>
          <div class="mt-6 flex flex-col gap-3 sm:flex-row">
            <button class="btn-secondary flex-1" data-action="cancel-locked-delete-wallet" type="button">Cancel</button>
            <button class="btn-danger flex-1 whitespace-nowrap" data-action="start-locked-delete-wallet-countdown" type="button">Yes, I have backed up my wallet</button>
          </div>
        </section>
      </div>
    `;
  }

  return `
    <div class="destructive-modal fixed inset-0 z-[70] flex items-center justify-center bg-slate-950/75 p-4 backdrop-blur-md">
      <section class="w-full max-w-md rounded-[2rem] border border-rose-400/50 bg-rose-950/90 p-6 text-center text-rose-100 shadow-[0_30px_120px_rgba(127,29,29,0.55)] sm:p-8">
        <p class="text-xs font-black uppercase tracking-[0.3em] text-rose-300">Deletion Pending</p>
        <h2 class="mt-3 text-2xl font-black text-rose-50">Deleting wallet files in</h2>
        <p class="mt-5 text-7xl font-black text-rose-50">${appState.lockedDeleteRemaining}</p>
        <p class="mt-5 text-sm leading-6 text-rose-100/80">This will permanently remove the encrypted local wallet from this device.</p>
        <button class="btn-secondary mt-6 w-full" data-action="cancel-locked-delete-wallet" type="button">Cancel</button>
      </section>
    </div>
  `;
}

function walletShell() {
  if (!appState.session) return "";
  return `
    <div class="mx-auto grid max-w-[1500px] gap-5 pb-24 lg:grid-cols-[280px_1fr] lg:pb-0">
      <aside class="glass hidden rounded-[2rem] p-5 lg:sticky lg:top-5 lg:block lg:h-[calc(100vh-2.5rem)]">
        <div class="mb-8 flex items-center gap-3">
          <div class="flex h-12 w-12 items-center justify-center rounded-2xl bg-acid text-xl font-black text-ink">VF</div>
          <div><p class="font-black">${escapeHtml(appState.session.wallet_name ?? "VaultForge")}</p></div>
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
          <p class="mt-2 break-all font-mono text-sm text-slate-300">${escapeHtml(shortAddress(appState.session.address))}</p>
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
  if (!appState.session) return "";
  return `
    <header class="glass flex flex-col gap-4 rounded-[2rem] p-5 sm:flex-row sm:items-center sm:justify-between">
      <div>
        <p class="text-sm uppercase tracking-[0.3em] text-slate-500">Overview</p>
        <h1 class="mt-1 text-4xl font-black">${escapeHtml(appState.session.wallet_name ?? "VaultForge")}</h1>
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
  if (appState.currentView === "send") return sendView();
  if (appState.currentView === "receive") return receiveView();
  if (appState.currentView === "swap") return swapView();
  if (appState.currentView === "assets") return assetsView();
  if (appState.currentView === "activity") return activityView();
  if (appState.currentView === "settings") return settingsView();
  return dashboardView();
}

function dashboardView() {
  if (!appState.session) return "";
  const topAssets = [...appState.session.assets]
    .sort((left, right) => assetValue(right) - assetValue(left))
    .map(assetCard)
    .join("");
  const recent = appState.session.activity.slice(0, 5).map(activityRow).join("") || emptyState("No recent activity", "Sign, send, swap, or change networks to build a local activity timeline.");
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
      </aside>
    </div>
  `;
}

function sendView() {
  if (appState.signedTransaction) return signedTransactionView(appState.signedTransaction);
  const selectedSymbol = appState.sendDraft.symbol || "ETH";

  return `
    <section class="glass max-w-3xl rounded-[2rem] p-6">
      <p class="text-sm uppercase tracking-[0.3em] text-slate-500">Transfer</p>
      <h2 class="mt-2 text-3xl font-black">Send crypto</h2>
      <p class="mt-3 text-sm leading-6 text-slate-400">Transactions are signed locally before broadcast to the chain RPC. Review the signature before funds leave your balance.</p>
      <form data-action="sign-transaction" class="mt-6 grid gap-4">
        <label class="space-y-2"><span class="text-sm font-bold text-slate-300">Recipient address</span><input class="field" name="to" data-recipient-address placeholder="${addressPlaceholder(selectedSymbol)}" value="${escapeHtml(appState.sendDraft.to)}" required /></label>
        <div class="grid gap-4 sm:grid-cols-2">
          <label class="space-y-2"><span class="text-sm font-bold text-slate-300">Asset</span>${assetSelect("symbol", selectedSymbol, "data-send-symbol")}</label>
          <label class="space-y-2"><span class="text-sm font-bold text-slate-300">Amount</span><input class="field" name="amount" type="number" min="0.000001" step="0.000001" value="${escapeHtml(appState.sendDraft.amount)}" required /></label>
        </div>
        <label class="space-y-2"><span class="text-sm font-bold text-slate-300">Note</span><input class="field" name="note" placeholder="Optional transaction memo" value="${escapeHtml(appState.sendDraft.note)}" /></label>
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
          <p class="mt-3 max-w-2xl text-sm leading-6 text-slate-400">The backend signed this EIP-1559 transaction with the derived private key. Broadcast only if the details match your intent.</p>
        </div>
        <span class="rounded-full bg-acid/15 px-3 py-1 text-xs font-black uppercase tracking-[0.2em] text-acid">Ready</span>
      </div>
      <div class="mt-6 grid gap-4 sm:grid-cols-2">
        ${signedDetail("From", shortAddress(signed.from))}
        ${signedDetail("To", shortAddress(signed.to))}
        ${signedDetail("Amount", `${formatWei(signed.amount, signed.decimals)} ${signed.symbol}`)}
        ${signedDetail("Network fee", `${formatWei(signed.feeAmount, signed.decimals)} ${signed.feeSymbol}`)}
        ${signedDetail("Total debit", `${formatWei(signed.totalDebit, signed.decimals)} ${signed.symbol}`)}
        ${signedDetail("Post-send balance", `${formatWei(signed.postBalance, signed.decimals)} ${signed.symbol}`)}
        ${signedDetail("Estimated value", money(signed.fiatValue))}
        ${signedDetail("Network", networkDisplayName(signed.network))}
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
  const network = selectedNetwork();
  const addr = addressForNetwork(network);
  const payload = receivePayload();
  const qrContent = payload ? appState.qrSvg || `<span class="text-sm font-bold text-slate-500">Generating QR...</span>` : `<span class="text-sm font-bold text-slate-500">Receive is not available for this network yet.</span>`;
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
            <p class="font-black">${escapeHtml(network.name)} receive payload</p>
            <span class="text-sm text-slate-400">${escapeHtml(networkDetail(network))}</span>
          </div>
          <p class="mt-3 break-all font-mono text-xs text-slate-400">${escapeHtml(payload)}</p>
        </div>
        <p class="mt-5 break-all font-mono text-sm text-slate-200">${escapeHtml(addr)}</p>
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
  const assets = appState.session?.assets ?? [];
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
        <div class="mt-5 space-y-3">${appState.session?.activity.map(activityRow).join("") || emptyState("No activity yet", "Your signed sends, swaps, and network changes will appear here.")}</div>
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
        <div class="mt-6 rounded-2xl border border-amber-400/25 bg-amber-400/10 p-4 text-sm text-amber-100">This build simulates balances and transactions. Connect audited chain clients and hardware-backed signing before using real funds.</div>
      </section>
      <section class="glass rounded-[2rem] p-6">
        <p class="text-sm uppercase tracking-[0.3em] text-acid">Security center</p>
        <h2 class="mt-2 text-3xl font-black">Local protection</h2>
        <div class="mt-6 grid gap-3 sm:grid-cols-2">
          ${securityTile("Storage", "AES-GCM encrypted")}
          ${securityTile("Key derivation", "Argon2 passphrase key")}
          ${securityTile("Mode", "ECDSA signing (EIP-1559)")}
          ${securityTile("Lock state", appState.session?.locked ? "Locked" : "Unlocked")}
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

function featureCard(title: string, body: string) {
  return `<div class="rounded-2xl border border-white/10 bg-white/[0.04] p-5"><h3 class="font-black">${title}</h3><p class="mt-2 text-sm leading-6 text-slate-400">${body}</p></div>`;
}

function passphraseMeter() {
  return `<div class="passphrase-meter" data-passphrase-meter data-score="0"><div class="passphrase-meter-track"><div></div></div><p class="mt-2 text-xs font-bold text-slate-500">Strength: <span data-passphrase-label>Too weak</span></p></div>`;
}

function navButton(view: string, label: string) {
  return `<button class="nav-item ${appState.currentView === view ? "active" : ""}" data-view="${view}" type="button">${label}</button>`;
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

function mobileNavButton(view: string, label: string) {
  return `<button class="mobile-nav-item ${appState.currentView === view ? "active" : ""}" data-view="${view}" type="button">${label}</button>`;
}

function assetCard(asset: Asset) {
  const value = assetValue(asset);
  const positive = asset.change_24h >= 0;
  const total = appState.session ? appState.session.assets.reduce((s, a) => s + assetValue(a), 0) : 0;
  const allocation = total ? (value / total) * 100 : 0;
  return `
    <article class="asset-card rounded-3xl border border-white/10 bg-white/[0.04] p-5">
      <div class="flex items-start justify-between gap-4">
        <div class="asset-card-header"><p class="truncate text-lg font-black">${escapeHtml(asset.symbol)}</p><p class="truncate text-sm text-slate-500">${escapeHtml(asset.name)}</p></div>
        <span class="asset-change rounded-full ${positive ? "bg-emerald-400/10 text-emerald-300" : "bg-rose-400/10 text-rose-300"} px-3 py-1 text-xs font-bold">${positive ? "+" : ""}${asset.change_24h.toFixed(2)}%</span>
      </div>
      <p class="asset-value mt-5 text-2xl font-black">${money(value)}</p>
      <p class="mt-1 text-sm text-slate-400">${formatWei(asset.balance, asset.decimals)} ${escapeHtml(asset.symbol)}</p>
      <div class="mt-4">
        <div class="flex justify-between text-xs font-bold text-slate-500"><span>Allocation</span><span>${allocation.toFixed(1)}%</span></div>
        <div class="mt-2 h-2 overflow-hidden rounded-full bg-slate-900"><div class="h-full rounded-full bg-acid" style="width: ${Math.max(2, allocation).toFixed(1)}%"></div></div>
      </div>
    </article>
  `;
}

function assetValue(asset: Asset) {
  return weiToNumber(asset.balance, asset.decimals) * asset.price_usd;
}

function portfolioChange() {
  const assets = appState.session?.assets ?? [];
  const total = assets.reduce((t, a) => t + assetValue(a), 0);
  if (!total) return 0;
  return assets.reduce((acc, asset) => acc + asset.change_24h * (assetValue(asset) / total), 0);
}

function emptyState(title: string, body: string) {
  return `<div class="rounded-3xl border border-dashed border-white/10 bg-white/[0.025] p-6 text-center"><p class="font-black text-slate-200">${escapeHtml(title)}</p><p class="mt-2 text-sm leading-6 text-slate-500">${escapeHtml(body)}</p></div>`;
}

function activityRow(item: Activity) {
  return `
    <article class="flex cursor-pointer flex-col gap-3 rounded-2xl border ${appState.selectedActivityId === item.id ? "border-acid/50 bg-acid/10" : "border-white/10 bg-white/[0.035]"} p-4 sm:flex-row sm:items-center sm:justify-between" data-action="select-activity" data-activity-id="${escapeHtml(item.id)}">
      <div><p class="font-black">${escapeHtml(item.title)}</p><p class="mt-1 text-sm text-slate-500">${escapeHtml(item.subtitle)} - ${new Date(item.timestamp).toLocaleString()}</p></div>
      <div class="text-left sm:text-right"><p class="font-mono font-bold">${escapeHtml(item.amount ?? "")}</p><p class="text-xs uppercase tracking-[0.2em] text-acid">${escapeHtml(item.status)}</p></div>
    </article>
  `;
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
        ${detailRow("Network", networkDisplayName(item.network ?? "n/a"))}
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
  return `<select class="field" name="${name}" ${attributes}>${appState.session?.assets.map((asset) => `<option value="${asset.symbol}" ${asset.symbol === selected ? "selected" : ""}>${asset.symbol} - ${asset.name}</option>`).join("") ?? ""}</select>`;
}

export function updateRecipientPlaceholder(symbol: string) {
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
  return `<select class="field" data-receive-network-id>${networks.map((network) => `<option value="${network.id}" ${network.id === appState.receiveNetworkId ? "selected" : ""}>${network.name} - ${networkDetail(network)}</option>`).join("")}</select>`;
}

function qrResilienceSelect() {
  return `<select class="field" data-receive-resilience>${qrResilienceOptions.map((option) => `<option value="${option.value}" ${option.value === appState.qrResilience ? "selected" : ""}>${option.label} (${option.value}) - ${option.detail}</option>`).join("")}</select>`;
}

function loadingBar() {
  return `<div class="fixed left-0 top-0 z-50 h-1 w-full overflow-hidden bg-slate-900"><div class="h-full w-1/2 animate-pulse bg-acid"></div></div>`;
}
