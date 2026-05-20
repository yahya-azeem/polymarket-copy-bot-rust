---
tags: [meta]
created: 2026-04-02
updated: 2026-04-02
---

# Wiki Index

This is the master catalog of all wiki pages. Organized by category. Updated on every ingest.

## Overview

- **[[wiki/overview]]** — High-level project description, features, and quick-start

## Architecture

- **[[wiki/architecture/bot-architecture]]** — Module structure, data flow, async task graph, startup sequence
- **[[wiki/architecture/whale-discovery]]** — How target whales are discovered and monitored

## Concepts

- **[[wiki/concepts/signature-types]]** — Polymarket signature types (EOA, Proxy, Gnosis Safe) and auto-detection
- **[[wiki/concepts/sizing-model]]** — How trade sizes are calculated (proportional vs literal, balance-aware)
- **[[wiki/concepts/adaptive-risk-tiers]]** — Three-tier TP/SL system based on account balance
- **[[wiki/concepts/order-types]]** — LIMIT, FOK, FAK, AUTO — when each is used and why

## Entities

- **[[wiki/entities/trade-executor]]** — `TradeExecutor`: CLOB authentication, order execution, balance scanning, position management
- **[[wiki/entities/bot-state]]** — `BotState`: Central orchestrator owning all subsystems, trade lifecycle, state recovery
- **[[wiki/entities/config]]** — `Config`: All configuration structs loaded from environment variables
- **[[wiki/entities/core-types]]** — `Trade`, `PositionState`, `CopyExecutionResult`, `Stats` — core data structures
- **[[wiki/entities/risk-manager]]** — Risk management: session caps, daily loss limits, per-market limits
- **[[wiki/entities/whale-cache]]** — Persistent cache of known whale addresses across sessions
- **[[wiki/entities/trade-monitor]]** — HTTP polling monitor for detecting whale trades
- **[[wiki/entities/websocket-monitor]]** — WebSocket monitor for live price streams and position updates
- **[[wiki/entities/position-tracker]]** — In-memory position tracking with Data API reconciliation

## Operations

- **[[wiki/operations/setup-guide]]** — Full setup instructions: env config, building, running
- **[[wiki/operations/troubleshooting]]** — Common issues: authentication failures, zero balance, geoblock

## Sources

- **[[raw/src/main.rs]]** — Entry point, tokio runtime, graceful shutdown
- **[[raw/src/bot.rs]]** — Bot orchestrator logic
- **[[raw/src/trader.rs]]** — Trade execution core
- **[[raw/src/config.rs]]** — Configuration system
- **[[raw/src/types.rs]]** — Core data types
- **[[raw/src/risk_manager.rs]]** — Risk management
- **[[raw/src/cache.rs]]** — Whale cache
- **[[raw/src/monitor.rs]]** — Trade monitoring
- **[[raw/src/websocket_monitor.rs]]** — WebSocket monitoring
- **[[raw/src/positions.rs]]** — Position tracking
- **[[raw/src/leaderboard.rs]]** — Polywhaler leaderboard scraper

## Queries

*(None yet — run a query to generate the first saved analysis)*
