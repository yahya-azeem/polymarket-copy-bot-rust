---
tags: [entity]
sources: [src/bot.rs]
created: 2026-04-02
updated: 2026-04-02
---

# BotState

**The central orchestrator — owns all subsystems and manages the trade lifecycle.**

## Struct

```rust
pub struct BotState {
    pub config: Config,
    pub executor: Arc<TradeExecutor>,
    pub positions: Arc<Mutex<PositionTracker>>,
    pub risk: Arc<Mutex<RiskManager>>,
    pub whale_cache: Arc<Mutex<WhaleCache>>,
    pub processed_trades: Mutex<BoundedDedup>,
    pub stats: Mutex<Stats>,
    pub last_trade_ms: Mutex<i64>,
    pub bot_start_time_ms: i64,
}
```

## Key Methods

### `handle_new_trade(trade)` — The core trade lifecycle

1. **Time filter** — skip trades before bot start time
2. **Rate limit** — enforce 5-second cooldown between trades
3. **Size filter** — skip trades below `MIN_WHALE_SIZE_USDC`
4. **Deduplication** — check `BoundedDedup` by tx_hash and composite key
5. **Side filter** — skip SELL trades if `COPY_SELLS=false`
6. **Size calculation** — compute copy notional via sizing model
7. **Share check** — for SELL trades, verify we have enough shares
8. **Risk check** — verify trade passes session/per-market/daily loss limits
9. **Execute** — call `executor.execute_copy_trade()`
10. **Record fill** — update risk manager and position tracker
11. **Exit** — if `EXIT_AFTER_FIRST_SELL_COPY` and this was a SELL

## PolymarketCopyBot

The outer struct that manages the event loop:

```rust
pub struct PolymarketCopyBot {
    state: Arc<BotState>,
    monitor: TradeMonitor,
    ws_monitor: Option<WebSocketMonitor>,
    last_reconcile_ms: i64,
    last_pnl_check_ms: i64,
}
```

### Event Loop (`run`)
1. Process WebSocket trades → immediate TP/SL + copy
2. Poll HTTP API → additional trade discovery
3. Dynamic PnL checks (10s if holding, 60s if flat)
4. Position reconciliation every 2 minutes
5. Sleep for `POLL_INTERVAL`

## Related
- [[wiki/architecture/bot-architecture]] — module structure
- [[wiki/entities/trade-executor]] — execution engine
