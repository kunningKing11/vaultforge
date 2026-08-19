import { escapeHtml, formatWei, money, weiToNumber } from "../format";
import { networkDisplayName, networks } from "../networks";
import { appState, networkDetail } from "../state";
import type { Activity, Asset, NetworkId, QrResilience } from "../types";

const qrResilienceOptions: Array<{ value: QrResilience; label: string; detail: string }> = [
  { value: "L", label: "Low", detail: "~7% recovery" },
  { value: "M", label: "Medium", detail: "~15% recovery" },
  { value: "Q", label: "Quartile", detail: "~25% recovery" },
  { value: "H", label: "High", detail: "~30% recovery" },
];

export function assetCard(asset: Asset) {
  const value = assetValue(asset);
  const positive = asset.change_24h >= 0;
  const total = appState.session
    ? appState.session.assets.reduce((sum, item) => sum + assetValue(item), 0)
    : 0;
  const allocation = total ? (value / total) * 100 : 0;
  return `
    <article class="asset-card rounded-3xl border border-white/10 bg-white/[0.04] p-5">
      <div class="flex items-start justify-between gap-4">
        <div class="asset-card-header"><p class="truncate text-lg font-black">${escapeHtml(asset.symbol)}</p><p class="truncate text-sm font-bold text-slate-500">${escapeHtml(asset.name)}</p></div>
        <span class="asset-change rounded-full ${positive ? "bg-emerald-400/10 text-emerald-300" : "bg-rose-400/10 text-rose-300"} px-3 py-1 text-xs font-bold">${positive ? "+" : ""}${asset.change_24h.toFixed(2)}%</span>
      </div>
      <p class="asset-value mt-5 text-2xl font-black">${money(value)}</p>
      <p class="mt-1 text-sm font-bold text-slate-400">${formatWei(asset.balance, asset.decimals)} ${escapeHtml(asset.symbol)}</p>
      <div class="mt-4">
        <div class="flex justify-between text-xs font-bold text-slate-500"><span>Allocation</span><span>${allocation.toFixed(1)}%</span></div>
        <div class="mt-2 h-2 overflow-hidden rounded-full bg-slate-900"><div class="theme-progress-accent h-full rounded-full" style="width: ${Math.max(2, allocation).toFixed(1)}%"></div></div>
      </div>
    </article>
  `;
}

export function assetValue(asset: Asset) {
  return weiToNumber(asset.balance, asset.decimals) * asset.price_usd;
}

export function emptyState(title: string, body: string) {
  return `<div class="rounded-3xl border border-dashed border-white/10 bg-white/[0.025] p-6 text-center"><p class="font-black text-slate-200">${escapeHtml(title)}</p><p class="mt-2 text-sm font-bold leading-6 text-slate-500">${escapeHtml(body)}</p></div>`;
}

export function activityRow(item: Activity) {
  return `
    <article class="flex cursor-pointer flex-col gap-3 rounded-2xl border ${appState.selectedActivityId === item.id ? "theme-activity-selected" : "border-white/10 bg-white/[0.035]"} p-4 sm:flex-row sm:items-center sm:justify-between" data-action="select-activity" data-activity-id="${escapeHtml(item.id)}">
      <div><p class="font-black">${escapeHtml(item.title)}</p><p class="mt-1 text-sm font-bold text-slate-500">${escapeHtml(item.subtitle)} - ${new Date(item.timestamp).toLocaleString()}</p></div>
      <div class="text-left sm:text-right"><p class="font-mono font-bold">${escapeHtml(item.amount ?? "")}</p><p class="theme-text-accent text-xs uppercase tracking-[0.2em]">${escapeHtml(item.status)}</p></div>
    </article>
  `;
}

export function activityDetails(item: Activity | null) {
  if (!item) {
    return `<section class="glass rounded-[2rem] p-6"><p class="text-sm font-bold text-slate-400">No activity selected.</p></section>`;
  }

  return `
    <section class="glass rounded-[2rem] p-6">
      <p class="text-sm font-bold uppercase tracking-[0.3em] text-slate-500">Activity details</p>
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

export function assetSelect(name: string, selected = "ETH", attributes = "") {
  return `<select class="field" name="${name}" ${attributes}>${appState.session?.assets.map((asset) => `<option value="${asset.symbol}" ${asset.symbol === selected ? "selected" : ""}>${asset.symbol} - ${asset.name}</option>`).join("") ?? ""}</select>`;
}

export function sendAssetSelect(selectedAssetId: string) {
  return `<select class="field" name="asset" data-send-asset>${
    appState.session?.assets
      .map((asset) => {
        const assetId = `${asset.network}:${asset.token_address ?? "native"}`;
        return `<option value="${escapeHtml(assetId)}" data-symbol="${escapeHtml(asset.symbol)}" ${assetId === selectedAssetId ? "selected" : ""}>${asset.symbol} - ${asset.name} (${networkDisplayName(asset.network)})</option>`;
      })
      .join("") ?? ""
  }</select>`;
}

export function decimalsForAsset(symbol: string, network: NetworkId, fallback: number) {
  return (
    appState.session?.assets.find((asset) => asset.symbol === symbol && asset.network === network)
      ?.decimals ??
    appState.session?.assets.find((asset) => asset.symbol === symbol)?.decimals ??
    fallback
  );
}

export function updateRecipientPlaceholder(symbol: string) {
  const input = document.querySelector<HTMLInputElement>("[data-recipient-address]");
  if (input) input.placeholder = addressPlaceholder(symbol);
}

export function addressPlaceholder(symbol: string) {
  const placeholders: Record<string, string> = {
    BTC: "bc1... / 1... / 3...",
    ETH: "0x...",
    FIL: "f1... / f3...",
    INJ: "inj1...",
    SOL: "Solana address",
    TRX: "T...",
    ZEC: "t1... / t3...",
  };
  return placeholders[symbol] ?? "0x...";
}

export function receiveNetworkSelect() {
  return `<select class="field" data-receive-network-id>${networks.map((network) => `<option value="${network.id}" ${network.id === appState.receiveNetworkId ? "selected" : ""}>${network.name} - ${networkDetail(network)}</option>`).join("")}</select>`;
}

export function qrResilienceSelect() {
  return `<select class="field" data-receive-resilience>${qrResilienceOptions.map((option) => `<option value="${option.value}" ${option.value === appState.qrResilience ? "selected" : ""}>${option.label} (${option.value}) - ${option.detail}</option>`).join("")}</select>`;
}

export function detailRow(label: string, value: string) {
  return `<div class="rounded-2xl border border-white/10 bg-white/[0.035] p-4"><p class="text-xs uppercase tracking-[0.22em] text-slate-500">${escapeHtml(label)}</p><p class="mt-2 break-all text-sm font-bold font-bold text-slate-200">${escapeHtml(value)}</p></div>`;
}

export function copyableDetailRow(label: string, value: string) {
  return `<div class="rounded-2xl border border-white/10 bg-white/[0.035] p-4"><div class="flex items-start justify-between gap-3"><div class="min-w-0"><p class="text-xs uppercase tracking-[0.22em] text-slate-500">${escapeHtml(label)}</p><p class="mt-2 break-all font-mono text-xs text-slate-200">${escapeHtml(value)}</p></div><button class="btn-secondary shrink-0 text-xs" data-action="copy-value" data-copy-value="${escapeHtml(value)}" type="button">Copy</button></div></div>`;
}

export function featureCard(title: string, body: string) {
  return `<div class="rounded-2xl border border-white/10 bg-white/[0.04] p-5"><h3 class="font-black">${title}</h3><p class="mt-2 text-sm font-bold leading-6 text-slate-400">${body}</p></div>`;
}

export function walletPasswordMeter() {
  return `<div class="wallet-password-meter" data-wallet-password-meter data-score="0"><div class="wallet-password-meter-track"><div></div></div><p class="mt-2 text-xs font-bold text-slate-500">Strength: <span data-wallet-password-label>Too weak</span></p></div>`;
}

export function iconCopy() {
  return `<svg aria-hidden="true" class="h-4 w-4" viewBox="0 0 24 24" fill="none"><path d="M8 8.5C8 7.12 9.12 6 10.5 6h6C17.88 6 19 7.12 19 8.5v8c0 1.38-1.12 2.5-2.5 2.5h-6A2.5 2.5 0 0 1 8 16.5v-8Z" stroke="currentColor" stroke-width="1.8"/><path d="M5 14.5v-8C5 5.12 6.12 4 7.5 4h6" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/></svg>`;
}

export function iconDownload() {
  return `<svg aria-hidden="true" class="h-4 w-4" viewBox="0 0 24 24" fill="none"><path d="M12 4v10m0 0 4-4m-4 4-4-4" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/><path d="M5 16.5V18a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-1.5" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/></svg>`;
}

export function loadingBar() {
  return `<div class="fixed left-0 top-0 z-50 h-1 w-full overflow-hidden bg-slate-900"><div class="theme-progress-accent h-full w-1/2 animate-pulse"></div></div>`;
}
