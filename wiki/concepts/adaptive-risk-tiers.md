---
tags: [concept]
sources: [src/trader.rs]
created: 2026-04-02
updated: 2026-04-02
---

# Adaptive Risk Tiers

**The bot automatically adjusts its take-profit and stop-loss strategy based on your account balance.**

## The Three Tiers

| Tier | Balance | TP | SL | Strategy |
|------|---------|----|----|----------|
| 1 — Protection | < $100 | 20% | 10% | Tight stops to preserve capital |
| 2 — Growth | $100–$1,000 | 50% | 25% | Looser limits to ride trends |
| 3 — Mirror | > $1,000 | None | None | Pure whale-mimicry, no independent exits |

## How It Works

1. On each PnL check cycle, the bot reads `your_balance_usdc`
2. Applies `calculate_adaptive_tpsl()` to determine TP/SL thresholds
3. For each held position, fetches the current best bid price
4. If PnL >= TP% → sells entire position
5. If PnL <= -SL% → sells entire position

## Why Three Tiers

- **Small accounts** need protection from variance — a 50% drawdown on $50 is devastating
- **Medium accounts** can tolerate wider swings — the bot lets winners run
- **Large accounts** follow the whale's conviction exactly — no second-guessing

## PnL Check Frequency

- **Holding positions**: every 10 seconds
- **Flat (no positions)**: every 60 seconds

## Edge Cases

- If balance falls below a tier boundary during a trade, the TP/SL for the original entry tier applies (no mid-trade repricing)
- TP/SL checks run asynchronously via `tokio::spawn` — they don't block trade detection
