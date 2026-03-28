use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

mod config;
mod logger;
mod monitor;
mod positions;
mod risk_manager;
mod trader;
mod websocket_monitor;

use config::Config;
use monitor::Trade;
use positions::PositionTracker;
use risk_manager::RiskManager;
use trader::TradeExecutor;
use websocket_monitor::WebSocketMonitor;

struct PolymarketCopyBot {
    config: Config,
    monitor: monitor::TradeMonitor,
    ws_monitor: Option<WebSocketMonitor>,
    executor: TradeExecutor,
    positions: Arc<Mutex<PositionTracker>>,
    risk: Arc<Mutex<RiskManager>>,
    processed_trades: HashSet<String>,
    bot_start_time_ms: i64,
    stats: Stats,
}

#[derive(Default)]
struct Stats {
    trades_detected: u64,
    trades_copied: u64,
    trades_failed: u64,
    total_volume: f64,
}

impl PolymarketCopyBot {
    async fn new(config: Config) -> Result<Self> {
        let monitor = monitor::TradeMonitor::new(config.clone());
        let executor = TradeExecutor::new(config.clone()).await?;
        let positions = Arc::new(Mutex::new(PositionTracker::default()));
        let risk = Arc::new(Mutex::new(RiskManager::new(config.clone(), positions.clone())));

        Ok(Self {
            config,
            monitor,
            ws_monitor: None,
            executor,
            positions,
            risk,
            processed_trades: HashSet::new(),
            bot_start_time_ms: current_time_ms(),
            stats: Stats::default(),
        })
    }

    async fn initialize(&mut self) -> Result<()> {
        info!("Polymarket Copy Trading Bot (Rust)");
        info!("Target wallet: {}", self.config.target_wallet);
        info!(
            "Position multiplier: {}%",
            self.config.trading.position_multiplier * 100.0
        );
        info!("Max trade size: {} USDC", self.config.trading.max_trade_size);
        info!("Order type: {}", self.config.trading.order_type);
        info!("Copy sells: {}", self.config.trading.copy_sells);
        info!("WebSocket: {}", self.config.monitoring.use_websocket);
        info!("Bot start time: {}", self.bot_start_time_ms);

        self.monitor.initialize(self.bot_start_time_ms);
        self.executor.initialize().await?;
        self.reconcile_positions().await;

        if self.config.monitoring.use_websocket {
            let mut ws = WebSocketMonitor::new(self.config.clone());
            if let Err(e) = ws.initialize().await {
                warn!("WebSocket init failed: {e}");
            } else {
                self.ws_monitor = Some(ws);
            }
        }

        Ok(())
    }

    async fn run(&mut self) -> Result<()> {
        info!("Bot started");
        loop {
            let mut ws_trades = Vec::new();
            if let Some(ws) = &mut self.ws_monitor {
                while let Some(trade) = ws.try_recv_trade() {
                    ws_trades.push(trade);
                }
            }
            for trade in ws_trades {
                self.handle_new_trade(trade).await;
            }

            match self.monitor.poll_for_new_trades().await {
                Ok(trades) => {
                    for trade in trades {
                        self.handle_new_trade(trade).await;
                    }
                }
                Err(e) => error!("Monitoring error: {e}"),
            }

            tokio::time::sleep(Duration::from_millis(self.config.monitoring.poll_interval_ms)).await;
        }
    }

    async fn handle_new_trade(&mut self, trade: Trade) {
        if trade.timestamp_ms < self.bot_start_time_ms {
            return;
        }

        let keys = self.trade_keys(&trade);
        if keys.iter().any(|k| self.processed_trades.contains(k)) {
            return;
        }
        for key in keys {
            self.processed_trades.insert(key);
        }

        self.stats.trades_detected += 1;
        info!(
            "NEW TRADE | {} {} {} USDC @ {} | token={} market={}",
            trade.side, trade.outcome, trade.size_usdc, trade.price, trade.token_id, trade.market
        );

        if trade.side == "SELL" && !self.config.trading.copy_sells {
            warn!("Skipping SELL trade (COPY_SELLS=false)");
            return;
        }

        let copy_notional = self.executor.calculate_copy_size(trade.size_usdc);

        if trade.side == "SELL" {
            let needed_shares = self
                .executor
                .calculate_shares_for_notional(copy_notional, trade.price);
            let guard = self.positions.lock().await;
            if let Some(pos) = guard.get_position(&trade.token_id) {
                if pos.shares < needed_shares {
                    warn!(
                        "Skipping SELL, insufficient shares. have={}, need={}",
                        pos.shares, needed_shares
                    );
                    return;
                }
            } else {
                warn!("Skipping SELL, no local position for token {}", trade.token_id);
                return;
            }
        }

        let risk_ok = {
            let guard = self.risk.lock().await;
            guard.check_trade(&trade, copy_notional).await
        };
        if let Err(reason) = risk_ok {
            warn!("Risk blocked trade: {reason}");
            return;
        }

        match self
            .executor
            .execute_copy_trade(&trade, Some(copy_notional))
            .await
        {
            Ok(fill) => {
                {
                    let mut risk = self.risk.lock().await;
                    risk.record_fill(&trade, fill.copy_notional, fill.copy_shares, fill.price, &fill.side)
                        .await;
                }
                self.stats.trades_copied += 1;
                self.stats.total_volume += fill.copy_notional;
                info!("Copied trade successfully: order_id={}", fill.order_id);

                if self.config.run.exit_after_first_sell_copy && fill.side == "SELL" {
                    info!("EXIT_AFTER_FIRST_SELL_COPY triggered, exiting");
                    std::process::exit(0);
                }
            }
            Err(e) => {
                self.stats.trades_failed += 1;
                error!("Failed to copy trade: {e}");
            }
        }
    }

    async fn reconcile_positions(&mut self) {
        match self.executor.get_positions().await {
            Ok(positions) => {
                let mut tracker = self.positions.lock().await;
                let (loaded, skipped) = tracker.load_from_data_api_positions(&positions);
                info!("Positions loaded: {} (skipped {})", loaded, skipped);
            }
            Err(e) => warn!("Positions reconciliation failed: {e}"),
        }
    }

    fn trade_keys(&self, trade: &Trade) -> Vec<String> {
        let mut keys = Vec::new();
        if !trade.tx_hash.is_empty() {
            keys.push(trade.tx_hash.clone());
        }
        keys.push(format!(
            "{}|{}|{}|{}|{}",
            trade.token_id, trade.side, trade.size_usdc, trade.price, trade.timestamp_ms
        ));
        keys
    }
}

fn current_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[tokio::main]
async fn main() -> Result<()> {
    logger::init();
    let config = Config::from_env()?;
    config.validate()?;

    let mut bot = PolymarketCopyBot::new(config).await?;
    bot.initialize().await?;
    bot.run().await
}
