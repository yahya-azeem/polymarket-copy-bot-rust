---
tags: [architecture]
sources: [src/bot.rs, src/monitor.rs, src/leaderboard.rs]
created: 2026-04-02
updated: 2026-04-02
---

# Whale Discovery

**How the bot finds and monitors target whales.**

## Sources of Whale Addresses

1. **TARGET_WALLETS env var** — Explicit comma-separated list of wallet addresses
2. **Polywhaler Leaderboard** — When `USE_POLYWHALER_LEADERBOARD=true`, fetches top 10 traders from `polywhaler.io` and appends to target list
3. **Whale Cache** — Persisted historical whale addresses loaded from `whale_cache.json`; used for state recovery even when a whale is no longer actively monitored

## Monitoring Pipeline

### HTTP Polling (`monitor.rs`)
- Polls the Polymarket Data API for recent trades by target wallets
- Scans for `WHALE_TRADE` on-chain events as alternate source
- Uses `BoundedDedup` (10,000 entries) to avoid re-processing trades
- Configurable polling interval (`POLL_INTERVAL`, default 2000ms)

### WebSocket Monitor (`websocket_monitor.rs`)
- Subscribes to asset WebSocket channels for live trade data
- Dynamically subscribes to new markets when a trade is detected via HTTP poll
- Trade data from WS is processed first (lower latency)

## State Recovery

On startup, the bot audits its entire portfolio against all historical whales:

1. Fetch your positions via Data API
2. Fetch each whale's positions
3. Collect all token IDs held by any whale
4. Flag any position YOU hold that NO whale holds → "abandoned position" warning
5. Clean up inactive whales from cache

This prevents "leaderboard drift" — holding positions that whales have already exited.

## Filtering

- `MIN_WHALE_SIZE_USDC` — Skip whale trades below this threshold
- Trade deduplication by `tx_hash` and by `(wallet, token_id, side, size, price, timestamp)`
