import QRCode from "qrcode";
import { pushToast } from "./toasts";
import { formatError, shortAddress } from "./format";
import { appState, receivePayload, selectedNetwork } from "./state";
import { theme } from "./theme";

export async function ensureReceiveQr(): Promise<boolean> {
  if (appState.currentView !== "receive" || !appState.session?.address || appState.session?.locked) return false;

  const payload = receivePayload();
  if (!payload) {
    if (appState.qrSvg || appState.qrKey || appState.qrGeneratingKey) {
      resetQr();
      return true;
    }
    return false;
  }

  const nextQrKey = `${payload}:${appState.qrResilience}`;
  if ((appState.qrKey === nextQrKey && appState.qrSvg) || appState.qrGeneratingKey === nextQrKey) return false;

  appState.qrGeneratingKey = nextQrKey;
  try {
    const svg = await QRCode.toString(payload, {
      type: "svg",
      margin: 2,
      errorCorrectionLevel: appState.qrResilience,
      color: { dark: theme.colors.qrDark, light: theme.colors.qrLight },
    });

    if (appState.qrGeneratingKey === nextQrKey) {
      appState.qrKey = nextQrKey;
      appState.qrSvg = svg;
      return true;
    }
    return false;
  } catch (error) {
    pushToast(formatError(error), "error");
    resetQr();
    return true;
  } finally {
    if (appState.qrGeneratingKey === nextQrKey) appState.qrGeneratingKey = "";
  }
}

export function resetQr(): void {
  appState.qrSvg = "";
  appState.qrKey = "";
}

export function downloadQrSvg(): void {
  if (!appState.qrSvg || !appState.session?.address) return;

  const net = selectedNetwork();
  const blob = new Blob([appState.qrSvg], { type: "image/svg+xml;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = `vaultforge-${net.id}-${shortAddress(appState.session.address).replace(/[^a-zA-Z0-9]/g, "")}-qr.svg`;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
  pushToast("QR code downloaded.", "success");
}
