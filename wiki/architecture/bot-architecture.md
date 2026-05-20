---
tags: [architecture]
sources: [src/main.rs, src/bot.rs, src/trader.rs]
created: 2026-04-02
updated: 2026-04-02
---

# Bot Architecture

**The Polymarket Copytrade Bot uses a modular async architecture with a central orchestrator owning all subsystems.**

## Module Map

| Module | File | Responsibility |
|--------|------|---------------|
| `main` | `src/main.rs` | Entry point, tokio runtime, graceful shutdown via `ctrlc` |
| `bot` | `src/bot.rs` | Central orchestrator (`PolymarketCopyBot` + `BotState`) |
| `config` | `src/config.rs` | Configuration from environment variables |
| `trader` | `src/trader.rs` | CLOB auth, order execution, balance scanning, position queries |
| `monitor` | `src/monitor.rs` | HTTP polling for new whale trades |
| `websocket_monitor` | `src/websocket_monitor.rs` | WebSocket connections for live price data |
| `positions` | `src/positions.rs` | In-memory position tracking with reconciliation |
| `risk_manager` | `src/risk_manager.rs` | Notional caps, daily loss limits, session tracking |
| `cache` | `src/cache.rs` | Persistent whale address cache |
| `types` | `src/types.rs` | Core data structures |
| `leaderboard` | `src/leaderboard.rs` | Polywhaler leaderboard scraper |
| `utils` | `src/utils.rs` | Helpers: `current_time_ms`, `BoundedDedup` |
| `logger` | `src/logger.rs` | Tracing/logging setup |

## Data Flow

```
                    ┌──────────────────┐
                    │   main.rs        │
                    │  (entry point)   │
                    └────────┬─────────┘
                             │
                    ┌────────▼─────────┐
                    │  PolymarketCopyBot │
                    │  (bot.rs)         │
                    │                   │
                    │  ┌─ BotState ────┐│
                    │  │ executor      ││
                    │  │ positions     ││
                    │  │ risk          ││
                    │  │ whale_cache   ││
                    │  │ processed_trades│
                    │  │ stats         ││
                    │  └───────────────┘│
                    └────────┬─────────┘
          ┌────────────┬────┼────┬──────────────┐
          ▼            ▼    ▼    ▼              ▼
     TradeMonitor  WS   Risk  Positions    Leaderboard
     (monitor.rs)  Monitor Mgr  Tracker     (scraper)
                        │                     │
                        └── TradeExecutor ────┘
                            (trader.rs)
                            │
                    ┌───────┴───────┐
                    ▼               ▼
              CLOB API        On-Chain RPC
           (Polymarket)     (Polygon Node)
```

## Startup Sequence

1. `main()` loads `.env`, installs rustls crypto, initializes logger
2. `Config::from_env()` parses all environment variables
3. `PolymarketCopyBot::new()`:
   - Optionally fetches Polywhaler leaderboard top 10
   - Creates `TradeMonitor` (HTTP polling engine)
   - Creates `TradeExecutor` (CLOB + signer)
   - Creates `PositionTracker` + `RiskManager`
   - Loads `WhaleCache` from disk
4. `bot.initialize()`:
   - `executor.initialize()` → auth with Polymarket CLOB
   - Fetches exchange balance + on-chain balances
   - Seeds WS subscriptions from current markets
   - Initializes `WebSocketMonitor`
   - `sync_state_with_whales()` → audits portfolio against whales
5. `bot.run()` → main event loop

## Main Event Loop

- **WebSocket trades** → immediate TP/SL check + copy execution
- **HTTP poll trades** → additional trade discovery
- **PnL checks** every 10s (if holding) or 60s (if flat)
- **Position reconciliation** every 2 minutes
- **Stale order cancellation** based on `ORDER_TIMEOUT_MINUTES`

## Concurrency Model

- Uses `tokio::spawn` for parallel trade processing
- `Arc<RwLock<>>` for shared mutable state (executor, positions)
- `Mutex` for simple state (stats, processed trades, cooldown)
- `AtomicU8` for the detected signature type (lock-free read)

## Key Design Decisions

- **One trade at a time per whale** — enforced by `BoundedDedup` deduplication
- **Rate limiting** — 5-second cooldown between trades (`TRADE_COOLDOWN_MS`)
- **Async all the way** — no blocking calls in event loop
- **Graceful shutdown** — tokio::select between bot.run() and ctrlc signal
