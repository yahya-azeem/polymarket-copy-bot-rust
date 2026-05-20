---
tags: [entity]
sources: [src/risk_manager.rs]
created: 2026-04-02
updated: 2026-04-02
---

# RiskManager

**Enforces trading limits: session notional caps, per-market limits, and daily loss protection.**

## Key Features
- **Session notional** — tracks total volume traded in session; rejects trades that would exceed `MAX_SESSION_NOTIONAL`
- **Per-market cap** — limits total exposure to any single market via `MAX_PER_MARKET_NOTIONAL`
- **Daily loss limit** — if PnL drops below `-MAX_DAILY_LOSS_PERCENT%` of starting equity, blocks new buys
- **Position tracking** — records fills and integrates with `PositionTracker` to compute PnL
