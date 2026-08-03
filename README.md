---
title: VaultForge Wallet
description: A local-first crypto wallet desktop app built with TypeScript, TailwindCSS, Rust, and Tauri.
markdownlint:
  MD033: false
---

<div align="center">
<pre>
Yb    dP    db    88   88 88     888888 888888  dP"Yb  88""Yb  dP""b8 888888 
 Yb  dP    dPYb   88   88 88       88   88__   dP   Yb 88__dP dP   `" 88__   
  YbdP    dP__Yb  Y8   8P 88  .o   88   88""   Yb   dP 88"Yb  Yb  "88 88""   
   YP    dP""""Yb `YbodP' 88ood8   88   88      YbodP  88  Yb  YboodP 888888 
</pre>
</div>

---

# VaultForge Wallet

A local-first crypto wallet desktop app built with a TypeScript frontend, TailwindCSS, and a Rust backend through Tauri.

## Features

- Create, import, lock, and unlock a wallet session
- Portfolio dashboard with token balances, fiat valuation, allocation, and weighted 24h change
- Sign and review chain-specific transactions before broadcasting them
- Basic real transfer paths for BTC, EVM native/ERC-20, Solana native/classic SPL assets, and Tron native
- Review provider-derived fees, total debit, USD value, and post-send balance estimates
- Solana SPL sends account for recipient associated token account (ATA) rent when needed
- Encrypted local wallet persistence using the app data directory
- Activity details with transaction hashes, signatures, payload hashes, and copy actions
- Security center with storage status, signing status, and local wallet clearing
- Passphrase confirmation and strength feedback for encrypted wallet setup
- Send, receive, swap, assets, activity, and settings screens
- Rust-backed Tauri commands for wallet state, validation, transaction signing, encrypted storage, provider-backed reads, broadcast, and status checks
- Responsive TailwindCSS UI with desktop sidebar

### Coming soon

- Non-native Tron token support (e.g., stablecoins)
- Ripple (XRP) support
- Zcash (ZEC) support with support for shielded addresses planned

For a full list of upcoming features (there are quite a few!), click [here](https://github.com/kunningKing11/vaultforge/blob/main/ROADMAP.md).

## Supported chains

- Bitcoin Mainnet (BTC)
- EVM: Ethereum Mainnet (ETH), Arbitrum One, Avalanche C-Chain (AVAX), Base, BNB Smart Chain (BNB), Monad (MON), Optimism, and Polygon (POL)
- Solana Mainnet (SOL)
- Tron Mainnet (TRX; native transfers only)

### Supported features

The table distinguishes implemented backend paths from network entries that are currently address or validation scaffolding. “No” means the feature is not implemented; it does not imply that the underlying chain lacks it.

| Chain             |              Native transfer              |     Token transfer      | NFTs / Filecoin storage | Shielded pools |
| :---------------- | :---------------------------------------: | :---------------------: | :---------------------: | :------------: |
| Arbitrum One      |                    Yes                    |         ERC-20          |           No            |       No       |
| Avalanche C-Chain |                    Yes                    |         ERC-20          |           No            |       No       |
| Base              |                    Yes                    |         ERC-20          |           No            |       No       |
| BNB Smart Chain   |                    Yes                    |         ERC-20          |           No            |       No       |
| Bitcoin           |          Yes (basic P2WPKH path)          |           No            |           No            |       No       |
| Ethereum Mainnet  |                    Yes                    |         ERC-20          |           No            |       No       |
| Filecoin          |                    No                     |           No            |           No            |       No       |
| Injective         |                    No                     |           No            |           No            |       No       |
| Monad             |                    Yes                    |         ERC-20          |           No            |       No       |
| Optimism          |                    Yes                    |         ERC-20          |           No            |       No       |
| Polygon           |                    Yes                    |         ERC-20          |           No            |       No       |
| Solana            |                    Yes                    |    Classic SPL Token    |           No            |       No       |
| Tron              |                    Yes                    | No (TRC-20 unavailable) |           No            |       No       |
| Zcash             | No (transparent address scaffolding only) |           No            |           No            |       No       |

## Development

### Setup

**Install dependencies:**

With `npm`:

```bash
npm install
```

With `bun`:

```bash
bun install
```

**Run the web frontend:**

With `npm`:

```bash
npm run dev
```

With `bun`:

```bash
bun run dev
```

**Run the Tauri desktop app:**

With `npm`:

```bash
npx tauri dev
```

With `bun`:

```bash
bunx tauri dev
```

**Build the frontend:**

With `npm`:

```bash
npm run build
```

With `bun`:

```bash
bun run build
```

Build the desktop bundle:

With `npm`:

```bash
npx tauri build
```

With `bun`:

```bash
bunx tauri build
```

**Check and fix frontend quality:**

With `npm`:

```bash
npm run check
npm run lint:fix
npm run format
```

With `bun`:

```bash
bun run check
bun run lint:fix
bun run format
```

`dev` and `build` run the non-mutating Oxlint, Oxfmt, and TypeScript checks before starting Vite. Run `./lint.sh` for the same checks or `./lint.sh --fix` to apply safe lint fixes and formatting first.

Every commit runs the Lefthook pre-commit checks including Oxlint, Oxfmt, TypeScript, `cargo check`, Rust Analyzer analysis, and the full Rust test suite. The Lefthook script runs every check before printing a pass/fail summary; a failed check blocks the commit.

You can the same script manually with `npm run hooks:check`. After cloning or installing dependencies, `npm install` installs the Git hook through the `prepare` script; run `npm run prepare` in an already-installed checkout.

**NOTE: Rust Analyzer must be installed in the active Rust toolchain (for example, `rustup component add rust-analyzer`) or its check will fail and block the commit.**

### Project Structure

- `src/` contains the TypeScript frontend, including event binding, command calls, app state, formatting, QR handling, network metadata, and shared types.
- `src/render.ts` composes the current application state into the root UI and coordinates QR refreshes.
- `src/views/` contains focused TypeScript HTML-template modules for screens, shell layout, shared UI fragments, locked-state UI, and toast markup.
- `src/toasts.ts` owns toast timing and animation behavior while using the toast template in `src/views/toast.ts`.
- `src-tauri/src/main.rs` wires the Tauri app, managed state, and command handlers.
- `src-tauri/src/commands/` contains Tauri command handlers split by domain: wallet lifecycle, transactions, and market data.
- `src-tauri/src/providers/` contains chain RPC/provider code for balances, fee data, broadcast, and transaction status.
- `src-tauri/src/tx/` contains chain-specific transaction construction, encoding, and signing code.
- `src-tauri/src/assets.rs` contains shared asset-cache helpers used by provider refresh paths.
- `src-tauri/src/activity.rs`, `assets.rs`, `derivation.rs`, `dto.rs`, `state.rs`, `storage.rs`, and `validation.rs` contain the backend domain support code used by commands:
  - `activity.rs`:
  - `assets.rs`:
  - `derivation.rs`: key derivation
  - `dto.rs`: serialization
  - `state.rs`:
  - `storage.rs`:
  - `validation.rs`:
