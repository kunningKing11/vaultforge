import { useToast } from "@coinbase/cds-web/overlays/useToast";
import { Link } from "@coinbase/cds-web/typography";
import { isTauri } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { ReactNode } from "react";

import { explorerUrl, networkDisplayName } from "../networks";

export function ExplorerLink({
  network,
  value,
  type,
  children,
}: {
  network: string;
  value: string;
  type: "tx" | "address";
  children?: ReactNode;
}) {
  const { show } = useToast();
  const url = explorerUrl(network, value, type);
  if (!url) return <>{children ?? value}</>;
  return (
    <Link
      href={url}
      openInNewWindow
      title={`View ${type === "tx" ? "transaction" : "address"} on ${networkDisplayName(network)} explorer`}
      onClick={(event) => {
        if (!isTauri()) return;
        event.preventDefault();
        void openUrl(url).catch(() =>
          show("Could not open the block explorer. Please restart the app and try again."),
        );
      }}
    >
      {children ?? value}
    </Link>
  );
}
