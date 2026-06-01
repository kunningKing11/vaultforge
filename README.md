# VaultForge Wallet

A local-first crypto wallet desktop app built with a TypeScript frontend, TailwindCSS, and a Rust backend through Tauri.

## Features

- Create, import, lock, and unlock a wallet session
- Portfolio dashboard with token balances, fiat valuation, allocation, and weighted 24h change
- Sign and review simulated transactions before broadcasting them
- Review simulated network fees, total debit, USD value, and post-send balances
- Encrypted local wallet persistence using the app data directory
- Activity details with transaction hashes, signatures, payload hashes, and copy actions
- Security center with storage status, simulated signing status, and local wallet clearing
- Passphrase confirmation and strength feedback for encrypted wallet setup
- Send, receive, swap, assets, activity, and settings screens
- Rust-backed Tauri commands for wallet state, validation, transaction simulation, encrypted storage, and network switching
- Responsive TailwindCSS UI with desktop sidebar and mobile bottom navigation

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

This is a fully functional local app foundation with simulated assets, signatures, encrypted storage, and transactions. It is not production mainnet wallet software. Before handling real funds, add audited key management, hardware wallet support, chain RPC integrations, production transaction signing, threat modeling, and external security review.
