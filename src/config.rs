use anyhow::{Result, bail};

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TradingConfig {
    pub copy_sells: bool,
    pub position_multiplier: f64,
    pub max_trade_size: f64,
    pub min_trade_size: f64,
    pub slippage_tolerance: f64,
    pub order_type: String,
    pub use_sizing_model: bool,
    pub sizing_multiplier: f64,
    pub target_balance_override: Option<f64>,
    pub max_percent_of_balance: f64,
    pub prefer_literal_whale_size: bool,
    pub order_timeout_minutes: u64,
    pub take_profit_percent: Option<f64>,
    pub stop_loss_percent: Option<f64>,
    pub min_whale_size_usdc: f64,
    pub auto_market_threshold: f64,
}

#[derive(Debug, Clone)]
pub struct RiskConfig {
    pub max_session_notional: f64,
    pub max_per_market_notional: f64,
    pub max_daily_loss_percent: f64,
}

#[derive(Debug, Clone)]
pub struct RunConfig {
    pub exit_after_first_sell_copy: bool,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MonitoringConfig {
    pub use_websocket: bool,
    pub use_user_channel: bool,
    pub poll_interval_ms: u64,
    pub ws_asset_ids: Vec<String>,
    pub ws_market_ids: Vec<String>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let target_wallets = parse_csv(&env("TARGET_WALLETS", &env("TARGET_WALLET", "")));
        let use_leaderboard = env("USE_POLYWHALER_LEADERBOARD", "false").to_lowercase() == "true";
        Ok(Self {
            target_wallets,
            use_polywhaler_leaderboard: use_leaderboard,
            private_key: env("PRIVATE_KEY", ""),
            polymarket_geo_token: optional_env("POLYMARKET_GEO_TOKEN"),
            trading: TradingConfig {
                copy_sells: env("COPY_SELLS", "true").to_lowercase() != "false",
                position_multiplier: env("POSITION_MULTIPLIER", "0.1").parse()?,
                max_trade_size: env("MAX_TRADE_SIZE", "100").parse()?,
                min_trade_size: env("MIN_TRADE_SIZE", "1").parse()?,
                slippage_tolerance: env("SLIPPAGE_TOLERANCE", "0.01").parse()?,
                order_type: env("ORDER_TYPE", "AUTO").to_uppercase(),
                use_sizing_model: env("USE_SIZING_MODEL", "true").to_lowercase() != "false",
                sizing_multiplier: env("SIZING_MULTIPLIER", "2.0").parse()?,
                target_balance_override: optional_env("TARGET_BALANCE_USDC")
                    .and_then(|v| v.parse::<f64>().ok()),
                max_percent_of_balance: env("MAX_PERCENT_OF_BALANCE", "0.10").parse()?,
                prefer_literal_whale_size: env("PREFER_LITERAL_WHALE_SIZE", "true").to_lowercase()
                    != "false",
                order_timeout_minutes: env("ORDER_TIMEOUT_MINUTES", "10").parse()?,
                take_profit_percent: optional_env("TAKE_PROFIT_PERCENT").and_then(|v| v.parse().ok()),
                stop_loss_percent: optional_env("STOP_LOSS_PERCENT").and_then(|v| v.parse().ok()),
                min_whale_size_usdc: env("MIN_WHALE_SIZE_USDC", "0").parse()?,
                auto_market_threshold: env("AUTO_MARKET_THRESHOLD", "5").parse()?,
            },
            risk: RiskConfig {
                max_session_notional: env("MAX_SESSION_NOTIONAL", "1000000").parse()?,
                max_per_market_notional: env("MAX_PER_MARKET_NOTIONAL", "1000000").parse()?,
                max_daily_loss_percent: env("MAX_DAILY_LOSS_PERCENT", "20.0").parse()?,
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
            simulation_mode: env("SIMULATION_MODE", "false").to_lowercase() == "true",
            polysimulator_api_key: optional_env("POLYSIMULATOR_API_KEY"),
            polymarket_signature_type: env("POLYMARKET_SIGNATURE_TYPE", "EOA").to_uppercase(),
            rpc_url: env("RPC_URL", ""),
        })
    }

    pub fn validate(&self) -> Result<()> {
        if self.target_wallets.is_empty() {
            bail!("Missing required config: TARGET_WALLETS (or TARGET_WALLET)");
        }
        if self.private_key.is_empty() && !self.simulation_mode {
            bail!("Missing required config: PRIVATE_KEY (Required when SIMULATION_MODE=false)");
        }
        if self.simulation_mode && self.polysimulator_api_key.is_none() {
            bail!(
                "Missing required config: POLYSIMULATOR_API_KEY (Required when SIMULATION_MODE=true)"
            );
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
