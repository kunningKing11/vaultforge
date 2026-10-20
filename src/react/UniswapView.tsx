import { Button } from "@coinbase/cds-web/buttons";
import { ContentCard } from "@coinbase/cds-web/cards/ContentCard";
import { TextInput } from "@coinbase/cds-web/controls/TextInput";
import { HStack, VStack } from "@coinbase/cds-web/layout";
import { Text } from "@coinbase/cds-web/typography";
import { useEffect, useRef, useState } from "react";

import { assetIconPath, chainIconPath } from "../assetIcons";
import { cryptoDisplaySymbol, formatError, formatWei, toWei } from "../format";
import { networkById } from "../networks";
import type { Asset, FiatCurrency, NetworkId } from "../types";
import { CdsSelect } from "./CdsSelect";
import { ExplorerLink } from "./ExplorerLink";
import { assetKey, sortAssetsForSelection, sortNetworksForSelection } from "./model";
import { slippageBasisPoints } from "./slippage";
import { SlippagePicker } from "./SlippagePicker";
import { SwapFiatValue } from "./SwapFiatValue";
import { SwapRoutePreview } from "./SwapRoutePreview";
import { SwapStepper } from "./SwapStepper";
import { quoteProvider } from "./uniswapFlow";
import type { UniswapController } from "./useUniswap";

export function UniswapView({
  assets,
  busy,
  controller,
  enabledNetworks,
  useCryptoSymbols = false,
  fiatCurrency = "USD",
  usdExchangeRate = 1,
}: {
  assets: Asset[];
  fiatCurrency?: FiatCurrency;
  usdExchangeRate?: number;
  enabledNetworks?: NetworkId[];
  useCryptoSymbols?: boolean;
  busy: boolean;
  controller: UniswapController;
}) {
  const [selectedNetwork, setSelectedNetwork] = useState("");
  const [destinationChoice, setDestinationChoice] = useState("");
  const [input, setInput] = useState("");
  const [output, setOutput] = useState("");
  const [amount, setAmount] = useState("");
  const [slippage, setSlippage] = useState("0.5");
  const [customSlippage, setCustomSlippage] = useState("");
  const [formError, setFormError] = useState<string | null>(null);
  const networks = sortNetworksForSelection(
    (enabledNetworks ?? controller.data.networks).filter(
      (id) => !enabledNetworks || enabledNetworks.includes(id),
    ),
    assets,
  );
  const network =
    networks.find((candidate) => candidate.id === selectedNetwork)?.id ?? networks[0]?.id;
  const tokens = sortAssetsForSelection(assets.filter((asset) => asset.network === network));
  const from = tokens.find((asset) => assetKey(asset) === input) ?? tokens[0];
  const destination =
    networks.find((candidate) => candidate.id === destinationChoice)?.id ?? network;
  const outputs = sortAssetsForSelection(
    assets.filter(
      (asset) => asset.network === destination && assetKey(asset) !== (from ? assetKey(from) : ""),
    ),
  );
  const to = outputs.find((asset) => assetKey(asset) === output) ?? outputs[0];
  const options = (choices: Asset[]) =>
    choices.map((asset) => ({
      value: assetKey(asset),
      icon: assetIconPath(asset),
      label: `${cryptoDisplaySymbol(asset.symbol, asset.unicode_symbol, useCryptoSymbols)} - ${asset.name}`,
    }));
  const fromKey = from ? assetKey(from) : "";
  const toKey = to ? assetKey(to) : "";
  useEffect(() => {
    if (destination && destinationChoice !== destination) setDestinationChoice(destination);
    if (network && selectedNetwork !== network) setSelectedNetwork(network);
    if (fromKey && input !== fromKey) setInput(fromKey);
    if (toKey && output !== toKey) setOutput(toKey);
  }, [destination, destinationChoice, fromKey, input, network, output, selectedNetwork, toKey]);
  const activeOperation = [...controller.data.operations]
    .reverse()
    .find((operation) => !operation.terminal);
  const active = Boolean(activeOperation);
  const [currentOperationId, setCurrentOperationId] = useState<string | null>(null);
  useEffect(() => {
    if (activeOperation) setCurrentOperationId(activeOperation.id);
    else if (!controller.data.operations.some((operation) => operation.id === currentOperationId))
      setCurrentOperationId(null);
  }, [activeOperation, controller.data.operations, currentOperationId]);
  const justCompleted = controller.data.operations.find(
    (operation) => operation.id === currentOperationId && operation.stage === "done",
  );
  const quote = controller.quote;
  const clearQuote = controller.clearQuote;
  useEffect(() => {
    clearQuote();
  }, [amount, clearQuote, customSlippage, destination, fromKey, network, slippage, toKey]);
  const change = (action: () => void) => {
    setCurrentOperationId(null);
    action();
    setFormError(null);
    controller.clearQuote();
  };
  const maximum = async () => {
    if (!from || !network) return;
    setFormError(null);
    try {
      if (from.token_address) {
        if (!/^\d+$/.test(from.balance) || BigInt(from.balance) <= 0n)
          throw new Error("No balance is available to swap.");
        change(() => setAmount(formatWei(from.balance, from.decimals, from.decimals)));
      } else {
        if (!to || !destination)
          throw new Error("Select a destination asset to calculate the network fee.");
        const next = await controller.requestQuote({
          network,
          destinationNetwork: destination,
          tokenIn: "0x0000000000000000000000000000000000000000",
          tokenOut: to.token_address ?? "0x0000000000000000000000000000000000000000",
          amountBaseUnits: "0",
          slippageBps: slippageBasisPoints(slippage === "Custom" ? customSlippage : slippage),
          maxAmount: true,
        });
        if (next)
          change(() => setAmount(formatWei(next.amount_base_units, from.decimals, from.decimals)));
      }
    } catch (failure) {
      setFormError(formatError(failure));
    }
  };
  let enteredBaseUnits = "0";
  try {
    if (from && amount) enteredBaseUnits = toWei(amount, from.decimals);
  } catch {
    enteredBaseUnits = "";
  }
  const nativeAsset = assets.find(
    (asset) => asset.network === network && asset.token_address == null,
  );
  const quoteOutput = quote
    ? assets.find(
        (asset) =>
          assetKey(asset) ===
          assetKey({ network: quote.destination_network, token_address: quote.token_out }),
      )
    : undefined;
  const quoteNative = quote
    ? assets.find((asset) => asset.network === quote.network && asset.token_address == null)
    : nativeAsset;
  const getQuote = () => {
    if (!network || !destination || !from || !to) return;
    try {
      const amountBaseUnits = toWei(amount, from.decimals);
      if (BigInt(amountBaseUnits) === 0n) throw new Error("Enter an amount greater than zero.");
      setFormError(null);
      void controller.requestQuote({
        network,
        destinationNetwork: destination,
        tokenIn: from.token_address ?? "0x0000000000000000000000000000000000000000",
        tokenOut: to.token_address ?? "0x0000000000000000000000000000000000000000",
        amountBaseUnits,
        slippageBps: slippageBasisPoints(slippage === "Custom" ? customSlippage : slippage),
      });
    } catch (failure) {
      setFormError(formatError(failure));
    }
  };
  const refresh = useRef(() => {});
  refresh.current = () => {
    if (busy || active || !amount || !from || !to) return;
    try {
      if (BigInt(toWei(amount, from.decimals)) === 0n) return;
      slippageBasisPoints(slippage === "Custom" ? customSlippage : slippage);
    } catch {
      return;
    }
    getQuote();
  };
  useEffect(() => {
    if (active) return;
    const initial = setTimeout(() => refresh.current(), 500);
    const timer = setInterval(() => refresh.current(), 15000);
    return () => {
      clearTimeout(initial);
      clearInterval(timer);
    };
  }, [active, amount, customSlippage, destination, fromKey, network, slippage, toKey]);
  return (
    <VStack gap={3}>
      <ContentCard background="bgElevation1" borderRadius={400} gap={3} padding={4}>
        <Text as="h2" font="title1">
          Swap tokens
        </Text>
        <Text color="fgMuted" font="body">
          Choose the assets and networks to swap between. Available routes are checked live.
        </Text>
        {controller.data.networks.length === 0 && (
          <Button variant="secondary" disabled={busy} onClick={() => void controller.load()}>
            Load swap networks
          </Button>
        )}
        {controller.data.networks.length > 0 && networks.length === 0 && (
          <Text font="body">Enable networks in Settings to select swap assets.</Text>
        )}
        {!active && (
          <SwapStepper
            stage={justCompleted ? "done" : "details"}
            routed={
              justCompleted
                ? Boolean(justCompleted.terms.relay)
                : Boolean(quote?.relay) || network !== destination
            }
          />
        )}
        {!active && justCompleted && (
          <Text font="headline" color="fgPositive">
            Swap complete
          </Text>
        )}
        {!active && (
          <VStack gap={2}>
            <CdsSelect
              label="From network"
              searchable
              disabled={busy || networks.length === 0}
              value={network ?? ""}
              options={networks.map((candidate) => ({
                value: candidate.id,
                label: candidate.name,
                icon: chainIconPath(candidate.id),
              }))}
              onChange={(value) =>
                change(() => {
                  setSelectedNetwork(value);
                  setInput("");
                  setOutput("");
                  setAmount("");
                })
              }
            />
            <CdsSelect
              label="From asset"
              searchable
              disabled={busy || tokens.length === 0}
              value={from ? assetKey(from) : ""}
              options={options(tokens)}
              onChange={(value) =>
                change(() => {
                  setInput(value);
                  setAmount("");
                })
              }
            />
            {from && (
              <Text color="fgMuted" font="caption">
                Cached balance: {formatWei(from.balance, from.decimals)} {from.symbol}. Live funds
                are checked before signing.
              </Text>
            )}
            <CdsSelect
              label="To network"
              searchable
              disabled={busy || networks.length === 0}
              value={destination ?? ""}
              options={networks.map((candidate) => ({
                value: candidate.id,
                label: candidate.name,
                icon: chainIconPath(candidate.id),
              }))}
              onChange={(value) =>
                change(() => {
                  setDestinationChoice(value);
                  setOutput("");
                })
              }
            />
            <CdsSelect
              label="To asset"
              searchable
              disabled={busy || outputs.length === 0}
              value={to ? assetKey(to) : ""}
              options={options(outputs)}
              onChange={(value) => change(() => setOutput(value))}
            />
            <HStack gap={2} alignItems="flex-end">
              <VStack flexGrow={1}>
                <TextInput
                  label="Amount to spend"
                  inputMode="decimal"
                  value={amount}
                  disabled={busy}
                  onChange={(event) => change(() => setAmount(event.target.value))}
                />
              </VStack>
              <Button
                variant="secondary"
                size="s"
                disabled={busy || !from}
                onClick={() => void maximum()}
              >
                Max
              </Button>
            </HStack>
            <SwapFiatValue
              baseUnits={enteredBaseUnits}
              asset={from}
              currency={fiatCurrency}
              rate={usdExchangeRate}
            />
            {from && !from.token_address && (
              <Text font="caption" color="fgMuted">
                Max reserves the estimated network fee.
              </Text>
            )}

            <SlippagePicker
              value={slippage}
              custom={customSlippage}
              disabled={busy}
              onChange={(value) => change(() => setSlippage(value))}
              onCustomChange={(value) => change(() => setCustomSlippage(value))}
            />
            {network && (!from || !to) && (
              <Text font="body">Load assets on the selected networks to request a swap route.</Text>
            )}
            <Button
              variant="secondary"
              disabled={busy || !from || !to || !amount}
              onClick={getQuote}
            >
              Refresh quote
            </Button>
            {controller.quotes.length > 1 && (
              <VStack gap={2} role="group" aria-label="Swap routes">
                <Text font="headline">Available routes</Text>
                {controller.quotes.map((option, index) => (
                  <VStack key={option.id} gap={1}>
                    <Button
                      variant={quote?.id === option.id ? "primary" : "secondary"}
                      aria-pressed={quote?.id === option.id}
                      disabled={busy}
                      onClick={() => controller.selectQuote(option.id)}
                    >
                      {quoteProvider(option)}:{" "}
                      {formatWei(
                        option.quoted_output,
                        option.output_decimals,
                        option.output_decimals,
                      )}{" "}
                      {option.output_symbol}
                      {index === 0 && option.quoted_output !== controller.quotes[1].quoted_output
                        ? " · Highest token output"
                        : ""}
                    </Button>
                    <Text font="caption" color="fgMuted">
                      Maximum {option.approval_required ? "approval" : "swap"} network fee:{" "}
                      {formatWei(option.approval_max_fee ?? option.swap_max_fee ?? "0", 18, 18)}{" "}
                      {networkById(option.network)?.nativeAsset.symbol}
                      {option.approval_required ? "; swap fee reviewed after approval" : ""}
                    </Text>
                  </VStack>
                ))}
                <Text font="caption" color="fgMuted">
                  Ranked by output tokens. Source network fees are separate.
                </Text>
              </VStack>
            )}
            {controller.unavailable.map((failure) => (
              <Text key={failure.provider} role="status" font="caption" color="fgWarning">
                {failure.provider} unavailable: {failure.error}
              </Text>
            ))}
            {quote && (
              <VStack background="bgSecondary" borderRadius={300} padding={3} gap={2}>
                <Text font="headline">{quoteProvider(quote)}</Text>
                <SwapRoutePreview route={quote.route} />
                <Text font="headline">
                  Expected:{" "}
                  {formatWei(quote.quoted_output, quote.output_decimals, quote.output_decimals)}{" "}
                  {quote.output_symbol}
                </Text>
                <SwapFiatValue
                  baseUnits={quote.quoted_output}
                  asset={quoteOutput}
                  currency={fiatCurrency}
                  rate={usdExchangeRate}
                />
                <Text font="body">
                  Minimum received:{" "}
                  {formatWei(quote.minimum_received, quote.output_decimals, quote.output_decimals)}{" "}
                  {quote.output_symbol}
                </Text>
                <SwapFiatValue
                  baseUnits={quote.minimum_received}
                  asset={quoteOutput}
                  currency={fiatCurrency}
                  rate={usdExchangeRate}
                />
                <Text font="body">
                  Maximum {quote.approval_required ? "approval" : "swap"} network fee:{" "}
                  {formatWei(quote.approval_max_fee ?? quote.swap_max_fee ?? "0", 18, 18)}{" "}
                  {networkById(quote.network)?.nativeAsset.symbol ?? "ETH"}
                </Text>
                <SwapFiatValue
                  baseUnits={quote.approval_max_fee ?? quote.swap_max_fee ?? ""}
                  asset={quoteNative}
                  currency={fiatCurrency}
                  rate={usdExchangeRate}
                />
                {quote.approval_required && (
                  <Text font="body">
                    Approval is required first. After it finalizes, review the swap's actual network
                    fee before confirming the swap. Cancelling later does not revoke an approval
                    already sent.
                  </Text>
                )}
                <Text color="fgMuted" font="caption">
                  Updates every 15 seconds. Quote expires at{" "}
                  {new Date(quote.deadline * 1000).toLocaleTimeString()}. Routing and pool fees are
                  included in the expected output.
                </Text>
                <Button
                  disabled={busy || quote.deadline * 1000 <= Date.now()}
                  onClick={() => void controller.start()}
                >
                  {quote.approval_required ? "Start approval" : "Start swap"}
                </Button>
              </VStack>
            )}
          </VStack>
        )}
        {(formError || controller.error) && (
          <Text role="alert" color="fgNegative" font="body">
            {formError ?? controller.error}
          </Text>
        )}
      </ContentCard>
      {controller.data.operations
        .filter((operation) => !operation.terminal)
        .reverse()
        .map((operation) => {
          const prepared =
            controller.review?.operation_id === operation.id
              ? controller.review
              : operation.prepared;
          return (
            <ContentCard
              key={operation.id}
              background="bgElevation1"
              borderRadius={400}
              gap={2}
              padding={4}
            >
              <SwapStepper stage={operation.stage} routed={Boolean(operation.terms.relay)} />
              <Text as="h3" font="title3">
                {operation.progress.title}
              </Text>
              <Text font="body" color="fgMuted">
                {operation.progress.subtitle}
              </Text>
              <Text font="body">
                {formatWei(
                  operation.amount_base_units,
                  operation.terms.input_decimals,
                  operation.terms.input_decimals,
                )}{" "}
                {operation.terms.input_symbol} → {operation.terms.output_symbol}
              </Text>
              <Text font="body">
                Minimum received:{" "}
                {formatWei(
                  operation.terms.minimum_received,
                  operation.terms.output_decimals,
                  operation.terms.output_decimals,
                )}{" "}
                {operation.terms.output_symbol}
              </Text>
              <SwapFiatValue
                baseUnits={operation.terms.minimum_received}
                asset={assets.find(
                  (asset) =>
                    assetKey(asset) ===
                    assetKey({
                      network: operation.terms.relay?.destination_network ?? operation.network,
                      token_address: operation.token_out,
                    }),
                )}
                currency={fiatCurrency}
                rate={usdExchangeRate}
              />
              {operation.terms.relay && <SwapRoutePreview route={operation.terms.relay.route} />}
              {operation.terms.relay?.destination_hash && (
                <Text font="body">
                  Destination:{" "}
                  <ExplorerLink
                    network={operation.terms.relay.destination_network}
                    value={operation.terms.relay.destination_hash}
                    type="tx"
                  />
                </Text>
              )}
              {operation.progress.hash && (
                <Text font="body">
                  Transaction:{" "}
                  <ExplorerLink
                    network={operation.network}
                    value={operation.progress.hash}
                    type="tx"
                  />
                </Text>
              )}
              {operation.last_error && (
                <Text color="fgWarning" font="body">
                  {operation.last_error}
                </Text>
              )}
              {!operation.terminal && prepared && (
                <VStack gap={2}>
                  <Text font="body">
                    Maximum {prepared.step} network fee:{" "}
                    {formatWei(
                      prepared.max_native_fee,
                      networkById(prepared.network)?.nativeAsset.decimals ?? 18,
                      18,
                    )}{" "}
                    {networkById(prepared.network)?.nativeAsset.symbol ?? "ETH"}
                  </Text>
                  <SwapFiatValue
                    baseUnits={prepared.max_native_fee}
                    asset={assets.find(
                      (asset) => asset.network === prepared.network && asset.token_address == null,
                    )}
                    currency={fiatCurrency}
                    rate={usdExchangeRate}
                  />
                  <Button
                    disabled={busy}
                    onClick={() => void controller.submit(operation.id, prepared.tx_hash)}
                  >
                    Confirm {prepared.step}
                  </Button>
                </VStack>
              )}
              {!operation.terminal && !operation.needs_sync && !prepared && (
                <Button disabled={busy} onClick={() => void controller.prepare(operation.id)}>
                  Prepare next transaction
                </Button>
              )}
              <HStack gap={2} flexWrap="wrap">
                {(operation.needs_sync ||
                  operation.terms.relay?.status === "failure" ||
                  operation.terms.relay?.status === "refund") && (
                  <Button
                    variant="secondary"
                    disabled={busy}
                    onClick={() => void controller.sync(operation.id)}
                  >
                    Check progress
                  </Button>
                )}
                {operation.attempt_status === "SubmissionUnknown" && (
                  <Button
                    variant="secondary"
                    disabled={busy}
                    onClick={() => void controller.submit(operation.id, operation.progress.hash)}
                  >
                    Retry saved transaction
                  </Button>
                )}
                {operation.can_cancel && (
                  <Button
                    variant="secondary"
                    disabled={busy}
                    onClick={() => void controller.cancel(operation.id)}
                  >
                    Cancel swap
                  </Button>
                )}
              </HStack>
              {operation.progress.status === "cancelled" && (
                <Text color="fgMuted" font="body">
                  No further transactions will be sent. Any completed approval remains on-chain.
                </Text>
              )}
            </ContentCard>
          );
        })}
    </VStack>
  );
}
