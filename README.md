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
- Review provider-derived fees, total debit, USD value, and post-send balance estimates
- Encrypted local wallet persistence using the app data directory
- Activity details with transaction hashes, signatures, payload hashes, and copy actions
- Security center with storage status, signing status, and local wallet clearing
- Passphrase confirmation and strength feedback for encrypted wallet setup
- Send, receive, swap, assets, activity, and settings screens
- Rust-backed Tauri commands for wallet state, validation, transaction signing, encrypted storage, provider-backed reads, broadcast, and status checks
- Responsive TailwindCSS UI with desktop sidebar and mobile bottom navigation

## Project Structure

- `src/` contains the TypeScript frontend, including rendering, event binding, command calls, app state, formatting, QR handling, network metadata, and shared types.
- `src-tauri/src/main.rs` wires the Tauri app, managed state, and command handlers.
- `src-tauri/src/commands/` contains Tauri command handlers split by domain: wallet lifecycle, transactions, and market data.
- `src-tauri/src/providers/` contains chain RPC/provider code for balances, fee data, broadcast, and transaction status.
- `src-tauri/src/tx/` contains chain-specific transaction construction, encoding, and signing code.
- `src-tauri/src/assets.rs` contains shared asset-cache helpers used by provider refresh paths.
- `src-tauri/src/storage.rs`, `state.rs`, `derivation.rs`, `validation.rs`, and `dto.rs` contain the backend domain support code used by commands.

## Development

Install dependencies:

```bash
npm install
```

Run the web frontend:

```bash
npm run dev
```

Run the Tauri desktop app:

```bash
npm run tauri dev
```

Build the frontend:

```bash
npm run build
```

Build the desktop bundle:

```bash
npm run tauri build
```
