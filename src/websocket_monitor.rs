use std::collections::HashSet;
use std::time::Duration;

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, warn};

use crate::config::Config;
use crate::types::Trade;
use crate::utils::current_time_ms;

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
    rx: Option<mpsc::Receiver<Trade>>,
    subscribe_tx: Option<mpsc::UnboundedSender<Vec<String>>>,
}

impl WebSocketMonitor {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            rx: None,
            subscribe_tx: None,
        }
    }

    pub async fn initialize(&mut self, seed_assets: Vec<String>) -> Result<()> {
        let (trade_tx, trade_rx) = mpsc::channel::<Trade>(1024);
        let (sub_tx, sub_rx) = mpsc::unbounded_channel::<Vec<String>>();

        let targets: HashSet<String> = self
            .config
            .target_wallets
            .iter()
            .map(|s| s.to_lowercase())
            .collect();

        let initial_assets: HashSet<String> = seed_assets
            .into_iter()
            .chain(self.config.monitoring.ws_asset_ids.iter().cloned())
            .filter(|s| !s.is_empty())
            .collect();

        tokio::spawn(ws_connection_loop(self.config.clone(), targets, initial_assets, trade_tx, sub_rx));

        self.rx = Some(trade_rx);
        self.subscribe_tx = Some(sub_tx);
        info!("WebSocket monitor initialized with adaptive subscriptions");
        Ok(())
    }

    pub fn try_recv_trade(&mut self) -> Option<Trade> {
        self.rx.as_mut()?.try_recv().ok()
    }

    /// Dynamically subscribe to new asset IDs (called when HTTP poller finds new markets)
    pub fn subscribe_assets(&self, asset_ids: Vec<String>) {
        if let Some(tx) = &self.subscribe_tx {
            let _ = tx.send(asset_ids);
        }
    }
}

async fn ws_connection_loop(
    config: Config,
    targets: HashSet<String>,
    initial_assets: HashSet<String>,
    trade_tx: mpsc::Sender<Trade>,
    mut sub_rx: mpsc::UnboundedReceiver<Vec<String>>,
) {
    let mut subscribed: HashSet<String> = initial_assets;
    let mut backoff_secs = 5u64;

    loop {
        let url = "wss://ws-subscriptions-clob.polymarket.com/ws/market";
        info!("WebSocket connecting to {url}...");

        let mut request_builder = tokio_tungstenite::tungstenite::handshake::client::Request::builder()
            .uri(url)
            .header("Host", "ws-subscriptions-clob.polymarket.com")
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36")
            .header("Origin", "https://polymarket.com")
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", tokio_tungstenite::tungstenite::handshake::client::generate_key());

        if let Some(token) = &config.polymarket_geo_token {
            let clean_token = token.trim_matches('\'').trim_matches('"');
            // Filter non-ASCII (like emojis) as they are illegal in HTTP headers and can cause UTF-8 parsing errors in some stacks
            let ascii_token: String = clean_token.chars().filter(|c| c.is_ascii()).collect();
            let cookie_val = format!("polymarket_geo_token={}", ascii_token);
            
            if let Ok(hv) = tokio_tungstenite::tungstenite::http::HeaderValue::from_str(&cookie_val) {
                request_builder = request_builder.header("Cookie", hv);
            }
        }

        let final_request = request_builder.body(()).unwrap();

        match connect_async(final_request).await {
            Ok((stream, _)) => {
                backoff_secs = 5; // reset on successful connect
                let (mut write, mut read) = stream.split();
                let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(20));

                // Send subscriptions for all known assets
                if !subscribed.is_empty() {
                    let asset_list: Vec<String> = subscribed.iter().cloned().collect();
                    for chunk in asset_list.chunks(50) {
                        let msg = serde_json::json!({
                            "type": "subscribe",
                            "channel": "market",
                            "assets_ids": chunk,
                        });
                        let text: String = msg.to_string();
                        if let Err(e) = write.send(Message::Text(text.into())).await {
                            error!("Failed to send WS subscription: {e}");
                        }
                    }
                    debug!("WebSocket subscribed to {} assets", subscribed.len());
                } else {
                    debug!("WebSocket connected (no assets to subscribe to yet — will adapt)");
                }

                // Main event loop
                let disconnect = loop {
                    tokio::select! {
                        biased;
                        _ = heartbeat_interval.tick() => {
                            // Send both text "PING" and binary Ping frame for bulletproof keep-alive
                            if let Err(e) = write.send(Message::Text("PING".to_string().into())).await {
                                warn!("Failed to send text WS heartbeat: {e}");
                                break false; 
                            }
                            if let Err(e) = write.send(Message::Ping(vec![].into())).await {
                                warn!("Failed to send binary WS heartbeat: {e}");
                                break false;
                            }
                        }
                        msg = read.next() => {
                            match msg {
                                Some(Ok(m)) if m.is_text() => {
                                    let raw = m.into_text().unwrap_or_default();
                                    if raw == "PING" {
                                        let _ = write.send(Message::Text("PONG".to_string().into())).await;
                                        continue;
                                    }
                                    if raw == "PONG" {
                                        continue;
                                    }
                                    if let Ok(event) = serde_json::from_str::<LastTradeMessage>(&raw) {
                                        if event.event_type.as_deref() != Some("last_trade_price") {
                                            continue;
                                        }
                                        if let Some(trade) = match_target_trade(&event, &targets) {
                                            if trade_tx.send(trade).await.is_err() {
                                                break true; // receiver dropped, shut down
                                            }
                                        }
                                    }
                                }
                                Some(Ok(Message::Ping(data))) => {
                                    let _ = write.send(Message::Pong(data)).await;
                                }
                                Some(Ok(Message::Pong(_))) => {
                                    // Received response to our binary ping
                                    continue;
                                }
                                Some(Ok(_)) => {}
                                Some(Err(e)) => {
                                    error!("WebSocket read error: {e}");
                                    break false; // reconnect
                                }
                                None => {
                                    warn!("WebSocket stream ended");
                                    break false; // reconnect
                                }
                            }
                        }
                        new_assets = sub_rx.recv() => {
                            match new_assets {
                                Some(ids) => {
                                    let new_ids: Vec<String> = ids
                                        .into_iter()
                                        .filter(|id| !id.is_empty() && subscribed.insert(id.clone()))
                                        .collect();
                                    if !new_ids.is_empty() {
                                        let msg = serde_json::json!({
                                            "type": "subscribe",
                                            "channel": "market",
                                            "assets_ids": new_ids,
                                        });
                                        let text: String = msg.to_string();
                                        if let Err(e) = write.send(Message::Text(text.into())).await {
                                            warn!("Failed to send dynamic subscription: {e}");
                                            break false; // reconnect
                                        }
                                        debug!("WebSocket dynamically subscribed to {} new assets (total: {})", new_ids.len(), subscribed.len());
                                    }
                                }
                                None => break true, // channel closed, shut down
                            }
                        }
                    }
                };

                if disconnect {
                    info!("WebSocket shutting down");
                    return;
                }
            }
            Err(e) => {
                error!("WebSocket connection failed: {e}");
            }
        }

        // Reconnect with exponential backoff
        warn!("WebSocket reconnecting in {backoff_secs}s...");
        tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
        backoff_secs = (backoff_secs * 2).min(60);
    }
}

fn match_target_trade(event: &LastTradeMessage, targets: &HashSet<String>) -> Option<Trade> {
    let m_addr = event.maker.as_ref().map(|v| v.to_lowercase());
    let t_addr = event.taker.as_ref().map(|v| v.to_lowercase());

    let matched_wallet = m_addr
        .as_ref()
        .filter(|a| targets.contains(a.as_str()))
        .cloned()
        .or_else(|| {
            t_addr
                .as_ref()
                .filter(|a| targets.contains(a.as_str()))
                .cloned()
        })?;

    let ts = event.timestamp.unwrap_or_else(current_time_ms);
    let ts = if ts < 1_000_000_000_000 { ts * 1000 } else { ts };

    Some(Trade {
        tx_hash: format!("ws-{}", current_time_ms()),
        timestamp_ms: ts,
        market: event.market.clone().unwrap_or_default(),
        token_id: event.asset_id.clone().unwrap_or_default(),
        side: event.side.clone().unwrap_or_else(|| "BUY".to_owned()),
        price: event
            .price
            .as_ref()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0),
        size_usdc: event
            .size
            .as_ref()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0),
        outcome: event
            .outcome
            .clone()
            .unwrap_or_else(|| "UNKNOWN".to_owned())
            .to_uppercase(),
        original_target_wallet: matched_wallet,
    })
}
