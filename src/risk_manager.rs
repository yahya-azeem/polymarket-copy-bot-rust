use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::warn;

use crate::config::Config;
use crate::types::Trade;
use crate::positions::PositionTracker;

pub struct RiskManager {
    config: Config,
    session_notional: f64,
    positions: Arc<Mutex<PositionTracker>>,
    
    // Daily loss tracking
    daily_start_equity: f64,
    last_reset_ms: i64,
}

impl RiskManager {
    pub fn new(config: Config, positions: Arc<Mutex<PositionTracker>>) -> Self {
        // Warn if risk limits are disabled (Bug #5)
        if config.risk.max_session_notional <= 0.0 {
            warn!("⚠️  MAX_SESSION_NOTIONAL is 0 (disabled). No session-level risk cap!");
        }
        if config.risk.max_per_market_notional <= 0.0 {
            warn!("⚠️  MAX_PER_MARKET_NOTIONAL is 0 (disabled). No per-market risk cap!");
        }

        let now = crate::utils::current_time_ms();
        Self {
            config,
            session_notional: 0.0,
            positions,
            daily_start_equity: 0.0, // Initialized on first check
            last_reset_ms: now,
        }
    }

    pub async fn check_trade(&mut self, trade: &Trade, copy_notional: f64, current_equity: f64) -> Result<(), String> {
        if copy_notional <= 0.0 {
            return Err("Copy notional is <= 0".to_owned());
        }

        let now = crate::utils::current_time_ms();
        
        // Handle Daily Reset (every 24h)
        if self.daily_start_equity <= 0.0 || (now - self.last_reset_ms) > 86_400_000 {
            self.daily_start_equity = current_equity;
            self.last_reset_ms = now;
            tracing::info!("🔄 RiskManager: Daily loss anchor reset to ${:.2}", current_equity);
        }

        // Daily Loss Check (only for BUYs)
        if trade.side == "BUY" && self.daily_start_equity > 0.0 {
            let drawdown = self.daily_start_equity - current_equity;
            let drawdown_pct = (drawdown / self.daily_start_equity) * 100.0;
            
            if drawdown_pct >= self.config.risk.max_daily_loss_percent {
                return Err(format!(
                    "Daily loss limit reached: {:.2}% drawdown (max={:.2}%)",
                    drawdown_pct, self.config.risk.max_daily_loss_percent
                ));
            }
        }

        if self.config.risk.max_session_notional > 0.0 {
            let next = self.session_notional + copy_notional;
            if next > self.config.risk.max_session_notional {
                return Err(format!(
                    "Session notional cap exceeded ({:.2} > {:.2})",
                    next, self.config.risk.max_session_notional
                ));
            }
        }

        if self.config.risk.max_per_market_notional > 0.0 && trade.side == "BUY" {
            let current = self.positions.lock().await.get_notional(&trade.token_id);
            let next = current + copy_notional;
            if next > self.config.risk.max_per_market_notional {
                return Err(format!(
                    "Per-market notional cap exceeded ({:.2} > {:.2})",
                    next, self.config.risk.max_per_market_notional
                ));
            }
        }

        Ok(())
    }

    pub async fn record_fill(
        &mut self,
        trade: &Trade,
        notional: f64,
        shares: f64,
        price: f64,
        side: &str,
    ) {
        self.session_notional += notional;
        self.positions
            .lock()
            .await
            .record_fill(trade, notional, shares, side, price);
    }
}
