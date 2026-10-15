import { expect, test } from "bun:test";

import { explorerUrl } from "../src/networks";

test("builds distinct transaction and address links on the selected chain", () => {
  expect(explorerUrl("ethereum", "0x123", "tx")).toBe("https://etherscan.io/tx/0x123");
  expect(explorerUrl("arbitrum_one", "0x123", "address")).toBe("https://arbiscan.io/address/0x123");
  expect(explorerUrl("ethereum_sepolia", "0x123", "address")).toBe(
    "https://sepolia.etherscan.io/address/0x123",
  );
});

test("preserves chain-specific account and transaction paths", () => {
  expect(explorerUrl("solana", "account", "address")).toBe("https://solscan.io/account/account");
  expect(explorerUrl("xrpl", "rAccount", "address")).toBe("https://xrpscan.com/account/rAccount");
  expect(explorerUrl("tron", "TAccount", "address")).toBe(
    "https://tronscan.org/#/address/TAccount",
  );
  expect(explorerUrl("bitcoin", "bc1account", "address")).toBe(
    "https://blockstream.info/address/bc1account",
  );
  expect(explorerUrl("filecoin", "cid", "tx")).toBe("https://filfox.info/en/message/cid");
  expect(explorerUrl("injective", "injAccount", "address")).toBe(
    "https://explorer.injective.network/account/injAccount",
  );
  expect(explorerUrl("zcash", "hash", "tx")).toBe("https://3xpl.com/zcash/transaction/hash");
});

test("returns no link for unknown networks or empty identifiers", () => {
  expect(explorerUrl("unknown", "address", "address")).toBeNull();
  expect(explorerUrl("ethereum", "", "tx")).toBeNull();
});
