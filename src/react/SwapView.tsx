import { Button } from "@coinbase/cds-web/buttons";
import { ContentCard } from "@coinbase/cds-web/cards/ContentCard";
import { HStack, VStack } from "@coinbase/cds-web/layout";
import { Text } from "@coinbase/cds-web/typography";
import { useState } from "react";

import type { Asset, FiatCurrency, NetworkId, WalletSession } from "../types";
import { BridgeView, type CctpRunner } from "./BridgeView";
import { UniswapView } from "./UniswapView";
import type { UniswapController } from "./useUniswap";

type SwapMode = "uniswap" | "cctp" | null;

export function SwapView({
  busy,
  run,
  onSession,
  assets,
  uniswap,
  enabledNetworks,
  useCryptoSymbols,
  fiatCurrency,
  usdExchangeRate,
}: {
  assets: Asset[];
  fiatCurrency: FiatCurrency;
  usdExchangeRate: number;
  enabledNetworks: NetworkId[];
  useCryptoSymbols: boolean;
  uniswap: UniswapController;
  busy: boolean;
  run: CctpRunner;
  onSession: (session: WalletSession) => void;
}) {
  const [mode, setMode] = useState<SwapMode>(null);
  return (
    <VStack gap={3}>
      {mode === null && (
        <HStack flexWrap="wrap" gap={3}>
          <ContentCard background="bgElevation1" borderRadius={400} gap={2} padding={4}>
            <Text as="h2" font="title1">
              Swap tokens
            </Text>
            <Text color="fgMuted" font="body">
              Swap tokens on the same chain or across chains.
            </Text>
            <Button
              onClick={() => {
                setMode("uniswap");
                void uniswap.load();
              }}
            >
              Swap tokens
            </Button>
          </ContentCard>
          <ContentCard background="bgElevation1" borderRadius={400} gap={2} padding={4}>
            <Text as="h2" font="title1">
              Cross-chain transfer
            </Text>
            <Text color="fgMuted" font="body">
              Move USDC between your supported networks with Circle CCTP.
            </Text>
            <Button onClick={() => setMode("cctp")}>Across chains</Button>
          </ContentCard>
        </HStack>
      )}
      {mode !== null && (
        <HStack>
          <Button
            variant="secondary"
            onClick={() => {
              uniswap.stop();
              setMode(null);
            }}
          >
            Back
          </Button>
        </HStack>
      )}
      {mode === "uniswap" && (
        <UniswapView
          assets={assets}
          enabledNetworks={enabledNetworks}
          useCryptoSymbols={useCryptoSymbols}
          busy={busy}
          controller={uniswap}
          fiatCurrency={fiatCurrency}
          usdExchangeRate={usdExchangeRate}
        />
      )}
      {mode === "cctp" && (
        <BridgeView
          assets={assets}
          enabledNetworks={enabledNetworks}
          busy={busy}
          run={run}
          onSession={onSession}
        />
      )}
    </VStack>
  );
}
