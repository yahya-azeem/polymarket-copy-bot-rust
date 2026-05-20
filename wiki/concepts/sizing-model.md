---
tags: [concept, design-decision]
sources: [src/trader.rs]
created: 2026-04-02
updated: 2026-04-02
---

# Sizing Model

**How the bot calculates how much to copy when a whale trades.**

## Core Formula

The sizing model has two modes, controlled by `USE_SIZING_MODEL`:

### Proportional Mirroring (default)

```
our_trade = (our_balance / whale_balance) × whale_trade_size
```

This always scales DOWN — the bot's balance is assumed to be smaller than the whale's.

### Literal Copy (small trades only)

If `PREFER_LITERAL_WHALE_SIZE=true` (default), small whale trades (≤$15) that fit within the per-trade budget are copied at full size:

```
our_trade = whale_trade_size
```

Conditions:
- Whale trade ≤ $15
- Our balance can afford it (within `MAX_PERCENT_OF_BALANCE`)

## Constraints Applied (in order)

1. **Proportional** or **Literal** result is chosen
2. **Capped at `MAX_TRADE_SIZE`**
3. **Floored at `MIN_TRADE_SIZE`** (minimum $1.00)
4. **Capped at `MAX_PERCENT_OF_BALANCE`** of your wallet (default 10%)
5. **Never exceeds the whale's actual trade size**

## Balance Sources

The model needs two balances:

**Your balance**: Multi-source scan:
- Gamma API (`public-profile`)
- Portfolio API (`/portfolio`)
- CLOB API (balance_allowance, brute-force all 4 signature types)
- Open orders total
- Data API (`/value`)

**Whale balance**: Data API (`/value` endpoint), or `TARGET_BALANCE_USDC` override.

## Why This Matters

Directly copying a whale's position size is usually impossible — a whale trading $10,000 could wipe out a $100 account. Proportional mirroring ensures the bot only risks what's appropriate for its balance. The literal copy exception handles the case where the whale itself makes a small trade that's affordable.

## Configuration

| Variable | Default | Effect |
|----------|---------|--------|
| `POSITION_MULTIPLIER` | 0.1 | Fallback multiplier when sizing model unavailable |
| `SIZING_MULTIPLIER` | 2.0 | Additional multiplier (applied to proportional) |
| `MAX_TRADE_SIZE` | 100 | Hard cap per trade |
| `MIN_TRADE_SIZE` | 1 | Minimum per trade ($1.00) |
| `MAX_PERCENT_OF_BALANCE` | 0.10 | Per-trade wallet cap (10%) |
| `PREFER_LITERAL_WHALE_SIZE` | true | Use literal copy for small trades |
| `TARGET_BALANCE_USDC` | — | Override whale's balance (fixed value) |

