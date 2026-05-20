---
tags: [entity]
sources: [src/config.rs]
created: 2026-04-02
updated: 2026-04-02
---

# Config

**All configuration structs loaded from environment variables with validation.**

## Config Struct

```rust
pub struct Config {
    pub target_wallets: Vec<String>,
    pub use_polywhaler_leaderboard: bool,
    pub private_key: String,
    pub polymarket_geo_token: Option<String>,
    pub trading: TradingConfig,
    pub risk: RiskConfig,
    pub run: RunConfig,
    pub monitoring: MonitoringConfig,
    pub simulation_mode: bool,
    pub polysimulator_api_key: Option<String>,
    pub polymarket_signature_type: String,
    pub rpc_url: String,
}
```

## Sub-Configs

### TradingConfig
| Field | Env Var | Default | Description |
|-------|---------|---------|-------------|
| copy_sells | COPY_SELLS | true | Copy SELL trades |
| position_multiplier | POSITION_MULTIPLIER | 0.1 | Fallback sizing multiplier |
| max_trade_size | MAX_TRADE_SIZE | 100 | Max USDC per trade |
| min_trade_size | MIN_TRADE_SIZE | 1 | Min USDC per trade |
| slippage_tolerance | SLIPPAGE_TOLERANCE | 0.01 | 1% slippage allowed |
| order_type | ORDER_TYPE | AUTO | LIMIT, FOK, FAK, or AUTO |
| use_sizing_model | USE_SIZING_MODEL | true | Enable proportional sizing |
| sizing_multiplier | SIZING_MULTIPLIER | 2.0 | Extra multiplier |
| target_balance_override | TARGET_BALANCE_USDC | — | Fixed whale balance |
| max_percent_of_balance | MAX_PERCENT_OF_BALANCE | 0.10 | Per-trade wallet % cap |
| prefer_literal_whale_size | PREFER_LITERAL_WHALE_SIZE | true | Literal copy for small trades |
| order_timeout_minutes | ORDER_TIMEOUT_MINUTES | 10 | Stale order cancellation |
| take_profit_percent | TAKE_PROFIT_PERCENT | — | Override adaptive TP |
| stop_loss_percent | STOP_LOSS_PERCENT | — | Override adaptive SL |
| min_whale_size_usdc | MIN_WHALE_SIZE_USDC | 0 | Minimum whale trade to copy |
| auto_market_threshold | AUTO_MARKET_THRESHOLD | 5 | Balance threshold for FOK |

### RiskConfig
| Field | Env Var | Default | Description |
|-------|---------|---------|-------------|
| max_session_notional | MAX_SESSION_NOTIONAL | 1000000 | Max total traded in session |
| max_per_market_notional | MAX_PER_MARKET_NOTIONAL | 1000000 | Max per single market |
| max_daily_loss_percent | MAX_DAILY_LOSS_PERCENT | 20.0 | Daily loss limit % |

### RunConfig
| Field | Env Var | Default | Description |
|-------|---------|---------|-------------|
| exit_after_first_sell_copy | EXIT_AFTER_FIRST_SELL_COPY | false | Exit after first SELL copy |

### MonitoringConfig
| Field | Env Var | Default | Description |
|-------|---------|---------|-------------|
| use_websocket | USE_WEBSOCKET | true | Enable WebSocket monitor |
| use_user_channel | USE_USER_CHANNEL | false | WS user channel |
| poll_interval_ms | POLL_INTERVAL | 2000 | HTTP poll interval |
| ws_asset_ids | WS_ASSET_IDS | — | Specific assets to watch |
| ws_market_ids | WS_MARKET_IDS | — | Specific markets to watch |

## Validation
- `TARGET_WALLETS` required (non-empty)
- `PRIVATE_KEY` required when `SIMULATION_MODE=false`
- `POLYSIMULATOR_API_KEY` required when `SIMULATION_MODE=true`

## Design Note
- Values are trimmed of whitespace
- Comma-separated lists are parsed with `parse_csv()` — trims and filters empties
- Optional env vars use `optional_env()` which returns `None` for empty strings
