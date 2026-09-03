import QRCode from "qrcode";

import { formatError, shortAddress } from "./format";
import { addressForNetwork, receivePayload, selectedNetwork } from "./selectors";
import { appState } from "./state";
import { getSelectedTheme, themes } from "./theme";
import { pushToast } from "./toasts";

let qrKey = "";
let qrGeneratingKey = "";

export async function ensureReceiveQr(): Promise<boolean> {
  const theme = themes[getSelectedTheme()];

  if (appState.navigation.currentView !== "receive" || appState.wallet.status !== "unlocked")
    return false;

  const payload = receivePayload();
  if (!payload) {
    if (appState.receive.qrSvg || qrKey || qrGeneratingKey) {
      resetQr();
      return true;
    }
    return false;
  }

  const nextQrKey = `${payload}:${appState.receive.qrResilience}`;
  if ((qrKey === nextQrKey && appState.receive.qrSvg) || qrGeneratingKey === nextQrKey)
    return false;

  qrGeneratingKey = nextQrKey;
  try {
    const svg = await QRCode.toString(payload, {
      type: "svg",
      margin: 2,
      errorCorrectionLevel: appState.receive.qrResilience,
      color: { dark: theme.colors.qrDark, light: theme.colors.qrLight },
    });

    if (qrGeneratingKey === nextQrKey && appState.wallet.status === "unlocked") {
      qrKey = nextQrKey;
      appState.receive.qrSvg = svg;
      return true;
    }
    return false;
  } catch (error) {
    pushToast(formatError(error), "error");
    resetQr();
    return true;
  } finally {
    if (qrGeneratingKey === nextQrKey) qrGeneratingKey = "";
  }
}

export function resetQr(): void {
  appState.receive.qrSvg = "";
  qrKey = "";
  qrGeneratingKey = "";
}

export function downloadQrSvg(): void {
  if (!appState.receive.qrSvg) return;

  const net = selectedNetwork();
  const address = addressForNetwork(net);
  if (!address) return;
  const blob = new Blob([appState.receive.qrSvg], { type: "image/svg+xml;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = `vaultforge-${net.id}-${shortAddress(address).replace(/[^a-zA-Z0-9]/g, "")}-qr.svg`;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
  pushToast("QR code downloaded.", "success");
}
