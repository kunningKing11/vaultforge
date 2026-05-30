# VaultForge Wallet

A local-first crypto wallet desktop app built with a TypeScript frontend, TailwindCSS, and a Rust backend through Tauri.

## Features

- Create, import, lock, and unlock a wallet session
- Portfolio dashboard with token balances and fiat valuation
- Send, receive, swap, assets, activity, and settings screens
- Rust-backed Tauri commands for wallet state, validation, transaction simulation, and network switching
- Responsive TailwindCSS UI for desktop and smaller screens

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

## Security note

This is a fully functional local app foundation with simulated assets and transactions. It is not production mainnet wallet software. Before handling real funds, add audited key derivation, encrypted persistence, hardware wallet support, chain RPC integrations, transaction signing, threat modeling, and external security review.
