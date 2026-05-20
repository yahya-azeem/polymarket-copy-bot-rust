---
tags: [entity]
sources: [src/websocket_monitor.rs]
created: 2026-04-02
updated: 2026-04-02
---

# WebSocketMonitor

**WebSocket-based monitor for live trade/price data from Polymarket's streaming API.**

## Key Details
- Connects to Polymarket's WebSocket channels for asset-specific updates
- Lower latency than HTTP polling — trade data arrives first via WS
- Dynamically subscribes to new markets/assets when discovered via HTTP polling
- Provides `try_recv_trade()` — non-blocking read of incoming trades
- Initialized during `bot.initialize()` with seed assets from first poll
- `subscribe_assets(asset_ids)` — adds new subscriptions at runtime
- Fully async — integrates with tokio event loop
