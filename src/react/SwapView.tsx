import { Button } from "@coinbase/cds-web/buttons";
import { ContentCard } from "@coinbase/cds-web/cards/ContentCard";
import { HStack, VStack } from "@coinbase/cds-web/layout";
import { Text } from "@coinbase/cds-web/typography";
import { useState } from "react";

import type { WalletSession } from "../types";
import { BridgeView, type CctpRunner } from "./BridgeView";

type SwapMode = "uniswap" | "cctp" | null;

function UniswapView() {
  return (
    <ContentCard background="bgElevation1" borderRadius={400} gap={3} padding={4}>
      <Text as="h2" font="title1">
        Swap tokens
      </Text>
      <Text font="body">
        Token swaps via Uniswap are not implemented yet. No funds will move. Swaps will exchange
        tokens on a single chain with a quote, slippage limit, and signed execution.
      </Text>
    </ContentCard>
  );
}

export function SwapView({
  busy,
  run,
  onSession,
}: {
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
              Uniswap: exchange tokens on one chain.
            </Text>
            <Button onClick={() => setMode("uniswap")}>Swap tokens</Button>
          </ContentCard>
          <ContentCard background="bgElevation1" borderRadius={400} gap={2} padding={4}>
            <Text as="h2" font="title1">
              Transfer USDC
            </Text>
            <Text color="fgMuted" font="body">
              Circle CCTP: move USDC between chains.
            </Text>
            <Button onClick={() => setMode("cctp")}>Transfer USDC</Button>
          </ContentCard>
        </HStack>
      )}
      {mode !== null && (
        <HStack>
          <Button variant="secondary" onClick={() => setMode(null)}>
            Back
          </Button>
        </HStack>
      )}
      {mode === "uniswap" && <UniswapView />}
      {mode === "cctp" && <BridgeView busy={busy} run={run} onSession={onSession} />}
    </VStack>
  );
}
