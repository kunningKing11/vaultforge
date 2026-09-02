# VaultForge Wallet Agent Instructions

## Product Direction

VaultForge is a production-grade, multichain, self-custody desktop wallet. New work must use real chain-backed balances, fees, transaction formats, signatures, broadcasts, and status tracking. Existing simulator behavior is temporary scaffolding and may be removed when the same area gains coherent real-wallet behavior.

Non-negotiable rules:

- Keep wallet identity, encrypted secrets, derivation, providers, portfolio snapshots, transaction stages, signing, broadcast/status tracking, and frontend DTOs separate.
- Treat RPC/provider data as the source of truth for funds. Never invent starter balances or mutate local balances as though a transaction moved real funds.
- Store authoritative crypto amounts as integer base units, never `f64`.
- Keep signing and secret handling in Rust. Prefer maintained, audited cryptographic primitives and zeroization-capable secret types.
- Keep mnemonics, seeds, private keys, and signing material inside encrypted storage and authenticated runtime boundaries. Add hardware-wallet support before recommending larger real-fund use.
- Preserve compilation and frontend/backend contracts while replacing legacy behavior.
- Implement chain-specific rules behind clear interfaces; do not force non-EVM chains into EVM assumptions.
- Do not claim support for a chain until derivation, validation, balances, fees, signing, broadcast, and status tracking exist or are explicitly marked unavailable.

## Current Chain Scope

The architecture must support Bitcoin; Ethereum, Monad, Polygon, Arbitrum One, Base, Optimism, and Avalanche C-Chain; Filecoin; Injective; Solana; Tron; and Zcash.

Implemented transfer paths are currently basic Bitcoin, EVM native/ERC-20, Solana native/classic SPL, and Tron native transfers. Treat other exposed chains as address/portfolio scaffolding unless their full provider and transaction paths exist.

For new chain work, establish shared account/provider boundaries first, then prioritize EVM, Bitcoin, Solana, and finally Injective, Filecoin, and Zcash with chain-specific correctness.

Lightning has frontend types but is absent from `rawNetworks`. Do not claim Lightning support without a real node/LSP, channel, invoice, payment, and liquidity strategy. Do not imply Zcash shielded support without viewing keys, note scanning, proving, and shielded transaction construction.

## Code Organization

Keep code DRY, but share only genuinely common plumbing. Chain-specific validation, fee, signing, and encoding behavior must remain explicit.

### Frontend

- `src/render.ts`: root composition, screen selection, and post-render coordination only.
- `src/views/onboarding.ts` and `locked.ts`: lifecycle and locked screens.
- `src/views/shell.ts`: desktop/mobile navigation and unlocked shell.
- `src/views/wallet.ts`: wallet screens.
- `src/views/shared.ts`: reusable templates, selectors, formatting, and loading UI.
- `src/views/toast.ts`: toast markup only; timing and DOM lifecycle stay in `src/toasts.ts`.
- `src/events.ts` and `src/commands.ts`: event binding and command behavior. Preserve existing `data-action`, `data-view`, form names, and escaping when editing templates.
- `src/state.ts`: typed application state. Model missing, locked, and unlocked wallets explicitly and normalize every backend session through the shared session-to-wallet transition.
- `src/selectors.ts`: derived reads.

Keep timer handles, callbacks, and controller details in their owning modules, not render state.

### Backend

- `src-tauri/src/main.rs`: module declarations, managed state, Tauri setup, and `generate_handler!` wiring only.
- `commands/wallet.rs`: create, import, unlock, lock, clear, and session commands.
- `commands/tx.rs`: signing, broadcast, compatibility swap flows, and transaction status.
- `commands/market.rs`: market data and provider-backed portfolio refresh.

Command modules orchestrate domain modules. Keep storage, encryption, derivation, validation, providers, and transaction formats in focused modules rather than command handlers or `main.rs`.

## Wallet Data And Security Contracts

- `Wallet`: unlocked in-memory identity, encrypted-wallet metadata, derived accounts, active selections, and provider-derived cached portfolio/activity. It must not treat cached assets as authoritative funds.
- `WalletPayload`: encrypted persisted name, timestamp, mnemonic/seed, account and derivation metadata, and persistent preferences. Cached balances are allowed only as explicitly stale snapshots. Any shape change must intentionally handle `StoredWalletFile.version`.
- `StoredWalletFile`: unencrypted envelope containing only version, wallet name, active network, salt, nonce, and ciphertext unless another plaintext field has a documented security reason.
- `StoredWalletMetadata`: minimal locked summary such as wallet name, active network, and storage version. It must not expose decrypted wallet or provider data.
- `WalletSession`: frontend DTO. It must match the frontend type in the same patch.

Current `WalletSession` contract:

```ts
type WalletSession = {
  has_wallet: boolean;
  locked: boolean;
  wallet_name: string | null;
  addresses?: Record<string, string> | null;
  fiat_currency: "USD" | "EUR" | "GBP" | "JPY" | null;
  usd_exchange_rate: number | null;
  assets: Asset[];
  activity: Activity[];
  enabled_networks: string[];
  auto_lock_timeout_secs: number | null;
};
```

`Asset.token_address` is optional and nullable. Native assets use `null`; ERC-20 assets use the contract; SPL assets use the mint. Signing must use this identifier, never the display name.

Session states:

- No wallet: `has_wallet = false`, `locked = false`, no addresses, and empty assets/activity.
- Locked: `has_wallet = true`, `locked = true`, optional wallet name, no secrets, and no provider data unless deliberately persisted as plaintext cache.
- Unlocked: real derived addresses and provider-derived portfolio/activity.

Never omit fields required by the frontend.

## Wallet Lifecycle

- Create: validate the password policy, generate a secure BIP39 mnemonic, derive documented chain paths, encrypt and persist the payload, initialize provider refresh, and return a complete session. Never use custom mnemonic word lists.
- Import: validate BIP39 word count and checksum, validate the password, derive and retain accounts, persist the encrypted payload, refresh providers, and return a complete session.
- Unlock: derive the key from the stored salt, decrypt and validate the payload, reconstruct runtime state, refresh or schedule provider data, and return a complete session. An already-loaded wallet still requires password validation or an explicit authenticated-session policy.
- Lock: zeroize and remove decrypted mnemonic/seed/key material, mark the app locked, and retain only minimal metadata.
- Clear: delete encrypted storage, clear runtime secrets and wallet-specific provider caches, and return a no-wallet session.

## Providers, Balances, And Transactions

Each chain family should expose address validation, native/token balance reads, fee estimation, unsigned draft construction, signed broadcast, and transaction status through a common provider boundary.

Chain-specific requirements:

- EVM: chain ID, RPC URL, native currency, token contracts, pending nonce, gas estimation, EIP-1559 where available, raw broadcast, and receipts. Current sends use `eth_feeHistory` with `eth_gasPrice` fallback but have no nonce reservation manager or user priority-fee policy.
- Bitcoin: UTXO discovery, fee rates, coin selection/PSBT or transaction construction, signing, broadcast, and confirmations.
- Solana: recent blockhash, native/SPL balances, associated token accounts, recipient ATA rent, construction, signing, send, and confirmation. Current token support is classic SPL, not Token-2022.
- Filecoin, Injective, Tron, and Zcash: use their native derivation, address, fee, signing, and broadcast rules.

Represent amounts in base units: wei/token units, satoshis, lamports/SPL units, zatoshis, attoFIL, and chain denomination units. Convert to decimal display strings only at the UI edge.

Model transaction stages explicitly: draft, estimated, approved, signed, broadcast, pending, confirmed, and failed/dropped/replaced. Records must retain enough chain-specific information to audit what was signed and sent, including network, accounts, asset identifier, base-unit amount, fees, nonce/sequence/blockhash/UTXOs, digest, signatures, broadcast ID, and confirmation status.

After broadcast, refresh provider balances and track pending state separately. Never create fake hashes, signatures, confirmations, fees, or local-only send/swap accounting in production paths.

Keep any simulator-only behavior isolated to explicit test fixtures.

## Verification

When wallet models, providers, signing, transactions, or DTOs change, run:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
bun run typecheck
```

Document any command that could not run.

Real-wallet changes require relevant tests for:

- BIP39 create/import validation and deterministic address derivation.
- Encryption round trips and locked-session secret isolation.
- Provider balance and fee parsing with mocked RPC responses.
- Draft validation and chain-valid signatures or serialized transactions.
- Broadcast/status provider errors.

Do not remove wallet or session fields without updating lifecycle, persistence, commands, frontend code, migrations, and tests together.
