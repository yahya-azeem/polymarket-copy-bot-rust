use std::collections::HashSet;

use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;
use tracing::{info, warn};

use crate::config::Config;

#[derive(Debug, Clone)]
pub struct Trade {
    pub tx_hash: String,
    pub timestamp_ms: i64,
    pub market: String,
    pub token_id: String,
    pub side: String,
    pub price: f64,
    pub size_usdc: f64,
    pub outcome: String,
    pub original_target_wallet: String,
}

#[derive(Debug, Deserialize)]
struct DataApiTrade {
    #[serde(rename = "transactionHash")]
    transaction_hash: Option<String>,
    id: Option<String>,
    timestamp: i64,
    #[serde(rename = "conditionId")]
    condition_id: Option<String>,
    market: Option<String>,
    asset: String,
    side: String,
    price: String,
    #[serde(rename = "usdcSize")]
    usdc_size: Option<String>,
    size: Option<String>,
    outcome: Option<String>,
}

pub struct TradeMonitor {
    client: Client,
    config: Config,
    last_processed_timestamp_ms: i64,
    processed_trade_ids: HashSet<String>,
}

impl TradeMonitor {
    pub fn new(config: Config) -> Self {
        Self {
            client: Client::new(),
            config,
            last_processed_timestamp_ms: 0,
            processed_trade_ids: HashSet::new(),
        }
    }

    pub fn initialize(&mut self, start_time_ms: i64) {
        self.last_processed_timestamp_ms = start_time_ms;
        info!("Monitor initialized at {start_time_ms}");
    }

    pub async fn poll_for_new_trades(&mut self) -> Result<Vec<Trade>> {
        let trades = self.fetch_trades_from_data_api().await?;
        if trades.is_empty() {
            return Ok(Vec::new());
        }

        let mut sorted = trades;
        sorted.sort_by_key(|t| t.timestamp_ms);

        let mut out = Vec::new();
        for trade in sorted {
            let trade_id = trade.tx_hash.clone();
            if self.processed_trade_ids.contains(&trade_id) {
                continue;
            }
            if trade.timestamp_ms <= self.last_processed_timestamp_ms {
                continue;
            }

            self.processed_trade_ids.insert(trade_id);
            self.last_processed_timestamp_ms = self.last_processed_timestamp_ms.max(trade.timestamp_ms);
            out.push(trade);
        }

        if self.processed_trade_ids.len() > 10_000 {
            let keep: HashSet<String> = self.processed_trade_ids.iter().take(5_000).cloned().collect();
            self.processed_trade_ids = keep;
        }

        Ok(out)
    }

    async fn fetch_trades_from_data_api(&self) -> Result<Vec<Trade>> {
        let start_seconds = (self.last_processed_timestamp_ms / 1000) + 1;
        let url = "https://data-api.polymarket.com/activity";
 
        let mut all_trades = Vec::new();
 
        for wallet in &self.config.target_wallets {
            let resp = self
                .client
                .get(url)
                .query(&[
                    ("user", wallet.to_lowercase()),
                    ("type", "TRADE".to_owned()),
                    ("limit", "50".to_owned()),
                    ("sortBy", "TIMESTAMP".to_owned()),
                    ("sortDirection", "DESC".to_owned()),
                    ("start", start_seconds.to_string()),
                ])
                .send()
                .await?;
 
            if !resp.status().is_success() {
                warn!("Data API for {} returned {}", wallet, resp.status());
                continue;
            }
 
            let rows: Vec<DataApiTrade> = resp.json().await.unwrap_or_default();
            for row in rows {
                all_trades.push(parse_data_trade(row, wallet.clone()));
            }
        }
 
        Ok(all_trades)
    }
}
 
fn parse_data_trade(api: DataApiTrade, original_target_wallet: String) -> Trade {
    let tx_hash = api
        .transaction_hash
        .or(api.id)
        .unwrap_or_else(|| format!("trade-{}", api.timestamp));
    let side = api.side.to_uppercase();
    let outcome = api
        .outcome
        .unwrap_or_else(|| "UNKNOWN".to_owned())
        .to_uppercase();
 
    let size_str = api.usdc_size.or(api.size).unwrap_or_else(|| "0".to_owned());
 
    Trade {
        tx_hash,
        timestamp_ms: api.timestamp * 1000,
        market: api.condition_id.or(api.market).unwrap_or_default(),
        token_id: api.asset,
        side,
        price: api.price.parse().unwrap_or(0.0),
        size_usdc: size_str.parse().unwrap_or(0.0),
        outcome,
        original_target_wallet,
    }
}
