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
- Activity transaction hashes and sender/recipient addresses link to their chain explorers
- Sign and review chain-specific transactions before broadcasting them
- Basic real transfer paths for BTC, EVM native/ERC-20, Solana native/classic SPL and Token-2022 assets, Tron native and configured TRC-20 assets, and XRP Ledger native XRP
- Review provider-derived fees, total debit, selected fiat value, and post-send balance estimates
- EVM, Solana, and Tron transactions recheck actual balances before signing
- Solana token sends validate live mint/account programs and extensions, prefer the sender’s ATA, and use checked transfers. Token-2022 transfer fees are calculated per source account from the active epoch schedule; the entered amount is the total token debit. Review shows recipient net amount, token fees, SOL fee and account rent separately. Custom transfer hooks, confidential transfers and other unreviewed extensions remain unavailable. Funded Token-2022 send validation is not yet available.
- XRP Ledger native sends use live account balance, reserve, fee, and sequence data; they support classic addresses, destination tags, and short on-ledger memos
- Encrypted local wallet storage using the app data directory with AES-GCM
- Activity details with transaction hashes, signatures, payload hashes, and copy actions
- Ordinary send status updates are saved in encrypted wallet storage and survive reloads
- Wallet-password confirmation and strength feedback for encrypted wallet setup
- One-time risk acknowledgment before wallet setup or unlock, with the full MIT License available in-app
- Send, receive, swap, assets, activity, and settings screens
- Same-chain and cross-chain (EVM) swap routes through Relay and LI.FI on supported EVM networks and Solana and direct single-pool Uniswap V3 support for same-chain ERC-20 pairs. Available providers are ranked by exact output.
- CCTP v2 support on EVM chains and Solana
- Responsive React and Coinbase Design System UI with desktop sidebar

Relay's authenticated quote service can take `VAULTFORGE_RELAY_API_KEY` from the Rust process environment for less rate limiting. Set it when launching the app (for development, `VAULTFORGE_RELAY_API_KEY=your-key bun run tauri dev`). The key is never included in frontend state or wallet storage. Similarly, LI.FI optionally uses `VAULTFORGE_LIFI_API_KEY` and Jupiter can use `VAULTFORGE_JUPITER_API_KEY`.

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

Bun applies a three-day minimum release age when resolving new direct and transitive npm dependencies. Existing `bun.lock` entries and lockfile changes produced by Dependabot are not rechecked against this policy. For an urgent, reviewed update, temporarily add its exact `name@version` to `minimumReleaseAgeExcludes` in `bunfig.toml`, then remove the exception when it is no longer needed.

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
bun fmt .
```

`dev` and `build` run the non-mutating Oxlint, Bun formatting, and Bun type checks before starting Vite. Run `./lint.sh` for the same checks or `./lint.sh --fix` to apply safe lint fixes and formatting first.

Formatter settings and the dependency release-age policy live in `bunfig.toml`.

`bun run lint` and the pre-commit hook also check that dependency names are alphabetized within each Cargo dependency table. Run `bun run lint:cargo-order` to check this separately.

Every commit checks 72-character commit-message lines, syncs Cargo's version from `package.json`, runs dependency installs, and stages the package and Cargo manifests/lockfiles. It then runs Oxlint, Bun formatting and type checks, the TypeScript unit tests, `cargo fmt --check`, and `cargo check`. Formatting checks staged files, and type checking uses a temporary snapshot of the staged project with the installed dependencies. Other checks use the working tree. Before every push, Lefthook runs Bun's dependency audit; the push validation runs TypeScript tests, verifies Cargo manifests match the lockfile with `cargo metadata --locked`, audits the resolved versions with `cargo audit --file src-tauri/Cargo.lock`, and runs the full Rust test suite. Each hook runs every command in its group before printing a pass/fail summary; a failed command blocks its commit or push.

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
