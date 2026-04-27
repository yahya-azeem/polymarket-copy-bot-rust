use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use crate::config::Config;
use crate::monitor::{self, TradeMonitor};
use crate::positions::PositionTracker;
use crate::risk_manager::RiskManager;
use crate::trader::TradeExecutor;
use crate::types::{Stats, Trade};
use crate::utils::{current_time_ms, BoundedDedup};
use crate::websocket_monitor::WebSocketMonitor;

const MAX_PROCESSED_TRADES: usize = 10_000;
const RECONCILE_INTERVAL_MS: i64 = 120_000; // 2 minutes (faster sync)
const TRADE_COOLDOWN_MS: i64 = 5_000; // 5 seconds between trades

pub struct BotState {
    pub config: Config,
    pub executor: Arc<TradeExecutor>,
    pub positions: Arc<Mutex<PositionTracker>>,
    pub risk: Arc<Mutex<RiskManager>>,
    pub processed_trades: Mutex<BoundedDedup>,
    pub stats: Mutex<Stats>,
    pub last_trade_ms: Mutex<i64>,
    pub bot_start_time_ms: i64,
}

pub struct PolymarketCopyBot {
    state: Arc<BotState>,
    monitor: TradeMonitor,
    ws_monitor: Option<WebSocketMonitor>,
    last_reconcile_ms: i64,
    last_pnl_check_ms: i64,
}

impl BotState {
    pub async fn handle_new_trade(&self, trade: Trade) {
        if trade.timestamp_ms < self.bot_start_time_ms {
            return;
        }

        // Rate limiting: enforce cooldown between trades
        let now = current_time_ms();
        {
            let last_trade = self.last_trade_ms.lock().await;
            if now - *last_trade < TRADE_COOLDOWN_MS {
                warn!("Rate limit: skipping trade ({}ms since last trade, cooldown={}ms)",
                    now - *last_trade, TRADE_COOLDOWN_MS);
                return;
            }
        }

        // Whale Trade Size Filtering (Phase 2)
        if trade.size_usdc < self.config.trading.min_whale_size_usdc {
            debug!("Skipping trade from {}: size {:.2} is below min_whale_size_usdc ({:.2})", 
                trade.original_target_wallet, trade.size_usdc, self.config.trading.min_whale_size_usdc);
            return;
        }

        let keys = self.trade_keys(&trade);
        {
            let mut processed = self.processed_trades.lock().await;
            if keys.iter().any(|k| processed.contains(k)) {
                return;
            }
            for key in keys {
                processed.insert(key);
            }
        }

        {
            let mut stats = self.stats.lock().await;
            stats.trades_detected += 1;
        }

        info!(
            "🕒 [t={}] NEW TRADE | wallet={} | {} {} {} USDC @ {} | token={} market={}",
            current_time_ms(), trade.original_target_wallet, trade.side, trade.outcome, trade.size_usdc, trade.price, trade.token_id, trade.market
        );

        if trade.side == "SELL" && !self.config.trading.copy_sells {
            warn!("Skipping SELL trade (COPY_SELLS=false)");
            return;
        }

        let copy_notional = self
            .executor
            .calculate_copy_size(trade.size_usdc, &trade.original_target_wallet)
            .await;

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
            let cash = self.executor.get_your_balance_usdc().await.unwrap_or(0.0);
            let pos_value = {
                let guard = self.positions.lock().await;
                guard.get_all().iter().map(|p| p.shares * p.avg_price).sum::<f64>()
            };
            let mut guard = self.risk.lock().await;
            guard.check_trade(&trade, copy_notional, cash + pos_value).await
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
                
                let mut stats = self.stats.lock().await;
                stats.trades_copied += 1;
                stats.total_volume += fill.copy_notional;
                *self.last_trade_ms.lock().await = current_time_ms();
                
                info!(
                    "✅ Copied: order_id={} | {:.2} USDC | volume={:.2} | copied={}/detected={}",
                    fill.order_id, fill.copy_notional, stats.total_volume,
                    stats.trades_copied, stats.trades_detected
                );

                if self.config.run.exit_after_first_sell_copy && fill.side == "SELL" {
                    info!("EXIT_AFTER_FIRST_SELL_COPY triggered, exiting");
                    std::process::exit(0);
                }
            }
            Err(e) => {
                let mut stats = self.stats.lock().await;
                stats.trades_failed += 1;
                error!("❌ Failed to copy trade: {e}");
            }
        }
    }

    fn trade_keys(&self, trade: &Trade) -> Vec<String> {
        let mut keys = Vec::new();
        if !trade.tx_hash.is_empty() {
            keys.push(format!("{}-{}", trade.original_target_wallet, trade.tx_hash));
        }
        keys.push(format!(
            "{}|{}|{}|{}|{}|{}",
            trade.original_target_wallet, trade.token_id, trade.side, trade.size_usdc, trade.price, trade.timestamp_ms
        ));
        keys
    }
}

impl PolymarketCopyBot {
    pub async fn new(mut config: Config) -> Result<Self> {
        if config.use_polywhaler_leaderboard {
            match crate::leaderboard::fetch_top_whales(10).await {
                Ok(whales) => {
                    for whale in whales {
                        if !config.target_wallets.contains(&whale) {
                            info!("🌟 Polywhaler Addon: Auto-following top trader {}", whale);
                            config.target_wallets.push(whale);
                        }
                    }
                }
                Err(e) => warn!("Failed to fetch leaderboard whales: {e}"),
            }
        }

        let monitor = monitor::TradeMonitor::new(config.clone());
        let executor = Arc::new(TradeExecutor::new(config.clone()).await?);
        let positions = Arc::new(Mutex::new(PositionTracker::default()));
        let risk = Arc::new(Mutex::new(RiskManager::new(config.clone(), positions.clone())));
        let now = current_time_ms();

        let state = Arc::new(BotState {
            config,
            executor,
            positions,
            risk,
            processed_trades: Mutex::new(BoundedDedup::new(MAX_PROCESSED_TRADES)),
            stats: Mutex::new(Stats::default()),
            last_trade_ms: Mutex::new(0),
            bot_start_time_ms: now,
        });

        Ok(Self {
            state,
            monitor,
            ws_monitor: None,
            last_reconcile_ms: now,
            last_pnl_check_ms: now,
        })
    }

    pub async fn initialize(&mut self) -> Result<()> {
        // Initialize executor FIRST (derives API keys and connects to CLOB)
        self.state.executor.initialize().await?;
        
        // Now it's safe to fetch balance
        let exchange_bal = self.state.executor.get_your_balance_usdc().await.unwrap_or(0.0);
        let addr = self.state.executor.get_address().await;

        info!("═══════════════════════════════════════════");
        info!("  Polymarket Copy Trading Bot (Rust)");
        info!("═══════════════════════════════════════════");
        info!("Your Address: {}", addr);
        info!("Signature Type: {}", self.state.config.polymarket_signature_type);

        if let Ok((usdc, usdce)) = self.state.executor.get_onchain_balances().await {
            info!("💰 WALLET HUB (On-Chain):");
            info!("   Native USDC:  {:.2}", usdc);
            info!("   Bridged USDce: {:.2} (Required for trading)", usdce);
        }

        info!("💰 EXCHANGE HUB (Polymarket):");
        info!("   Balance: {:.2} USDC", exchange_bal);

        if exchange_bal <= 1.0 {
            warn!("💡 ALERT: Exchange balance appears low ({:.2} USDC).", exchange_bal);
            warn!("   If your wallet has funds, you must DEPOSIT them at Polymarket.com.");
        }
        info!("═══════════════════════════════════════════");
        
        // Seed WebSocket subscriptions
        let seed_assets: Vec<String> = if let Ok(trades) = self.monitor.poll_for_new_trades(true).await {
            trades.into_iter().map(|t| t.token_id).collect()
        } else {
            Vec::new()
        };

        let mut ws = WebSocketMonitor::new(self.state.config.clone());
        ws.initialize(seed_assets).await?;
        self.ws_monitor = Some(ws);
        
        info!("Monitor initialized at {}", current_time_ms());
        
        // Final Phase: Cross-reconcile with Whales (State Recovery)
        self.sync_state_with_whales().await;
        
        Ok(())
    }

    pub async fn run(&mut self) -> Result<()> {
        info!("Bot started — monitoring for trades...");

        loop {
            // WS Trades
            let mut ws_trades = Vec::new();
            if let Some(ws) = &mut self.ws_monitor {
                while let Some(trade) = ws.try_recv_trade() {
                    ws_trades.push(trade);
                }
            }
            for trade in ws_trades {
                let state = self.state.clone();
                
                // DYNAMIC PNL CHECK: If this price update is for a token we OWN, check TP/SL immediately
                let token_id = trade.token_id.clone();
                let state_for_pnl = self.state.clone();
                tokio::spawn(async move {
                    let has_pos = {
                        let guard = state_for_pnl.positions.lock().await;
                        guard.get_position(&token_id).map(|p| p.shares > 0.0).unwrap_or(false)
                    };
                    if has_pos {
                        let _ = state_for_pnl.executor.check_profit_taking(state_for_pnl.positions.clone(), Some(token_id)).await;
                    }
                });

                tokio::spawn(async move {
                    state.handle_new_trade(trade).await;
                });
            }

            // Poll HTTP API
            match self.monitor.poll_for_new_trades(false).await {
                Ok(trades) => {
                    for trade in trades {
                        // Dynamically subscribe WS to new markets
                        if let Some(ws) = &self.ws_monitor {
                            ws.subscribe_assets(vec![trade.token_id.clone()]);
                        }
                        
                        let state = self.state.clone();
                        tokio::spawn(async move {
                            state.handle_new_trade(trade).await;
                        });
                    }
                }
                Err(e) => error!("Monitoring error: {e}"),
            }

            // Periodic PnL / Profit-Taking Check (Adaptive)
            let now = current_time_ms();
            let is_holding = {
                let guard = self.state.positions.lock().await;
                guard.get_all().iter().any(|p| p.shares > 0.0)
            };
            
            let pnl_interval = if is_holding { 10_000 } else { 60_000 }; // 10s if holding, 60s if flat
            
            if now - self.last_pnl_check_ms > pnl_interval {
                let state = self.state.clone();
                tokio::spawn(async move {
                    if let Err(e) = state.executor.check_profit_taking(state.positions.clone(), None).await {
                        warn!("PnL check failed: {e}");
                    }
                });
                self.last_pnl_check_ms = now;
            }

            // Periodic position reconciliation (every 5 minutes)
            if now - self.last_reconcile_ms > RECONCILE_INTERVAL_MS {
                self.reconcile_positions().await;
                self.last_reconcile_ms = now;
            }

            tokio::time::sleep(Duration::from_millis(self.state.config.monitoring.poll_interval_ms)).await;
        }
    }

    async fn reconcile_positions(&mut self) {
        // Clear stale limit orders (Phase 2)
        if let Err(e) = self.state.executor.cancel_stale_orders().await {
            warn!("Failed to clean up stale orders: {e}");
        }

        match self.state.executor.get_positions().await {
            Ok(positions) => {
                let mut tracker = self.state.positions.lock().await;
                let (loaded, skipped) = tracker.load_from_data_api_positions(&positions);
                if loaded > 0 {
                    info!("Positions reconciled: {} loaded, {} skipped", loaded, skipped);
                }
            }
            Err(e) => warn!("Positions reconciliation failed: {e}"),
        }
    }

    async fn sync_state_with_whales(&self) {
        info!("🕒 [t={}] Syncing portfolio state with whales (State Recovery)...", current_time_ms());
        
        // 1. Get your positions
        let your_positions = match self.state.executor.get_positions().await {
            Ok(p) => p,
            Err(e) => {
                warn!("State Recovery: Could not fetch your positions: {e}");
                return;
            },
        };
        
        if your_positions.is_empty() {
            info!("✅ Portfolio is clean. No existing positions to reconcile.");
            return;
        }

        // 2. Get all whale positions (collect unique token IDs held by whales)
        let mut whale_tokens = std::collections::HashSet::new();
        for whale in &self.state.config.target_wallets {
            if let Ok(positions) = self.state.executor.get_positions_for_user(whale).await {
                for pos in positions {
                    if let Some(token_id) = pos.get("asset").and_then(|v| v.as_str()) {
                        whale_tokens.insert(token_id.to_string());
                    }
                }
            }
        }

        // 3. Identify abandoned positions
        let mut abandoned_count = 0;
        for pos in your_positions {
            let token_id = pos.get("asset").and_then(|v| v.as_str()).unwrap_or("");
            let size = pos.get("size").and_then(|v| v.as_str()).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
            
            if size > 0.0 && !whale_tokens.contains(token_id) {
                abandoned_count += 1;
                warn!("⚠️ ABANDONED POSITION: Whales have exited {}, but you still hold it.", token_id);
                info!("💡 TIP: If you want the bot to automatically exit these, you can enable 'AUTO_EXIT_ABANDONED=true' (Coming soon).");
            }
        }
        
        if abandoned_count == 0 {
            info!("✅ State Recovery complete: All local positions are currently held by whales.");
        } else {
            warn!("🚨 State Recovery complete: {} abandoned positions found. Consider manual review/exit.", abandoned_count);
        }
    }
}
