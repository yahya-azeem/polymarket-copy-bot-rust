---
tags: [meta]
created: 2026-04-02
updated: 2026-04-02
---

# Wiki Log

Chronological record of all wiki operations. Entries use consistent prefix: `## [YYYY-MM-DD] operation | Title`

## [2026-04-02] ingest | Initial Wiki Bootstrap

Created the entire wiki infrastructure from project source code analysis:

- **CLAW.md** — Schema/instruction manual for this agent (the wiki maintainer)
- **wiki/index.md** — Content index cataloging all pages
- **wiki/log.md** — This file, append-only chronological log
- **wiki/overview.md** — High-level project overview
- **wiki/architecture/bot-architecture.md** — Module structure and data flow
- **wiki/architecture/whale-discovery.md** — Whale discovery pipeline
- **wiki/concepts/signature-types.md** — Polymarket signature types and auto-detection
- **wiki/concepts/sizing-model.md** — Trade sizing model
- **wiki/concepts/adaptive-risk-tiers.md** — Three-tier TP/SL system
- **wiki/concepts/order-types.md** — Order type selection
- **wiki/entities/trade-executor.md** — TradeExecutor deep-dive
- **wiki/entities/bot-state.md** — BotState orchestrator
- **wiki/entities/config.md** — Configuration system
- **wiki/entities/core-types.md** — Core data structures
- **wiki/entities/risk-manager.md** — Risk management
- **wiki/entities/whale-cache.md** — Whale cache
- **wiki/entities/trade-monitor.md** — Trade monitoring
- **wiki/entities/websocket-monitor.md** — WebSocket monitor
- **wiki/entities/position-tracker.md** — Position tracker
- **wiki/operations/setup-guide.md** — Setup guide
- **wiki/operations/troubleshooting.md** — Troubleshooting guide

Sources ingested: `src/main.rs`, `src/bot.rs`, `src/trader.rs`, `src/config.rs`, `src/types.rs`, `README.md`
