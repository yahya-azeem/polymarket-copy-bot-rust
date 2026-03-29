use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use futures_util::StreamExt;
use serde::Deserialize;
use tokio::sync::mpsc::{self, Receiver};
use tokio_tungstenite::connect_async;
use tracing::{error, info};

use crate::config::Config;
use crate::monitor::Trade;

#[derive(Debug, Deserialize)]
struct LastTradeMessage {
    event_type: Option<String>,
    asset_id: Option<String>,
    market: Option<String>,
    side: Option<String>,
    price: Option<String>,
    size: Option<String>,
    timestamp: Option<i64>,
    outcome: Option<String>,
    maker: Option<String>,
    taker: Option<String>,
}

pub struct WebSocketMonitor {
    config: Config,
    rx: Option<Receiver<Trade>>,
}

impl WebSocketMonitor {
    pub fn new(config: Config) -> Self {
        Self { config, rx: None }
    }

    pub async fn initialize(&mut self) -> Result<()> {
        let url = if self.config.monitoring.use_user_channel {
            "wss://ws-subscriptions-clob.polymarket.com/ws/user"
        } else {
            "wss://ws-subscriptions-clob.polymarket.com/ws/market"
        };

        let (stream, _) = connect_async(url).await?;
        let (_, mut read) = stream.split();
        let (tx, rx) = mpsc::channel::<Trade>(1024);
        let targets: HashSet<String> = self.config.target_wallets.iter().map(|s| s.to_lowercase()).collect();

        tokio::spawn(async move {
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(m) if m.is_text() => {
                        let raw = m.into_text().unwrap_or_default();
                        if raw == "PING" || raw == "PONG" {
                            continue;
                        }
                        let parsed = serde_json::from_str::<LastTradeMessage>(&raw);
                        let Ok(event) = parsed else {
                            continue;
                        };
                        if event.event_type.as_deref() != Some("last_trade_price") {
                            continue;
                        }

                        let m_addr = event.maker.as_ref().map(|v| v.to_lowercase());
                        let t_addr = event.taker.as_ref().map(|v| v.to_lowercase());
 
                        let matched_wallet = if let Some(addr) = m_addr {
                            if targets.contains(&addr) { Some(addr) } else { None }
                        } else {
                            None
                        };
 
                        let matched_wallet = matched_wallet.or_else(|| {
                            if let Some(addr) = t_addr {
                                if targets.contains(&addr) { Some(addr) } else { None }
                            } else {
                                None
                            }
                        });
 
                        let Some(original_target_wallet) = matched_wallet else {
                            continue;
                        };

                        let ts = event.timestamp.unwrap_or_else(now_ms);
                        let ts = if ts < 1_000_000_000_000 { ts * 1000 } else { ts };
                        let trade = Trade {
                            tx_hash: format!("ws-{}", now_ms()),
                            timestamp_ms: ts,
                            market: event.market.unwrap_or_default(),
                            token_id: event.asset_id.unwrap_or_default(),
                            side: event.side.unwrap_or_else(|| "BUY".to_owned()),
                            price: event.price.and_then(|v| v.parse().ok()).unwrap_or(0.0),
                            size_usdc: event.size.and_then(|v| v.parse().ok()).unwrap_or(0.0),
                            outcome: event
                                .outcome
                                .unwrap_or_else(|| "UNKNOWN".to_owned())
                                .to_uppercase(),
                            original_target_wallet,
                        };
                        if tx.send(trade).await.is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        error!("WebSocket read error: {e}");
                        break;
                    }
                }
            }
        });

        self.rx = Some(rx);
        info!("WebSocket monitor initialized");
        Ok(())
    }

    pub fn try_recv_trade(&mut self) -> Option<Trade> {
        if let Some(rx) = &mut self.rx {
            rx.try_recv().ok()
        } else {
            None
        }
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
