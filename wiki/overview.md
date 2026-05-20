---
tags: [architecture]
sources: [src/main.rs, src/bot.rs, src/trader.rs]
created: 2026-04-02
updated: 2026-04-02
---

# Polymarket Copytrade Bot — Overview

**A high-performance, async Rust bot that mirrors whale trades on Polymarket with adaptive risk management and multi-source balance detection.**

## Quick Summary

The bot monitors target wallets ("whales") on Polymarket and automatically copies their trades at a proportionally scaled size. It supports multiple account types (EOA, Gnosis Safe, Polymarket Proxy), three tiers of risk management based on account balance, and multiple data sources for balance detection.

## Key Features

- **Adaptive scalability** — 3-tier TP/SL (Protection / Growth / Mirror) based on your balance
- **Proportional mirroring** — Scales whale trades to your balance ratio
- **Polywhaler leaderboard integration** — Auto-follow top traders
- **Multi-source balance detection** — CLOB API, Data API, open orders (supports USDC, USDC.e, and pUSD)
- **Account type auto-detection** — Automatically detects EOA vs Proxy vs Gnosis Safe
- **Geoblock support** — GEO token for restricted regions (note: buggy, blocked markets still fail)

## Architecture at a Glance

```
main.rs ──→ bot.rs (orchestrator) ──→ trader.rs (execution)
                              ├──→ monitor.rs (HTTP polling)
                              ├──→ websocket_monitor.rs (live prices)
                              ├──→ risk_manager.rs (safety)
                              └──→ positions.rs (state tracking)
```

## Authentication Flow

1. Start → load config from env → create signer from private key
2. Auto-detect account type via Gamma API (`public-profile` endpoint)
3. Initialize CLOB client → authenticate with correct signature type + funder
4. Sync on-chain balances → check geoblock → ensure approvals
5. Seed WebSocket subscriptions → recover state from whales → enter main loop

## Learn More

- [[wiki/architecture/bot-architecture]] — detailed module breakdown
- [[wiki/concepts/signature-types]] — Polymarket account types explained
- [[wiki/concepts/sizing-model]] — how trade sizes are calculated
- [[wiki/concepts/adaptive-risk-tiers]] — the 3-tier TP/SL system
