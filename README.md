---
title: VaultForge Wallet
description: A local-first crypto wallet desktop app built with TypeScript, TailwindCSS, Rust, and Tauri.
markdownlint:
  MD033: false
---

<div align="center">
<pre>
'##::::'##::::'###::::'##::::'##:'##:::::::'########:'########::'#######::'########:::'######:::'########:
 ##:::: ##:::'## ##::: ##:::: ##: ##:::::::... ##..:: ##.....::'##.... ##: ##.... ##:'##... ##:: ##.....::
 ##:::: ##::'##:. ##:: ##:::: ##: ##:::::::::: ##:::: ##::::::: ##:::: ##: ##:::: ##: ##:::..::: ##:::::::
 ##:::: ##:'##:::. ##: ##:::: ##: ##:::::::::: ##:::: ######::: ##:::: ##: ########:: ##::'####: ######:::
. ##:: ##:: #########: ##:::: ##: ##:::::::::: ##:::: ##...:::: ##:::: ##: ##.. ##::: ##::: ##:: ##...::::
:. ## ##::: ##.... ##: ##:::: ##: ##:::::::::: ##:::: ##::::::: ##:::: ##: ##::. ##:: ##::: ##:: ##:::::::
::. ###:::: ##:::: ##:. #######:: ########:::: ##:::: ##:::::::. #######:: ##:::. ##:. ######::: ########:
:::...:::::..:::::..:::.......:::........:::::..:::::..:::::::::.......:::..:::::..:::......::::........::
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

## Project Structure

- `src/` contains the TypeScript frontend, including rendering, event binding, command calls, app state, formatting, QR handling, network metadata, and shared types.
- `src-tauri/src/main.rs` wires the Tauri app, managed state, and command handlers.
- `src-tauri/src/commands/` contains Tauri command handlers split by domain: wallet lifecycle, transactions, and market data.
- `src-tauri/src/providers/` contains chain RPC/provider code for balances, fee data, broadcast, and transaction status.
- `src-tauri/src/tx/` contains chain-specific transaction construction, encoding, and signing code.
- `src-tauri/src/assets.rs` contains shared asset-cache helpers used by provider refresh paths.
- `src-tauri/src/activity.rs`, `assets.rs`, `derivation.rs`, `dto.rs`, `state.rs`, `storage.rs`, and `validation.rs` contain the backend domain support code used by commands:
  - `activity.rs`:
  - `assets.rs`:
  - `derivation.rs`: key derivation and
  - `dto.rs`: serialization and
  - `state.rs`:
  - `storage.rs`:
  - `validation.rs`:

## Development

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

**All build commands run `eslint --fix src/` before starting Vite.**
