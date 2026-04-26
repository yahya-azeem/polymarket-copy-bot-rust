use std::time::Duration;

use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;
use tracing::{debug, error, info, warn};
use crate::types::Trade;

use crate::config::Config;
use crate::utils::BoundedDedup;


#[derive(Debug, Deserialize, Default)]
struct DataApiTrade {
    #[serde(rename = "transactionHash", default)]
    transaction_hash: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    timestamp: i64,
    #[serde(rename = "conditionId", default)]
    condition_id: Option<String>,
    #[serde(default)]
    market: Option<String>,
    #[serde(default)]
    asset: Option<String>,
    #[serde(default)]
    side: Option<String>,
    #[serde(default)]
    price: Option<serde_json::Value>,
    #[serde(rename = "usdcSize", default)]
    usdc_size: Option<serde_json::Value>,
    #[serde(default)]
    size: Option<serde_json::Value>,
    #[serde(default)]
    outcome: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DataApiResponse {
    value: Vec<DataApiTrade>,
}

pub struct TradeMonitor {
    client: Client,
    config: Config,
    last_processed_timestamp_ms: i64,
    processed_trade_ids: BoundedDedup,
    check_count: u64,
}

impl TradeMonitor {
    pub fn new(config: Config) -> Self {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36",
            ),
        );

        Self {
            client: Client::builder().default_headers(headers).build().unwrap_or_else(|_| Client::new()),
            config,
            last_processed_timestamp_ms: 0,
            processed_trade_ids: BoundedDedup::new(10_000),
            check_count: 0,
        }
    }

    #[allow(dead_code)]
    pub fn initialize(&mut self, start_time_ms: i64) {
        self.last_processed_timestamp_ms = start_time_ms;
        info!("Monitor initialized at {start_time_ms}");
    }

    pub async fn poll_for_new_trades(&mut self, seed_only: bool) -> Result<Vec<Trade>> {
        self.check_count += 1;
        
        // Log "Active" status every 200 checks (~6.6 minutes at 2s interval)
        if !seed_only && self.check_count % 200 == 0 {
            info!("🔍 Monitoring [{} Whales] — Loop #{} | Last Check: {} | No new trades recently.", 
                self.config.target_wallets.len(), self.check_count, crate::utils::current_time_ms());
        }

        let trades = self.fetch_trades_from_data_api().await?;
        if trades.is_empty() {
            return Ok(Vec::new());
        }

        let mut sorted = trades;
        sorted.sort_by_key(|t| t.timestamp_ms);

        if seed_only {
            // In seed mode, we return all recent trades to identify active markets
            // but we mark them as processed so we don't copy old history.
            for trade in &sorted {
                self.processed_trade_ids.insert(trade.tx_hash.clone());
            }
            return Ok(sorted);
        }

        let mut out = Vec::new();
        for trade in sorted {
            let trade_id = trade.tx_hash.clone();
            if !self.processed_trade_ids.insert(trade_id) {
                // Already seen — BoundedDedup.insert returns false if duplicate
                continue;
            }
            if trade.timestamp_ms <= self.last_processed_timestamp_ms {
                continue;
            }

            self.last_processed_timestamp_ms = self.last_processed_timestamp_ms.max(trade.timestamp_ms);
            out.push(trade);
        }

        Ok(out)
    }

    async fn fetch_trades_from_data_api(&self) -> Result<Vec<Trade>> {
        let mut all_trades = Vec::new();

        for wallet in &self.config.target_wallets {
            debug!("Polling Data API for wallet: {}", wallet);
            let rows = self.fetch_with_retry(wallet).await;
            if !rows.is_empty() {
                debug!("Found {} activity items for {}", rows.len(), wallet);
            }
            for row in rows {
                all_trades.push(parse_data_trade(row, wallet.clone()));
            }
        }

        Ok(all_trades)
    }

    /// Fetches trades with exponential backoff retry (max 3 attempts)
    async fn fetch_with_retry(&self, wallet: &str) -> Vec<DataApiTrade> {
        let url = "https://data-api.polymarket.com/activity";
        let max_attempts = 3u32;

        for attempt in 1..=max_attempts {
            let resp = self
                .client
                .get(url)
                .query(&[
                    ("user", wallet.to_lowercase()),
                    ("type", "TRADE".to_owned()),
                    ("limit", "20".to_owned()),
                    ("sortBy", "TIMESTAMP".to_owned()),
                    ("sortDirection", "DESC".to_owned()),
                ])
                .send()
                .await;

            match resp {
                Ok(r) if r.status().is_success() => {
                    let text = r.text().await.unwrap_or_default();
                    
                    // Simple, direct parsing that worked in test_api.rs
                    if let Ok(trades) = serde_json::from_str::<Vec<DataApiTrade>>(&text) {
                        debug!("Successfully extracted {} trades for {}", trades.len(), wallet);
                        return trades;
                    } 
                    
                    // Fallback for OData-style { value: [] }
                    if let Ok(resp) = serde_json::from_str::<DataApiResponse>(&text) {
                        debug!("Successfully extracted {} trades for {} (OData)", resp.value.len(), wallet);
                        return resp.value;
                    }

                    error!("❌ DATA API format unknown. Body: {}", text);
                    return Vec::new();
                }
                Ok(r) => {
                    let status = r.status();
                    if attempt >= max_attempts {
                        warn!("Data API for {} returned {} after {} attempts", wallet, status, attempt);
                        return Vec::new();
                    }
                    let delay = Duration::from_millis(500 * 2u64.pow(attempt - 1));
                    warn!("Data API {} returned {}, retry in {:?} ({}/{})", wallet, status, delay, attempt, max_attempts);
                    tokio::time::sleep(delay).await;
                }
                Err(e) => {
                    if attempt >= max_attempts {
                        warn!("Data API request for {} failed after {} attempts: {e}", wallet, attempt);
                        return Vec::new();
                    }
                    let delay = Duration::from_millis(500 * 2u64.pow(attempt - 1));
                    warn!("Data API error: {e}, retry in {:?} ({}/{})", delay, attempt, max_attempts);
                    tokio::time::sleep(delay).await;
                }
            }
        }

        Vec::new()
    }
}

fn parse_data_trade(api: DataApiTrade, original_target_wallet: String) -> Trade {
    let tx_hash = api
        .transaction_hash
        .or(api.id)
        .unwrap_or_else(|| format!("trade-{}", api.timestamp));
    let side = api.side.unwrap_or_else(|| "UNKNOWN".to_owned()).to_uppercase();
    let outcome = api
        .outcome
        .unwrap_or_else(|| "UNKNOWN".to_owned())
        .to_uppercase();

    let price = match api.price {
        Some(serde_json::Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(serde_json::Value::String(s)) => s.parse().unwrap_or(0.0),
        _ => 0.0,
    };

    let size_val = match api.usdc_size.or(api.size) {
        Some(serde_json::Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(serde_json::Value::String(s)) => s.parse().unwrap_or(0.0),
        _ => 0.0,
    };

    Trade {
        tx_hash,
        timestamp_ms: api.timestamp * 1000,
        market: api.condition_id.or(api.market).unwrap_or_default(),
        token_id: api.asset.unwrap_or_default(),
        side,
        price,
        size_usdc: size_val,
        outcome,
        original_target_wallet,
    }
}
