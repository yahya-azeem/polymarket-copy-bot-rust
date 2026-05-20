---
tags: [entity]
sources: [src/cache.rs]
created: 2026-04-02
updated: 2026-04-02
---

# WhaleCache

**Persistent cache of known whale addresses saved to `whale_cache.json` across bot sessions.**

## Purpose
- Preserves historical whale addresses so state recovery works even for whales no longer in the active target list
- Enables cache cleanup (removing whales with 0 positions and 0 activity)
- Prevents "leaderboard drift" by keeping historical context

## Key Operations
- `load()` — Read cache from disk on startup
- `save()` — Persist to disk on updates
- `add_whales(addresses)` — Add new whales to cache
- `remove_whale(address)` — Remove inactive whales
- `get_all()` — Get all known whale addresses for state recovery audit
