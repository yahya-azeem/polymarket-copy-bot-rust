---
tags: [entity]
sources: [src/trader.rs]
created: 2026-04-02
updated: 2026-05-17
---

# TradeExecutor

**The core execution engine — handles CLOB authentication, balance scanning, order placement, and position queries.**

## Struct

```rust
pub struct TradeExecutor {
    config: Config,
    signer: Arc<RwLock<Option<PrivateKeySigner>>>,
    clob: Arc<RwLock<Option<AuthClient>>>,
    _creds: Arc<RwLock<Option<Credentials>>>,
    http: Client,
    detected_sig_type: AtomicU8,
    detected_proxy: Arc<RwLock<Option<Address>>>,
}
```

## Key Public Methods

### Lifecycle
- `new(config)` — Creates the executor, initializes signer and HTTP client
- `initialize()` — Authenticates with Polymarket CLOB, detects account type, syncs balance

### Balance
- `get_your_balance_usdc()` — Multi-source CLOB balance scan (CLOB API, Data API, open orders)
- `get_onchain_balances()` — Reads native USDC, bridged USDC.e, and **pUSD** from Polygon RPC
- `get_target_balance_usdc(whale)` — Reads whale balance from Data API

### Trading
- `calculate_copy_size(original_size, wallet)` — Applies the sizing model
- `calculate_shares_for_notional(notional, price)` — Converts dollar amount to shares
- `execute_copy_trade(trade, notional_override)` — Places the actual order
- `cancel_stale_orders()` — Cancels orders older than `ORDER_TIMEOUT_MINUTES`

### Risk
- `check_profit_taking(positions_tracker, token)` — TP/SL check for held positions

## Internal Details

### Balance Scanning (`get_your_balance_usdc`)
A comprehensive audit that tries every available source and takes the max:
1. **CLOB API** (`balance-allowance`) — brute-force all 3 signature types (Eoa, Proxy, GnosisSafe) first without token_id, then across all 3 tokens: USDC (`0x3c499c54...`), USDC.e (`0x2791Bca1...`), and pUSD (`0xC011a7E1...`)
2. **Open Orders** — sum of price × size for all open orders
3. **Data API** (`/value?user=`) — portfolio value endpoint

> **Note**: The Gamma API `/public-profile` no longer returns balance fields, and `/portfolio` returns 404. These were removed as balance sources.

Includes a hardcoded fallback address (`0x83A6487eE74712F2d2f703554e2A0D0704443916`) as a known working backup.

### pUSD Token (Polymarket V2)
On **April 28, 2026** Polymarket upgraded to V2, replacing USDC.e with **pUSD** (`0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB`) as the collateral token. The bot queries all three tokens and returns the max balance found. On-chain balance detection also checks for pUSD across both the signer EOA and detected proxy address.

### Order Placement
Both LIMIT and market (FOK/FAK) orders follow the same pattern:
1. Build order via CLOB SDK builder
2. Override `order.maker` with detected proxy address (if applicable)
3. Override `order.signatureType` with the correct u8 value
4. Sign with `clob.sign(signer, order)`
5. Submit with `clob.post_order(signed)`

### Price Protection
- Non-LIMIT BUY orders cap the price at 20% above the whale's entry (max 0.99)
- Slippage tolerance applied: BUY = price × (1 + slippage), SELL = price × (1 - slippage)

### Auto-Detection (`discover_account_details`)
Queries Gamma API `/public-profile` to detect proxy wallets. If found, sets signature type to Gnosis Safe and stores the proxy address for maker overrides.

## Dependencies
- `polymarket_client_sdk` — CLOB client, order types, authentication
- `alloy` — Ethereum types, RPC provider, signers
- `reqwest` — HTTP client for REST APIs
- `tokio::sync::RwLock` — Shared mutable state

## Related
- [[wiki/concepts/signature-types]] — account type detection
- [[wiki/concepts/sizing-model]] — trade sizing
- [[wiki/concepts/order-types]] — order type selection
