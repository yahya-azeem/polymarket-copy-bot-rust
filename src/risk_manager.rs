use std::sync::Arc;

use tokio::sync::Mutex;

use crate::config::Config;
use crate::monitor::Trade;
use crate::positions::PositionTracker;

pub struct RiskManager {
    config: Config,
    session_notional: f64,
    positions: Arc<Mutex<PositionTracker>>,
}

impl RiskManager {
    pub fn new(config: Config, positions: Arc<Mutex<PositionTracker>>) -> Self {
        Self {
            config,
            session_notional: 0.0,
            positions,
        }
    }

    pub async fn check_trade(&self, trade: &Trade, copy_notional: f64) -> Result<(), String> {
        if copy_notional <= 0.0 {
            return Err("Copy notional is <= 0".to_owned());
        }

        if self.config.risk.max_session_notional > 0.0 {
            let next = self.session_notional + copy_notional;
            if next > self.config.risk.max_session_notional {
                return Err(format!(
                    "Session notional cap exceeded ({} > {})",
                    next, self.config.risk.max_session_notional
                ));
            }
        }

        if self.config.risk.max_per_market_notional > 0.0 && trade.side == "BUY" {
            let current = self.positions.lock().await.get_notional(&trade.token_id);
            let next = current + copy_notional;
            if next > self.config.risk.max_per_market_notional {
                return Err(format!(
                    "Per-market notional cap exceeded ({} > {})",
                    next, self.config.risk.max_per_market_notional
                ));
            }
        }

        Ok(())
    }

    pub async fn record_fill(&mut self, trade: &Trade, notional: f64, shares: f64, price: f64, side: &str) {
        self.session_notional += notional;
        self.positions
            .lock()
            .await
            .record_fill(trade, notional, shares, side, price);
    }
}
