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

The architecture must support Bitcoin; Ethereum, Monad, Polygon, Arbitrum One, Base, Optimism, and Avalanche C-Chain; Filecoin; Injective; Solana; Tron; XRP Ledger; and Zcash.

Implemented transfer paths are currently basic Bitcoin, EVM native/ERC-20, Solana native/classic SPL, Tron native, and XRP Ledger native transfers. Treat other exposed chains as address/portfolio scaffolding unless their full provider and transaction paths exist.

For new chain work, establish shared account/provider boundaries first, then prioritize EVM, Bitcoin, Solana, and finally Injective, Filecoin, and Zcash with chain-specific correctness.

Lightning has frontend types but is absent from `rawNetworks`. Do not claim Lightning support without a real node/LSP, channel, invoice, payment, and liquidity strategy. Do not imply Zcash shielded support without viewing keys, note scanning, proving, and shielded transaction construction.

## Code Organization

Keep code DRY, but share only genuinely common plumbing. Chain-specific validation, fee, signing, and encoding behavior must remain explicit.

In every TypeScript and JavaScript file, group imports in this order: runtime-provided
`bun:` and `node:` modules, third-party packages, local relative modules, then side-effect
and stylesheet imports. Sort module specifiers alphabetically within each group, regardless
of the imported binding names, and sort named imports alphabetically by their imported
names. Use braces for named imports, including single named imports; omit braces only for
default imports. Oxfmt and Oxlint enforce the repository-wide ordering.

### Frontend

- `src/main.tsx`: React root and Coinbase Design System global styles only.
- `src/react/App.tsx`: React screen composition, user actions, timers, dialogs, and view components. Keep Tauri calls and side effects in explicit callbacks/hooks; protect real-wallet commands with a synchronous operation gate.
- `src/react/model.ts`: typed application state and pure derived reads. Model missing, locked, and unlocked wallets explicitly and normalize every backend session through `walletStateFromSession()` and `applyWalletSession()`.
- `src/react/styles.css`: small layout and responsive rules layered on top of CDS tokens. Prefer stable CDS components; use native controls only where the CDS equivalent is deprecated or experimental.
- `src/walletApi.ts`: frontend command boundary. Preserve the Rust `WalletSession` DTO and avoid invoking Tauri directly from view components.

Keep timer handles, callbacks, and controller details in their owning modules, not render state.

### Backend

- `src-tauri/src/lib.rs`: module declarations, managed state, Tauri setup, and `generate_handler!` wiring only.
- `src-tauri/src/main.rs`: thin desktop entry point that calls the shared library `run` function.
- `commands/wallet.rs`: create, import, unlock, lock, clear, and session commands.
- `commands/tx.rs`: signing, broadcast, compatibility swap flows, and transaction status.
- `commands/market.rs`: market data and provider-backed portfolio refresh.
- `commands/swap.rs`: thin Uniswap command boundary; `uniswap.rs` owns durable swap orchestration, with providers and encoding in their existing focused modules.

Command modules orchestrate domain modules. Keep storage, encryption, derivation, validation, providers, and transaction formats in focused modules rather than command handlers or `main.rs`.

## Wallet Data And Security Contracts

- `Wallet`: unlocked in-memory identity, encrypted-wallet metadata, derived accounts, active selections, and provider-derived cached portfolio/activity. It must not treat cached assets as authoritative funds.
- `WalletPayload`: encrypted persisted name, timestamp, mnemonic/seed, optional BIP-39 passphrase, account and derivation metadata, and persistent preferences. Cached balances are allowed only as explicitly stale snapshots. Any shape change must intentionally handle `StoredWalletFile.version`.
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
  use_crypto_symbols: boolean;
  hide_below_usd: number;
};
```

`hide_below_usd` is the USD value below which assets are hidden from portfolio views. It defaults to 1 for new wallets and legacy payloads without the field, so no envelope version bump is required; the settings command rejects non-finite or negative amounts.

`Asset.token_address` is optional and nullable. Native assets use `null`; ERC-20 assets use the contract; SPL assets use the mint. Asset identity is the pair of network ID and nullable token identifier, never a ticker or display name. EVM contracts compare case-insensitively; Solana mints and Tron identifiers compare exactly. Use the shared identity helpers for selection and cache lookups. Both `symbol` and `unicode_symbol` are display metadata; native/token signing dispatch depends on the presence of `token_address`.

Network tokens may omit `coinGeckoId` when no independent price ID exists; native assets still require it. Such tokens use the per-mint metadata/price fallback when held and remain separate in asset grouping. Never assign a receipt token its underlying asset's price ID. Solana uses curated registry names and includes registered zero-balance selections only after a complete successful classic SPL account read; live amounts and decimals remain authoritative. RPC failure retains cached assets rather than proving zero balances.

`SignedTransaction.tokenAddress` retains the selected token identifier. New activity entries carry `asset`; legacy simulated swaps may also carry `counter_asset`, using `{ network, token_address }`. These optional encrypted activity fields remain compatible with storage versions 2–7, so no envelope version bump is required. Older entries without them have unknown asset identity; do not reconstruct it from a ticker. The `swap_tokens` command stays a no-op for compatibility: it returns the unchanged session without changing balances, creating activity, persisting storage, signing, or broadcasting. The Swap screen supports same-chain and cross-chain routed swaps, with a separate Transfer USDC flow using explicit CCTP operation commands.

`Wallet` and `WalletPayload` retain encrypted `operations` for recovery. New saves use storage version 16; versions 2–15 remain readable, with missing operations defaulting to an empty collection and a missing `Operation.kind` defaulting to `cctp`. Operations are tagged `Cctp`, `Send`, or `Swap`; missing legacy `Operation.swap` defaults to absent. A `Send` operation records one signed attempt per broadcast for EVM, XRP Ledger, Solana, and Tron native transfers: Rust persists the raw bytes, expected hash, nonce/sequence/blockhash, fee, and signature before the first network request, marks submission unknown before broadcasting, and records submitted, uncertain, or failed outcomes. Unlock reconciles signed, submitted, and uncertain transaction hashes without signing or rebroadcasting, and persists changed records only for the same wallet generation and unchanged operation snapshot. Recovery failure relocks and clears secrets only for that generation; a newer wallet session must remain untouched. `EvmNonceManager.restore_operations` protects unresolved CCTP and Uniswap attempts at the account level, while `restore_send_operations` restores ordinary EVM send nonces per nonce without blocking the account; non-EVM sends refuse a second signed transfer while one is unresolved, and an explicitly cancelled signed send is marked failed so recovery does not resurrect its protection. Unlocked session activity is derived from operation records rather than duplicated in the activity cache, so a send operation renders as `kind: "send"` and CCTP as `kind: "cctp"`. Bitcoin native sends are not yet recorded as operations. CCTP V2 polling supports 56 directed mainnet attestation routes among the seven implemented EVM networks with Circle-native USDC and Solana. EVM source proof uses canonical burn logs; Solana uses a successful native deposit/CPI and its program-owned MessageSent account, retaining slots separately from EVM block coordinates. EVM destinations verify pairs by `receiveMessage` simulation; Solana destinations use low-s signature recovery against enabled on-chain attesters, exact thresholds, and distinct signer ordering. Solana verification also pins mainnet genesis; it does not imply successful mint simulation or execution. Cross-family routes use permissionless destination callers and Solana mint recipients use the recipient wallet's USDC ATA. `CctpData` retains optional burn evidence, the off-chain nonce, polling deadlines, and a simulation marker that defaults to false. Storage version 12 records optional EVM chain/block coordinates and native Solana proof coordinates, plus a separate native signature-verification marker. Versions 2–11 remain readable; missing authentication or source proof never implies mint readiness. `poll_cctp_attestation` is read-only, guarded by wallet generation and unchanged operation snapshots, and performs one bounded polling tick with persisted backoff. CCTP mint execution and the Transfer USDC screen are available for EVM destinations; Solana native execution is gated off until a funded integration proves the route, and live testnet proof is still outstanding.

Swap execution first requests exact-input Relay protocol-v2 routes on the eight implemented EVM networks. Native/ERC-20 inputs and ERC-20 destinations are supported; native destinations and non-EVM routes fail explicitly in this EVM solver executor. The provider may choose V2, V3, V4, or other liquidity; the wallet does not claim that every pool version has been exercised with funds. `tx/relay.rs` recomputes the EIP-712 order ID using the pinned v1 schema and builds the deposit locally, binding network, amount, tokens, recipient, minimum, and source refund ownership to a pinned solver/depository. Foreign deposit addresses, extra signature steps, external destination calls, and unsupported order shapes are rejected. `VAULTFORGE_RELAY_API_KEY` is a Rust-only process configuration, never a frontend or persisted wallet secret. Source deposit finality keeps a routed operation pending. Completion requires a Relay status bound to the saved source hash and a canonical successful destination receipt proving net ERC-20 transfers of at least the minimum to the wallet, plus destination finality. Service failure/refund labels do not prove returned funds. Storage version 14 retains optional `SwapData.relay` terms and destination evidence; missing fields remain absent for older wallets.

All eight EVM networks have a direct same-chain ERC-20 single-V3-pool path using pinned SwapRouter02/QuoterV2 deployments. Eligible requests quote Relay, direct Uniswap, and LI.FI concurrently, retain successful providers independently, and rank exact integer output amounts without subtracting source gas. The frontend defaults to the highest output, permits any available choice, and retains an explicit provider preference across refresh while available. Cross-chain EVM requests use Relay only; same-chain native inputs may also use LI.FI. The direct Uniswap path requires no API key. Signing always uses the selected backend quote ID and never switches provider. It builds and verifies canonical deadline-wrapped calls and re-quotes the same pool before signing/submission. Both paths use backend quote IDs, encrypted signed approval/send attempts persisted before broadcast, generation/snapshot checks, explicit fee consent, and nonce recovery. Only definitely unsent attempts may be superseded; a finalized routed deposit cannot be cancelled or prepared again. EVM swap approvals advance after a successful canonical included receipt, with live allowance rechecked before signing. Their consumed nonces are restored per nonce rather than blocking the account; other unresolved operations retain their account protection. Finality continues polling independently, and `UniswapOperationView.can_prepare` controls readiness separately from `needs_sync`. A confirmed approval also permits cancellation of the unexecuted swap intent without erasing the approval receipt. Read-only route/quote refresh runs every 15 seconds while editing and can be triggered manually; it never signs or submits. The preview renders only disclosed stages/fees and uses local unknown-icon fallbacks. A funded end-to-end integration remains outstanding.

LI.FI now participates in same-chain EVM quote comparisons alongside Relay and direct Uniswap V3. Its executor accepts validated GenericSwapFacetV3 single/multiple-swap calls, including native inputs or outputs; unsupported facets and bridges return an unavailable-route error. Solana Mainnet same-chain swaps are enabled through Relay and LI.FI for SOL and classic SPL assets, using validated Jupiter exact-input instructions. Solana cross-chain routes, Token-2022, other spending programs, extra signers, and unsupported transaction versions remain unavailable. Available providers are ranked by exact output, and a failed provider does not remove working alternatives. Solana swaps use native blockhashes, signatures, fee estimation, account-rent reserves, simulation, broadcast, and commitment tracking. Signed bytes are encrypted before broadcast; uncertain submissions reconcile by signature without automatic signing or resending. Expiry releases protection only after finalized-height and fresh history checks. New saves use storage version 16, retaining optional provider transactions and Jupiter terms in `SwapData.api`; versions 2–15 remain readable. `swap_routes.rs` owns additional provider terms and quote verification; `swap_solana.rs` owns native execution, while `tx/swap_solana.rs` validates instructions. `VAULTFORGE_LIFI_API_KEY` is optional Rust-only process configuration. Solana instructions accept at most 25 bps of input fees to the pinned LI.FI collector, bind the remainder to Jupiter, and reject unreviewed extra transfers. Read-only guards have no signer or writable-account access; account creation retains exact owners and reviewed rent.

Direct Jupiter uses `/order` + `/execute` for reviewed, wallet-paid Metis v0 orders. Optional `SwapData.api.jupiter` terms retain the original order, message hash, blockhash expiry height and review deadline. The wallet is the sole signer/payer, making its first signature the final transaction ID before execution. The original message/blockhash is never refreshed; submission uncertainty is persisted before HTTP, and an attempted order is never automatically re-executed. Both unlock and UI recovery require chain transaction/message and minimum-delivery evidence before successful completion. Expiry protection additionally requires an invalid finalized blockhash and fresh missing-signature history. Declared platform fees are bound to collection instructions and the quoted mint/rate/amount. Explicit SOL/SPL fee transfers and embedded `route`/`shared_accounts_route` platform fees preserve total input and net minimum output; token fee collector mints are checked on-chain before signing and submission. Only a validation copy has fees removed for the shared trade checks; the original message is signed and executed. Net quoted output is never reduced a second time. The frontend platform-fee display uses the original input base units, input decimals, input asset USD price and declared basis-point rate; it shows a percentage and approximate USD amount in route choices, quote details and active operation review. This valuation is display-only and remains in USD regardless of wallet currency. RFQ/extra signers, sponsorship, other engines, unreviewed fee layouts/programs and v1 messages remain unavailable. `VAULTFORGE_JUPITER_API_KEY` is Rust-only process configuration. Funded direct Jupiter execution remains unverified.

Ordinary send status checks capture the wallet generation, activity ID, and current observation before querying providers, then recheck identity and the observation snapshot before committing. Provider polling resolves a durable operation (send or CCTP leg) by network and hash first, then the legacy activity cache, and updates the matching attempt or entry. Optional `Activity.tx_status` and `TransactionAttempt.tx_status` retain typed observations in encrypted storage; missing legacy fields default to absent and must not imply finality. Changed block evidence is saved even if the cached label is unchanged. Failed saves restore both the previous label and observation. Finalized evidence is not downgraded by stale lookups. `TxStatus` fields and its execution/finality enum values serialize as snake_case; older PascalCase enum values remain readable. The status command returns `TxStatus | null`, mirrored by `walletApi.ts`. A successful missing-receipt lookup clears non-final send evidence to pending; RPC errors preserve the last observation and do not prove reorganization or failure.

Implemented providers expose `fetch_*_tx_observation` reads returning `TxStatus`, with shared construction, optional metadata validation, legacy-label conversion, and provider routing. EVM finality requires a finalized RPC head covering the receipt and a matching canonical receipt block; unavailable finalized tags fail closed to inclusion. EVM JSON-RPC reads try the configured endpoint first, then verified same-network public fallbacks, returning the first success; networks without a verified spare use the configured endpoint only. Bitcoin checks best-chain membership and counts confirmations from the provider tip; six confirmations meet the application's settlement policy, not an irreversible consensus guarantee. Tron finality comes from a matching `/walletsolidity` receipt, which supersedes non-solidified evidence; its omitted protobuf success default is accepted only for complete identified receipts. Solana uses the explicit commitment status and retains slots separately from block heights; validated XRP Ledger results are final. Missing metadata remains absent, and no confirmation count is invented.

Operation records retain `required_source_finality`, defaulting to finalized for new and legacy records. Rust route builders must choose any less restrictive requirement explicitly; no UI or automatic fallback may weaken it. Mint entry and completion require successful source-burn evidence meeting that recorded requirement, independently of attestation availability and finalized mint execution. Activity distinguishes waiting for source finality from waiting for attestation and attestation availability. Attested-message binding, destination-specific authentication, EVM mint execution with replay recovery, and the Transfer USDC screen are implemented; Solana native execution is gated off pending a funded integration proof, and live testnet proof is still outstanding. Frontend polling continues for non-final evidence and legacy sends lacking finality evidence, excludes operation-driven CCTP and swap activity, and rejects stale identity/observation snapshots. Session merging always uses backend evidence rather than preserving frontend terminal overrides.

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

Transaction-status support is declared per network by `transactionStatusProvider` in the shared `src/networks.json` registry (schema version 2); `null` means unavailable. The TypeScript normalizer validates the provider family and required endpoint before Cargo generates the Rust registry. Status lookup, operation recovery, frontend polling, and legacy status adapters use that declaration. Registry membership or network `kind` alone does not imply status support. Observation providers receive the selected `NetworkConfig` and use its endpoint; encrypted wallet storage versions are independent of this registry schema.

Chain-specific requirements:

- EVM: chain ID, RPC URL, native currency, token contracts, pending nonce, gas estimation, EIP-1559 where available, raw broadcast, and receipts. Current sends use `eth_feeHistory` with `eth_gasPrice` fallback but have no nonce reservation manager or user priority-fee policy.
- Bitcoin: UTXO discovery, fee rates, coin selection/PSBT or transaction construction, signing, broadcast, and confirmations.
- Solana: recent blockhash, native/SPL balances, associated token accounts, recipient ATA rent, construction, signing, send, and confirmation. Current token support is classic SPL, not Token-2022.
- XRP Ledger: classic-address validation, BIP44 account derivation, drop-denominated native balance, live reserve/fee/sequence reads, canonical `Payment` encoding and signing, raw submit, and validated transaction status. Support destination tags and bounded memos; do not treat issued currencies, MPTs, NFTs, X-addresses, or path payments as native XRP transfers.
- Filecoin, Injective, Tron, and Zcash: use their native derivation, address, fee, signing, and broadcast rules.

Represent amounts in base units: wei/token units, satoshis, lamports/SPL units, zatoshis, attoFIL, and chain denomination units. Convert to decimal display strings only at the UI edge.

Model transaction stages explicitly: draft, estimated, approved, signed, broadcast, pending, confirmed, and failed/dropped/replaced. Records must retain enough chain-specific information to audit what was signed and sent, including network, accounts, asset identifier, base-unit amount, fees, nonce/sequence/blockhash/UTXOs, digest, signatures, broadcast ID, and confirmation status.

After broadcast, refresh provider balances and track pending state separately. Never create fake hashes, signatures, confirmations, fees, or local-only send/swap accounting in production paths.

Keep any simulator-only behavior isolated to explicit test fixtures.

## Verification

All Bun unit-test commands use `bun test --parallel`, which runs test files in parallel.

When adding tests, keep them alphabetically ordered: Rust test functions by function name within each mirrored test file, and TypeScript test blocks by title within their containing scope. Run `bun run lint:test-order` to check the order or `bun run lint:test-order:fix` to sort them.

With Lefthook installed, local Git operations run these unit tests:

- Pre-commit: `bun test --parallel` runs the TypeScript tests. Rust unit tests do not run at commit time; the hook runs `cargo check` and `cargo fmt --check`.
- Pre-push: `bun test --parallel` runs the TypeScript tests again, and `cargo test --manifest-path src-tauri/Cargo.toml` runs the Rust tests. The hook also runs Rust Analyzer analysis and a dependency audit.
- GitHub Actions: the Quality workflow runs `bun test --parallel` and `cargo test --manifest-path src-tauri/Cargo.toml` for pull requests and pushes to `main`.

Local hooks run these checks for each commit or push, even if an earlier check fails, and block the operation when any check fails. Tests run against the current checkout, including unstaged changes.

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
