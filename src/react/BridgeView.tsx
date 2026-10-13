import { Combobox } from "@coinbase/cds-web/alpha/combobox";
import { Button } from "@coinbase/cds-web/buttons";
import { ContentCard } from "@coinbase/cds-web/cards/ContentCard";
import { TextInput } from "@coinbase/cds-web/controls";
import { Box, HStack, VStack } from "@coinbase/cds-web/layout";
import { DefaultStepperProgressHorizontal, Stepper } from "@coinbase/cds-web/stepper";
import type { StepperProgressProps } from "@coinbase/cds-web/stepper";
import { Text } from "@coinbase/cds-web/typography";
import { useCallback, useEffect, useRef, useState } from "react";

import { formatWei, toWei } from "../format";
import { networkById, networkDisplayName } from "../networks";
import type { CctpPreparedStep, CctpState, WalletSession } from "../types";
import { walletApi } from "../walletApi";

export type CctpRunner = <T>(action: () => Promise<T>) => Promise<T | null>;
const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));
// Finished steps read green; the in-progress step keeps the primary blue.
const CctpStepProgress = (props: StepperProgressProps) => (
  <DefaultStepperProgressHorizontal
    {...props}
    activeFill="bgPrimary"
    completeFill="bgPositive"
    visitedFill="bgPositive"
  />
);
const stepLabel = (step: CctpPreparedStep["step"]) =>
  ({
    approval: "USDC approval",
    burn: "USDC burn",
    mint: "USDC mint",
    destination_setup: "Solana account setup",
  })[step];
const STAGES = [
  { id: "details", label: "Details" },
  { id: "approve", label: "Approve" },
  { id: "burn", label: "Burn" },
  { id: "attestation", label: "Attestation" },
  { id: "mint", label: "Mint" },
  { id: "done", label: "Done" },
] as const;
type Stage = (typeof STAGES)[number]["id"];
type CctpOperation = CctpState["operations"][number];
const TERMINAL_STEPS = ["Completed", "Cancelled"];
const reviewStage = (step: CctpPreparedStep["step"]): Stage =>
  step === "approval" ? "approve" : step === "burn" ? "burn" : "mint";
const operationStage = (operation: CctpOperation): Stage => {
  if (operation.prepared) return reviewStage(operation.prepared.step);
  switch (operation.step) {
    case "Approval":
      return "approve";
    case "Burn":
      return "burn";
    case "AwaitingAttestation":
      return "attestation";
    case "Mint":
      return "mint";
    default:
      return "done";
  }
};
// const routeKey = (route: CctpState["routes"][number]) => `${route.source}:${route.destination}`;
const name = (network: string) => networkDisplayName(network).replaceAll("_", " ");
const isEvmNetwork = (network: string) => networkById(network)?.kind === "evm";
const attemptStatusText = (status: string | null) => {
  switch (status) {
    case "Signed":
      return "signed, not yet submitted";
    case "Submitted":
      return "submitted, awaiting confirmation";
    case "SubmissionUnknown":
      return "submission uncertain — reconciling";
    case "Confirmed":
      return "confirmed";
    case "Failed":
      return "failed";
    case "Superseded":
      return "superseded by verified execution";
    default:
      return status ?? "";
  }
};

export function BridgeView({
  busy,
  run,
  onSession,
}: {
  busy: boolean;
  run: CctpRunner;
  onSession: (session: WalletSession) => void;
}) {
  const [data, setData] = useState<CctpState>({ routes: [], operations: [] });
  const [selectedSource, setSelectedSource] = useState("ethereum_sepolia");
  const [selectedDestination, setSelectedDestination] = useState("arbitrum_sepolia");
  const [amount, setAmount] = useState("");
  const [recipient, setRecipient] = useState("");
  const [review, setReview] = useState<CctpPreparedStep | null>(null);
  const [autoConfirmed, setAutoConfirmed] = useState(false);
  const [autoRunning, setAutoRunning] = useState(false);
  const [showNewForm, setShowNewForm] = useState(false);
  const alive = useRef(true);
  const nextPoll = useRef(new Map<string, number>());
  const autoCancel = useRef(false);
  const lastFocusedId = useRef<string | null>(null);
  const load = useCallback(async () => {
    const state = await walletApi.getCctpState();
    if (alive.current) setData(state);
    return state;
  }, []);

  useEffect(() => {
    alive.current = true;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const tick = async () => {
      await run(async () => {
        const state = await walletApi.getCctpState();
        if (cancelled) return;
        setData(state);
        const active =
          state.operations.find(
            (operation) =>
              operation.needs_sync || !["Completed", "Cancelled"].includes(operation.step),
          ) ?? state.operations.find((operation) => operation.needs_sync);
        if (active && (nextPoll.current.get(active.id) ?? 0) <= Date.now()) {
          // Reads only: polling never prepares, signs, submits or authorizes.
          const session = await walletApi.syncCctpTransfer({ operationId: active.id });
          if (cancelled) return;
          onSession(session);
          const observation = session.activity.find((activity) => activity.id === active.id);
          if (
            observation &&
            (observation.title !== active.progress.title ||
              JSON.stringify(observation.tx_status) !== JSON.stringify(active.progress.tx_status))
          ) {
            const refreshed = await walletApi.refreshPortfolio();
            if (cancelled) return;
            onSession(refreshed.session);
          }
          if (active.step === "AwaitingAttestation" && !active.can_mint) {
            const progress = await walletApi.pollCctpAttestation({ operationId: active.id });
            if (cancelled) return;
            onSession(progress.session);
            nextPoll.current.set(
              active.id,
              Date.now() + Math.max(15, progress.retry_after_secs) * 1000,
            );
          }
        }
        if (!cancelled) await load();
      });
      if (!cancelled) timer = setTimeout(() => void tick(), 15000);
    };
    void tick();
    return () => {
      cancelled = true;
      alive.current = false;
      clearTimeout(timer);
    };
  }, [load, onSession, run]);

  const busyRef = useRef(busy);
  useEffect(() => {
    busyRef.current = busy;
  }, [busy]);
  const route = data.routes.find(
    (route) => route.source === selectedSource && route.destination === selectedDestination,
  );
  const sourceNetworks = [
    ...new Set(
      data.routes.filter((route) => isEvmNetwork(route.source)).map((route) => route.source),
    ),
  ].sort((a, b) => name(a).localeCompare(name(b)));
  const destinationNetworks = [
    ...new Set(
      data.routes
        .filter((route) => route.source === selectedSource && isEvmNetwork(route.destination))
        .map((route) => route.destination),
    ),
  ].sort((a, b) => name(a).localeCompare(name(b)));
  const sourceOptions = sourceNetworks.map((network) => ({ label: name(network), value: network }));
  const destinationOptions = destinationNetworks.map((network) => ({
    label: name(network),
    value: network,
  }));
  const active = data.operations.find((operation) => !TERMINAL_STEPS.includes(operation.step));
  const focused = active ?? data.operations[data.operations.length - 1] ?? null;
  const focusedId = focused?.id ?? null;
  useEffect(() => {
    if (lastFocusedId.current !== focusedId) {
      lastFocusedId.current = focusedId;
      autoCancel.current = true;
      setAutoRunning(false);
      setAutoConfirmed(false);
      setShowNewForm(false);
    }
  }, [focusedId]);
  const estimate =
    review && focused && review.operation_id === focused.id ? review : (focused?.prepared ?? null);
  const showReview = Boolean(
    estimate && focused && estimate.operation_id === focused.id && autoConfirmed && !autoRunning,
  );
  const manualReview = showReview ? estimate : null;
  const stage: Stage = !focused
    ? "details"
    : showReview && estimate
      ? reviewStage(estimate.step)
      : operationStage(focused);
  const done = focused?.step === "Completed";
  const terminal = focused ? TERMINAL_STEPS.includes(focused.step) : false;
  const create = async () => {
    if (!route || !route.executable) return;
    autoCancel.current = true;
    setAutoRunning(false);
    setAutoConfirmed(false);
    setShowNewForm(false);
    setReview(null);
    await run(async () => {
      const result = await walletApi.createCctpTransfer({
        sourceNetwork: route.source,
        destinationNetwork: route.destination,
        recipient: recipient || route.destination_address,
        amountBaseUnits: toWei(amount, 6),
      });
      if (!alive.current) return;
      onSession(result.session);
      setAmount("");
      await load();
      try {
        const prepared = await walletApi.prepareCctpStep({ operationId: result.operation_id });
        if (alive.current) setReview(prepared);
      } catch {
        // Estimate stays empty; Refresh estimate retries.
      }
      await load();
    });
  };
  const prepare = async (id: string) => {
    setReview(null);
    await run(async () => {
      const result = await walletApi.prepareCctpStep({ operationId: id });
      if (!alive.current) return;
      setReview(result);
      await load();
    });
  };
  const submit = async () => {
    if (!manualReview) return;
    const reviewed = manualReview;
    setReview(null);
    await run(async () => {
      try {
        const session = await walletApi.submitCctpStep({
          operationId: reviewed.operation_id,
          expectedHash: reviewed.tx_hash,
        });
        if (alive.current) onSession(session);
      } finally {
        if (alive.current) {
          await load();
          const refreshed = await walletApi.refreshPortfolio().catch(() => null);
          if (alive.current && refreshed) onSession(refreshed.session);
        }
      }
    });
  };
  const stopAuto = useCallback(() => {
    autoCancel.current = true;
  }, []);
  const cancel = async () => {
    const id = focused?.id;
    if (!id || focused?.step !== "Approval") return;
    autoCancel.current = true;
    setAutoRunning(false);
    await run(async () => {
      const session = await walletApi.cancelCctpTransfer({ operationId: id });
      if (!alive.current) return;
      onSession(session);
      setReview(null);
      await load();
    });
  };
  const startAuto = () => {
    const operationId = focused?.id;
    if (!operationId || autoRunning || !focused || TERMINAL_STEPS.includes(focused.step)) return;
    if (!estimate || estimate.operation_id !== operationId) return;
    autoCancel.current = false;
    setAutoConfirmed(true);
    setAutoRunning(true);
    void (async () => {
      const waitForIdle = async () => {
        for (let i = 0; i < 30 && !autoCancel.current && alive.current; i++) {
          if (!busyRef.current) return true;
          await sleep(1000);
        }
        return !busyRef.current;
      };
      try {
        for (let round = 0; round < 6 && !autoCancel.current && alive.current; round++) {
          const state = await load().catch(() => null);
          if (!state || !alive.current || autoCancel.current) break;
          const op = state.operations.find((operation) => operation.id === operationId);
          if (!op || TERMINAL_STEPS.includes(op.step)) break;
          if (op.step === "AwaitingAttestation" && !op.can_mint && !op.prepared) {
            let ready = false;
            for (let w = 0; w < 40 && !autoCancel.current && alive.current; w++) {
              await sleep(15000);
              if (autoCancel.current || !alive.current) break;
              if (!(await waitForIdle())) break;
              await run(async () => {
                const progress = await walletApi.pollCctpAttestation({ operationId });
                if (alive.current) onSession(progress.session);
              });
              if (!alive.current || autoCancel.current) break;
              await run(async () => {
                const session = await walletApi.syncCctpTransfer({ operationId });
                if (alive.current) onSession(session);
              });
              const fresh = await load().catch(() => null);
              const current = fresh?.operations.find((operation) => operation.id === operationId);
              if (!current || TERMINAL_STEPS.includes(current.step)) {
                ready = true;
                break;
              }
              if (current.can_mint || current.step === "Mint" || current.prepared) {
                ready = true;
                break;
              }
            }
            if (!ready && !autoCancel.current) break;
            continue;
          }
          let prepared = op.prepared;
          if (!prepared) {
            if (!(await waitForIdle())) break;
            const next = await run(async () => walletApi.prepareCctpStep({ operationId }));
            if (!alive.current || autoCancel.current || !next) break;
            setReview(next);
            await load();
            prepared = next;
          } else if (prepared.operation_id !== operationId) {
            break;
          }
          const hash = prepared.tx_hash;
          if (!(await waitForIdle())) break;
          const submitted = await run(async () => {
            const session = await walletApi.submitCctpStep({ operationId, expectedHash: hash });
            if (alive.current) {
              onSession(session);
              setReview(null);
            }
            return true;
          });
          if (!submitted || !alive.current || autoCancel.current) break;
          await load();
          const refreshed = await walletApi.refreshPortfolio().catch(() => null);
          if (alive.current && refreshed) onSession(refreshed.session);
          const before = op.step;
          for (let w = 0; w < 12 && !autoCancel.current && alive.current; w++) {
            await sleep(5000);
            if (autoCancel.current || !alive.current) break;
            if (!(await waitForIdle())) break;
            await run(async () => {
              const session = await walletApi.syncCctpTransfer({ operationId });
              if (alive.current) onSession(session);
            });
            const fresh = await load().catch(() => null);
            const current = fresh?.operations.find((operation) => operation.id === operationId);
            if (!current) break;
            if (TERMINAL_STEPS.includes(current.step) || current.step !== before) break;
            if (current.attempt_hash === hash && current.attempt_status === "Confirmed") break;
            if (current.step === "AwaitingAttestation" && (current.can_mint || current.prepared))
              break;
          }
        }
      } finally {
        if (alive.current) {
          setAutoRunning(false);
          setShowNewForm(false);
          await load().catch(() => null);
        }
      }
    })();
  };
  const fee = estimate?.max_native_fee ?? "0";
  const gasDecimals = estimate?.solana_context ? 9 : 18;
  const destination =
    data.operations.find((operation) => operation.id === estimate?.operation_id)?.destination ?? "";
  const destinationDecimals = networkById(destination)?.nativeAsset.decimals ?? 18;
  const destinationSymbol = networkById(destination)?.nativeAsset.symbol ?? "ETH";
  const destinationBudget =
    estimate?.context?.provisional_destination_fee ??
    estimate?.solana_context?.provisional_destination_fee;
  const gasSymbol = estimate ? (networkById(estimate.network)?.nativeAsset.symbol ?? "ETH") : "ETH";
  return (
    <VStack gap={3}>
      <ContentCard background="bgElevation1" borderRadius={400} gap={3} padding={4}>
        <Text as="h2" font="title1">
          Transfer USDC between chains
        </Text>
        <Text color="fgMuted" font="body">
          Circle CCTP burns native USDC on the source chain and mints it on the destination. One
          confirmation runs every step automatically.
        </Text>
        <Stepper
          direction="horizontal"
          steps={STAGES.map((stage) => ({ ...stage }))}
          activeStepId={stage}
          complete={done}
          StepperProgressComponent={CctpStepProgress}
        />
        {!active && (!focused || showNewForm) && (
          <VStack gap={2}>
            <Combobox
              accessibilityLabel="Source chain"
              label="From"
              value={
                sourceNetworks.includes(selectedSource)
                  ? selectedSource
                  : (sourceOptions[0]?.value ?? "")
              }
              options={sourceOptions}
              disabled={busy || sourceOptions.length === 0}
              onChange={(value) => {
                if (value === null) return;
                setSelectedSource(value);
                const nextDestination = data.routes.find(
                  (candidate) => candidate.source === value && isEvmNetwork(candidate.destination),
                )?.destination;
                if (nextDestination) setSelectedDestination(nextDestination);
                setRecipient("");
              }}
            />
            <Combobox
              accessibilityLabel="Destination chain"
              label="To"
              value={
                destinationNetworks.includes(selectedDestination)
                  ? selectedDestination
                  : (destinationOptions[0]?.value ?? "")
              }
              options={destinationOptions}
              disabled={busy || destinationOptions.length === 0}
              onChange={(value) => {
                if (value === null) return;
                setSelectedDestination(value);
                setRecipient("");
              }}
            />
            <TextInput
              label="USDC amount"
              inputMode="decimal"
              value={amount}
              onChange={(event) => setAmount(event.target.value)}
              disabled={busy}
            />
            <TextInput
              label="Destination wallet address"
              value={recipient || route?.destination_address || ""}
              onChange={(event) => setRecipient(event.target.value)}
              disabled={busy}
            />
            {route && (
              <Text font="caption" color="fgMuted">
                Source account: {route.sender}. Standard protocol fee: 0 USDC. Keep native gas funds
                on both chains.
              </Text>
            )}
            <Button disabled={busy || !route?.executable || !amount} onClick={() => void create()}>
              Create transfer
            </Button>
          </VStack>
        )}
        {focused && terminal && (
          <VStack gap={2}>
            <Text font="headline">
              {formatWei(focused.amount_base_units, 6)} USDC · {name(focused.source)} →{" "}
              {name(focused.destination)}
            </Text>
            <Text font="body">{focused.progress.title}</Text>
            <Text font="body">
              {done
                ? "Transfer complete. The USDC is on the destination chain."
                : "This transfer was cancelled. Funds stay on the source chain."}{" "}
              Recipient: {focused.recipient}
            </Text>
            <Button
              disabled={busy}
              onClick={() => {
                setReview(null);
                setShowNewForm(true);
              }}
            >
              New transfer
            </Button>
          </VStack>
        )}
        {focused && !terminal && (
          <VStack gap={2}>
            <Text font="headline">
              {formatWei(focused.amount_base_units, 6)} USDC · {name(focused.source)} →{" "}
              {name(focused.destination)}
            </Text>
            <Text font="body">{focused.progress.title}</Text>
            {!autoConfirmed && (
              <Box background="bgSecondary" borderRadius={300} padding={3}>
                <VStack gap={2}>
                  <Text as="h3" font="title3">
                    Estimated fees
                  </Text>
                  {estimate ? (
                    <VStack gap={2}>
                      <Text font="body">
                        You send {formatWei(focused.amount_base_units, 6)} USDC to{" "}
                        {focused.recipient}. Protocol fee: 0 USDC. Expected receipt:{" "}
                        {formatWei(focused.amount_base_units, 6)} USDC.
                      </Text>
                      <Text font="body">
                        Maximum {stepLabel(estimate.step)} gas:{" "}
                        {formatWei(fee, gasDecimals, gasDecimals)} {gasSymbol} on{" "}
                        {name(estimate.network)}.
                      </Text>
                      {estimate.context?.provisional_burn_fee && (
                        <Text font="body">
                          Provisional later burn budget:{" "}
                          {formatWei(estimate.context.provisional_burn_fee, 18)} {gasSymbol}.
                        </Text>
                      )}
                      {estimate.solana_context && (
                        <Text font="body">
                          Account rent: {formatWei(estimate.solana_context.rent_lamports, 9, 9)}{" "}
                          SOL.{" "}
                          {estimate.step === "destination_setup"
                            ? "Creates a permanent lookup table and the recipient token account when needed. Lookup-table rent remains deposited."
                            : "Pays for the Circle message or nonce account."}
                        </Text>
                      )}
                      {estimate.step !== "mint" && destinationBudget && (
                        <Text font="body">
                          Provisional destination gas budget:{" "}
                          {formatWei(destinationBudget, destinationDecimals, destinationDecimals)}{" "}
                          {destinationSymbol}. The final mint fee is covered by this budget.
                        </Text>
                      )}
                      <Text font="body">
                        One confirmation runs every step automatically: approval when needed, burn,
                        attestation wait, then mint.
                      </Text>
                    </VStack>
                  ) : (
                    <Text font="body">Preparing estimated fees…</Text>
                  )}
                  {focused.last_error && (
                    <Text color="fgWarning" font="body">
                      {focused.last_error}
                    </Text>
                  )}
                  {focused.step !== "Approval" && (
                    <Text font="body">
                      The burn step is already prepared. This transfer can no longer be cancelled.
                    </Text>
                  )}
                  <HStack gap={2}>
                    <Button disabled={busy || autoRunning || !estimate} onClick={() => startAuto()}>
                      Yes, start automatically
                    </Button>
                    <Button
                      variant="secondary"
                      disabled={busy || autoRunning}
                      onClick={() => void prepare(focused.id)}
                    >
                      Refresh estimate
                    </Button>
                    {focused.step === "Approval" && (
                      <Button
                        variant="secondary"
                        disabled={busy || autoRunning}
                        onClick={() => void cancel()}
                      >
                        Cancel transfer
                      </Button>
                    )}
                  </HStack>
                </VStack>
              </Box>
            )}
            {autoConfirmed && (
              <Box background="bgSecondary" borderRadius={300} padding={3}>
                <VStack gap={2}>
                  <Text as="h3" font="title3">
                    {autoRunning ? "Running automatically" : "Automatic run paused"}
                  </Text>
                  <Text font="body">{focused.progress.title}</Text>
                  {focused.attempt_status && (
                    <Text font="body">
                      Latest {(focused.attempt_step ?? "transaction").toLowerCase()} transaction
                      {focused.attempt_hash ? ` ${focused.attempt_hash}` : ""}:{" "}
                      {attemptStatusText(focused.attempt_status)}
                    </Text>
                  )}
                  {["Burn", "AwaitingAttestation", "Mint"].includes(focused.step) && (
                    <Text font="body">
                      The source burn cannot be refunded by closing or cancelling this screen. Keep
                      this saved transfer until the destination mint completes.
                    </Text>
                  )}
                  {focused.attempt_error && focused.attempt_error !== focused.last_error && (
                    <Text color="fgWarning" font="body">
                      {focused.attempt_error}
                    </Text>
                  )}
                  {focused.last_error && (
                    <Text color="fgWarning" font="body">
                      {focused.last_error}
                    </Text>
                  )}
                  <HStack gap={2}>
                    {autoRunning ? (
                      <Button variant="secondary" onClick={() => stopAuto()}>
                        Stop
                      </Button>
                    ) : (
                      <Button disabled={busy || !estimate} onClick={() => startAuto()}>
                        Resume automatically
                      </Button>
                    )}
                    {!autoRunning && focused.step === "Approval" && (
                      <Button variant="secondary" disabled={busy} onClick={() => void cancel()}>
                        Cancel transfer
                      </Button>
                    )}
                    {!autoRunning &&
                      (focused.needs_sync || !TERMINAL_STEPS.includes(focused.step)) && (
                        <Button
                          variant="secondary"
                          disabled={busy}
                          onClick={() =>
                            void run(async () => {
                              const session = await walletApi.syncCctpTransfer({
                                operationId: focused.id,
                              });
                              if (!alive.current) return;
                              onSession(session);
                              await load();
                            })
                          }
                        >
                          Check progress
                        </Button>
                      )}
                  </HStack>
                  {!autoRunning &&
                    !TERMINAL_STEPS.includes(focused.step) &&
                    (focused.step !== "AwaitingAttestation" ||
                      focused.can_mint ||
                      focused.prepared) && (
                      <HStack gap={2}>
                        <Button
                          variant="secondary"
                          disabled={busy}
                          onClick={() => void prepare(focused.id)}
                        >
                          Review{" "}
                          {focused.can_mint || focused.step === "Mint"
                            ? "mint"
                            : "next transaction"}{" "}
                          manually
                        </Button>
                        {focused.step === "Mint" && (
                          <Button
                            variant="secondary"
                            disabled={busy}
                            onClick={() => {
                              setReview(null);
                              void run(async () => {
                                const progress = await walletApi.refreshCctpAttestation({
                                  operationId: focused.id,
                                });
                                if (!alive.current) return;
                                onSession(progress.session);
                                await load();
                              });
                            }}
                          >
                            Refresh attestation
                          </Button>
                        )}
                      </HStack>
                    )}
                </VStack>
              </Box>
            )}
            {manualReview && (
              <VStack gap={2}>
                <Text as="h3" font="title3">
                  Authorize {stepLabel(manualReview.step)}
                </Text>
                <Text font="body">
                  {formatWei(manualReview.amount_base_units, 6)} USDC to {manualReview.recipient}.
                  Protocol fee: 0 USDC. Expected receipt:{" "}
                  {formatWei(manualReview.amount_base_units, 6)} USDC.
                </Text>
                <Text font="body">
                  Maximum {stepLabel(manualReview.step)} gas:{" "}
                  {formatWei(fee, gasDecimals, gasDecimals)} {gasSymbol} on{" "}
                  {name(manualReview.network)}.
                </Text>
                <HStack gap={2}>
                  <Button variant="secondary" disabled={busy} onClick={() => setReview(null)}>
                    Close review
                  </Button>
                  <Button disabled={busy} onClick={() => void submit()}>
                    Submit {stepLabel(manualReview.step)}
                  </Button>
                </HStack>
              </VStack>
            )}
          </VStack>
        )}
      </ContentCard>
    </VStack>
  );
}
