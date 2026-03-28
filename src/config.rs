use anyhow::{Result, bail};

#[derive(Debug, Clone)]
pub struct Config {
    pub target_wallet: String,
    pub private_key: String,
    pub rpc_url: String,
    pub polymarket_geo_token: Option<String>,
    pub chain_id: u64,
    pub trading: TradingConfig,
    pub risk: RiskConfig,
    pub run: RunConfig,
    pub monitoring: MonitoringConfig,
}

#[derive(Debug, Clone)]
pub struct TradingConfig {
    pub copy_sells: bool,
    pub position_multiplier: f64,
    pub max_trade_size: f64,
    pub min_trade_size: f64,
    pub slippage_tolerance: f64,
    pub order_type: String,
}

#[derive(Debug, Clone)]
pub struct RiskConfig {
    pub max_session_notional: f64,
    pub max_per_market_notional: f64,
}

#[derive(Debug, Clone)]
pub struct RunConfig {
    pub exit_after_first_sell_copy: bool,
}

#[derive(Debug, Clone)]
pub struct MonitoringConfig {
    pub use_websocket: bool,
    pub use_user_channel: bool,
    pub poll_interval_ms: u64,
    pub ws_asset_ids: Vec<String>,
    pub ws_market_ids: Vec<String>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let _ = dotenvy::dotenv();
        Ok(Self {
            target_wallet: env("TARGET_WALLET", ""),
            private_key: env("PRIVATE_KEY", ""),
            rpc_url: env("RPC_URL", "https://polygon-rpc.com"),
            polymarket_geo_token: optional_env("POLYMARKET_GEO_TOKEN"),
            chain_id: 137,
            trading: TradingConfig {
                copy_sells: env("COPY_SELLS", "true").to_lowercase() != "false",
                position_multiplier: env("POSITION_MULTIPLIER", "0.1").parse()?,
                max_trade_size: env("MAX_TRADE_SIZE", "100").parse()?,
                min_trade_size: env("MIN_TRADE_SIZE", "1").parse()?,
                slippage_tolerance: env("SLIPPAGE_TOLERANCE", "0.02").parse()?,
                order_type: env("ORDER_TYPE", "FOK").to_uppercase(),
            },
            risk: RiskConfig {
                max_session_notional: env("MAX_SESSION_NOTIONAL", "0").parse()?,
                max_per_market_notional: env("MAX_PER_MARKET_NOTIONAL", "0").parse()?,
            },
            run: RunConfig {
                exit_after_first_sell_copy: env("EXIT_AFTER_FIRST_SELL_COPY", "false")
                    .to_lowercase()
                    == "true",
            },
            monitoring: MonitoringConfig {
                use_websocket: env("USE_WEBSOCKET", "true").to_lowercase() != "false",
                use_user_channel: env("USE_USER_CHANNEL", "false").to_lowercase() == "true",
                poll_interval_ms: env("POLL_INTERVAL", "2000").parse()?,
                ws_asset_ids: parse_csv(&env("WS_ASSET_IDS", "")),
                ws_market_ids: parse_csv(&env("WS_MARKET_IDS", "")),
            },
        })
    }

    pub fn validate(&self) -> Result<()> {
        if self.target_wallet.is_empty() {
            bail!("Missing required config: TARGET_WALLET");
        }
        if self.private_key.is_empty() {
            bail!("Missing required config: PRIVATE_KEY");
        }
        Ok(())
    }
}

fn env(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_owned())
}

fn optional_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

fn parse_csv(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}
