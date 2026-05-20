---
tags: [concept, design-decision]
sources: [src/trader.rs]
created: 2026-04-02
updated: 2026-04-02
---

# Order Types

**The bot dynamically selects the optimal order type based on balance and market conditions.**

## Available Types

| Type | Constant | Description |
|------|----------|-------------|
| LIMIT | `OrderType::GTC` | Good-till-cancelled limit order at whale's entry price |
| FOK | `OrderType::FOK` | Fill-or-Kill — executes fully or cancels |
| FAK | `OrderType::FAK` | Fill-and-Kill — executes partially or fully, any remainder cancels |
| AUTO | — | Dynamic selection between LIMIT and FOK |

## AUTO Mode Logic

When `ORDER_TYPE=AUTO` (the default):

1. Check `your_balance >= AUTO_MARKET_THRESHOLD` (default $5)
2. If **balance >= threshold** → use **FOK** for guaranteed execution
3. If **balance < threshold** → use **LIMIT** at whale's entry price

This means well-funded accounts get instant execution; smaller accounts patiently wait for a limit fill.

## Price Protection

For non-LIMIT orders, the bot enforces a price ceiling:
- Max 20% above the whale's entry price (capped at 0.99)
- Protects against overpaying when the order book has moved against you

## Tick Size Precision

The bot matches the order book's price precision:
- Reads the best bid/ask from the CLOB order book
- Counts significant decimal places (min 2, max 4)
- Rounds the order price to match

This prevents "invalid tick size" validation errors from the Polymarket API.

## Market vs Limit

| Aspect | LIMIT | FOK/FAK |
|--------|-------|---------|
| Execution certainty | Low (may not fill) | High (immediate) |
| Price control | Exact (whale's price) | Market (current book) |
| Best for | Small accounts | Well-funded accounts |
| Slippage | None | Controlled by SLIPPAGE_TOLERANCE |

## Related

- [[wiki/concepts/sizing-model]] — determines trade size before order type selection
