import { Box, HStack, VStack } from "@coinbase/cds-web/layout";
import { Text } from "@coinbase/cds-web/typography";
import { Fragment } from "react";

import { assetIconPath, chainIconPath, UNKNOWN_ASSET_ICON } from "../assetIcons";
import { networkById } from "../networks";
import type { SwapRouteNode } from "../types";

/** Render only steps and fees returned by the quote, never guessed ticker routes. */
export function SwapRoutePreview({ route }: { route: SwapRouteNode[] }) {
  if (route.length === 0) return null;
  return (
    <VStack gap={2}>
      <Text font="headline">Route</Text>
      <HStack
        role="list"
        accessibilityLabel="Swap route"
        alignItems="center"
        gap={2}
        flexWrap="wrap"
      >
        {route.map((node, index) => (
          <Fragment key={`${index}:${node.network}:${node.token_address}`}>
            {index > 0 && (
              <Text aria-hidden="true" font="title3">
                →
              </Text>
            )}
            <HStack role="listitem" gap={1} alignItems="center">
              <Box position="relative" width={40} height={40} flexShrink={0}>
                <Box
                  as="img"
                  src={assetIconPath(node)}
                  alt={`${node.symbol} token`}
                  width={36}
                  height={36}
                  borderRadius={1000}
                  onError={(event) => {
                    if (event.currentTarget.getAttribute("src") !== UNKNOWN_ASSET_ICON)
                      event.currentTarget.src = UNKNOWN_ASSET_ICON;
                  }}
                />
                <Box position="absolute" bottom={0} right={0} background="bg" borderRadius={1000}>
                  <Box
                    as="img"
                    src={chainIconPath(node.network)}
                    alt={`${networkById(node.network)?.name ?? "Unknown chain"} network`}
                    width={18}
                    height={18}
                    borderRadius={1000}
                    onError={(event) => {
                      if (event.currentTarget.getAttribute("src") !== UNKNOWN_ASSET_ICON)
                        event.currentTarget.src = UNKNOWN_ASSET_ICON;
                    }}
                  />
                </Box>
              </Box>
              <VStack>
                <Text font="label1">{node.symbol}</Text>
                {node.pool_fee !== null && (
                  <Text font="caption" color="fgMuted">
                    {node.pool_fee / 10000}% pool fee
                  </Text>
                )}
                {node.via && (
                  <Text font="caption" color="fgMuted">
                    {node.via}
                    {node.pool_fee === null && node.via !== "Relay bridge"
                      ? " · fee unavailable"
                      : ""}
                  </Text>
                )}
              </VStack>
            </HStack>
          </Fragment>
        ))}
      </HStack>
    </VStack>
  );
}
