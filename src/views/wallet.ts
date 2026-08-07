import { escapeHtml, formatWei, money, shortAddress } from "../format";
import { networkDisplayName } from "../networks";
import {
  addressForNetwork,
  appState,
  networkDetail,
  receivePayload,
  selectedActivity,
  selectedNetwork,
} from "../state";
import type { SignedTransaction } from "../types";
import {
  activityDetails,
  activityRow,
  addressPlaceholder,
  assetCard,
  assetSelect,
  assetValue,
  decimalsForAsset,
  emptyState,
  iconCopy,
  iconDownload,
  qrResilienceSelect,
  receiveNetworkSelect,
  sendAssetSelect,
} from "./shared";

export function walletView() {
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
  const recent =
    appState.session.activity.slice(0, 5).map(activityRow).join("") ||
    emptyState(
      "No recent activity",
      "Sign, send, swap, or change networks to build a local activity timeline.",
    );
  const change = portfolioChange();
  return `
    <div class="grid gap-5 xl:grid-cols-[1.35fr_0.75fr]">
      <div class="min-w-0 space-y-5">
        <section class="glass min-w-0 overflow-hidden rounded-[2rem] p-6">
          <div class="flex flex-col gap-6 lg:flex-row lg:items-end lg:justify-between">
            <div class="min-w-0">
              <p class="theme-text-accent text-sm font-bold uppercase tracking-[0.3em]">Portfolio</p>
              <h2 class="mt-3 max-w-2xl text-3xl font-black tracking-tight sm:text-4xl">Multi-asset wallet with transaction controls.</h2>
            </div>
            <div class="grid shrink-0 gap-3 sm:grid-cols-1">
              <div class="theme-panel-accent min-w-0 rounded-2xl border p-4 text-right">
                <p class="text-sm font-bold text-slate-400">Weighted 24h</p>
                <p class="max-w-full break-words text-2xl font-black leading-none sm:text-3xl ${change >= 0 ? "text-emerald-300" : "text-rose-300"}">${change >= 0 ? "+" : ""}${change.toFixed(2)}%</p>
              </div>
            </div>
          </div>
          <div class="asset-carousel mt-6">
            <div id="portfolio-assets" class="asset-scroll" data-horizontal-scroll tabindex="0" aria-label="Portfolio assets">${topAssets || emptyState("No assets", "Create or import a wallet to populate simulated balances.")}</div>
            <div class="asset-scrollbar" data-horizontal-scrollbar role="scrollbar" tabindex="0" aria-controls="portfolio-assets" aria-label="Scroll portfolio assets horizontally" aria-orientation="horizontal" aria-valuemin="0" aria-valuemax="0" aria-valuenow="0">
              <div class="asset-scrollbar-thumb" data-horizontal-scrollbar-thumb></div>
            </div>
          </div>
        </section>
        <section class="glass rounded-[2rem] p-6">
          <div class="mb-5 flex items-center justify-between"><h2 class="text-xl font-black">Recent activity</h2><button class="theme-text-accent text-sm font-bold font-bold" data-view="activity">View all</button></div>
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
  const selectedAssetId = `${appState.sendDraft.network || "ethereum"}:${appState.sendDraft.token_address ?? "native"}`;

  return `
    <section class="glass max-w-3xl rounded-[2rem] p-6">
      <p class="text-sm font-bold uppercase tracking-[0.3em] text-slate-500">Transfer</p>
      <h2 class="mt-2 text-3xl font-black">Send crypto</h2>
      <p class="mt-3 text-sm font-bold leading-6 text-slate-400">Transactions are signed locally before broadcast to the chain RPC. Review the signature before funds leave your balance.</p>
      <form data-action="sign-transaction" class="mt-6 grid gap-4">
        <label class="space-y-2"><span class="text-sm font-bold font-bold text-slate-300">Recipient address</span><input class="field" name="to" data-recipient-address placeholder="${addressPlaceholder(selectedSymbol)}" value="${escapeHtml(appState.sendDraft.to)}" required /></label>
        <div class="grid gap-4 sm:grid-cols-2">
          <label class="space-y-2"><span class="text-sm font-bold font-bold text-slate-300">Asset</span>${sendAssetSelect(selectedAssetId)}</label>
          <label class="space-y-2"><span class="text-sm font-bold font-bold text-slate-300">Amount</span><input class="field" name="amount" type="number" min="0.000001" step="0.000001" value="${escapeHtml(appState.sendDraft.amount)}" required /></label>
        </div>
        <label class="space-y-2"><span class="text-sm font-bold font-bold text-slate-300">Note</span><input class="field" name="note" placeholder="Optional transaction memo" value="${escapeHtml(appState.sendDraft.note)}" /></label>
        <button class="btn-primary justify-self-start" type="submit">Sign transaction</button>
      </form>
    </section>
  `;
}

function signedTransactionView(signed: SignedTransaction) {
  const feeDecimals = decimalsForAsset(signed.feeSymbol, signed.network, signed.decimals);
  const chainReferenceLabel =
    signed.network === "bitcoin"
      ? "Input model"
      : signed.network === "solana"
        ? "Blockhash"
        : "Nonce";
  return `
    <section class="glass max-w-4xl rounded-[2rem] p-6">
      <div class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
        <div>
          <p class="theme-text-accent text-sm font-bold uppercase tracking-[0.3em]">Signed transfer</p>
          <h2 class="mt-2 text-3xl font-black">Review signature</h2>
          <p class="mt-3 max-w-2xl text-sm font-bold leading-6 text-slate-400">The backend signed this chain-specific transaction with the derived key material. Broadcast only if the details match your intent.</p>
        </div>
        <span class="theme-pill-accent rounded-full border-0 px-3 py-1 text-xs font-black uppercase tracking-[0.2em]">Ready</span>
      </div>
      <div class="mt-6 grid gap-4 sm:grid-cols-2">
        ${signedDetail("From", shortAddress(signed.from))}
        ${signedDetail("To", shortAddress(signed.to))}
        ${signedDetail("Amount", `${formatWei(signed.amount, signed.decimals)} ${signed.symbol}`)}
        ${signedDetail("Network fee", `${formatWei(signed.feeAmount, feeDecimals)} ${signed.feeSymbol}`)}
        ${signedDetail("Total debit", `${formatWei(signed.totalDebit, signed.decimals)} ${signed.symbol}`)}
        ${signedDetail("Post-send balance", `${formatWei(signed.postBalance, signed.decimals)} ${signed.symbol}`)}
        ${signedDetail("Estimated value", money(signed.fiatValue))}
        ${signedDetail("Network", networkDisplayName(signed.network))}
        ${signedDetail(chainReferenceLabel, signed.nonce)}
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

function receiveView() {
  const network = selectedNetwork();
  const address = addressForNetwork(network);
  const payload = receivePayload();
  const qrContent = payload
    ? appState.qrSvg ||
      `<span class="text-sm font-bold font-bold text-slate-500">Generating QR...</span>`
    : `<span class="text-sm font-bold font-bold text-slate-500">Receive is not available for this network yet.</span>`;
  const qrActionsDisabled = payload ? "" : "disabled";
  return `
    <section class="glass max-w-3xl rounded-[2rem] p-6">
      <p class="text-sm font-bold uppercase tracking-[0.3em] text-slate-500">Receive</p>
      <h2 class="mt-2 text-3xl font-black">Deposit address</h2>
      <div class="mt-6 grid gap-4 sm:grid-cols-2">
        <label class="space-y-2"><span class="text-sm font-bold font-bold text-slate-300">Receive network</span>${receiveNetworkSelect()}</label>
        <label class="space-y-2"><span class="text-sm font-bold font-bold text-slate-300">QR resilience</span>${qrResilienceSelect()}</label>
      </div>
      <div class="theme-panel-accent mt-6 rounded-3xl border border-dashed p-6 text-center">
        <div class="theme-glow qr-code mx-auto flex h-56 w-56 items-center justify-center rounded-2xl bg-white p-4">${qrContent}</div>
        <div class="mt-4 flex flex-col justify-center gap-3 sm:flex-row">
          <button class="btn-secondary" data-action="copy-qr" type="button" ${qrActionsDisabled}>${iconCopy()} Copy SVG</button>
          <button class="btn-secondary" data-action="download-qr" type="button" ${qrActionsDisabled}>${iconDownload()} Download SVG</button>
        </div>
        <div class="mt-5 rounded-2xl border border-white/10 bg-white/[0.04] p-4 text-left">
          <div class="flex flex-col gap-1 sm:flex-row sm:items-center sm:justify-between">
            <p class="font-black">${escapeHtml(network.name)} receive payload</p>
            <span class="text-sm font-bold text-slate-400">${escapeHtml(networkDetail(network))}</span>
          </div>
          <p class="mt-3 break-all font-mono text-xs text-slate-400">${escapeHtml(payload)}</p>
        </div>
        <p class="mt-5 break-all font-mono text-sm font-bold text-slate-200">${escapeHtml(address)}</p>
        <button class="btn-primary mt-5" data-action="copy-receive-address" type="button">Copy address</button>
      </div>
    </section>
  `;
}

function swapView() {
  return `
    <section class="glass max-w-3xl rounded-[2rem] p-6">
      <p class="text-sm font-bold uppercase tracking-[0.3em] text-slate-500">Exchange</p>
      <h2 class="mt-2 text-3xl font-black">Swap assets</h2>
      <form data-action="swap-tokens" class="mt-6 grid gap-4">
        <div class="grid gap-4 sm:grid-cols-2">
          <label class="space-y-2"><span class="text-sm font-bold font-bold text-slate-300">From</span>${assetSelect("fromSymbol")}</label>
          <label class="space-y-2"><span class="text-sm font-bold font-bold text-slate-300">To</span>${assetSelect("toSymbol", "USDC")}</label>
        </div>
        <label class="space-y-2"><span class="text-sm font-bold font-bold text-slate-300">Amount</span><input class="field" name="amount" type="number" min="0.000001" step="0.000001" required /></label>
        <button class="btn-primary justify-self-start" type="submit">Execute simulated swap</button>
      </form>
    </section>
  `;
}

function assetsView() {
  const assets = appState.session?.assets ?? [];
  return `
    <section class="glass rounded-[2rem] p-6">
      <div class="mb-5 flex items-center justify-between"><h2 class="text-2xl font-black">Assets</h2><span class="text-sm font-bold text-slate-500">${assets.length} tracked</span></div>
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
        <p class="text-sm font-bold uppercase tracking-[0.3em] text-slate-500">Preferences</p>
        <h2 class="mt-2 text-3xl font-black">Wallet settings</h2>
        <div class="mt-6 rounded-2xl border border-amber-400/25 bg-amber-400/10 p-4 text-sm font-bold text-amber-100">This build simulates balances and transactions. Connect audited chain clients and hardware-backed signing before using real funds.</div>
      </section>
      <section class="glass rounded-[2rem] p-6">
        <p class="theme-text-accent text-sm font-bold uppercase tracking-[0.3em]">Security center</p>
        <h2 class="mt-2 text-3xl font-black">Local protection</h2>
        <div class="mt-6 grid gap-3 sm:grid-cols-2">
          ${securityTile("Storage", "AES-GCM encrypted")}
          ${securityTile("Key derivation", "Argon2 passphrase key")}
          ${securityTile("Mode", "ECDSA signing (EIP-1559)")}
          ${securityTile("Lock state", appState.session?.locked ? "Locked" : "Unlocked")}
        </div>
        <div class="mt-6 rounded-2xl border border-rose-400/25 bg-rose-400/10 p-4">
          <h3 class="font-black text-rose-100">Danger zone</h3>
          <p class="mt-2 text-sm font-bold leading-6 text-rose-100/80">Remove the encrypted local wallet file and return this app to onboarding.</p>
          <button class="btn-danger mt-4" data-action="clear-wallet" type="button">Clear local wallet</button>
        </div>
      </section>
    </div>
  `;
}

function signedDetail(label: string, value: string, mono = false) {
  return `
    <div class="rounded-2xl border border-white/10 bg-white/[0.035] p-4">
      <p class="text-xs uppercase tracking-[0.22em] text-slate-500">${escapeHtml(label)}</p>
      <p class="mt-2 ${mono ? "break-all font-mono text-xs" : "text-sm font-bold font-bold"} text-slate-200">${escapeHtml(value)}</p>
    </div>
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

function securityTile(label: string, value: string) {
  return `<div class="rounded-2xl border border-white/10 bg-white/[0.035] p-4"><p class="text-xs uppercase tracking-[0.22em] text-slate-500">${escapeHtml(label)}</p><p class="mt-2 font-black text-slate-100">${escapeHtml(value)}</p></div>`;
}

function portfolioChange() {
  const assets = appState.session?.assets ?? [];
  const total = assets.reduce((sum, asset) => sum + assetValue(asset), 0);
  if (!total) return 0;
  return assets.reduce((sum, asset) => sum + asset.change_24h * (assetValue(asset) / total), 0);
}
