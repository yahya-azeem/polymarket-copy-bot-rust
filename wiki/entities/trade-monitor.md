---
tags: [entity]
sources: [src/monitor.rs]
created: 2026-04-02
updated: 2026-04-02
---

# TradeMonitor

**HTTP polling engine that detects new whale trades by querying the Polymarket Data API.**

## Key Details
- Polls `https://data-api.polymarket.com/positions` for recent trades by target wallets
- Also attempts on-chain event scanning via `WHALE_TRADE` events
- Initial seed poll (`poll_for_new_trades(true)`) fetches a wider window to catch existing positions
- Regular polls use incremental deduplication via `BoundedDedup`
- Configurable interval via `POLL_INTERVAL` (default 2000ms)
