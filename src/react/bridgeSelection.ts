import { networkById } from "../networks";
import type { Asset, CctpState, NetworkId } from "../types";
import { sortNetworksForSelection } from "./model";

/** Select only executable, registry-backed routes and use the same defaults as the controls. */
export function bridgeSelection(
  routes: CctpState["routes"],
  assets: readonly Asset[],
  enabledNetworks: readonly NetworkId[] | undefined,
  sourceChoice: string,
  destinationChoice: string,
) {
  const executableRoutes = routes.flatMap((route) => {
    const source = networkById(route.source);
    const destination = networkById(route.destination);
    if (
      !route.executable ||
      source?.kind !== "evm" ||
      destination?.kind !== "evm" ||
      (enabledNetworks &&
        (!enabledNetworks.includes(source.id) || !enabledNetworks.includes(destination.id)))
    )
      return [];
    return [{ ...route, source: source.id, destination: destination.id }];
  });
  const sourceNetworks = sortNetworksForSelection(
    [...new Set(executableRoutes.map((route) => route.source))],
    assets,
  ).map((network) => network.id);
  const source = sourceNetworks.find((id) => id === sourceChoice) ?? sourceNetworks[0] ?? "";
  const destinationNetworks = sortNetworksForSelection(
    [
      ...new Set(
        executableRoutes
          .filter((route) => route.source === source)
          .map((route) => route.destination),
      ),
    ],
    assets,
  ).map((network) => network.id);
  const destination =
    destinationNetworks.find((id) => id === destinationChoice) ?? destinationNetworks[0] ?? "";
  const route = executableRoutes.find(
    (route) => route.source === source && route.destination === destination,
  );
  return { source, destination, sourceNetworks, destinationNetworks, route };
}
