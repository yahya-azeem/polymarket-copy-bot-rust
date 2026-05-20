---
tags: [entity]
sources: [src/positions.rs]
created: 2026-04-02
updated: 2026-04-02
---

# PositionTracker

**In-memory tracker that maintains the bot's view of its own positions, reconciled against the Data API.**

## Key Details
- Stores positions as `PositionState` objects in memory
- `load_from_data_api_positions(positions)` — reconciles against Data API response
- Returns `(loaded, skipped)` counts for logging
- `get_position(token_id)` — look up a specific position
- `get_all()` — returns all tracked positions
- Used by `BotState` for share validation (ensuring we have enough to sell)
- Used by `RiskManager` for PnL/daily loss calculations
