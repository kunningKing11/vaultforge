---
title: VaultForge Wallet
description: A local-first crypto wallet desktop app built with React, Coinbase Design System, Rust, and Tauri.
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

A local-first crypto wallet desktop app built with a React frontend, Coinbase Design System, and a Rust backend through Tauri.

## Features

- Create, import, lock, and unlock a wallet session
- Optional BIP-39 passphrase when creating or importing a wallet
- Portfolio dashboard with token balances, allocation, weighted 24h change, optional Unicode crypto symbols, and USD, EUR, GBP, or JPY valuation selectable during setup or in settings
- Activity transaction hashes and sender/recipient addresses link to their chain explorers. Sidebar addresses are clickable while retaining separate copy buttons; the shared EVM address opens on Ethereum's explorer.
- Portfolio and Assets share a “Group across chains” toggle remembered in the wallet file; new wallets start with grouping enabled. Known registry assets with the same price ID combine into one display card; hovering or keyboard-focusing it shows per-chain balances and fiat values only when at least two chains hold a nonzero balance. Zero-balance chains are omitted. Grouping does not change stored balances or send selections, and unknown token contracts remain separate.
- The Assets view can filter holdings by one or more chains and paginate the filtered, grouped cards with 12, 24, or 48 cards per page; “All chains” restores the complete list, and each chain option shows its icon.
- Save the wallet name, display currency, auto-lock timeout, crypto-symbol preference, hidden-asset cutoff, and chain-grouping preference together from settings
- Sign and review chain-specific transactions before broadcasting them
- Basic real transfer paths for BTC, EVM native/ERC-20, Solana native/classic SPL and reviewed Token-2022 assets, Tron native and configured TRC-20 assets, and XRP Ledger native XRP
- Review provider-derived fees, total debit, selected fiat value, and post-send balance estimates
- EVM native and ERC-20 sends recheck live chain balances before signing
- Portfolio discovery reads both classic SPL and Token-2022 accounts and verifies their mint programs and decimals. Incomplete token reads retain cached balances. Token-2022 sends support reviewed passive extensions and transfer-fee tokens. Same-chain swaps support reviewed Jupiter v2 exact-input routes through Jupiter, Relay and LI.FI; route availability depends on the returned instructions and fee layout. Optional Solana capability metadata is a cached display hint; signing always rechecks the chain.
- Solana token sends validate live mint/account programs and extensions, prefer the sender’s ATA, and use checked transfers. Token-2022 transfer fees are calculated per source account from the active epoch schedule; the entered amount is the total token debit. Review shows recipient net amount, token fees, SOL fee and account rent separately. Signing and submission recheck live terms; changed terms require a fresh review. Signed bytes and review terms are encrypted before broadcast, and uncertain token submissions reconcile by signature without resubmission. Custom transfer hooks, confidential transfers and other unreviewed extensions remain unavailable. Funded Token-2022 send validation remains outstanding.
- XRP Ledger native sends use live account balance, reserve, fee, and sequence data; they support classic addresses, destination tags, and short on-ledger memos
- Encrypted local wallet persistence using the app data directory
- Activity details with transaction hashes, signatures, payload hashes, and copy actions
- Ordinary send status updates are saved in encrypted wallet storage and survive reloads
- Security center with storage status, signing status, and local wallet clearing
- Wallet-password confirmation and strength feedback for encrypted wallet setup
- One-time risk acknowledgment before wallet setup or unlock, with the full MIT License available in-app
- Send, receive, swap, assets, activity, and settings screens
- Same-chain and cross-chain swap routes through Relay on supported EVM networks, compared with direct single-pool Uniswap V3 for same-chain ERC-20 pairs. LI.FI now participates in same-chain EVM quote comparisons alongside Relay and direct Uniswap V3. Its executor accepts validated GenericSwapFacetV3 single/multiple-swap calls, including native inputs or outputs; unsupported facets and bridges return an unavailable-route error. Solana Mainnet same-chain swaps support SOL, classic SPL and reviewed Token-2022 assets through Jupiter, Relay and LI.FI. Token-2022 routes require validated `route_v2` or `shared_accounts_route_v2` exact-input instructions, including the correct token programs, mint accounts, ATAs and extended account rent. Solana cross-chain routes, custom transfer hooks, confidential transfers, other spending programs, extra signers and unsupported transaction versions remain unavailable. Available providers are ranked by exact output, and a failed provider does not remove working alternatives. Quotes retain slippage limits, explicit fee review, and encrypted restart recovery. Funded end-to-end validation remains outstanding.
- Rust-backed Tauri commands for wallet state, validation, transaction signing, encrypted storage, provider-backed reads, broadcast, and status checks
- Searchable source/destination network and token selection shared by Send and Swap; Max fills the selected token balance at full precision, while native Max uses live route estimates to reserve gas. Approximate values in the wallet's selected currency appear below the spend amount, expected/minimum output, and network fee; unavailable prices are labelled explicitly. The form includes rounded slippage presets and Custom, route icons with chain badges, disclosed fees, automatic 15-second quote refresh, and manual refresh. The separate USDC transfer flow retains Circle CCTP.
- Responsive React and Coinbase Design System UI with desktop sidebar

A successful on-chain approval enables the next swap transaction immediately; it does not wait for Base's full finality. The wallet rechecks spending permission, reviews the swap fee separately, and continues tracking finality in the background.

Relay's authenticated quote service accepts `VAULTFORGE_RELAY_API_KEY` from the Rust process environment. Set it when launching the app (for development, `VAULTFORGE_RELAY_API_KEY=your-key bun run tauri dev`). The key is never included in frontend state or wallet storage. An authentication error explains when routing needs configuration. Route previews show the stages disclosed by the provider; undisclosed pool fees are labelled unavailable, not guessed. Source finality alone never completes a routed swap: the wallet checks the destination's canonical receipt, ERC-20 transfer amount, recipient, and finality. Relay-reported failures/refunds are labelled as service reports; they do not prove a refund balance.

LI.FI optionally accepts `VAULTFORGE_LIFI_API_KEY` from the Rust process environment for its authenticated quota; unauthenticated requests are rate limited. Like the Relay key, it never enters frontend state or wallet storage. Solana swaps use native blockhashes, signatures, fee estimation, account-rent reserves, simulation, broadcast, and commitment tracking. Signed bytes are encrypted before broadcast; uncertain submissions reconcile by signature without automatic signing or resending. Expiry releases protection only after finalized-height, invalid finalized/confirmed blockhash and fresh missing-signature history checks. Every Solana swap provider uses the same transaction/message/signature and net-delivery verification during UI sync and unlock recovery. Missing legacy delivery evidence triggers verification rather than implying completion; existing finalized chain observations are retained. Token balance refresh reads both token programs and preserves cached balances on incomplete reads. New saves use storage version 17, retaining optional provider transactions, Jupiter order terms, verified token/fee snapshots and delivery evidence in `SwapData.api`, plus optional Solana token-send review terms on operations; versions 2–16 remain readable.

Direct Jupiter `/order` + `/execute` participates in same-chain Solana comparisons for reviewed wallet-paid Metis v0 orders. The original message and blockhash are preserved; signed bytes and request terms are encrypted before execution. Recovery uses the saved transaction signature without automatic re-execution, and completion requires chain transaction and minimum-delivery evidence. Declared Jupiter platform fees are validated against the collection instructions, fee mint and total input or net minimum output. Token fee collectors are checked on-chain before preparation and submission; fees already included in the quote are not deducted again. Reviewed explicit Token-2022 platform-fee transfers bind their gross debit, fee schedule, mint and decimals. Embedded Jupiter v2 platform-fee collector layouts remain unavailable until separately reviewed. Transfer taxes are disclosed separately from provider fees in route choices, quote details and active review; the minimum received is the net amount credited to the wallet. Changed token programs, extensions or active fee schedules require a fresh quote. Route choices, the selected quote and active swap review show the platform fee as a percentage and an approximate USD value calculated from the original input amount and its USD price. Missing prices are labelled unavailable. RFQ/extra signers, sponsorship, other engines and unreviewed instruction layouts remain unavailable. Set `VAULTFORGE_JUPITER_API_KEY` in the Rust process environment when launching the app. It never enters wallet storage or frontend state. Funded Jupiter integration remains unverified.

### Coming soon

- Tron swaps (the current LI.FI TVM route cannot yet pass the wallet's transaction validation and signing flow)
- Zcash (ZEC) support with support for shielded addresses planned

For a full list of upcoming features (there are quite a few!), click [here](https://github.com/kunningKing11/vaultforge/blob/main/ROADMAP.md).

## Supported chains

- Bitcoin Mainnet (BTC)
- EVM: Ethereum Mainnet (ETH), Arbitrum One, Avalanche C-Chain (AVAX), Base, BNB Smart Chain (BNB), Monad (MON), Optimism, and Polygon (POL)
- Solana Mainnet (SOL)
- Tron Mainnet (TRX, USDT, USD1, TRUMP, and the legacy USDC TRC-20 contract, with balance and transfer support)
- XRP Ledger Mainnet (XRP; native transfers only)

## Token registry

The bundled tokens are summarised below; exact contracts, mints and decimals are in [the registry](src/networks.json). Native coins are listed under Supported chains above.

| Chain / token group                     | Tokens                                                                    |
| :-------------------------------------- | :------------------------------------------------------------------------ |
| Ethereum, Avalanche C-Chain, Polygon    | AAVE, LINK, UNI (UNI.e on Avalanche), USDC, USDT; Polygon also has USDC.e |
| Ethereum — additional                   | H (Humanity), PEPE, SHIB                                                  |
| Arbitrum One, Base, Optimism            | AAVE, LINK, UNI, USDC                                                     |
| BNB Chain                               | AAVE, LINK, UNI                                                           |
| Monad                                   | LINK, USDC                                                                |
| Solana — Jupiter                        | JUP, JLP, JupSOL, JupUSD                                                  |
| Solana — Jupiter Lend receipts          | JUICED, jlUSDC, jlUSDT, jlWSOL, jlEURC, jlUSDS, jlUSDG, jleUSDG, jlePYUSD |
| Solana — stablecoins and wrapped assets | USDC, USDT, USDS, USDe, USD1, EURC, cbBTC, WSOL                           |
| Solana — staking                        | JitoSOL, mSOL, BNSOL                                                      |
| Solana — DeFi / DePIN                   | AAVE, LINK, UNI, RAY, ORCA, MET, JTO, PYTH, KMNO, DRIFT, W, HNT, RENDER   |
| Solana — Pump                           | PUMP                                                                      |
| Solana — memecoins                      | BONK, WIF, PEPE, PENGU, FARTCOIN, TRUMP, MELANIA, USELESS                 |
| Tron                                    | USDT, USD1, TRUMP and legacy USDC TRC-20 balances and transfers           |

Bridged, CCIP and ERC-677 variants remain separate assets by contract or mint, even when their tickers match.

<details>
<summary>Registry sources and verification</summary>

- EVM and bridged tokens: [Uniswap token list](https://tokens.uniswap.org), [UNI documentation](https://developers.uniswap.org/docs/ecosystem/governance/uni), [Chainlink documentation](https://docs.chain.link/resources/link-token-contracts), [Avalanche bridge registry](https://github.com/ava-labs/avalanche-bridge-resources/blob/main/avalanche_contract_address.json), and [Wormhole token registry](https://github.com/wormhole-foundation/wormhole-token-list/blob/main/content/by_dest.csv). Solana Wormhole mints use eight decimals; the separate Chainlink LINK mint uses nine.
- Humanity uses the [migrated Ethereum H contract](https://announcements.bybit.com/en/article/bybit-will-support-the-humanity-h-contract-migration-bltb4caa60378a0ed28/), with its name, symbol and decimals verified on-chain.
- Ethereum memecoins: [PEPE](https://www.pepe.vip) and [SHIB](https://shib.io/tokens/shib), with contracts and decimals verified on-chain. Pepe publishes raster artwork only, so its bundled SVG is a pinned [Web3 Icons](https://github.com/0xa3k5/web3icons) vector recreation.
- [Solana PEPE](https://jup.ag/tokens/PEPEqnuuCDbBC89p1u9vpnP1KQ2oj1xTcQBsjt9X55m) uses the Jupiter-verified classic SPL mint with four on-chain decimals and per-mint pricing; it retains a separate asset identity from Ethereum PEPE.
- Solana: [Jupiter's verified token metadata](https://developers.jup.ag/docs/tokens/token-information) and Lend asset catalogues for the [main](https://api.jup.ag/lend/v1/earn/tokens) and [Ethena](https://api.jup.ag/lend/v1/earn/tokens?market=ethena) markets, checked against finalized mainnet mint accounts. The Token-2022 receipts jlUSDG, jleUSDG and jlePYUSD retain separate mint identities and per-mint pricing; their icons use Jupiter’s official Lend SVG. [PUMP](https://pump.fun/pump-token) uses its official Token-2022 mint and bundled issuer SVG. CoinGecko IDs are matched by exact Solana mint using its [asset registry](https://api.coingecko.com/api/v3/coins/list?include_platform=true).

</details>

### Supported features

The table distinguishes implemented backend paths from network entries that are currently address or validation scaffolding. “No” means the feature is not implemented; it does not imply that the underlying chain lacks it.

| Chain             | Native transfer            | Token transfer          | NFTs / Filecoin storage / Shielded pools (whichever are applicable) |
| :---------------- | :------------------------- | :---------------------- | ------------------------------------------------------------------: |
| Arbitrum One      | Yes                        | ERC-20                  |                                                                  No |
| Avalanche C-Chain | Yes                        | ERC-20                  |                                                                  No |
| Base              | Yes                        | ERC-20                  |                                                                  No |
| BNB Smart Chain   | Yes                        | ERC-20                  |                                                                  No |
| Bitcoin           | Yes (BIP84 P2WPKH account) | No                      |                                                                  No |
| Ethereum Mainnet  | Yes                        | ERC-20                  |                                                                  No |
| Filecoin          | No                         | No                      |                                                                  No |
| Injective         | No                         | No                      |                                                                  No |
| Monad             | Yes                        | ERC-20                  |                                                                  No |
| Optimism          | Yes                        | ERC-20                  |                                                                  No |
| Polygon           | Yes                        | ERC-20                  |                                                                  No |
| Solana            | Yes                        | SPL / Token-2022        |                                                                  No |
| Tron              | Yes                        | Yes (configured TRC-20) |      No (wallet transaction validator supports EVM and Solana only) |
| XRP Ledger        | Yes (classic addresses)    | No                      |                                                                  No |
| Zcash             | No                         | No                      |                                                                  No |

## Development

### Chain and coin icons

`src/icon-manifest.json` curates official project artwork, issuer source pages and brand kits, local filenames, SHA-256 checksums, chain mappings, and token mappings by network and contract/mint. Artwork is bundled in `public/icons/` for offline display. Official SVGs are preferred; original issuer PNG and JPEG icons are also accepted when a reviewed SVG is unavailable. Raster icons retain their published bytes; embedded raster content inside SVGs is rejected. Some chain assets are official website icons or wordmarks rather than standalone symbols; project branding terms still apply.

Run `bun run icons:download` to download the pinned assets (Bun and `unzip` are required for Circle's USDC archive), or `bun run icons:check` to verify existing files without network access. Changed upstream files fail checksum validation; review them before updating the manifest. The downloader rejects active SVG content; USDT uses Tether’s official green SVG logo mark. Explicit manifest transforms extract the original Bitcoin and BNB symbol paths from wordmarks and normalize Injective’s inline SVG attributes, apply its official `#4D3DFF` brand purple, and remove the OP token preview background and padding. Render uses its original symbol paths with the requested white fill on a red circle. Original symbol geometry is preserved.

For published wordmarks, `svgViewBox` records a reviewed viewport crop that displays the original symbol without changing its paths. Icons without an available reviewed issuer source retain the question-mark fallback. Mint-published artwork records its metadata URI alongside the issuer page and download URL in the manifest. The TRUMP icon comes from [the official Trump token site](https://gettrumpmemes.com/images/trump-favicon-256.png). JLP, JupSOL and JupUSD use Jupiter-hosted token icons; BNSOL uses the logo Jupiter serves for Binance’s exact verified mint. Every Jupiter Lend receipt, including jlUSDS and JUICED, uses its own Jupiter interface token image selected by mint, rather than borrowing the underlying asset or generic Jupiter icon.

Pre-commit and `bun run lint` enforce alphabetical icon IDs and network-map keys. Token mappings are grouped by icon, then sorted by network ID and contract. Run `bun run lint:icon-order:fix` to sort the manifest.

Use `assetIconPath(asset)` and `chainIconPath(network)` from `src/assetIcons.ts` in image elements. Unknown identities return the bundled question-mark icon. Native ETH on Base, Optimism, and Arbitrum uses the ETH coin icon while the chain helper returns the corresponding chain badge. Dashboard and Assets share one `AssetCard` that uses these helpers for a coin icon and a small chain badge; missing or failed images use the question-mark fallback. Adding an icon does not declare transaction support. The question-mark artwork comes from Feather under the MIT license, included at `public/icons/fallback/LICENSE.txt`.

### Setup

Use the Bun version specified by `packageManager` in [package.json](package.json); CI and release builds use the same version. Bun performs type checking, and the installed TypeScript dependency supports editors.

**Install dependencies:**

```bash
bun install
```

**Run the web frontend:**

```bash
bun run dev
```

**Run the Tauri desktop app:**

```bash
bunx tauri dev
```

**Build the frontend:**

```bash
bun run build
```

Build the desktop bundle:

```bash
bun run prepare-tauri
bunx tauri build
```

`prepare-tauri` records SHA-256 hashes for `src-tauri/icons/icon.svg` and every generated desktop, Android, and iOS icon in the ignored local `src-tauri/icons/icon-hashes.json` cache. It skips `tauri icon` when the source, manifest, and all generated products still match, and regenerates the complete set when any file is missing or changed.

The manually dispatched Release workflow builds signed updater packages using the existing `TAURI_SIGNING_PRIVATE_KEY` and optional password secrets. `scripts/prepare-release-assets.mts` collects all six architecture builds, retains updater signatures, and generates `latest.json` with package URLs and signature contents. Linux DEB/RPM/AppImage installs have separate entries; macOS uses `.app.tar.gz` archives and Windows uses NSIS installers. Missing or ambiguous updater packages and missing/empty signatures block publication. The Settings updater endpoint becomes usable once a release containing these files is published; local workflow changes do not repair existing GitHub releases.

**Check and fix frontend quality:**

```bash
bun run check
bun test --parallel
bun run lint:fix
bun run format
```

`dev` and `build` run the non-mutating Oxlint, Oxfmt, and Bun type checks before starting Vite. Run `./lint.sh` for the same checks or `./lint.sh --fix` to apply safe lint fixes and formatting first.

Every commit checks 72-character commit-message lines, syncs Cargo's version from `package.json`, runs dependency installs, and stages the package and Cargo manifests/lockfiles. It then runs Oxlint, Oxfmt, Bun type checking, the TypeScript unit tests, `cargo fmt --check`, and `cargo check`. Before every push, Lefthook runs Bun's dependency audit; the push validation runs TypeScript tests, verifies Cargo manifests match the lockfile with `cargo metadata --locked`, audits the resolved versions with `cargo audit --file src-tauri/Cargo.lock`, and runs the full Rust test suite. Each hook runs every command in its group before printing a pass/fail summary; a failed command blocks its commit or push.

You can run the same checks manually with `bun run hooks:check -- commit` or `bun run hooks:check -- push`. After cloning or installing dependencies, `bun install` installs the Git hooks through the `prepare` script; run `bun run prepare` in an already-installed checkout.

`src/networks.json` is the source of truth for network identities, provider URLs, native assets, configured ERC-20 assets, decimals, and CoinGecko identifiers.

### Project Structure

- `src/main.tsx` mounts the React frontend and Coinbase Design System providers/styles.
- `src/react/App.tsx` contains the React shell, lifecycle screens, wallet views, Tauri action orchestration, and UI effects.
- `src/react/model.ts` owns typed frontend state, session normalization, and pure derived wallet reads.
- `src/react/styles.css` provides responsive layout rules using Coinbase Design System tokens; legacy SVGs remain in `src/assets/icons/`.
- `src/` also contains the Tauri API boundary, amount formatting, recovery-phrase checks, network registry, scrollbar behavior, and shared types.
- `scripts/generate-network-registry.ts` validates and normalizes the network registry for the Rust build.
- `tests/` mirrors the frontend modules covered by Bun unit tests.
- `src-tauri/src/lib.rs` wires the shared Tauri app, managed state, and command handlers; `src-tauri/src/main.rs` is the thin desktop launcher.
- `src-tauri/src/commands/` contains Tauri command handlers split by domain: wallet lifecycle, transactions, and market data.
- `src-tauri/src/providers/` contains chain RPC/provider code for balances, fee data, broadcast, and transaction status.
- `src-tauri/src/registry.rs` exposes the normalized network, provider, native-asset, token, and pricing configuration embedded by `build.rs`.
- `src-tauri/src/tests/` mirrors the Rust module structure for unit tests while keeping test code out of implementation files.
- `src-tauri/src/tx/` contains chain-specific transaction construction, encoding, and signing code.
- `src-tauri/src/cctp.rs` validates CCTP V2 Standard USDC routes among all supported EVM chains, plus a private Ethereum/Arbitrum Sepolia counterpart. `src-tauri/src/tx/cctp.rs` builds contract calls; `src-tauri/src/providers/cctp.rs` performs EVM balance, allowance, fee, and gas preparation. Solana source execution preparation is explicitly unavailable; these declarations and tests do not establish live transfer support or enable testnet wallet networks.
- `src-tauri/src/cctp_attestation.rs` binds V2 burn logs to attested messages and saves polling deadlines; `src-tauri/src/providers/cctp_attestation.rs` fetches Circle messages and verifies EVM destinations through contract simulation. `src-tauri/src/providers/cctp_solana.rs` validates native Solana transaction/MessageSent-account evidence and recovers attestation signatures against the destination program's enabled on-chain attesters and threshold; this is distinct from mint simulation. The read-only `poll_cctp_attestation` command resumes polling by operation ID. Mint execution and the CCTP UI remain unavailable.
- `src-tauri/src/operations.rs` tracks encrypted operation records, checks transitions, reconciles transaction statuses on unlock, and derives activity. CCTP mint entry requires bound attestation data, destination-specific authentication, and matching source-finality evidence; execution still requires its own fresh preflight.
- `src-tauri/src/assets.rs` contains shared asset-cache helpers used by provider refresh paths.
- `src-tauri/src/activity.rs`, `assets.rs`, `derivation.rs`, `dto.rs`, `state.rs`, `storage.rs`, and `validation.rs` contain the backend domain support code used by commands:
  - `activity.rs`:
  - `assets.rs`:
  - `derivation.rs`: key derivation
  - `dto.rs`: serialization
  - `state.rs`:
  - `storage.rs`:
  - `validation.rs`:

## License

VaultForge is licensed under the [MIT License](LICENSE). It is provided "as is," without warranty; see the license for the full terms.
