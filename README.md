# Polymarket-Kalshi Arbitrage Bot 🦀

A Rust trading bot for detecting arbitrage opportunities between Polymarket and Kalshi prediction markets.

[![Telegram](https://img.shields.io/badge/Telegram-@toptrendev_66-2CA5E0?style=for-the-badge&logo=telegram)](https://t.me/TopTrenDev_66)
[![Twitter](https://img.shields.io/badge/Twitter-@toptrendev-1DA1F2?style=for-the-badge&logo=twitter)](https://x.com/toptrendev)
[![Discord](https://img.shields.io/badge/Discord-toptrendev-5865F2?style=for-the-badge&logo=discord)](https://discord.com/users/648385188774019072)

## Structure

```
src/
├── main.rs                  # Entry point
├── lib.rs                   # Module exports
├── event.rs                 # Event data structures
├── event_matcher.rs         # Match events across platforms
├── arbitrage_detector.rs    # Detect price discrepancies
├── bot.rs                   # Bot orchestration
├── clients.rs               # Polymarket & Kalshi API clients
├── trade_executor.rs        # Execute trades
├── position_tracker.rs      # Track positions & profits
├── settlement_checker.rs    # Check event settlements
└── polymarket_blockchain.rs # Polygon blockchain integration
```

## Setup

1. **Install Rust**:

   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **Configure `.env`**:

   ```bash
   POLYGON_RPC_URL=https://polygon-rpc.com
   POLYMARKET_WALLET_PRIVATE_KEY=0x...
   KALSHI_API_KEY=your_key
   KALSHI_API_SECRET=your_secret
   ```

3. **Build & Run**:
   ```bash
   cargo build --release
   cargo run --release
   ```

## How It Works

1. Fetches events from Polymarket (GraphQL) and Kalshi (REST)
2. Matches similar events across platforms
3. Compares YES/NO token prices
4. Detects arbitrage when `YES_price + NO_price < $1.00`
5. Executes trades on both platforms
6. Tracks positions and settlements

## Platforms

| Platform   | Type           | Blockchain      | Currency   |
| ---------- | -------------- | --------------- | ---------- |
| Polymarket | Decentralized  | Polygon         | USDC       |
| Kalshi     | CFTC-regulated | Solana/TRON/BSC | USD/Crypto |
