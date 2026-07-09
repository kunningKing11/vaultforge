# Roadmap for this project

This is a roadmap of features that will be built for this project. They are in loose expected implementation order, so things might change regarding the timeline.

---

## Main priority list/short term

The below items are the priority for this project and an item will probably get checked off approximately biweekly.

- [ ] Full support for transfers on the below chains:
  - [x] BTC basic transfers
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
    - [ ] EIP-1559 fee estimation via `eth_feeHistory`
  - [x] SOL basic transfers
    - [x] native SOL transfers
    - [x] SPL token transfers
    - [x] recipient ATA rent estimation
    - [ ] Token-2022 support
    - [ ] pre-sign live balance refresh
  - [ ] TRX
- [ ] Slider for network priority fee when sending
- [ ] Actual cross-chain swaps (non-simulated)
  - [ ] via NEAR Intents
  - [ ] sponsored swaps (using paymaster/ERC-4337 or smart contract deposits)
- [x] Public GitHub repository
- [ ] Live pre-sign balance refresh
  - [ ] EVM native and ERC-20
  - [ ] SOL native and SPL
  - [ ] reconcile stale cached balances after broadcast
- [ ] Proper fee estimation engine + preflight checks
  - [ ] EVM: `eth_feeHistory` / priority fee strategy
  - [ ] EVM: simulate contract calls before signing where possible
  - [ ] SOL: simulate transaction before signing/broadcast
  - [ ] BTC: better fee target selection
  - [ ] show all native fee/rent/funding debits clearly in UI
- [ ] Built-in nonce manager for EVM and EVM-like chains to avoid reliance on potentially inaccurate 3rd-party data and avoiding transaction failures in certain cases - this will need to sync to RPC on sending transactions. The wallet should be trusted if there are any pending transactions but this could be improved by tracking any pending transactions.
- [ ] Improved sync functionality to sync as much as possible of any stored wallet data.
- [ ] Full support for more chains, including (but not limited to):
  - [ ] Ripple (XRP)
  - [ ] edgeX (EDGE) - support for trading derivatives
  - [ ] Hyperliquid (HYPE) - support for trading perps (perpetuals)
  - [ ] Injective (INJ)
  - [ ] Algorand (ALGO)
  - [ ] Zcash (ZEC) - support transparent AND shielded addresses
  - [ ] Monero (XMR) - alternative to Zcash
- [ ] Fuzzers to catch problems regarding the internal wallet logic to account for the multitude of possible scenarios and catch bugs before they appear

---

## Stretch goals/long term

These should be expected to be worked on only occasionally and as the main list gets smaller since they will probably take a significant amount of time compared to the main list tasks while not moving the needle as much. This is not to say that they are not important or useful; just that there is more sense in working on the other items first.

- [ ] NFT support on relevant chains:
  - [ ] EVM
  - [ ] SOL
  - [ ] TRX
  - [ ] ALGO
- [ ] Support for the Open Wallet Standard
- [ ] Native DeFi support:
  [ ] Native Aerodrome Finance LPing (liquidity providing)
  [ ] Native Aave lending
- [ ] (hopefully) Filecoin storage integration so you can easily store, download, and access files stored on Filecoin
- [ ] (hopefully) Integration with Tor for anonymity when sending to RPC
