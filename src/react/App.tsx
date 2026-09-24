import type { ThemeVars } from "@coinbase/cds-common/core/theme";
import { Select } from "@coinbase/cds-web/alpha/select";
import { Button } from "@coinbase/cds-web/buttons";
import { ContentCard } from "@coinbase/cds-web/cards/ContentCard";
import { Checkbox, NativeTextArea, Switch, TextInput } from "@coinbase/cds-web/controls";
import { useMediaQuery } from "@coinbase/cds-web/hooks/useMediaQuery";
import { Box, Grid, HStack, VStack } from "@coinbase/cds-web/layout";
import { Sidebar, SidebarItem } from "@coinbase/cds-web/navigation";
import { Modal, PortalProvider } from "@coinbase/cds-web/overlays";
import { useToast } from "@coinbase/cds-web/overlays/useToast";
import { MediaQueryProvider, ThemeProvider } from "@coinbase/cds-web/system";
import { Text } from "@coinbase/cds-web/typography";
import { ProgressBar } from "@coinbase/cds-web/visualizations";
import { isTauri } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import QRCode from "qrcode";
import {
  type CSSProperties,
  type SubmitEventHandler,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";

import appLogoUrl from "../../src-tauri/icons/icon.svg";
import activityIcon from "../assets/icons/activity.svg?raw";
import assetsIcon from "../assets/icons/assets.svg?raw";
import copyIcon from "../assets/icons/copy.svg?raw";
import dashboardIcon from "../assets/icons/dashboard.svg?raw";
import downloadIcon from "../assets/icons/download.svg?raw";
import lockIcon from "../assets/icons/lock.svg?raw";
import receiveIcon from "../assets/icons/receive.svg?raw";
import refreshIcon from "../assets/icons/refresh.svg?raw";
import sendUpIcon from "../assets/icons/send-up.svg?raw";
import sendIcon from "../assets/icons/send.svg?raw";
import settingsIcon from "../assets/icons/settings.svg?raw";
import swapIcon from "../assets/icons/swap.svg?raw";
import walletIcon from "../assets/icons/wallet.svg?raw";
import { fiatCurrencies } from "../currencies";
import {
  cryptoDisplaySymbol,
  formatError,
  formatWei,
  money,
  shortAddress,
  toWei,
  usdToFiat,
  weiToNumber,
} from "../format";
import { networkById, networkDisplayName, networks } from "../networks";
import { hasValidRecoveryPhraseWordCount } from "../recoveryPhrase";
import {
  installScrollbarBehavior,
  syncHorizontalScrollbars,
  syncVerticalScrollbar,
} from "../scrollbars";
import type {
  Activity,
  Asset,
  FiatCurrency,
  NetworkId,
  QrResilience,
  RefreshWarning,
  SendDraft,
  SessionCommand,
  SignedTransaction,
  View,
  WalletRefreshResult,
  WalletSession,
} from "../types";
import { walletApi } from "../walletApi";
import { walletPasswordStrength } from "../walletPassword";
import {
  addressForNetwork,
  applyWalletSession,
  type AppState,
  type ColorScheme,
  createInitialAppState,
  createSetupWizardState,
  emptySendDraft,
  networkLabel,
  receivePayload,
  selectedActivity,
  sortAssetsByValue,
  type WalletState,
} from "./model";
import { vaultForgeTheme } from "./theme";

const MINIMUM_SPLASH_DURATION_MS = 650;
const AUTO_LOCK_OPTIONS = [
  { label: "Off", value: "0" },
  { label: "5 minutes", value: "300" },
  { label: "10 minutes", value: "600" },
  { label: "15 minutes", value: "900" },
  { label: "30 minutes", value: "1800" },
  { label: "1 hour", value: "3600" },
] as const;
const QR_RESILIENCE_OPTIONS: ReadonlyArray<{ value: QrResilience; label: string }> = [
  { value: "L", label: "Low (~7% recovery)" },
  { value: "M", label: "Medium (~15% recovery)" },
  { value: "Q", label: "Quartile (~25% recovery)" },
  { value: "H", label: "High (~30% recovery)" },
];

const NAV_ITEMS: ReadonlyArray<{
  view: View;
  label: string;
  icon: string;
}> = [
  { view: "dashboard", label: "Dashboard", icon: dashboardIcon },
  { view: "send", label: "Send", icon: sendUpIcon },
  { view: "receive", label: "Receive", icon: receiveIcon },
  { view: "swap", label: "Swap", icon: swapIcon },
  { view: "assets", label: "Assets", icon: assetsIcon },
  { view: "activity", label: "Activity", icon: activityIcon },
  { view: "settings", label: "Settings", icon: settingsIcon },
];

type ToastTone = "error" | "info" | "success" | "warning";

type ToastFn = (message: string, tone: ToastTone) => void;

export function VaultForgeApp() {
  const [colorScheme, setColorScheme] = useColorScheme();

  return (
    <MediaQueryProvider>
      <ThemeProvider activeColorScheme={colorScheme} theme={vaultForgeTheme}>
        <PortalProvider>
          <WalletApplication colorScheme={colorScheme} setColorScheme={setColorScheme} />
        </PortalProvider>
      </ThemeProvider>
    </MediaQueryProvider>
  );
}

function WalletApplication({
  colorScheme,
  setColorScheme,
}: {
  colorScheme: ColorScheme;
  setColorScheme: (scheme: ColorScheme) => void;
}) {
  const { show: showCdsToast } = useToast();
  const [state, setState] = useState<AppState>(createInitialAppState);
  const [sessionLoaded, setSessionLoaded] = useState(false);
  const stateRef = useRef(state);
  const operationRef = useRef(false);
  const lockRequestedRef = useRef(false);
  const refreshIdRef = useRef(0);
  const autoLockStopRef = useRef<() => void>(() => undefined);
  const lockWalletRef = useRef<() => void>(() => undefined);
  const toastRef = useRef<ToastFn>(() => undefined);

  useEffect(() => {
    stateRef.current = state;
  }, [state]);

  const toast = useCallback<ToastFn>(
    (message, tone) => {
      const variant =
        tone === "error"
          ? "bgNegative"
          : tone === "success"
            ? "bgPositive"
            : tone === "warning"
              ? "bgWarning"
              : "bgPrimary";
      showCdsToast(message, { variant });
    },
    [showCdsToast],
  );

  useEffect(() => {
    toastRef.current = toast;
  }, [toast]);

  const commitSession = useCallback((session: WalletSession) => {
    if (lockRequestedRef.current && session.has_wallet && !session.locked) return;
    setState((current) =>
      lockRequestedRef.current && session.has_wallet && !session.locked
        ? current
        : applyWalletSession(current, session),
    );
  }, []);

  const setBusy = useCallback((busy: boolean) => {
    setState((current) => ({ ...current, operation: { busy } }));
  }, []);

  const beginOperation = useCallback(() => {
    if (operationRef.current || lockRequestedRef.current) return false;
    operationRef.current = true;
    setBusy(true);
    return true;
  }, [setBusy]);

  const endOperation = useCallback(() => {
    operationRef.current = false;
    setBusy(false);
  }, [setBusy]);

  const invalidatePortfolioRefresh = useCallback(() => {
    refreshIdRef.current += 1;
    setState((current) => ({ ...current, portfolio: { status: "idle" } }));
  }, []);

  const lockWallet = useCallback(async () => {
    if (lockRequestedRef.current) return;
    const startedOperation = beginOperation();
    lockRequestedRef.current = true;

    autoLockStopRef.current();
    invalidatePortfolioRefresh();
    setState((current) => ({
      ...current,
      dialogs: { ...current.dialogs, unlockPasswordVisible: false },
    }));

    let backendLocked = false;
    try {
      await walletApi.lockWallet();
      backendLocked = true;
      commitSession(await walletApi.getWallet());
      setState((current) => ({
        ...current,
        navigation: { currentView: "dashboard", selectedActivityId: "" },
      }));
      toast("Wallet locked.", "success");
    } catch (error) {
      if (backendLocked) {
        setState((current) => ({
          ...current,
          wallet: {
            status: "locked",
            name: current.wallet.status === "missing" ? "Wallet" : current.wallet.name,
          },
          send: { draft: emptySendDraft(), signedTransaction: null },
          navigation: { currentView: "dashboard", selectedActivityId: "" },
        }));
      } else {
        lockRequestedRef.current = false;
      }
      toast(formatError(error), "error");
    } finally {
      if (startedOperation) endOperation();
    }
  }, [beginOperation, commitSession, endOperation, invalidatePortfolioRefresh, toast]);

  lockWalletRef.current = () => void lockWallet();

  useAutoLock(state.wallet, autoLockStopRef, lockWalletRef);

  const runSessionCommand = useCallback(
    async (command: SessionCommand, action: () => Promise<WalletSession | null>) => {
      if (!beginOperation()) return false;
      try {
        const session = await action();
        if (session) commitSession(session);
        toast(successMessage(command), "success");
        return true;
      } catch (error) {
        toast(formatError(error), "error");
        return false;
      } finally {
        endOperation();
      }
    },
    [beginOperation, commitSession, endOperation, toast],
  );

  const runRefreshCommand = useCallback(
    async (command: SessionCommand, action: () => Promise<WalletRefreshResult>) => {
      if (!beginOperation()) return false;
      if (command === "refresh_portfolio") {
        setState((current) => ({ ...current, portfolio: { status: "refreshing" } }));
      }
      try {
        const result = await action();
        commitSession(result.session);
        setState((current) => ({
          ...current,
          portfolio: {
            status:
              command === "refresh_portfolio" && result.warnings.length > 0 ? "stale" : "idle",
          },
        }));
        result.warnings.forEach((warning) => toast(refreshWarningMessage(warning), "warning"));
        toast(successMessage(command), "success");
        return true;
      } catch (error) {
        if (command === "refresh_portfolio") {
          setState((current) => ({ ...current, portfolio: { status: "stale" } }));
        }
        toast(formatError(error), "error");
        return false;
      } finally {
        endOperation();
      }
    },
    [beginOperation, commitSession, endOperation, toast],
  );

  const refreshPortfolioInBackground = useCallback(async () => {
    const refreshId = ++refreshIdRef.current;
    setState((current) => ({ ...current, portfolio: { status: "refreshing" } }));

    try {
      const result = await walletApi.refreshPortfolio();
      if (refreshId !== refreshIdRef.current || stateRef.current.wallet.status !== "unlocked")
        return;
      commitSession(result.session);
      setState((current) => ({
        ...current,
        portfolio: { status: result.warnings.length > 0 ? "stale" : "idle" },
      }));
      result.warnings.forEach((warning) => toast(refreshWarningMessage(warning), "warning"));
    } catch {
      if (refreshId === refreshIdRef.current) {
        setState((current) => ({ ...current, portfolio: { status: "stale" } }));
      }
    }
  }, [commitSession, toast]);

  const refreshPortfolio = useCallback(async () => {
    if (operationRef.current || stateRef.current.portfolio.status === "refreshing") return;
    invalidatePortfolioRefresh();
    await runRefreshCommand("refresh_portfolio", () => walletApi.refreshPortfolio());
  }, [invalidatePortfolioRefresh, runRefreshCommand]);

  const completeSetup = useCallback(async () => {
    const wizard = stateRef.current.onboarding;
    if (wizard.flow === "import" && !hasValidRecoveryPhraseWordCount(wizard.recoveryPhrase)) {
      toast("Recovery phrase must contain 12, 15, 18, 21, or 24 words.", "error");
      return;
    }
    if (wizard.flow === "create" && !wizard.acknowledgedBackup) {
      toast("Confirm that you backed up your recovery phrase before creating a wallet.", "error");
      return;
    }
    if (
      wizard.flow === "create" &&
      wizard.recoveryPhrase.trim().split(/\s+/).length !== wizard.wordCount
    ) {
      toast(
        "Recovery phrase length does not match your selection. Go back and regenerate it.",
        "error",
      );
      return;
    }

    const success = await runRefreshCommand(
      wizard.flow === "import" ? "import_wallet" : "create_wallet",
      () =>
        wizard.flow === "import"
          ? walletApi.importWallet({
              name: wizard.name || undefined,
              mnemonic: wizard.recoveryPhrase.trim(),
              walletPassword: wizard.walletPassword,
              fiatCurrency: wizard.fiatCurrency,
              enabledNetworks: wizard.enabledNetworks,
              autoLockTimeoutSecs: wizard.autoLockTimeoutSecs,
              useCryptoSymbols: wizard.useCryptoSymbols,
            })
          : walletApi.createWallet({
              name: wizard.name || "Primary Wallet",
              walletPassword: wizard.walletPassword,
              fiatCurrency: wizard.fiatCurrency,
              enabledNetworks: wizard.enabledNetworks,
              autoLockTimeoutSecs: wizard.autoLockTimeoutSecs,
              useCryptoSymbols: wizard.useCryptoSymbols,
              mnemonic: wizard.recoveryPhrase,
            }),
    );

    if (success) {
      setState((current) => ({ ...current, onboarding: createSetupWizardState() }));
    }
  }, [runRefreshCommand, toast]);

  const unlockWallet = useCallback(
    async (walletPassword: string) => {
      if (operationRef.current) return false;
      lockRequestedRef.current = false;
      const success = await runRefreshCommand("unlock_wallet", () =>
        walletApi.unlockWallet({ walletPassword }),
      );
      if (!success) lockRequestedRef.current = true;
      if (success) {
        setState((current) => ({
          ...current,
          dialogs: { ...current.dialogs, deleteWallet: { step: "idle", secondsRemaining: 10 } },
        }));
        void refreshPortfolioInBackground();
      }
      return success;
    },
    [refreshPortfolioInBackground, runRefreshCommand],
  );

  const signTransaction = useCallback(
    async (draft: SendDraft) => {
      const wallet = stateRef.current.wallet;
      const selectedNetwork = networkById(draft.network);
      if (wallet.status !== "unlocked" || !selectedNetwork) {
        toast("Please select a supported network.", "error");
        return;
      }
      if (
        draft.destinationTag !== null &&
        (!Number.isInteger(draft.destinationTag) ||
          draft.destinationTag < 0 ||
          draft.destinationTag > 0xffff_ffff)
      ) {
        toast("Destination tag must be an integer from 0 to 4,294,967,295.", "error");
        return;
      }
      if (draft.destinationTag !== null && draft.network !== "xrpl") {
        toast("Destination tags are only supported for XRP Ledger payments.", "error");
        return;
      }

      const asset = wallet.assets.find((candidate) => {
        if (candidate.network !== draft.network) return false;
        if (draft.token_address === null) return candidate.token_address == null;
        if (candidate.token_address == null) return false;
        return selectedNetwork.kind === "evm"
          ? candidate.token_address.toLowerCase() === draft.token_address.toLowerCase()
          : candidate.token_address === draft.token_address;
      });
      if (!asset) {
        toast("Selected asset is no longer available.", "error");
        return;
      }
      if (!beginOperation()) return;

      setState((current) => ({
        ...current,
        send: { ...current.send, draft },
      }));
      try {
        const signedTransaction = await walletApi.signTransaction({
          to: draft.to,
          symbol: asset.symbol,
          network: draft.network,
          tokenAddress: draft.token_address,
          amount: toWei(draft.amount || "0", asset.decimals),
          note: draft.note,
          destinationTag: draft.destinationTag,
        });
        if (!lockRequestedRef.current && stateRef.current.wallet.status === "unlocked") {
          setState((current) => ({
            ...current,
            send: { ...current.send, signedTransaction },
          }));
          toast("Transaction signed locally.", "success");
        }
      } catch (error) {
        toast(formatError(error), "error");
      } finally {
        endOperation();
      }
    },
    [beginOperation, endOperation, toast],
  );

  const broadcastSignedTransaction = useCallback(async () => {
    const signed = stateRef.current.send.signedTransaction;
    if (!signed || !window.confirm("Broadcast this signed transaction to the chain RPC?")) return;
    const success = await runSessionCommand("send_transaction", () =>
      walletApi.sendTransaction({ signed }),
    );
    if (success) {
      setState((current) => ({
        ...current,
        send: { draft: emptySendDraft(), signedTransaction: null },
      }));
    }
  }, [runSessionCommand]);

  const swapTokens = useCallback(
    async (fromSymbol: string, toSymbol: string, amount: string) => {
      const wallet = stateRef.current.wallet;
      const decimals =
        wallet.status === "unlocked"
          ? (wallet.assets.find((asset) => asset.symbol === fromSymbol)?.decimals ?? 18)
          : 18;
      await runSessionCommand("swap_tokens", () =>
        walletApi.swapTokens({ fromSymbol, toSymbol, amount: toWei(amount, decimals) }),
      );
    },
    [runSessionCommand],
  );

  const saveWalletSettings = useCallback(
    async (settings: Parameters<typeof walletApi.updateWalletSettings>[0]) => {
      if (!beginOperation()) return false;
      try {
        commitSession(await walletApi.updateWalletSettings(settings));
        toast("Wallet settings updated.", "success");
        return true;
      } catch (error) {
        toast(formatError(error), "error");
        return false;
      } finally {
        endOperation();
      }
    },
    [beginOperation, commitSession, endOperation, toast],
  );

  const clearWallet = useCallback(async () => {
    invalidatePortfolioRefresh();
    const success = await runSessionCommand("clear_wallet", () => walletApi.clearWallet());
    if (success) {
      setState((current) => ({
        ...current,
        navigation: { currentView: "dashboard", selectedActivityId: "" },
        onboarding: createSetupWizardState(),
        send: { draft: emptySendDraft(), signedTransaction: null },
        dialogs: { ...current.dialogs, deleteWallet: { step: "idle", secondsRemaining: 10 } },
      }));
    }
  }, [invalidatePortfolioRefresh, runSessionCommand]);

  useDeleteCountdown(state.dialogs.deleteWallet.step, clearWallet, setState);
  usePendingTransactionPolling(state.wallet, setState, refreshPortfolioInBackground);
  useScrollbarSync();

  useEffect(() => {
    let cancelled = false;
    const startedAt = performance.now();

    async function loadSession() {
      setBusy(true);
      if (!isTauri()) {
        setBusy(false);
        setSessionLoaded(true);
        return;
      }
      try {
        const session = await walletApi.getWallet();
        if (!cancelled) commitSession(session);
      } catch (error) {
        if (!cancelled) toastRef.current(formatError(error), "error");
      } finally {
        const remaining = MINIMUM_SPLASH_DURATION_MS - (performance.now() - startedAt);
        if (remaining > 0)
          await new Promise<void>((resolve) => window.setTimeout(resolve, remaining));
        if (!cancelled) {
          setBusy(false);
          setSessionLoaded(true);
        }
      }
    }

    void loadSession();
    return () => {
      cancelled = true;
    };
  }, [commitSession, setBusy]);

  const selectView = useCallback((view: View) => {
    setState((current) => ({
      ...current,
      navigation: { ...current.navigation, currentView: view },
    }));
  }, []);

  const copyText = useCallback(
    async (value: string, message: string) => {
      try {
        await writeText(value);
        toast(message, "success");
      } catch (error) {
        toast(formatError(error), "error");
      }
    },
    [toast],
  );

  const checkForUpdates = useCallback(async () => {
    if (!isTauri()) {
      toast("Update checks are available in the desktop app.", "info");
      return;
    }
    if (!beginOperation()) return;
    try {
      const update = await check();
      if (!update) {
        toast("VaultForge is up to date.", "info");
        return;
      }
      const notes = update.body ? `\n\nRelease notes:\n${update.body}` : "";
      if (
        !window.confirm(
          `VaultForge ${update.version} is available.${notes}\n\nInstall and restart now?`,
        )
      ) {
        return;
      }
      toast(`Downloading VaultForge ${update.version}…`, "info");
      await update.downloadAndInstall((event) => {
        if (event.event === "Finished") toast("Update downloaded. Installing…", "info");
      });
      await relaunch();
    } catch (error) {
      toast(formatError(error), "error");
    } finally {
      endOperation();
    }
  }, [beginOperation, endOperation, toast]);

  return (
    <Box background="bg" className="app">
      {state.operation.busy && <LoadingBar />}
      {!sessionLoaded ? (
        <SplashScreen />
      ) : state.wallet.status === "missing" ? (
        <OnboardingView
          colorScheme={colorScheme}
          completeSetup={completeSetup}
          setColorScheme={setColorScheme}
          setState={setState}
          state={state}
          toast={toast}
        />
      ) : state.wallet.status === "locked" ? (
        <LockedView
          busy={state.operation.busy}
          onDelete={() =>
            setState((current) => ({
              ...current,
              dialogs: {
                ...current.dialogs,
                deleteWallet: { step: "confirm", secondsRemaining: 10 },
              },
            }))
          }
          onUnlock={unlockWallet}
        />
      ) : (
        <WalletShell
          checkForUpdates={checkForUpdates}
          clearWallet={() =>
            setState((current) => ({
              ...current,
              dialogs: {
                ...current.dialogs,
                deleteWallet: { step: "confirm", secondsRemaining: 10 },
              },
            }))
          }
          colorScheme={colorScheme}
          copyText={copyText}
          onBroadcast={broadcastSignedTransaction}
          onLock={() => void lockWallet()}
          onRefresh={() => void refreshPortfolio()}
          onSign={signTransaction}
          onSaveSettings={saveWalletSettings}
          onSwap={swapTokens}
          selectView={selectView}
          setColorScheme={setColorScheme}
          setState={setState}
          state={state}
          toast={toast}
        />
      )}
      <DeleteWalletDialog setState={setState} state={state} />
      <PageScrollbar />
    </Box>
  );
}

function useColorScheme(): [ColorScheme, (scheme: ColorScheme) => void] {
  const [colorScheme, setColorSchemeState] = useState<ColorScheme>(() => {
    const stored = window.localStorage.getItem("vaultforge-color-scheme");
    return stored === "light" || stored === "dark" ? stored : "dark";
  });
  const setColorScheme = useCallback((scheme: ColorScheme) => {
    window.localStorage.setItem("vaultforge-color-scheme", scheme);
    setColorSchemeState(scheme);
  }, []);
  return [colorScheme, setColorScheme];
}

function useAutoLock(
  wallet: WalletState,
  stopRef: React.RefObject<() => void>,
  lockRef: React.RefObject<() => void>,
) {
  useEffect(() => {
    let timer: number | null = null;
    let lastActivity = Date.now();
    const timeoutMs = wallet.status === "unlocked" ? (wallet.autoLockTimeoutSecs ?? 0) * 1000 : 0;

    const stop = () => {
      if (timer !== null) {
        window.clearTimeout(timer);
        timer = null;
      }
    };
    const schedule = () => {
      stop();
      if (!timeoutMs) return;
      const remaining = timeoutMs - (Date.now() - lastActivity);
      timer = window.setTimeout(
        () => {
          if (remaining <= 0) lockRef.current();
          else schedule();
        },
        Math.max(0, remaining),
      );
    };
    const recordActivity = () => {
      lastActivity = Date.now();
      schedule();
    };

    stopRef.current = stop;
    if (timeoutMs) {
      schedule();
      document.addEventListener("click", recordActivity);
      document.addEventListener("keydown", recordActivity);
    }
    return () => {
      stop();
      document.removeEventListener("click", recordActivity);
      document.removeEventListener("keydown", recordActivity);
    };
  }, [
    lockRef,
    stopRef,
    wallet.status,
    wallet.status === "unlocked" ? wallet.autoLockTimeoutSecs : 0,
  ]);
}

function useDeleteCountdown(
  step: AppState["dialogs"]["deleteWallet"]["step"],
  clearWallet: () => Promise<void>,
  setState: React.Dispatch<React.SetStateAction<AppState>>,
) {
  useEffect(() => {
    if (step !== "countdown") return;
    let seconds = 10;
    const timer = window.setInterval(() => {
      seconds -= 1;
      if (seconds <= 0) {
        window.clearInterval(timer);
        void clearWallet();
        return;
      }
      setState((current) => ({
        ...current,
        dialogs: {
          ...current.dialogs,
          deleteWallet: { step: "countdown", secondsRemaining: seconds },
        },
      }));
    }, 1_000);
    return () => window.clearInterval(timer);
  }, [clearWallet, setState, step]);
}

function usePendingTransactionPolling(
  wallet: WalletState,
  setState: React.Dispatch<React.SetStateAction<AppState>>,
  onSettled: () => void,
) {
  const walletRef = useRef(wallet);
  useEffect(() => {
    walletRef.current = wallet;
  }, [wallet]);

  const hasPending =
    wallet.status === "unlocked" &&
    wallet.activity.some((item) => item.status === "pending" && item.hash && item.network);

  useEffect(() => {
    if (!hasPending) return;
    let polling = false;
    const poll = async () => {
      if (polling) return;
      polling = true;
      try {
        const currentWallet = walletRef.current;
        if (currentWallet.status !== "unlocked") return;
        const pending = currentWallet.activity.filter(
          (item) => item.status === "pending" && item.hash && item.network,
        );
        const changes = await Promise.all(
          pending.map(async (item) => {
            try {
              return [
                item.id,
                await walletApi.checkTransactionStatus({
                  txHash: item.hash!,
                  network: item.network!,
                }),
              ] as const;
            } catch {
              return [item.id, null] as const;
            }
          }),
        );
        const statuses = new Map(
          changes.filter((entry): entry is [string, string] => entry[1] !== null),
        );
        if (statuses.size === 0) return;
        const settled = pending.some((item) => {
          const status = statuses.get(item.id);
          return status && status !== "pending";
        });
        setState((current) => {
          if (current.wallet.status !== "unlocked") return current;
          return {
            ...current,
            wallet: {
              ...current.wallet,
              activity: current.wallet.activity.map((item) => ({
                ...item,
                status: statuses.get(item.id) ?? item.status,
              })),
            },
          };
        });
        if (settled) void onSettled();
      } finally {
        polling = false;
      }
    };
    const timer = window.setInterval(() => void poll(), 10_000);
    return () => window.clearInterval(timer);
  }, [hasPending, onSettled, setState]);
}

function useScrollbarSync() {
  useEffect(() => {
    installScrollbarBehavior();
  }, []);
  useLayoutEffect(() => {
    syncHorizontalScrollbars();
    syncVerticalScrollbar();
  });
}

function OnboardingView({
  colorScheme,
  completeSetup,
  setColorScheme,
  setState,
  state,
  toast,
}: {
  colorScheme: ColorScheme;
  completeSetup: () => Promise<void>;
  setColorScheme: (scheme: ColorScheme) => void;
  setState: React.Dispatch<React.SetStateAction<AppState>>;
  state: AppState;
  toast: ToastFn;
}) {
  const wizard = state.onboarding;

  const updateWizard = (update: (current: AppState["onboarding"]) => AppState["onboarding"]) => {
    setState((current) => ({ ...current, onboarding: update(current.onboarding) }));
  };
  const chooseFlow = (flow: "create" | "import") => {
    updateWizard((current) => ({
      ...current,
      flow,
      step: 2,
      recoveryPhrase: "",
      recoveryPhraseVisible: false,
      acknowledgedBackup: false,
    }));
  };
  const goBack = () =>
    updateWizard((current) => ({ ...current, step: Math.max(1, current.step - 1) }));
  const goNext = async () => {
    const current = state.onboarding;
    if (current.step === 2) {
      if (current.walletPassword.length < 8) {
        toast("Wallet password must be at least 8 characters.", "error");
        return;
      }
      if (current.walletPassword !== current.confirmWalletPassword) {
        toast("Wallet passwords do not match.", "error");
        return;
      }
      if (!walletPasswordStrength(current.walletPassword).meetsPolicy) {
        toast("Use a stronger wallet password to continue.", "error");
        return;
      }
    }
    if (current.step === 4 && current.enabledNetworks.length === 0) {
      toast("Enable at least one network.", "error");
      return;
    }
    if (current.step >= 5) {
      await completeSetup();
      return;
    }
    const phraseWordCount = current.recoveryPhrase.trim().split(/\s+/).filter(Boolean).length;
    if (current.step === 3 && current.flow === "create" && phraseWordCount !== current.wordCount) {
      try {
        const recoveryPhrase = await walletApi.generateMnemonic(current.wordCount);
        updateWizard((wizardState) =>
          wizardState.wordCount === current.wordCount && wizardState.flow === "create"
            ? {
                ...wizardState,
                recoveryPhrase,
                recoveryPhraseVisible: false,
                acknowledgedBackup: false,
                step: 4,
              }
            : wizardState,
        );
      } catch {
        toast("Failed to generate recovery phrase.", "error");
      }
      return;
    }
    updateWizard((wizardState) => ({ ...wizardState, step: wizardState.step + 1 }));
  };

  return (
    <Box className="onboarding">
      <VStack alignSelf="center" className="onboarding-intro" gap={4} padding={4}>
        <HStack alignItems="center" className="onboarding-brand" gap={2}>
          <LegacyIcon size="l" svg={walletIcon} />
          <Text as="h1" className="onboarding-brand-title" font="display2">
            VaultForge
          </Text>
        </HStack>
        <Text as="p" color="fgMuted" font="title2">
          Local-first, multichain self-custody wallet.
        </Text>
        <Grid columnMin="170px" gap={2}>
          <InfoCard
            body="Encrypted wallet storage and Rust-backed signing."
            title="Local control"
          />
          <InfoCard
            body="Provider-derived balances and network-aware transfers."
            title="Real chain data"
          />
          <InfoCard body="Bitcoin, EVM, Solana, Tron, and XRP Ledger paths." title="Multichain" />
        </Grid>
      </VStack>
      <ContentCard
        alignSelf="center"
        background="bgElevation1"
        borderRadius={400}
        className="onboarding-card"
        gap={3}
        padding={4}
      >
        <HStack alignItems="center" justifyContent="space-between">
          <VStack gap={0.5}>
            <Text color="fgMuted" font="label2">
              Wallet setup
            </Text>
            <Text as="h2" font="title1">
              {setupTitle(wizard.step, wizard.flow)}
            </Text>
          </VStack>
          <ColorSchemeToggle colorScheme={colorScheme} setColorScheme={setColorScheme} />
        </HStack>
        <SetupProgress step={wizard.step} />
        {wizard.step === 1 && <SetupChoice onChoose={chooseFlow} />}
        {wizard.step === 2 && <SetupIdentity updateWizard={updateWizard} wizard={wizard} />}
        {wizard.step === 3 && <SetupRecovery updateWizard={updateWizard} wizard={wizard} />}
        {wizard.step === 4 && <SetupPreferences updateWizard={updateWizard} wizard={wizard} />}
        {wizard.step === 5 && <SetupConfirm updateWizard={updateWizard} wizard={wizard} />}
        {wizard.step > 1 && (
          <SetupActions
            onBack={goBack}
            onNext={() => void goNext()}
            submitLabel={
              wizard.step === 5
                ? wizard.flow === "import"
                  ? "Import wallet"
                  : "Create wallet"
                : "Next"
            }
          />
        )}
      </ContentCard>
    </Box>
  );
}

function LockedView({
  busy,
  onDelete,
  onUnlock,
}: {
  busy: boolean;
  onDelete: () => void;
  onUnlock: (walletPassword: string) => Promise<boolean>;
}) {
  const [walletPassword, setWalletPassword] = useState("");
  const [visible, setVisible] = useState(false);
  const submit: SubmitEventHandler<HTMLFormElement> = (event) => {
    event.preventDefault();
    void onUnlock(walletPassword).then((success) => {
      if (success) setWalletPassword("");
    });
  };

  return (
    <Box className="app locked-layout" padding={3}>
      <ContentCard
        background="bgElevation1"
        borderRadius={400}
        gap={3}
        maxWidth="460px"
        padding={4}
        width="100%"
      >
        <VStack alignItems="center" gap={1}>
          <LegacyIcon size="l" svg={lockIcon} />
          <Text color="fgMuted" font="label2">
            Wallet locked
          </Text>
          <Text as="h1" font="title1">
            Unlock VaultForge
          </Text>
          <Text color="fgMuted" font="body" textAlign="center">
            Enter wallet password to access locally encrypted wallet session.
          </Text>
        </VStack>
        <form onSubmit={submit}>
          <VStack gap={2}>
            <TextInput
              autoComplete="current-password"
              end={
                <Button
                  onClick={() => setVisible((current) => !current)}
                  size="s"
                  transparent
                  type="button"
                >
                  {visible ? "Hide" : "Show"}
                </Button>
              }
              label="Wallet password"
              onChange={(event) => setWalletPassword(event.target.value)}
              required
              type={visible ? "text" : "password"}
              value={walletPassword}
            />
            <Button block loading={busy} type="submit">
              Unlock wallet
            </Button>
          </VStack>
        </form>
        <Box borderedTop paddingTop={3}>
          <VStack gap={2}>
            <Text color="fgNegative" font="label1">
              Danger zone
            </Text>
            <Text color="fgMuted" font="body">
              Delete only after verifying recovery phrase backup.
            </Text>
            <Button block onClick={onDelete} variant="negative">
              Delete stored wallet
            </Button>
          </VStack>
        </Box>
      </ContentCard>
    </Box>
  );
}

function WalletShell({
  checkForUpdates,
  clearWallet,
  colorScheme,
  copyText,
  onBroadcast,
  onLock,
  onRefresh,
  onSign,
  onSaveSettings,
  onSwap,
  selectView,
  setColorScheme,
  setState,
  state,
  toast,
}: {
  checkForUpdates: () => Promise<void>;
  clearWallet: () => void;
  colorScheme: ColorScheme;
  copyText: (value: string, message: string) => Promise<void>;
  onBroadcast: () => Promise<void>;
  onLock: () => void;
  onRefresh: () => void;
  onSign: (draft: SendDraft) => Promise<void>;
  onSaveSettings: (
    settings: Parameters<typeof walletApi.updateWalletSettings>[0],
  ) => Promise<boolean>;
  onSwap: (fromSymbol: string, toSymbol: string, amount: string) => Promise<void>;
  selectView: (view: View) => void;
  setColorScheme: (scheme: ColorScheme) => void;
  setState: React.Dispatch<React.SetStateAction<AppState>>;
  state: AppState;
  toast: ToastFn;
}) {
  const wallet = state.wallet;
  const isSidebarCollapsed = useMediaQuery("(max-width: 1199px)");
  if (wallet.status !== "unlocked") return null;

  return (
    <div className={`shell${isSidebarCollapsed ? " shell-sidebar-collapsed" : ""}`}>
      <aside className="sidebar">
        <Sidebar
          accessibilityLabel="Wallet sections"
          background="bgElevation1"
          borderedEnd={false}
          borderRadius={400}
          collapsed={isSidebarCollapsed}
          elevation={1}
          height="100%"
          logo={(collapsed) => (
            <HStack alignItems="center" gap={2} justifyContent={collapsed ? "center" : undefined}>
              <LegacyIcon size="l" svg={walletIcon} />
              {!collapsed && <Text font="title3">{wallet.name}</Text>}
            </HStack>
          )}
          renderEnd={(collapsed) =>
            collapsed ? null : <AddressList copyText={copyText} wallet={wallet} />
          }
          styles={{
            content: isSidebarCollapsed
              ? { alignItems: "center", marginInlineStart: 0 }
              : undefined,
            // Stretch the end section to absorb all free sidebar height so the
            // nav above keeps exactly its natural height. The address list
            // fills this section and scrolls internally instead of spilling
            // past the sidebar.
            end: {
              width: "100%",
              marginTop: 0,
              paddingTop: 16,
              flex: "1 1 auto",
              minHeight: 0,
              display: "flex",
              flexDirection: "column",
              overflow: "hidden",
            },
            logo: isSidebarCollapsed ? { alignItems: "center", paddingInlineStart: 0 } : undefined,
          }}
          width="100%"
        >
          {NAV_ITEMS.map((item) => (
            <SidebarItem
              active={state.navigation.currentView === item.view}
              Component={LegacySidebarItemContent}
              icon="home"
              key={item.view}
              onClick={() => selectView(item.view)}
              title={item.label}
              tooltipContent={item.label}
            />
          ))}
        </Sidebar>
      </aside>
      <nav aria-label="Wallet sections" className="mobile-nav">
        {NAV_ITEMS.map((item) => (
          <Button
            accessibilityLabel={item.label}
            aria-current={state.navigation.currentView === item.view ? "page" : undefined}
            className="mobile-nav-button"
            key={item.view}
            onClick={() => selectView(item.view)}
            start={<LegacyIcon svg={item.icon} />}
            variant={state.navigation.currentView === item.view ? "primary" : "secondary"}
          >
            <span className="mobile-nav-label">{item.label}</span>
          </Button>
        ))}
      </nav>
      <main className="main">
        <TopBar
          onLock={onLock}
          onRefresh={onRefresh}
          selectView={selectView}
          state={state}
          wallet={wallet}
        />
        <div className="main-scroll" data-main-scroll>
          <WalletView
            checkForUpdates={checkForUpdates}
            clearWallet={clearWallet}
            colorScheme={colorScheme}
            copyText={copyText}
            onBroadcast={onBroadcast}
            onSign={onSign}
            onSaveSettings={onSaveSettings}
            onSwap={onSwap}
            selectView={selectView}
            setColorScheme={setColorScheme}
            setState={setState}
            state={state}
            toast={toast}
            wallet={wallet}
          />
        </div>
      </main>
    </div>
  );
}

function WalletView({
  checkForUpdates,
  clearWallet,
  colorScheme,
  copyText,
  onBroadcast,
  onSign,
  onSaveSettings,
  onSwap,
  selectView,
  setColorScheme,
  setState,
  state,
  toast,
  wallet,
}: {
  checkForUpdates: () => Promise<void>;
  clearWallet: () => void;
  colorScheme: ColorScheme;
  copyText: (value: string, message: string) => Promise<void>;
  onBroadcast: () => Promise<void>;
  onSign: (draft: SendDraft) => Promise<void>;
  onSaveSettings: (
    settings: Parameters<typeof walletApi.updateWalletSettings>[0],
  ) => Promise<boolean>;
  onSwap: (fromSymbol: string, toSymbol: string, amount: string) => Promise<void>;
  selectView: (view: View) => void;
  setColorScheme: (scheme: ColorScheme) => void;
  setState: React.Dispatch<React.SetStateAction<AppState>>;
  state: AppState;
  toast: ToastFn;
  wallet: Extract<WalletState, { status: "unlocked" }>;
}) {
  if (state.navigation.currentView === "send") {
    return (
      <SendView
        busy={state.operation.busy}
        onBroadcast={onBroadcast}
        onSign={onSign}
        setState={setState}
        state={state}
        wallet={wallet}
      />
    );
  }
  if (state.navigation.currentView === "receive") {
    return (
      <ReceiveView
        copyText={copyText}
        setState={setState}
        state={state}
        toast={toast}
        wallet={wallet}
      />
    );
  }
  if (state.navigation.currentView === "swap")
    return <SwapView busy={state.operation.busy} onSwap={onSwap} wallet={wallet} />;
  if (state.navigation.currentView === "assets") return <AssetsView wallet={wallet} />;
  if (state.navigation.currentView === "activity")
    return <ActivityView copyText={copyText} setState={setState} state={state} wallet={wallet} />;
  if (state.navigation.currentView === "settings") {
    return (
      <SettingsView
        busy={state.operation.busy}
        checkForUpdates={checkForUpdates}
        clearWallet={clearWallet}
        colorScheme={colorScheme}
        onSave={onSaveSettings}
        setColorScheme={setColorScheme}
        wallet={wallet}
      />
    );
  }
  return <DashboardView selectView={selectView} wallet={wallet} />;
}

function LoadingBar() {
  return (
    <Box background="bgPrimary" height="4px" position="fixed" top="0" width="100%" zIndex={10} />
  );
}

function SplashScreen() {
  return (
    <Box className="splash" padding={3}>
      <VStack alignItems="center" gap={2}>
        <img alt="VaultForge" className="logo" height={64} src={appLogoUrl} width={64} />
        <Text font="title2">Opening VaultForge</Text>
        <Text color="fgMuted" font="body">
          Loading encrypted wallet session…
        </Text>
      </VStack>
    </Box>
  );
}

function LegacyIcon({ size = "m", svg }: { size?: "s" | "m" | "l"; svg: string }) {
  return (
    <span
      aria-hidden="true"
      className={`icon icon-${size}`}
      dangerouslySetInnerHTML={{ __html: svg }}
    />
  );
}

function LegacySidebarItemContent({
  color,
  isCollapsed,
  title,
}: {
  color: ThemeVars.Color;
  isCollapsed?: boolean;
  title: string;
}) {
  const item = NAV_ITEMS.find((candidate) => candidate.label === title);
  if (!item) return null;

  return (
    <HStack
      alignItems="center"
      className={
        isCollapsed ? "sidebar-item-content sidebar-item-collapsed" : "sidebar-item-content"
      }
      color={color}
      gap={isCollapsed ? 0 : 2}
      justifyContent={isCollapsed ? "center" : undefined}
      paddingX={isCollapsed ? 0 : 2}
      paddingY={isCollapsed ? 0 : 2}
    >
      <LegacyIcon svg={item.icon} />
      {!isCollapsed && (
        <Text color={color} font="headline">
          {title}
        </Text>
      )}
    </HStack>
  );
}

function PageScrollbar() {
  return (
    <div
      aria-label="Scroll main content vertically"
      aria-orientation="vertical"
      aria-valuemax={0}
      aria-valuemin={0}
      aria-valuenow={0}
      className="main-scrollbar"
      data-vertical-scrollbar="main"
      role="scrollbar"
      tabIndex={0}
    >
      <div data-vertical-scrollbar-thumb />
    </div>
  );
}

function InfoCard({ body, title }: { body: string; title: string }) {
  return (
    <ContentCard background="bgSecondary" borderRadius={300} gap={1} padding={3}>
      <Text font="label1">{title}</Text>
      <Text color="fgMuted" font="body">
        {body}
      </Text>
    </ContentCard>
  );
}

function setupTitle(step: number, flow: "create" | "import") {
  const titles: Record<number, string> = {
    1: "Get started",
    2: flow === "import" ? "Import wallet" : "Create wallet",
    3: "Recovery phrase",
    4: "Preferences",
    5: "Confirm backup",
  };
  return titles[step] ?? "Wallet setup";
}

function SetupProgress({ step }: { step: number }) {
  return (
    <HStack className="setup-progress" gap={0.5} width="100%">
      {["Flow", "Identity", "Seed", "Settings", "Confirm"].map((label, index) => (
        <HStack
          alignItems="center"
          flexBasis="0%"
          flexGrow={1}
          gap={0.5}
          justifyContent="center"
          key={label}
          minWidth="0"
        >
          <Box
            alignItems="center"
            background={step === index + 1 ? "bgPrimary" : "bgSecondary"}
            borderRadius={400}
            color={step === index + 1 ? "fgInverse" : "fgMuted"}
            display="flex"
            height="28px"
            justifyContent="center"
            width="28px"
          >
            <Text font="label2">{index + 1}</Text>
          </Box>
          <Text
            className="setup-progress-label"
            color={step === index + 1 ? "fg" : "fgMuted"}
            font="label2"
          >
            {label}
          </Text>
        </HStack>
      ))}
    </HStack>
  );
}

function SetupChoice({ onChoose }: { onChoose: (flow: "create" | "import") => void }) {
  return (
    <VStack gap={2}>
      <Text color="fgMuted" font="body">
        Create new wallet or import existing BIP39 recovery phrase.
      </Text>
      <Button block onClick={() => onChoose("create")}>
        Create new wallet
      </Button>
      <Button block onClick={() => onChoose("import")} variant="secondary">
        Import existing wallet
      </Button>
    </VStack>
  );
}

function SetupIdentity({
  updateWizard,
  wizard,
}: {
  updateWizard: (update: (current: AppState["onboarding"]) => AppState["onboarding"]) => void;
  wizard: AppState["onboarding"];
}) {
  const strength = walletPasswordStrength(wizard.walletPassword);
  const strengthColor: ThemeVars.Color =
    strength.score >= 3 ? "fgPositive" : strength.score === 2 ? "fgWarning" : "fgNegative";
  return (
    <VStack gap={2}>
      <TextInput
        label="Wallet name"
        onChange={(event) => updateWizard((current) => ({ ...current, name: event.target.value }))}
        placeholder="Primary Vault"
        value={wizard.name}
      />
      <TextInput
        end={
          <Button
            onClick={() =>
              updateWizard((current) => ({
                ...current,
                walletPasswordVisible: !current.walletPasswordVisible,
              }))
            }
            size="s"
            transparent
            type="button"
          >
            {wizard.walletPasswordVisible ? "Hide" : "Show"}
          </Button>
        }
        label="Wallet password"
        minLength={8}
        onChange={(event) =>
          updateWizard((current) => ({ ...current, walletPassword: event.target.value }))
        }
        placeholder="Minimum 8 characters"
        type={wizard.walletPasswordVisible ? "text" : "password"}
        value={wizard.walletPassword}
      />
      <Box background="bgSecondary" borderRadius={300} padding={2} width="100%">
        <VStack gap={1} width="100%">
          <HStack alignItems="center" justifyContent="space-between" width="100%">
            <Text color="fgMuted" font="label2">
              Password strength:
            </Text>
            <Text color={strengthColor} font="label1">
              {strength.label}
            </Text>
          </HStack>
          <ProgressBar
            accessibilityLabel={`Password strength: ${strength.label}`}
            color={strengthColor}
            progress={strength.score / 4}
            style={{ width: "100%" }}
            weight="heavy"
          />
        </VStack>
      </Box>
      <TextInput
        end={
          <Button
            onClick={() =>
              updateWizard((current) => ({
                ...current,
                walletPasswordVisible: !current.walletPasswordVisible,
              }))
            }
            size="s"
            transparent
            type="button"
          >
            {wizard.walletPasswordVisible ? "Hide" : "Show"}
          </Button>
        }
        label="Confirm wallet password"
        minLength={8}
        onChange={(event) =>
          updateWizard((current) => ({ ...current, confirmWalletPassword: event.target.value }))
        }
        type={wizard.walletPasswordVisible ? "text" : "password"}
        value={wizard.confirmWalletPassword}
      />
    </VStack>
  );
}

function SetupRecovery({
  updateWizard,
  wizard,
}: {
  updateWizard: (update: (current: AppState["onboarding"]) => AppState["onboarding"]) => void;
  wizard: AppState["onboarding"];
}) {
  if (wizard.flow === "import") {
    return (
      <VStack gap={2}>
        <Text color="fgMuted" font="body">
          Enter recovery phrase exactly as backed up.
        </Text>
        <NativeTextArea
          accessibilityLabel="Recovery phrase"
          autoCapitalize="none"
          autoComplete="off"
          className="native-textarea"
          onChange={(event) =>
            updateWizard((current) => ({ ...current, recoveryPhrase: event.target.value }))
          }
          placeholder="12, 15, 18, 21, or 24 word phrase"
          spellCheck={false}
          value={wizard.recoveryPhrase}
        />
      </VStack>
    );
  }

  return (
    <VStack gap={2}>
      <Text color="fgMuted" font="body">
        Choose BIP39 recovery phrase length. Phrase generates before confirmation.
      </Text>
      <Grid columns={2} gap={2}>
        {[12, 24].map((count) => (
          <Button
            key={count}
            onClick={() =>
              updateWizard((current) => ({
                ...current,
                wordCount: count as 12 | 24,
                recoveryPhrase: current.wordCount === count ? current.recoveryPhrase : "",
                acknowledgedBackup: current.wordCount === count && current.acknowledgedBackup,
              }))
            }
            variant={wizard.wordCount === count ? "primary" : "secondary"}
          >
            {count} words
          </Button>
        ))}
      </Grid>
      <CdsSelect
        accessibilityLabel="Custom recovery phrase length"
        onChange={(value) =>
          updateWizard((current) => ({
            ...current,
            wordCount: Number(value) as 15 | 18 | 21,
            recoveryPhrase: current.wordCount === Number(value) ? current.recoveryPhrase : "",
            acknowledgedBackup: current.wordCount === Number(value) && current.acknowledgedBackup,
          }))
        }
        options={[
          { label: "15 words", value: "15" },
          { label: "18 words", value: "18" },
          { label: "21 words", value: "21" },
        ]}
        value={[15, 18, 21].includes(wizard.wordCount) ? String(wizard.wordCount) : "15"}
      />
    </VStack>
  );
}

function SetupPreferences({
  updateWizard,
  wizard,
}: {
  updateWizard: (update: (current: AppState["onboarding"]) => AppState["onboarding"]) => void;
  wizard: AppState["onboarding"];
}) {
  return (
    <VStack gap={3}>
      <VStack gap={1}>
        <Text font="label1">Networks</Text>
        <Grid columnMin="130px" gap={1}>
          {networks.map((network) => (
            <Checkbox
              checked={wizard.enabledNetworks.includes(network.id)}
              key={network.id}
              onChange={(event) =>
                updateWizard((current) => ({
                  ...current,
                  enabledNetworks: event.target.checked
                    ? [...new Set([...current.enabledNetworks, network.id])]
                    : current.enabledNetworks.filter((id) => id !== network.id),
                }))
              }
            >
              {network.nickname ?? network.name}
            </Checkbox>
          ))}
        </Grid>
      </VStack>
      <CdsSelect
        label="Auto-lock timeout"
        onChange={(value) =>
          updateWizard((current) => ({
            ...current,
            autoLockTimeoutSecs: value === "0" ? null : Number(value),
          }))
        }
        options={AUTO_LOCK_OPTIONS.map((option) => ({
          label: option.label,
          value: option.value,
        }))}
        value={wizard.autoLockTimeoutSecs === null ? "0" : String(wizard.autoLockTimeoutSecs)}
      />
      <CdsSelect
        label="Display currency"
        onChange={(value) =>
          updateWizard((current) => ({ ...current, fiatCurrency: value as FiatCurrency }))
        }
        options={fiatCurrencies.map((currency) => ({
          label: `${currency.label} (${currency.code})`,
          value: currency.code,
        }))}
        value={wizard.fiatCurrency}
      />
      <Switch
        checked={wizard.useCryptoSymbols}
        onChange={(event) =>
          updateWizard((current) => ({ ...current, useCryptoSymbols: event.target.checked }))
        }
      >
        Use crypto symbols where available
      </Switch>
    </VStack>
  );
}

function SetupConfirm({
  updateWizard,
  wizard,
}: {
  updateWizard: (update: (current: AppState["onboarding"]) => AppState["onboarding"]) => void;
  wizard: AppState["onboarding"];
}) {
  const visiblePhrase = wizard.recoveryPhraseVisible
    ? wizard.recoveryPhrase
    : wizard.recoveryPhrase.replace(/\S/g, "•");
  return (
    <VStack gap={2}>
      <Text color="fgMuted" font="body">
        {wizard.flow === "import"
          ? "Confirm recovery phrase before import."
          : "Write down recovery phrase before creating wallet."}
      </Text>
      <NativeTextArea
        accessibilityLabel="Recovery phrase"
        autoCapitalize="none"
        autoComplete="off"
        className="native-textarea"
        readOnly
        spellCheck={false}
        value={visiblePhrase}
      />
      <Button
        onClick={() =>
          updateWizard((current) => ({
            ...current,
            recoveryPhraseVisible: !current.recoveryPhraseVisible,
          }))
        }
        size="s"
        variant="secondary"
      >
        {wizard.recoveryPhraseVisible ? "Hide recovery phrase" : "Show recovery phrase"}
      </Button>
      {wizard.flow === "create" && (
        <Checkbox
          checked={wizard.acknowledgedBackup}
          onChange={(event) =>
            updateWizard((current) => ({ ...current, acknowledgedBackup: event.target.checked }))
          }
        >
          I wrote down recovery phrase and understand it is needed for recovery.
        </Checkbox>
      )}
      <Text color="fgMuted" font="body">
        {wizard.wordCount} words · {wizard.enabledNetworks.length} networks ·{" "}
        {wizard.autoLockTimeoutSecs
          ? `${wizard.autoLockTimeoutSecs / 60} min auto-lock`
          : "Auto-lock off"}
      </Text>
    </VStack>
  );
}

function SetupActions({
  onBack,
  onNext,
  submitLabel,
}: {
  onBack: () => void;
  onNext: () => void;
  submitLabel: string;
}) {
  return (
    <HStack gap={2}>
      <Button block onClick={onBack} variant="secondary">
        Back
      </Button>
      <Button block onClick={onNext}>
        {submitLabel}
      </Button>
    </HStack>
  );
}

function ColorSchemeToggle({
  colorScheme,
  setColorScheme,
}: {
  colorScheme: ColorScheme;
  setColorScheme: (scheme: ColorScheme) => void;
}) {
  return (
    <Switch
      checked={colorScheme === "dark"}
      onChange={(event) => setColorScheme(event.target.checked ? "dark" : "light")}
    >
      Dark mode
    </Switch>
  );
}

function TopBar({
  onLock,
  onRefresh,
  selectView,
  state,
  wallet,
}: {
  onLock: () => void;
  onRefresh: () => void;
  selectView: (view: View) => void;
  state: AppState;
  wallet: Extract<WalletState, { status: "unlocked" }>;
}) {
  const isCompact = useMediaQuery("(max-width: 640px)");
  const status =
    state.portfolio.status === "refreshing"
      ? "Updating balances…"
      : state.portfolio.status === "stale"
        ? "Portfolio data may be out of date. Refresh to retry."
        : "";
  return (
    <ContentCard
      background="bgElevation1"
      borderRadius={400}
      className="topbar"
      gap={2}
      padding={3}
    >
      <HStack alignItems="center" flexWrap="wrap" gap={2} justifyContent="space-between">
        <VStack gap={0.5}>
          <Text as="h1" font="title1">
            {wallet.name}
          </Text>
          {status && (
            <Text color="fgMuted" font="body" role="status">
              {status}
            </Text>
          )}
        </VStack>
        <HStack className="topbar-actions" flexWrap="wrap" gap={1}>
          <Button
            accessibilityLabel="Refresh portfolio"
            borderRadius={isCompact ? 1000 : undefined}
            disabled={state.portfolio.status === "refreshing"}
            height={isCompact ? "40px" : undefined}
            onClick={onRefresh}
            padding={isCompact ? 0 : undefined}
            size="s"
            start={isCompact ? undefined : <LegacyIcon size="s" svg={refreshIcon} />}
            variant="secondary"
            width={isCompact ? "40px" : undefined}
          >
            {isCompact ? (
              <Box as="span" alignItems="center" display="flex" justifyContent="center">
                <LegacyIcon size="s" svg={refreshIcon} />
              </Box>
            ) : (
              "Refresh"
            )}
          </Button>
          <Button
            accessibilityLabel="Lock wallet"
            borderRadius={isCompact ? 1000 : undefined}
            height={isCompact ? "40px" : undefined}
            onClick={onLock}
            padding={isCompact ? 0 : undefined}
            size="s"
            start={isCompact ? undefined : <LegacyIcon size="s" svg={lockIcon} />}
            variant="secondary"
            width={isCompact ? "40px" : undefined}
          >
            {isCompact ? (
              <Box as="span" alignItems="center" display="flex" justifyContent="center">
                <LegacyIcon size="s" svg={lockIcon} />
              </Box>
            ) : (
              "Lock"
            )}
          </Button>
          <Button
            accessibilityLabel="Send funds"
            onClick={() => selectView("send")}
            size="s"
            start={<LegacyIcon size="s" svg={sendIcon} />}
          >
            <span className="topbar-send-label">Send funds</span>
            <span className="topbar-send-label-short">Send</span>
          </Button>
        </HStack>
      </HStack>
    </ContentCard>
  );
}

function AddressList({
  copyText,
  wallet,
}: {
  copyText: (value: string, message: string) => Promise<void>;
  wallet: Extract<WalletState, { status: "unlocked" }>;
}) {
  const addresses = new Set<string>();
  const items = wallet.enabledNetworks.flatMap((id) => {
    const network = networkById(id);
    if (!network) return [];
    const address = addressForNetwork(wallet, network);
    if (!address || addresses.has(network.addressKey)) return [];
    addresses.add(network.addressKey);
    return [{ address, label: network.addressKey === "evm" ? "EVM" : network.name }];
  });
  return (
    <Box
      borderedTop
      className="address-list"
      overflow="auto"
      paddingTop={2}
      style={{ flex: "1 1 auto", minHeight: 0 }}
      width="100%"
    >
      <VStack gap={1} minWidth="0" width="100%">
        <Text color="fgMuted" font="label2">
          Addresses
        </Text>
        {items.map((item) => (
          <HStack
            alignItems="center"
            className="address-row"
            gap={1}
            justifyContent="space-between"
            key={item.label}
          >
            <Text className="address-text" mono overflow="truncate">
              {item.label}: {shortAddress(item.address)}
            </Text>
            <Button
              accessibilityLabel={`Copy ${item.label} address`}
              borderRadius={1000}
              className="icon-button"
              height="36px"
              minWidth="36px"
              onClick={() => void copyText(item.address, "Address copied.")}
              padding={0}
              size="s"
              variant="secondary"
            >
              <LegacyIcon size="s" svg={copyIcon} />
            </Button>
          </HStack>
        ))}
      </VStack>
    </Box>
  );
}

function DashboardView({
  selectView,
  wallet,
}: {
  selectView: (view: View) => void;
  wallet: Extract<WalletState, { status: "unlocked" }>;
}) {
  const assets = visibleAssets(wallet);
  const totalUsd = assets.reduce((sum, asset) => sum + assetValueUsd(asset), 0);
  const change = portfolioChange(wallet.assets);
  return (
    <Grid className="dashboard-grid" gap={3}>
      <VStack gap={3}>
        <ContentCard background="bgElevation1" borderRadius={400} gap={3} padding={4}>
          <HStack alignItems="start" flexWrap="wrap" gap={3} justifyContent="space-between">
            <Text as="h2" font="title1">
              Portfolio
            </Text>
            <VStack alignItems="end" gap={0.5}>
              <Text color="fgMuted" font="label2">
                Total value
              </Text>
              <Text font="title1">
                {money(usdToFiat(totalUsd, wallet.usdExchangeRate), wallet.fiatCurrency)}
              </Text>
              <Text color={change >= 0 ? "fgPositive" : "fgNegative"} font="label1">
                {change >= 0 ? "+" : ""}
                {change.toFixed(2)}% 24h
              </Text>
            </VStack>
          </HStack>
          {assets.length > 0 ? (
            <>
              <div className="asset-scroll" data-horizontal-scroll>
                <HStack gap={2}>
                  {assets.map((asset) => (
                    <AssetCard asset={asset} key={assetKey(asset)} wallet={wallet} />
                  ))}
                </HStack>
              </div>
              <div
                aria-label="Scroll portfolio assets horizontally"
                aria-orientation="horizontal"
                aria-valuemax={0}
                aria-valuemin={0}
                aria-valuenow={0}
                className="horizontal-scrollbar"
                data-horizontal-scrollbar
                role="scrollbar"
                tabIndex={0}
              >
                <div data-horizontal-scrollbar-thumb />
              </div>
            </>
          ) : (
            <EmptyState
              body="Unlock or refresh wallet to load provider balances."
              title="No assets"
            />
          )}
        </ContentCard>
        <ContentCard background="bgElevation1" borderRadius={400} gap={2} padding={3}>
          <HStack className="dashboard-activity-header" justifyContent="space-between">
            <Text as="h2" font="title2">
              Recent activity
            </Text>
            <Button onClick={() => selectView("activity")} size="s" transparent>
              View all
            </Button>
          </HStack>
          <VStack gap={1}>
            {wallet.activity.slice(0, 5).map((item) => (
              <ActivityRow item={item} key={item.id} />
            ))}
            {wallet.activity.length === 0 && (
              <EmptyState
                body="Signed sends, swaps, and wallet events appear here."
                title="No recent activity"
              />
            )}
          </VStack>
        </ContentCard>
      </VStack>
      <ContentCard
        alignSelf="start"
        background="bgElevation1"
        borderRadius={400}
        className="quick-actions"
        gap={2}
        padding={3}
      >
        <Text as="h2" font="title2">
          Quick actions
        </Text>
        <Button block onClick={() => selectView("send")}>
          Send
        </Button>
        <Button block onClick={() => selectView("receive")} variant="secondary">
          Receive
        </Button>
        <Button block onClick={() => selectView("swap")} variant="secondary">
          Swap
        </Button>
      </ContentCard>
    </Grid>
  );
}

function SendView({
  busy,
  onBroadcast,
  onSign,
  setState,
  state,
  wallet,
}: {
  busy: boolean;
  onBroadcast: () => Promise<void>;
  onSign: (draft: SendDraft) => Promise<void>;
  setState: React.Dispatch<React.SetStateAction<AppState>>;
  state: AppState;
  wallet: Extract<WalletState, { status: "unlocked" }>;
}) {
  if (state.send.signedTransaction) {
    return (
      <SignedTransactionView
        busy={busy}
        onBroadcast={onBroadcast}
        setState={setState}
        signed={state.send.signedTransaction}
        wallet={wallet}
      />
    );
  }
  return (
    <SendForm
      busy={busy}
      draft={state.send.draft}
      onSign={onSign}
      setState={setState}
      wallet={wallet}
    />
  );
}

function SendForm({
  busy,
  draft,
  onSign,
  setState,
  wallet,
}: {
  busy: boolean;
  draft: SendDraft;
  onSign: (draft: SendDraft) => Promise<void>;
  setState: React.Dispatch<React.SetStateAction<AppState>>;
  wallet: Extract<WalletState, { status: "unlocked" }>;
}) {
  const selectedAssetId = `${draft.network}:${draft.token_address ?? "native"}`;
  const selectedSymbol = draft.symbol || "ETH";
  const updateDraft = (update: (current: SendDraft) => SendDraft) => {
    setState((current) => ({
      ...current,
      send: { ...current.send, draft: update(current.send.draft) },
    }));
  };
  const submit: SubmitEventHandler<HTMLFormElement> = (event) => {
    event.preventDefault();
    void onSign(draft);
  };
  return (
    <ContentCard background="bgElevation1" borderRadius={400} gap={3} maxWidth="760px" padding={4}>
      <VStack gap={1}>
        <Text color="fgMuted" font="label2">
          Transfer
        </Text>
        <Text as="h2" font="title1">
          Send crypto
        </Text>
        <Text color="fgMuted" font="body">
          Transactions are signed locally before broadcast. Review signed details before confirming
          the transaction.
        </Text>
      </VStack>
      <form onSubmit={submit}>
        <VStack gap={2}>
          <TextInput
            label="Recipient address"
            onChange={(event) => updateDraft((current) => ({ ...current, to: event.target.value }))}
            placeholder={addressPlaceholder(selectedSymbol)}
            required
            value={draft.to}
          />
          <Grid columnMin="200px" gap={2}>
            <CdsSelect
              label="Asset"
              onChange={(value) => {
                const [network, tokenAddress] = value.split(":");
                const asset = wallet.assets.find(
                  (candidate) =>
                    `${candidate.network}:${candidate.token_address ?? "native"}` === value,
                );
                if (!asset || !networkById(network)) return;
                updateDraft((current) => ({
                  ...current,
                  symbol: asset.symbol,
                  network: network as NetworkId,
                  token_address: tokenAddress === "native" ? null : tokenAddress,
                }));
              }}
              options={wallet.assets.map((asset) => ({
                label: `${displayAssetSymbol(asset, wallet)} - ${asset.name} (${networkDisplayName(asset.network)})`,
                value: `${asset.network}:${asset.token_address ?? "native"}`,
              }))}
              value={selectedAssetId}
            />
            <TextInput
              label="Amount"
              inputMode="decimal"
              onChange={(event) =>
                updateDraft((current) => ({ ...current, amount: event.target.value }))
              }
              required
              value={draft.amount}
            />
          </Grid>
          <TextInput
            label="Destination tag (XRP only)"
            max="4294967295"
            min="0"
            onChange={(event) =>
              updateDraft((current) => ({
                ...current,
                destinationTag: event.target.value === "" ? null : Number(event.target.value),
              }))
            }
            placeholder="Required by some exchanges"
            step="1"
            type="number"
            value={draft.destinationTag ?? ""}
          />
          <TextInput
            label="Note"
            onChange={(event) =>
              updateDraft((current) => ({ ...current, note: event.target.value }))
            }
            placeholder="Optional transaction memo"
            value={draft.note}
          />
          <Button loading={busy} type="submit">
            Sign transaction
          </Button>
        </VStack>
      </form>
    </ContentCard>
  );
}

function SignedTransactionView({
  busy,
  onBroadcast,
  setState,
  signed,
  wallet,
}: {
  busy: boolean;
  onBroadcast: () => Promise<void>;
  setState: React.Dispatch<React.SetStateAction<AppState>>;
  signed: SignedTransaction;
  wallet: Extract<WalletState, { status: "unlocked" }>;
}) {
  const feeDecimals = decimalsForAsset(signed.feeSymbol, signed.network, signed.decimals, wallet);
  return (
    <ContentCard background="bgElevation1" borderRadius={400} gap={3} maxWidth="960px" padding={4}>
      <VStack gap={1}>
        <Text color="fgPrimary" font="label2">
          Signed transfer
        </Text>
        <Text as="h2" font="title1">
          Review signature
        </Text>
        <Text color="fgMuted" font="body">
          Broadcast only if each detail matches intent.
        </Text>
      </VStack>
      <Grid columnMin="240px" gap={2}>
        <Detail label="From" value={shortAddress(signed.from)} />
        <Detail label="To" value={shortAddress(signed.to)} />
        {signed.destinationTag !== null && signed.destinationTag !== undefined && (
          <Detail label="Destination tag" value={String(signed.destinationTag)} />
        )}
        <Detail
          label="Amount"
          value={`${formatWei(signed.amount, signed.decimals)} ${displaySymbolForSigned(signed, wallet)}`}
        />
        <Detail
          label="Network fee"
          value={`${formatWei(signed.feeAmount, feeDecimals)} ${signed.feeSymbol}`}
        />
        <Detail
          label="Total debit"
          value={`${formatWei(signed.totalDebit, signed.decimals)} ${displaySymbolForSigned(signed, wallet)}`}
        />
        <Detail
          label="Post-send balance"
          value={`${formatWei(signed.postBalance, signed.decimals)} ${displaySymbolForSigned(signed, wallet)}`}
        />
        <Detail
          label="Estimated value"
          value={money(usdToFiat(signed.fiatValue, wallet.usdExchangeRate), wallet.fiatCurrency)}
        />
        <Detail label="Network" value={networkDisplayName(signed.network)} />
        <Detail label={transactionReferenceLabel(signed.network)} value={signed.nonce} />
        <Detail label="Signed" value={new Date(signed.signedAt).toLocaleString()} />
      </Grid>
      <Detail label="Payload hash" mono value={signed.payloadHash} />
      <Detail label="Signature" mono value={signed.signature} />
      <HStack flexWrap="wrap" gap={2}>
        <Button loading={busy} onClick={() => void onBroadcast()}>
          Broadcast signed transaction
        </Button>
        <Button
          onClick={() =>
            setState((current) => ({
              ...current,
              send: { ...current.send, signedTransaction: null },
            }))
          }
          variant="secondary"
        >
          Edit transaction
        </Button>
      </HStack>
    </ContentCard>
  );
}

function ReceiveView({
  copyText,
  setState,
  state,
  toast,
  wallet,
}: {
  copyText: (value: string, message: string) => Promise<void>;
  setState: React.Dispatch<React.SetStateAction<AppState>>;
  state: AppState;
  toast: ToastFn;
  wallet: Extract<WalletState, { status: "unlocked" }>;
}) {
  const availableNetworks = wallet.enabledNetworks.flatMap((id) => {
    const network = networkById(id);
    return network ? [network] : [];
  });
  const network =
    availableNetworks.find((item) => item.id === state.receive.networkId) ?? availableNetworks[0];
  const address = network ? addressForNetwork(wallet, network) : "";
  const payload = network ? receivePayload(wallet, network) : "";
  const qrSvg = useQrSvg(payload, state.receive.qrResilience, toast);

  if (!network)
    return (
      <EmptyState
        body="Enable a network during setup before receiving funds."
        title="No receive network"
      />
    );

  const download = () => {
    if (!qrSvg || !address) return;
    const blob = new Blob([qrSvg], { type: "image/svg+xml;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = `vaultforge-${network.id}-${shortAddress(address).replace(/[^a-zA-Z0-9]/g, "")}-qr.svg`;
    document.body.appendChild(link);
    link.click();
    link.remove();
    URL.revokeObjectURL(url);
    toast("QR code downloaded.", "success");
  };

  return (
    <ContentCard background="bgElevation1" borderRadius={400} gap={3} maxWidth="760px" padding={4}>
      <VStack gap={1}>
        <Text color="fgMuted" font="label2">
          Receive
        </Text>
        <Text as="h2" font="title1">
          Deposit address
        </Text>
      </VStack>
      <Grid
        className="receive-grid"
        gap={2}
        templateColumns="minmax(0, min(220px, calc((100% - var(--space-2)) / 2))) minmax(0, 1fr)"
      >
        <CdsSelect
          className="receive-select"
          label="Receive network"
          onChange={(value) =>
            setState((current) => ({
              ...current,
              receive: { ...current.receive, networkId: value as NetworkId },
            }))
          }
          options={availableNetworks.map((item) => ({
            label: `${item.name} - ${networkLabel(item, wallet.useCryptoSymbols)}`,
            value: item.id,
          }))}
          style={{ width: "100%" }}
          value={network.id}
        />
        <CdsSelect
          className="receive-select"
          label="QR resilience"
          onChange={(value) =>
            setState((current) => ({
              ...current,
              receive: { ...current.receive, qrResilience: value as QrResilience },
            }))
          }
          options={QR_RESILIENCE_OPTIONS}
          style={{ width: "100%" }}
          value={state.receive.qrResilience}
        />
      </Grid>
      <VStack alignItems="center" background="bgPrimaryWash" borderRadius={300} gap={2} padding={3}>
        <Box
          alignItems="center"
          background="bg"
          borderRadius={200}
          className="qr"
          display="flex"
          height="224px"
          justifyContent="center"
          width="224px"
        >
          {payload && qrSvg ? (
            <div dangerouslySetInnerHTML={{ __html: qrSvg }} />
          ) : (
            <Text color="fgMuted" font="body">
              {payload ? "Generating QR…" : "Receive not available for this network."}
            </Text>
          )}
        </Box>
        <HStack flexWrap="wrap" gap={1} justifyContent="center">
          <Button
            disabled={!qrSvg}
            onClick={() => qrSvg && void copyText(qrSvg, "QR SVG copied.")}
            size="s"
            start={<LegacyIcon size="s" svg={copyIcon} />}
            variant="secondary"
          >
            Copy SVG
          </Button>
          <Button
            disabled={!qrSvg}
            onClick={download}
            size="s"
            start={<LegacyIcon size="s" svg={downloadIcon} />}
            variant="secondary"
          >
            Download SVG
          </Button>
        </HStack>
        <VStack gap={0.5} width="100%">
          <Text font="label1">{network.name} receive payload</Text>
          <Text mono overflow="break">
            {payload || "No address derived for this network."}
          </Text>
          <Text mono overflow="break">
            {address || ""}
          </Text>
        </VStack>
        <Button
          disabled={!address}
          onClick={() => void copyText(address, "Receive address copied.")}
        >
          Copy address
        </Button>
      </VStack>
    </ContentCard>
  );
}

function SwapView({
  busy,
  onSwap,
  wallet,
}: {
  busy: boolean;
  onSwap: (fromSymbol: string, toSymbol: string, amount: string) => Promise<void>;
  wallet: Extract<WalletState, { status: "unlocked" }>;
}) {
  const [fromSymbol, setFromSymbol] = useState(wallet.assets[0]?.symbol ?? "ETH");
  const [toSymbol, setToSymbol] = useState(
    wallet.assets.find((asset) => asset.symbol === "USDC")?.symbol ??
      wallet.assets[1]?.symbol ??
      "USDC",
  );
  const [amount, setAmount] = useState("");
  const submit: SubmitEventHandler<HTMLFormElement> = (event) => {
    event.preventDefault();
    void onSwap(fromSymbol, toSymbol, amount);
  };
  const options = wallet.assets.map((asset) => ({
    label: `${displayAssetSymbol(asset, wallet)} - ${asset.name}`,
    value: asset.symbol,
  }));
  return (
    <ContentCard background="bgElevation1" borderRadius={400} gap={3} maxWidth="760px" padding={4}>
      <VStack gap={1}>
        <Text color="fgMuted" font="label2">
          Exchange
        </Text>
        <Text as="h2" font="title1">
          Swap assets
        </Text>
      </VStack>
      <form onSubmit={submit}>
        <VStack gap={2}>
          <Grid columnMin="200px" gap={2}>
            <CdsSelect label="From" onChange={setFromSymbol} options={options} value={fromSymbol} />
            <CdsSelect label="To" onChange={setToSymbol} options={options} value={toSymbol} />
          </Grid>
          <TextInput
            label="Amount"
            inputMode="decimal"
            onChange={(event) => setAmount(event.target.value)}
            required
            value={amount}
          />
          <Button loading={busy} type="submit">
            Execute simulated swap
          </Button>
        </VStack>
      </form>
    </ContentCard>
  );
}

function AssetsView({ wallet }: { wallet: Extract<WalletState, { status: "unlocked" }> }) {
  const assets = visibleAssets(wallet);
  return (
    <ContentCard background="bgElevation1" borderRadius={400} gap={3} padding={4}>
      <HStack justifyContent="space-between">
        <Text as="h2" font="title1">
          Assets
        </Text>
        <Text color="fgMuted" font="body">
          {assets.length} tracked
        </Text>
      </HStack>
      <Grid className="assets-grid" columnMin="280px" gap={2}>
        {assets.map((asset) => (
          <AssetCard asset={asset} key={assetKey(asset)} wallet={wallet} />
        ))}
        {assets.length === 0 && (
          <EmptyState
            body="Unlock or refresh wallet to view provider balances."
            title="No assets tracked"
          />
        )}
      </Grid>
    </ContentCard>
  );
}

function ActivityView({
  copyText,
  setState,
  state,
  wallet,
}: {
  copyText: (value: string, message: string) => Promise<void>;
  setState: React.Dispatch<React.SetStateAction<AppState>>;
  state: AppState;
  wallet: Extract<WalletState, { status: "unlocked" }>;
}) {
  const selected = selectedActivity(wallet, state.navigation.selectedActivityId);
  return (
    <Grid className="activity-grid" gap={3}>
      <ContentCard background="bgElevation1" borderRadius={400} gap={2} padding={3}>
        <Text as="h2" font="title1">
          Activity
        </Text>
        <VStack gap={1}>
          {wallet.activity.map((item) => (
            <ActivityRow
              active={selected?.id === item.id}
              item={item}
              key={item.id}
              onClick={() =>
                setState((current) => ({
                  ...current,
                  navigation: { ...current.navigation, selectedActivityId: item.id },
                }))
              }
            />
          ))}
          {wallet.activity.length === 0 && (
            <EmptyState
              body="Signed sends, swaps, and wallet changes appear here."
              title="No activity yet"
            />
          )}
        </VStack>
      </ContentCard>
      <ActivityDetails copyText={copyText} item={selected} />
    </Grid>
  );
}

function SettingsView({
  busy,
  checkForUpdates,
  clearWallet,
  colorScheme,
  onSave,
  setColorScheme,
  wallet,
}: {
  busy: boolean;
  checkForUpdates: () => Promise<void>;
  clearWallet: () => void;
  colorScheme: ColorScheme;
  onSave: (settings: Parameters<typeof walletApi.updateWalletSettings>[0]) => Promise<boolean>;
  setColorScheme: (scheme: ColorScheme) => void;
  wallet: Extract<WalletState, { status: "unlocked" }>;
}) {
  const [name, setName] = useState(wallet.name);
  const [fiatCurrency, setFiatCurrency] = useState(wallet.fiatCurrency);
  const [autoLockTimeoutSecs, setAutoLockTimeoutSecs] = useState(wallet.autoLockTimeoutSecs);
  const [useCryptoSymbols, setUseCryptoSymbols] = useState(wallet.useCryptoSymbols);
  const submit: SubmitEventHandler<HTMLFormElement> = async (event) => {
    event.preventDefault();
    await onSave({ name, fiatCurrency, autoLockTimeoutSecs, useCryptoSymbols });
  };
  return (
    <ContentCard background="bgElevation1" borderRadius={400} gap={3} maxWidth="760px" padding={4}>
      <VStack gap={1}>
        <Text color="fgMuted" font="label2">
          Preferences
        </Text>
        <Text as="h2" font="title1">
          Wallet settings
        </Text>
      </VStack>
      <form onSubmit={(event) => void submit(event)}>
        <VStack gap={3}>
          <TextInput
            label="Wallet name"
            maxLength={48}
            onChange={(event) => setName(event.target.value)}
            required
            value={name}
          />
          <CdsSelect
            label="Auto-lock timeout"
            onChange={(value) => setAutoLockTimeoutSecs(value === "0" ? null : Number(value))}
            options={AUTO_LOCK_OPTIONS.map((option) => ({
              label: option.label,
              value: option.value,
            }))}
            value={autoLockTimeoutSecs === null ? "0" : String(autoLockTimeoutSecs)}
          />
          <CdsSelect
            label="Display currency"
            onChange={(value) => setFiatCurrency(value as FiatCurrency)}
            options={fiatCurrencies.map((currency) => ({
              label: `${currency.label} (${currency.code})`,
              value: currency.code,
            }))}
            value={fiatCurrency}
          />
          <Switch
            checked={useCryptoSymbols}
            onChange={(event) => setUseCryptoSymbols(event.target.checked)}
          >
            Use crypto symbols where available
          </Switch>
          <ColorSchemeToggle colorScheme={colorScheme} setColorScheme={setColorScheme} />
          <Button loading={busy} type="submit">
            Save settings
          </Button>
        </VStack>
      </form>
      <Box borderedTop paddingTop={3}>
        <HStack alignItems="center" flexWrap="wrap" gap={2} justifyContent="space-between">
          <VStack gap={0.5}>
            <Text font="label1">Application updates</Text>
            <Text color="fgMuted" font="body">
              Check GitHub Releases for newer VaultForge version.
            </Text>
          </VStack>
          <Button onClick={() => void checkForUpdates()} size="s" variant="secondary">
            Check for updates
          </Button>
        </HStack>
      </Box>
      <Box background="bgNegativeWash" borderRadius={300} padding={3}>
        <VStack gap={1}>
          <Text color="fgNegative" font="label1">
            Danger zone
          </Text>
          <Text color="fgMuted" font="body">
            Remove encrypted local wallet file only after recovery phrase backup verification.
          </Text>
          <Button onClick={clearWallet} variant="negative">
            Clear local wallet
          </Button>
        </VStack>
      </Box>
    </ContentCard>
  );
}

function DeleteWalletDialog({
  setState,
  state,
}: {
  setState: React.Dispatch<React.SetStateAction<AppState>>;
  state: AppState;
}) {
  const dialog = state.dialogs.deleteWallet;
  if (dialog.step === "idle") return null;
  const cancel = () =>
    setState((current) => ({
      ...current,
      dialogs: { ...current.dialogs, deleteWallet: { step: "idle", secondsRemaining: 10 } },
    }));
  if (dialog.step === "countdown") {
    return (
      <Modal
        accessibilityLabel="Wallet deletion countdown"
        background="bgElevation2"
        borderRadius={400}
        disableOverlayPress
        onRequestClose={cancel}
        padding={4}
        role="alertdialog"
        visible
      >
        <VStack alignItems="center" gap={2}>
          <Text color="fgNegative" font="label2">
            Deletion pending
          </Text>
          <Text as="h2" font="title1">
            Deleting wallet files in
          </Text>
          <Text color="fgNegative" font="display2">
            {dialog.secondsRemaining}
          </Text>
          <Button block onClick={cancel} variant="secondary">
            Cancel
          </Button>
        </VStack>
      </Modal>
    );
  }
  return (
    <Modal
      accessibilityLabel="Delete stored wallet"
      background="bgElevation2"
      borderRadius={400}
      disableOverlayPress
      onRequestClose={cancel}
      padding={4}
      role="alertdialog"
      visible
    >
      <VStack gap={2}>
        <Text color="fgNegative" font="label2">
          Destructive action
        </Text>
        <Text as="h2" font="title1">
          Delete stored wallet?
        </Text>
        <Text color="fgMuted" font="body">
          You can lose funds if recovery phrase is not backed up and usable.
        </Text>
        <HStack gap={2}>
          <Button block onClick={cancel} variant="secondary">
            Cancel
          </Button>
          <Button
            block
            onClick={() =>
              setState((current) => ({
                ...current,
                dialogs: {
                  ...current.dialogs,
                  deleteWallet: { step: "countdown", secondsRemaining: 10 },
                },
              }))
            }
            variant="negative"
          >
            I backed up recovery phrase
          </Button>
        </HStack>
      </VStack>
    </Modal>
  );
}

function AssetCard({
  asset,
  wallet,
}: {
  asset: Asset;
  wallet: Extract<WalletState, { status: "unlocked" }>;
}) {
  const valueUsd = assetValueUsd(asset);
  const totalUsd = wallet.assets.reduce((sum, item) => sum + assetValueUsd(item), 0);
  const allocation = totalUsd ? (valueUsd / totalUsd) * 100 : 0;
  return (
    <ContentCard
      background="bgSecondary"
      borderRadius={300}
      className="asset-card"
      gap={2}
      minWidth="0"
      padding={3}
    >
      <HStack alignItems="start" justifyContent="space-between">
        <VStack gap={0.25}>
          <Text font="title3">{displayAssetSymbol(asset, wallet)}</Text>
          <Text color="fgMuted" font="body">
            {asset.name} on {networkDisplayName(asset.network)}
          </Text>
        </VStack>
        <Text color={asset.change_24h >= 0 ? "fgPositive" : "fgNegative"} font="label2">
          {asset.change_24h >= 0 ? "+" : ""}
          {asset.change_24h.toFixed(2)}%
        </Text>
      </HStack>
      <Text font="title2">
        {money(usdToFiat(valueUsd, wallet.usdExchangeRate), wallet.fiatCurrency)}
      </Text>
      <Text color="fgMuted" font="body">
        {formatWei(asset.balance, asset.decimals)} {displayAssetSymbol(asset, wallet)}
      </Text>
      <VStack gap={0.5}>
        <HStack justifyContent="space-between">
          <Text color="fgMuted" font="label2">
            Allocation
          </Text>
          <Text color="fgMuted" font="label2">
            {allocation.toFixed(1)}%
          </Text>
        </HStack>
        <Box background="bgTertiary" borderRadius={400} height="8px" overflow="hidden">
          <Box
            background="bgPrimary"
            borderRadius={400}
            height="100%"
            style={{ width: `${Math.max(2, allocation).toFixed(1)}%` }}
          />
        </Box>
      </VStack>
    </ContentCard>
  );
}

function ActivityRow({
  active = false,
  item,
  onClick,
}: {
  active?: boolean;
  item: Activity;
  onClick?: () => void;
}) {
  return (
    <Button
      block
      borderRadius={300}
      className="activity-row"
      onClick={onClick}
      transparent={!active}
      variant={active ? "primary" : "secondary"}
    >
      <HStack
        alignItems="center"
        className="activity-row-content"
        flexWrap="wrap"
        justifyContent="space-between"
        width="100%"
      >
        <VStack alignItems="start" gap={0.25}>
          <Text font="label1">{item.title}</Text>
          <Text color={active ? "fgInverse" : "fgMuted"} font="body">
            {item.subtitle} · {new Date(item.timestamp).toLocaleString()}
          </Text>
        </VStack>
        <VStack alignItems="end" className="activity-row-amount" gap={0.25}>
          <Text mono>{item.amount ?? ""}</Text>
          <Text color={active ? "fgInverse" : "fgPrimary"} font="label2">
            {item.status}
          </Text>
        </VStack>
      </HStack>
    </Button>
  );
}

function ActivityDetails({
  copyText,
  item,
}: {
  copyText: (value: string, message: string) => Promise<void>;
  item: Activity | null;
}) {
  if (!item)
    return (
      <EmptyState body="Select an activity to inspect details." title="No activity selected" />
    );
  const details = [
    ["Status", item.status],
    ["Amount", item.amount ?? "N/A"],
    ["Fee", item.fee ?? "N/A"],
    ["Network", networkDisplayName(item.network ?? "N/A")],
    ["Timestamp", new Date(item.timestamp).toLocaleString()],
  ];
  const copyableDetails = [
    ["Transaction hash", item.hash],
    ["From", item.from],
    ["To", item.to],
    ["Payload hash", item.payload_hash],
    ["Signature", item.signature],
  ];
  return (
    <ContentCard alignSelf="start" background="bgElevation1" borderRadius={400} gap={2} padding={3}>
      <Text color="fgMuted" font="label2">
        Activity details
      </Text>
      <Text as="h2" font="title2">
        {item.title}
      </Text>
      {details.map(([label, value]) => (
        <Detail key={label} label={label} value={value} />
      ))}
      {copyableDetails
        .filter((entry): entry is [string, string] => Boolean(entry[1]))
        .map(([label, value]) => (
          <Detail copyText={copyText} key={label} label={label} mono value={value} />
        ))}
    </ContentCard>
  );
}

function Detail({
  copyText,
  label,
  mono = false,
  value,
}: {
  copyText?: (value: string, message: string) => Promise<void>;
  label: string;
  mono?: boolean;
  value: string;
}) {
  return (
    <Box background="bgSecondary" borderRadius={200} padding={2}>
      <HStack alignItems="start" gap={1} justifyContent="space-between">
        <VStack gap={0.5} minWidth="0">
          <Text color="fgMuted" font="label2">
            {label}
          </Text>
          <Text mono={mono} overflow="break">
            {value}
          </Text>
        </VStack>
        {copyText && (
          <Button
            accessibilityLabel={`Copy ${label}`}
            borderRadius={1000}
            className="icon-button"
            height="36px"
            minWidth="36px"
            onClick={() => void copyText(value, `${label} copied.`)}
            padding={0}
            size="s"
            variant="secondary"
          >
            <LegacyIcon size="s" svg={copyIcon} />
          </Button>
        )}
      </HStack>
    </Box>
  );
}

function EmptyState({ body, title }: { body: string; title: string }) {
  return (
    <Box background="bgSecondaryWash" borderRadius={300} padding={3} textAlign="center">
      <VStack gap={1}>
        <Text font="label1">{title}</Text>
        <Text color="fgMuted" font="body">
          {body}
        </Text>
      </VStack>
    </Box>
  );
}

function CdsSelect({
  accessibilityLabel,
  className,
  label,
  onChange,
  options,
  style,
  value,
}: {
  accessibilityLabel?: string;
  className?: string;
  label?: string;
  onChange: (value: string) => void;
  options: ReadonlyArray<{ label: string; value: string }>;
  style?: CSSProperties;
  value: string;
}) {
  return (
    <Select
      accessibilityLabel={accessibilityLabel ?? label}
      className={className}
      label={label}
      onChange={(next) => {
        if (next !== null) onChange(next);
      }}
      options={options.map((option) => ({ ...option }))}
      style={style}
      value={value}
    />
  );
}

function useQrSvg(payload: string, resilience: QrResilience, toast: ToastFn): string {
  const [svg, setSvg] = useState("");
  useEffect(() => {
    let active = true;
    setSvg("");
    if (!payload)
      return () => {
        active = false;
      };
    void QRCode.toString(payload, {
      color: { dark: "#111827", light: "#ffffff" },
      errorCorrectionLevel: resilience,
      margin: 2,
      type: "svg",
    })
      .then((nextSvg) => {
        if (active) setSvg(nextSvg);
      })
      .catch((error: unknown) => {
        if (active) toast(formatError(error), "error");
      });
    return () => {
      active = false;
    };
  }, [payload, resilience, toast]);
  return svg;
}

function visibleAssets(wallet: Extract<WalletState, { status: "unlocked" }>) {
  return sortAssetsByValue(
    wallet.assets.filter((asset) => asset.price_usd <= 0 || assetValueUsd(asset) >= 1),
  );
}

function assetValueUsd(asset: Asset) {
  return weiToNumber(asset.balance, asset.decimals) * asset.price_usd;
}

function portfolioChange(assets: Asset[]) {
  const total = assets.reduce((sum, asset) => sum + assetValueUsd(asset), 0);
  if (!total) return 0;
  return assets.reduce((sum, asset) => sum + asset.change_24h * (assetValueUsd(asset) / total), 0);
}

function assetKey(asset: Asset) {
  return `${asset.network}:${asset.token_address ?? "native"}`;
}

function displayAssetSymbol(asset: Asset, wallet: Extract<WalletState, { status: "unlocked" }>) {
  return cryptoDisplaySymbol(asset.symbol, asset.unicode_symbol, wallet.useCryptoSymbols);
}

function decimalsForAsset(
  symbol: string,
  network: NetworkId,
  fallback: number,
  wallet: Extract<WalletState, { status: "unlocked" }>,
) {
  return (
    wallet.assets.find((asset) => asset.symbol === symbol && asset.network === network)?.decimals ??
    wallet.assets.find((asset) => asset.symbol === symbol)?.decimals ??
    fallback
  );
}

function displaySymbolForSigned(
  signed: SignedTransaction,
  wallet: Extract<WalletState, { status: "unlocked" }>,
) {
  const asset = wallet.assets.find(
    (candidate) => candidate.symbol === signed.symbol && candidate.network === signed.network,
  );
  return cryptoDisplaySymbol(signed.symbol, asset?.unicode_symbol, wallet.useCryptoSymbols);
}

function transactionReferenceLabel(network: NetworkId) {
  if (network === "bitcoin") return "Transaction model";
  if (network === "solana") return "Recent blockhash";
  if (network === "tron") return "Resource model";
  if (network === "xrpl") return "Sequence";
  return "Nonce";
}

function addressPlaceholder(symbol: string) {
  const placeholders: Record<string, string> = {
    BTC: "bc1... / 1... / 3...",
    ETH: "0x...",
    FIL: "f1... / f3...",
    INJ: "inj1...",
    SOL: "Solana address",
    TRX: "T...",
    XRP: "r...",
    ZEC: "t1... / t3...",
  };
  return placeholders[symbol] ?? "0x...";
}

function successMessage(command: SessionCommand) {
  const messages: Record<SessionCommand, string> = {
    create_wallet: "Wallet created. Recovery phrase was generated in Rust backend.",
    import_wallet: "Wallet imported successfully.",
    unlock_wallet: "Wallet unlocked.",
    update_wallet_settings: "Wallet settings updated.",
    send_transaction: "Signed transaction broadcast to RPC provider.",
    swap_tokens: "Swap completed in local simulator.",
    clear_wallet: "Local wallet cleared.",
    refresh_portfolio: "Portfolio refreshed.",
  };
  return messages[command];
}

function refreshWarningMessage({ kind, subject }: RefreshWarning) {
  return kind === "balance"
    ? `${subject} balance refresh failed. Try again for accurate balance.`
    : `${subject} refresh failed. Try again for accurate value.`;
}
