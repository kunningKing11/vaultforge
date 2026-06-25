# VaultForge Wallet Agent Instructions

## Project Goal

VaultForge must become a real, production-grade, multichain self-custody wallet. Do not design new work around simulated balances, fake fees, fake signatures, or local-only transaction mutation. Existing simulator behavior is temporary legacy scaffolding and may be skipped or replaced directly when implementing coherent real-wallet functionality.

The target product is a desktop wallet that can create/import a wallet, derive real addresses, fetch real balances, estimate real fees, sign real transactions, broadcast them through chain RPCs, and track transaction status.

## Real Wallet Principles

- Prefer real chain-backed behavior over simulator compatibility.
- Preserve compileability and frontend/backend contracts while replacing fake behavior.
- Use audited, maintained wallet and cryptography primitives where practical.
- Store and process crypto amounts as integer base units, not `f64`.
- Treat private keys, mnemonics, seeds, and signing material as high-risk secrets.
- Avoid claiming support for a chain until address derivation, balance reads, fee estimation, signing, broadcasting, and status tracking are implemented or intentionally marked unavailable.
- Implement production wallet behavior chain-by-chain behind clear interfaces instead of mixing unrelated chain rules into one generic path.

## Supported Chain Scope

The frontend currently defines or exposes these network families and assets. Backend architecture should support this scope, even if implementation lands incrementally.

- EVM: Ethereum, Monad, Polygon, Arbitrum One, Base, Optimism, Avalanche C-Chain
- Bitcoin
- Solana
- Zcash
- Filecoin
- Injective

Lightning has frontend type definitions but is not currently present in `rawNetworks`. Do not claim Lightning support until there is a real node, LSP, channel, invoice, payment, and liquidity strategy.

## Architecture Boundary

Keep these concerns separate:

- Wallet identity and encrypted secrets
- Chain account derivation
- Chain RPC/provider clients
- Portfolio balance snapshots
- Transaction drafts and simulations
- Signing
- Broadcast and transaction status tracking
- Frontend session DTOs

Do not let local UI state or simulator state become the source of truth for real funds.

## Backend Module Layout

Keep `src-tauri/src/main.rs` limited to Tauri application setup, managed state registration, module declarations, and `generate_handler!` wiring.

Place Tauri command handlers under `src-tauri/src/commands/` by responsibility:

- `commands/wallet.rs` for wallet lifecycle commands such as create, import, unlock, lock, clear, and session reads.
- `commands/tx.rs` for signing, broadcasting, swap compatibility flows, and transaction status commands.
- `commands/market.rs` for market-data and price refresh commands.

Command modules may orchestrate domain modules, but core wallet, storage, derivation, provider, validation, and transaction-format logic should stay in their dedicated modules. Do not grow `main.rs` or a single command file into a catch-all.

## Backend Data Model Contract

The backend should have distinct data shapes for persistent encrypted wallet data, unlocked runtime wallet data, derived chain accounts, chain-backed balances, transaction drafts, signed transactions, and frontend sessions.

### `Wallet`

`Wallet` is the unlocked in-memory domain model. It should represent identity, encrypted-wallet metadata, and derived accounts. It should not treat fake starter balances as authoritative funds.

Expected responsibilities:

- Wallet display name
- Wallet creation/import timestamp
- Encrypted mnemonic or seed material after unlock only
- Chain account metadata
- Active account/network selection
- Cached portfolio and activity snapshots derived from providers

If `Wallet` includes `assets` or `activity`, those fields must be treated as provider-derived cache or temporary compatibility data, not as the source of truth for real funds.

### `WalletPayload`

`WalletPayload` is the encrypted persisted wallet payload.

It should contain sensitive and stateful data required to reconstruct an unlocked wallet:

- Wallet name
- Created/imported timestamp
- Mnemonic or seed material
- Account indexes and derivation metadata
- User labels/preferences that need persistence

It should not contain fake balances as permanent truth. Balance snapshots may be cached only if clearly treated as stale cache and refreshed from RPC before financial decisions.

Any `WalletPayload` shape change must intentionally handle `StoredWalletFile.version`.

### `StoredWalletFile`

`StoredWalletFile` is the outer unencrypted storage envelope.

Allowed plaintext metadata:

- Storage version
- Wallet display name
- Active network
- Encryption salt
- Encryption nonce
- Ciphertext

Avoid storing addresses, balances, activity, mnemonics, seeds, or derivation data outside ciphertext unless there is a documented product/security reason.

### `StoredWalletMetadata`

`StoredWalletMetadata` is the minimal locked-state summary available before unlock.

It may include:

- Wallet name
- Active network
- Storage version

It must not pretend that full wallet, address, balance, or activity data is available while locked unless that data was intentionally stored in plaintext.

### `WalletSession`

`WalletSession` is the frontend-facing DTO returned by Tauri commands. It must match `src/main.ts` exactly or the frontend type must be updated in the same patch.

Current frontend contract:

```ts
type WalletSession = {
  has_wallet: boolean;
  locked: boolean;
  wallet_name: string | null;
  address: string | null;
  addresses?: Record<string, string> | null;
  assets: Asset[];
  activity: Activity[];
};
```

Session rules:

- No wallet: return `has_wallet = false`, `locked = false`, no address, empty assets, and empty activity.
- Locked wallet: return `has_wallet = true`, `locked = true`, wallet name if available, no decrypted secrets, and no provider data unless intentionally persisted as plaintext cache.
- Unlocked wallet: return real derived addresses and provider-derived portfolio/activity data.

Never return a `WalletSession` missing fields expected by the frontend.

## Wallet Lifecycle

### Create Wallet

Real `create_wallet` behavior must:

1. Validate passphrase policy.
2. Generate a valid BIP39 mnemonic using cryptographically secure randomness.
3. Derive chain accounts using documented derivation paths.
4. Persist encrypted wallet payload.
5. Initialize provider-backed portfolio refresh.
6. Return a complete `WalletSession`.

Do not generate mnemonics from custom word lists.

### Import Wallet

Real `import_wallet` behavior must:

1. Validate mnemonic word count and checksum.
2. Validate passphrase policy.
3. Derive chain accounts using documented derivation paths.
4. Persist encrypted wallet payload.
5. Refresh balances from chain providers.
6. Return a complete `WalletSession`.

Do not derive addresses and discard them.

### Unlock Wallet

Real `unlock_wallet` behavior must:

1. Read stored wallet envelope.
2. Derive decryption key from passphrase and stored salt.
3. Decrypt and validate wallet payload.
4. Reconstruct unlocked wallet/account state.
5. Refresh or schedule refresh of balances and activity from providers.
6. Return a complete `WalletSession`.

Unlocking an already-loaded wallet must still validate the passphrase or use an explicit authenticated session policy.

### Lock Wallet

Real `lock_wallet` behavior must:

1. Remove decrypted mnemonic/seed/key material from memory.
2. Remove encryption key material from memory.
3. Mark the app locked.
4. Preserve only minimal locked metadata.

### Clear Wallet

Real `clear_wallet` behavior must:

1. Delete the encrypted wallet file.
2. Clear in-memory wallet state and key material.
3. Clear provider caches if they expose wallet-specific data.
4. Return a no-wallet `WalletSession`.

## Chain Provider And RPC Architecture

Each chain family should have a provider implementation behind a common trait or interface.

Provider responsibilities:

- Validate addresses for that chain.
- Fetch native balances.
- Fetch supported token balances.
- Estimate transaction fees.
- Build unsigned transaction drafts.
- Broadcast signed transactions.
- Fetch transaction status and receipts.

EVM providers must support chain ID, RPC URL, native currency, token contracts, nonce retrieval, gas estimation, EIP-1559 where available, raw transaction broadcast, and receipt polling.

Bitcoin providers must support UTXO discovery, fee rate estimation, PSBT or transaction construction, signing strategy, broadcast, and confirmation tracking.

Solana providers must support recent blockhash retrieval, account balance reads, SPL token account reads, transaction construction, signing, send, and confirmation tracking.

Zcash support must explicitly distinguish transparent and shielded support. Do not imply shielded support unless viewing keys, note scanning, proving, and transaction construction are implemented.

Filecoin and Injective support must use chain-appropriate derivation, address formats, fee models, signing formats, and broadcast APIs.

## Balance Model

Real balances must come from chain RPCs or trusted provider APIs.

Do not use starter balances for real wallet state. Do not mutate local balances as if funds moved. After a transaction broadcasts, refresh balances from providers and track pending state separately.

Represent amounts as integer base units:

- EVM native and ERC-20: wei/token base units with decimals metadata
- Bitcoin: satoshis
- Solana: lamports and SPL base units
- Zcash: zatoshis
- Filecoin: attoFIL
- Injective: chain denomination base units

UI formatting can convert integer base units to decimal strings at the edge.

## Transaction Model

Separate transaction states:

- Draft
- Simulated/estimated
- User-approved
- Signed
- Broadcast
- Pending
- Confirmed
- Failed/dropped/replaced

Do not use fake payload hashes or fake signatures for production transaction flow.

Transaction records should store enough chain-specific data to audit what was signed and broadcast:

- Chain/network ID
- From account/address
- To address
- Asset/token identifier
- Amount in base units
- Fee parameters
- Nonce/sequence/blockhash/UTXO inputs as applicable
- Unsigned transaction digest where applicable
- Signature(s)
- Broadcast transaction hash/signature
- Status and confirmation metadata

## Signing And Key Management

Signing must use real chain transaction formats.

Required direction:

- Use real BIP39 mnemonic generation/import.
- Use documented derivation paths per chain.
- Avoid keeping secrets in plain strings longer than necessary.
- Prefer zeroization-capable types for secret material.
- Keep signing logic in backend/Rust, not frontend JavaScript.
- Add hardware wallet support before recommending real funds at larger value.

Fake signatures based on hashing wallet metadata are not acceptable for production paths.

## Frontend Contract

The frontend may keep `assets` and `activity` in `WalletSession`, but backend values must represent real provider-derived portfolio and transaction data, or explicitly unavailable states.

When backend DTOs change, update TypeScript types and rendering logic in the same patch.

Before committing DTO changes, verify:

```bash
npx tsc --noEmit
cargo check
cargo test
```

## Migration From Simulator Code

Simulator code may be removed directly if replaced by real wallet behavior in the same area.

Simulator-only code includes:

- Custom fake mnemonic generation
- Starter balances
- Local-only balance mutation for sends
- Local-only swap accounting
- Static/fake fee calculation
- Fake transaction signatures
- Fake confirmed activity records
- Hardcoded RPC health values

Do not add new simulator-only features unless they are clearly isolated test fixtures.

## Multichain Implementation Strategy

The app must be architected for all supported chains, but implementation can land incrementally.

Preferred order:

1. Establish shared wallet/account/provider abstractions.
2. Implement real EVM support across defined EVM chains.
3. Implement Bitcoin.
4. Implement Solana.
5. Implement Injective, Filecoin, and Zcash with chain-specific correctness.

Do not force non-EVM chains into EVM assumptions.

## Verification Requirements

Before committing wallet model, provider, signing, transaction, or DTO changes, run:

```bash
cargo check
cargo test
npx tsc --noEmit
```

If a command cannot be run, document the reason.

Minimum test coverage for real wallet changes:

- BIP39 create/import validation
- Deterministic address derivation per supported chain
- Encryption/decryption round trip
- Locked session does not expose decrypted secrets
- Provider balance parsing with mocked RPC responses
- Fee estimation parsing with mocked RPC responses
- Transaction draft validation
- Signing produces chain-valid signatures or serialized transactions
- Broadcast/status code handles provider errors

## Prohibited Shortcuts

- Do not use `f64` for authoritative crypto amounts.
- Do not invent balances without provider data.
- Do not mark simulated sends as real broadcasts.
- Do not use fake signatures in production paths.
- Do not claim multichain support for a chain without chain-specific derivation, validation, balance, fee, signing, broadcast, and status behavior.
- Do not remove fields from wallet structs or session DTOs without updating all dependent lifecycle, persistence, command, frontend, and test code in the same patch.
- Do not store secrets outside encrypted payloads unless explicitly justified and reviewed.
