# Polymarket Copy Trading Bot (Rust)

Rust rewrite of the original TypeScript copytrading bot.

## Features

- Polls Polymarket Data API for target wallet trades
- Optional WebSocket ingestion for faster trade detection
- Copies BUY and optional SELL trades
- Position tracking and risk caps (session/per-market notional)
- Authenticated CLOB order placement with `polymarket-client-sdk`
- Balance/allowance refresh via CLOB authenticated endpoint

## Setup

1. Copy `.env.example` to `.env`
2. Fill `TARGET_WALLET`, `PRIVATE_KEY`, and `RPC_URL`
3. Build:

```bash
cargo check
```

4. Run:

```bash
cargo run
```
