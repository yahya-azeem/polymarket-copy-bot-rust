---
tags: [entity]
sources: [src/types.rs]
created: 2026-04-02
updated: 2026-04-02
---

# Core Types

**The central data structures used across all modules.**

## Trade

Represents a detected whale trade to be copied.

```rust
pub struct Trade {
    pub tx_hash: String,
    pub timestamp_ms: i64,
    pub market: String,
    pub token_id: String,
    pub side: String,         // "BUY" or "SELL"
    pub price: f64,
    pub size_usdc: f64,
    pub outcome: String,
    pub original_target_wallet: String,
}
```

## PositionState

Tracks a held position for TP/SL calculations.

```rust
pub struct PositionState {
    pub token_id: String,
    pub market: String,
    pub outcome: String,
    pub shares: f64,
    pub cost_basis: f64,     // total USDC spent
    pub avg_price: f64,
}
```

## CopyExecutionResult

Returned after a trade is executed.

```rust
pub struct CopyExecutionResult {
    pub order_id: String,
    pub copy_notional: f64,
    pub copy_shares: f64,
    pub price: f64,
    pub side: String,
}
```

## Stats

Runtime statistics accumulated during the session.

```rust
pub struct Stats {
    pub trades_detected: u64,
    pub trades_copied: u64,
    pub trades_failed: u64,
    pub total_volume: f64,
}
```
