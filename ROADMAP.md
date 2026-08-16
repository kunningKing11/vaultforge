# Roadmap for this project

This is a roadmap of features that will be built for this project. They are in loose expected implementation order, so things might change regarding the timeline.

---

## Main priority list/short term

The below items are the priorities for this project and an item will probably get checked off approximately biweekly.

"[chain name] basic transfers" means support normal transfers (excluding NFTs) on the specified chain.

- [ ] Full support for transfers on the below chains:
  - [x] Bitcoin (BTC) basic transfers
    - [x] UTXO discovery
    - [x] fee-rate fetch
    - [x] coin selection
    - [x] signing
    - [x] broadcast
    - [x] status polling
    - [ ] multiple BTC address types / account scanning (e.g., not just `bc1q` addresses)
  - [x] EVM basic transfers
    - [x] native transfers
    - [x] ERC-20 transfers
    - [x] native gas balance checks
    - [x] token contract address tracking
    - [ ] pending nonce handling / local nonce manager
    - [x] EIP-1559 fee estimation via `eth_feeHistory`
  - [x] Solana (SOL) basic transfers
    - [x] native SOL transfers
    - [x] SPL token transfers
    - [x] recipient ATA rent estimation
    - [ ] Token-2022 support
    - [x] pre-sign live balance refresh
  - [ ] Tron (TRX) basic transfers
    - [x] native TRX transfers
    - [ ] Tron token transfers
  - [ ] Ripple (XRP) basic transfers
    - [ ] native XRP transfers
    - [ ] Ripple token transfers
  - [ ] Zcash (ZEC) basic transfers
    - [ ] transparent pool transfers
    - [ ] shielded pool transfers
      - [ ] Orchard
      - [ ] Ironwood
      - [ ] Sapling
    - [ ] cross-pool transfers
    - [ ] "migrate legacy pool notes" button to migrate to Ironwood
      - [ ] randomized TX submission to avoid inadvertently revealing shielded amount
- [ ] Slider for network priority fee when sending using priority fee percentiles
- [ ] Actual cross-chain swaps (non-simulated)
  - [ ] via NEAR Intents
  - [ ] via major DEXs
    - [ ] Jupiter
    - [ ] Aerodrome
    - [ ] Uniswap
    - [ ] Raydium
    - [ ] Meteora
  - [ ] gasless/sponsored swaps (using paymaster/ERC-4337 or smart contract deposits)
- [x] Public GitHub repository
- [ ] Live pre-sign balance refresh
  - [ ] EVM native and ERC-20
  - [x] SOL native and SPL
  - [ ] reconcile stale cached balances after broadcast
- [ ] Proper fee estimation engine + preflight checks
  - [ ] EVM: `eth_feeHistory` / priority fee strategy
  - [ ] EVM: simulate contract calls before signing where possible
  - [x] SOL: simulate signed transaction before broadcast
  - [ ] BTC: better fee target selection
  - [ ] show all native fee/rent/funding debits clearly in UI
- [ ] Built-in nonce manager for EVM and EVM-like chains to avoid reliance on potentially inaccurate 3rd-party data and avoiding transaction failures in certain cases - this will need to sync to RPC on sending transactions. The wallet should be trusted if there are any pending transactions but this could be improved by tracking any pending transactions.
- [ ] Improved sync functionality to sync as much as possible of any stored wallet data.
- [ ] Full support for more chains, including (but not limited to)
  - [ ] edgeX (EDGE) - support for trading derivatives
  - [ ] Hyperliquid (HYPE) - support for trading perps (perpetuals)
  - [ ] Injective (INJ)
  - [ ] Algorand (ALGO)
  - [ ] Monero (XMR) - alternative to Zcash
- [ ] Fuzzers to catch problems regarding the internal wallet logic to account for the multitude of possible scenarios and catch bugs before they appear

---

## Stretch goals/long term

These should be expected to be worked on only occasionally and as the main list gets smaller since they will probably take a significant amount of time compared to the main list tasks while not moving the needle as much. This is not to say that they are not important or useful; just that there is more sense in working on the other items first.

- [ ] NFT support on relevant chains
  - [ ] EVM
  - [ ] SOL
  - [ ] TRX
  - [ ] ALGO
- [ ] Support for the Open Wallet Standard
- [ ] Native DeFi support
  - [ ] Native Aerodrome Finance LPing (liquidity providing)
  - [ ] Native Aave lending
- [ ] (hopefully) Filecoin storage integration so you can easily store, download, and access files stored on Filecoin
- [ ] (hopefully) Integration with Tor for anonymity when sending to RPC
