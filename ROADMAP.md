# Roadmap for this project

This is a roadmap of features that will be built for this project. They are in loose implementation order, so things might change regarding the timeline.

---

1. [ ] Full support for transfers on the below chains:
  [x] BTC
  [x] EVM
  [ ] SOL w/ some sort of token account cost (rent) estimation
  [ ] TRX
2. [ ] Slider for network priority fee when sending
3. [ ] Support multiple BTC address types instead of just `bc1q`
4. [ ] Actual cross-chain swaps (non-simulated)
  [ ] via NEAR Intents
  [ ] sponsored swaps (using paymaster/ERC-4337 or smart contract deposits)
5. [x] Public GitHub repository
6. [ ] Proper fee estimation engine + smart contract signing parsing to understand what will actually happen and predict if it will fail or not
7. [ ] Built-in nonce manager for EVM and EVM-like chains to avoid reliance on potentially inaccurate 3rd-party data and avoiding transaction failures in certain cases - this will need to sync to RPC on sending transactions. The wallet should be trusted if there are any pending transactions but this could be improved by tracking any pending transactions.
8. [ ] Improved sync functionality to sync as much as possible of any stored wallet data.
9. [ ] Full support for more chains, including (but not limited to):
  [ ] Ripple (XRP)
  [ ] edgeX (EDGE) - support for trading derivatives
  [ ] Hyperliquid (HYPE) - support for trading perps (perpetuals)
  [ ] Injective (INJ)
  [ ] Algorand (ALGO)
  [ ] Zcash (ZEC) - support transparent AND shielded addresses
  [ ] Monero (XMR) - alternative to Zcash
10. [ ] NFT support on relevant chains:
  [ ] EVM
  [ ] SOL
  [ ] TRX
  [ ] ALGO
11. [ ] Support for the Open Wallet Standard
12. [ ] Native DeFi support:
  [ ] Native Aerodrome Finance LPing (liquidity providing)
  [ ] Native Aave lending
13. [ ] (hopefully) Filecoin storage integration so you can easily store, download, and access files stored on Filecoin
14. [ ] (hopefully) Integration with Tor for anonymity when sending to RPC
