use std::time::{SystemTime, UNIX_EPOCH};
use std::str::FromStr;
use std::sync::Arc;
 
use alloy::signers::local::PrivateKeySigner;
use anyhow::{Result, anyhow, bail};
use polymarket_client_sdk::POLYGON;
use polymarket_client_sdk::auth::{Credentials, ExposeSecret};
use polymarket_client_sdk::auth::{Normal, state::Authenticated};
use polymarket_client_sdk::auth::Signer as _;
use polymarket_client_sdk::clob::types::request::{
    BalanceAllowanceRequest, OrderBookSummaryRequest, UpdateBalanceAllowanceRequest,
};
use polymarket_client_sdk::clob::types::{Amount, AssetType, OrderType, Side, SignatureType};
use polymarket_client_sdk::clob::{Client as ClobClient, Config as ClobConfig};
use polymarket_client_sdk::types::{Decimal, U256};
use reqwest::Client;
use serde_json::Value;
use tokio::sync::RwLock;
use tracing::{error, info, warn};
 
use crate::config::Config;
use crate::monitor::Trade;
 
const DATA_API_POSITIONS: &str = "https://data-api.polymarket.com/positions";
const CLOB_HOST: &str = "https://clob.polymarket.com";
const SIM_BASE: &str = "https://api.polysimulator.com/v1";
 
type AuthClient = ClobClient<Authenticated<Normal>>;
 
#[derive(Debug, Clone)]
pub struct CopyExecutionResult {
    pub order_id: String,
    pub copy_notional: f64,
    pub copy_shares: f64,
    pub price: f64,
    pub side: String,
}
 
pub struct TradeExecutor {
    config: Config,
    signer: Arc<RwLock<Option<PrivateKeySigner>>>,
    clob: Arc<RwLock<Option<AuthClient>>>,
    creds: Arc<RwLock<Option<Credentials>>>,
    http: Client,
}
 
impl TradeExecutor {
    pub async fn new(config: Config) -> Result<Self> {
        let signer = if !config.simulation_mode {
            Some(PrivateKeySigner::from_str(&config.private_key)?.with_chain_id(Some(POLYGON)))
        } else {
            None
        };
        
        Ok(Self {
            config,
            signer: Arc::new(RwLock::new(signer)),
            clob: Arc::new(RwLock::new(None)),
            creds: Arc::new(RwLock::new(None)),
            http: Client::new(),
        })
    }
 
    pub async fn initialize(&self) -> Result<()> {
        if self.config.simulation_mode {
            info!("Simulation Mode Enabled. Skipping Polymarket CLOB authentication.");
            return Ok(());
        }
 
        let signer_guard = self.signer.read().await;
        let signer = signer_guard.as_ref().ok_or_else(|| anyhow!("signer missing in live mode"))?;
 
        let unauth = ClobClient::new(
            CLOB_HOST,
            ClobConfig::builder().use_server_time(true).build(),
        )?;
 
        let creds = unauth
            .create_or_derive_api_key(signer, None)
            .await
            .map_err(|e| anyhow!("failed to derive/create api key: {e}"))?;
 
        let auth = unauth
            .authentication_builder(signer)
            .credentials(creds.clone())
            .signature_type(SignatureType::Eoa)
            .authenticate()
            .await?;
 
        let _ = auth.api_keys().await?;
        info!("API credentials initialized.");
 
        {
            *self.creds.write().await = Some(creds);
            *self.clob.write().await = Some(auth);
        }
 
        self.check_geoblock().await?;
        self.ensure_approvals().await?;
        Ok(())
    }
 
    pub async fn calculate_copy_size(&self, original_size: f64, target_wallet: &str) -> f64 {
        let t = &self.config.trading;
        let mut size = if t.use_sizing_model {
            match self.compute_sizing_model_notional(original_size, target_wallet).await {
                Ok(v) => v,
                Err(err) => {
                    warn!("Sizing model unavailable for {}, falling back to POSITION_MULTIPLIER: {err}", target_wallet);
                    original_size * t.position_multiplier
                }
            }
        } else {
            original_size * t.position_multiplier
        };
        size = size.min(t.max_trade_size);
        let market_min = if t.order_type == "FOK" || t.order_type == "FAK" {
            1.0
        } else {
            t.min_trade_size
        };
        size = size.max(market_min);
        (size * 100.0).round() / 100.0
    }
 
    pub fn calculate_shares_for_notional(&self, notional: f64, price: f64) -> f64 {
        let shares = if price > 0.0 { notional / price } else { 0.0 };
        (shares * 10_000.0).round() / 10_000.0
    }
 
    async fn compute_sizing_model_notional(&self, target_position_size: f64, target_wallet: &str) -> Result<f64> {
        let now = current_time_ms();
        info!("🕒 [t={}] Refreshing fresh LIVE balance for sizing...", now);
        
        let your_balance = self.get_your_balance_usdc().await?;
        info!("💰 [t={}] Fresh Cash: {:.4} USDC", current_time_ms(), your_balance);
  
        let target_balance = if let Some(v) = self.config.trading.target_balance_override {
            v
        } else {
            self.get_target_balance_usdc(target_wallet).await?
        };
 
        if your_balance <= 0.0 {
            bail!("your_balance is <= 0");
        }
        if target_balance <= 0.0 {
            bail!("target_balance is <= 0");
        }
 
        let multiplier = self.config.trading.sizing_multiplier;
        let proportional = (your_balance / target_balance) * target_position_size * multiplier;
        let literal = target_position_size * multiplier;
  
        let mut chosen = if self.config.trading.prefer_literal_whale_size && (proportional < literal) {
            info!("🕒 [t={}] Info: Using LITERAL fallback (Target Size * Multiplier)", current_time_ms());
            literal
        } else {
            info!("🕒 [t={}] Info: Using PROPORTIONAL mirroring", current_time_ms());
            proportional
        };
 
        // Safety: Cap at X% of your own wallet balance
        let wallet_cap = your_balance * self.config.trading.max_percent_of_balance;
        
        if chosen > wallet_cap {
            info!("Sizing: capping trade to {:.2} USDC ({}% of wallet buffer)", wallet_cap, self.config.trading.max_percent_of_balance * 100.0);
            chosen = wallet_cap;
        }
 
        info!(
            "Sizing model | wallet={} your_bal={:.2} target_bal={:.2} target_sz={:.2} chosen={:.2}",
            target_wallet,
            your_balance,
            target_balance,
            target_position_size,
            chosen
        );
        Ok(chosen)
    }
 
    async fn get_your_balance_usdc(&self) -> Result<f64> {
        if self.config.simulation_mode {
            let api_key = self.config.polysimulator_api_key.as_ref().ok_or_else(|| anyhow!("SIM API KEY missing"))?;
            let url = format!("{}/portfolio", SIM_BASE);
            let resp = self.http.get(url).bearer_auth(api_key).send().await?;
            let res: Value = resp.json().await?;
            return Ok(res.get("cash").and_then(|v| v.as_f64()).unwrap_or(0.0));
        }
  
        let clob = self
            .clob
            .read()
            .await
            .as_ref()
            .cloned()
            .ok_or_else(|| anyhow!("trader not initialized"))?;
        let bal = clob
            .balance_allowance(
                BalanceAllowanceRequest::builder()
                    .asset_type(AssetType::Collateral)
                    .signature_type(SignatureType::Eoa)
                    .build(),
            )
            .await?;
        Ok(bal.balance.to_string().parse::<f64>().unwrap_or(0.0))
    }
 
    async fn get_target_balance_usdc(&self, target_wallet: &str) -> Result<f64> {
        let t_start = current_time_ms();
        info!("🕒 [t={}] Fetching target balance ({})", t_start, target_wallet);
        let url = "https://data-api.polymarket.com/value";
        let resp = self
            .http
            .get(url)
            .query(&[("user", target_wallet.to_lowercase())])
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("target value endpoint returned {}", resp.status());
        }
  
        let value: Value = resp.json().await?;
        let target_val = if let Some(arr) = value.as_array() {
            arr.get(0).and_then(|row| row.get("value")).and_then(to_f64)
        } else {
            value.get("value").and_then(to_f64).or_else(|| to_f64(&value))
        }.unwrap_or(0.0);
  
        info!("🕒 [t={}] Target Value: {:.4} USDC", current_time_ms(), target_val);
        Ok(target_val)
    }
 
    pub async fn execute_copy_trade(
        &self,
        original_trade: &Trade,
        copy_notional_override: Option<f64>,
    ) -> Result<CopyExecutionResult> {
        if self.config.simulation_mode {
            return self.execute_simulation_trade(original_trade, copy_notional_override).await;
        }
  
        self.check_geoblock().await?;
  
        let copy_notional = if let Some(v) = copy_notional_override {
            v
        } else {
            self.calculate_copy_size(original_trade.size_usdc, &original_trade.original_target_wallet).await
        };
        self.validate_balance_or_shares(&original_trade.side, copy_notional, &original_trade.token_id)
            .await?;
 
        let token_id = U256::from_str(&original_trade.token_id)?;
        let clob = self
            .clob
            .read()
            .await
            .as_ref()
            .cloned()
            .ok_or_else(|| anyhow!("trader not initialized"))?;
  
        info!("🕒 [t={}] Fetching LIVE Order Book for token {}", current_time_ms(), original_trade.token_id);
        let book = clob
            .order_book(&OrderBookSummaryRequest::builder().token_id(token_id).build())
            .await?;
        info!("🕒 [t={}] Order Book Refreshed", current_time_ms());
  
        let best_price = if original_trade.side == "BUY" {
            book.asks
                .first()
                .map(|s| s.price)
                .unwrap_or(Decimal::from_str(&original_trade.price.to_string())?)
        } else {
            book.bids
                .first()
                .map(|s| s.price)
                .unwrap_or(Decimal::from_str(&original_trade.price.to_string())?)
        };
  
        info!("🕒 [t={}] Best Match Price: {}", current_time_ms(), best_price);
 
        let mut px = best_price;
        if original_trade.side == "BUY" {
            px *= Decimal::from_str(&(1.0 + self.config.trading.slippage_tolerance).to_string())?;
        } else {
            px *= Decimal::from_str(&(1.0 - self.config.trading.slippage_tolerance).to_string())?;
        }
        let px_f = px.to_string().parse::<f64>().unwrap_or(original_trade.price);
        let px_f = px_f.clamp(0.01, 0.99);
        let price = Decimal::from_str(&format!("{:.4}", px_f))?;
        let copy_shares = self.calculate_shares_for_notional(copy_notional, px_f);
 
        let order_type = self.config.trading.order_type.as_str();
        let side = if original_trade.side == "BUY" { Side::Buy } else { Side::Sell };
 
        let signer_guard = self.signer.read().await;
        let signer = signer_guard.as_ref().ok_or_else(|| anyhow!("signer missing in live mode"))?;
 
        let post = if order_type == "LIMIT" {
            let order = clob
                .limit_order()
                .token_id(token_id)
                .price(price)
                .size(Decimal::from_str(&copy_shares.to_string())?)
                .side(side)
                .order_type(OrderType::GTC)
                .build()
                .await?;
            let signed = clob.sign(signer, order).await?;
            clob.post_order(signed).await?
        } else {
            let order_t = if order_type == "FAK" {
                OrderType::FAK
            } else {
                OrderType::FOK
            };
            let amount = if original_trade.side == "BUY" {
                Amount::usdc(Decimal::from_str(&copy_notional.to_string())?)?
            } else {
                Amount::shares(Decimal::from_str(&copy_shares.to_string())?)?
            };
 
            let order = clob
                .market_order()
                .token_id(token_id)
                .amount(amount)
                .side(side)
                .order_type(order_t)
                .build()
                .await?;
            let signed = clob.sign(signer, order).await?;
            info!("🕒 [t={}] Order signed, submitting to Polymarket CLOB...", current_time_ms());
            clob.post_order(signed).await?
        };
  
        info!("🕒 [t={}] Order Result: success={}", current_time_ms(), post.success);
 
        if !post.success {
            let msg = post.error_msg.unwrap_or_else(|| "unknown post order error".to_owned());
            bail!("order placement failed: {msg}");
        }
 
        Ok(CopyExecutionResult {
            order_id: post.order_id,
            copy_notional,
            copy_shares,
            price: px_f,
            side: original_trade.side.clone(),
        })
    }
 
    async fn execute_simulation_trade(&self, original_trade: &Trade, copy_notional_override: Option<f64>) -> Result<CopyExecutionResult> {
        let copy_notional = if let Some(v) = copy_notional_override {
            v
        } else {
            self.calculate_copy_size(original_trade.size_usdc, &original_trade.original_target_wallet).await
        };
 
        let api_key = self.config.polysimulator_api_key.as_ref().ok_or_else(|| anyhow!("SIM API KEY missing"))?;
        let url = format!("{}/trade", SIM_BASE);
 
        let payload = serde_json::json!({
            "market_id": original_trade.market,
            "outcome": original_trade.outcome,
            "side": original_trade.side,
            "quantity_usdc": copy_notional,
            "original_tx_hash": original_trade.tx_hash
        });
 
        info!("🕒 [t={}] Submitting SIMULATION trade to PolySimulator...", current_time_ms());
        let resp = self.http.post(url)
            .bearer_auth(api_key)
            .json(&payload)
            .send()
            .await?;
 
        if !resp.status().is_success() {
            let status = resp.status();
            let err_body = resp.text().await.unwrap_or_default();
            bail!("Simulation trade failed ({}): {}", status, err_body);
        }
 
        let res: Value = resp.json().await?;
        let order_id = res.get("order_id").and_then(|v| v.as_str()).unwrap_or("sim-order").to_owned();
        let fill_price = res.get("fill_price").and_then(|v| v.as_f64()).unwrap_or(original_trade.price);
 
        info!("🕒 [t={}] Simulation Order Success: order_id={}", current_time_ms(), order_id);
 
        Ok(CopyExecutionResult {
            order_id,
            copy_notional,
            copy_shares: copy_notional / fill_price,
            price: fill_price,
            side: original_trade.side.clone(),
        })
    }
 
    pub async fn get_positions(&self) -> Result<Vec<Value>> {
        if self.config.simulation_mode {
            let api_key = self.config.polysimulator_api_key.as_ref().ok_or_else(|| anyhow!("SIM API KEY missing"))?;
            let url = format!("{}/portfolio", SIM_BASE);
            let resp = self.http.get(url).bearer_auth(api_key).send().await?;
            if !resp.status().is_success() { return Ok(Vec::new()); }
            let res: Value = resp.json().await?;
            return Ok(res.get("positions").and_then(|v| v.as_array()).cloned().unwrap_or_default());
        }
 
        let signer_guard = self.signer.read().await;
        let addr = signer_guard.as_ref().map(|s| s.address().to_string().to_lowercase()).unwrap_or_default();
        if addr.is_empty() { return Ok(Vec::new()); }
 
        let resp = self
            .http
            .get(DATA_API_POSITIONS)
            .query(&[("user", addr), ("limit", "500".to_owned())])
            .send()
            .await?;
        if !resp.status().is_success() {
            return Ok(Vec::new());
        }
        let rows: Vec<Value> = resp.json().await.unwrap_or_default();
        Ok(rows)
    }
 
    async fn validate_balance_or_shares(
        &self,
        side: &str,
        copy_notional: f64,
        token_id: &str,
    ) -> Result<()> {
        info!("🕒 [t={}] Validating {} requirements...", current_time_ms(), side);
        if self.config.simulation_mode {
            return Ok(()); // Handled by PolySimulator server
        }
 
        if side == "SELL" {
            let positions = self.get_positions().await?;
            info!("🕒 [t={}] User positions fetched and reconciled", current_time_ms());
            let pos = positions.iter().find(|p| {
                p.get("asset_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.eq_ignore_ascii_case(token_id))
                    .unwrap_or(false)
            });
            let shares = pos
                .and_then(|p| p.get("size"))
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            if shares <= 0.0 {
                bail!("insufficient shares to sell");
            }
            return Ok(());
        }
 
        let clob = self
            .clob
            .read()
            .await
            .as_ref()
            .cloned()
            .ok_or_else(|| anyhow!("trader not initialized"))?;
        let bal = clob
            .balance_allowance(
                BalanceAllowanceRequest::builder()
                    .asset_type(AssetType::Collateral)
                    .signature_type(SignatureType::Eoa)
                    .build(),
            )
            .await?;
        let amount = bal.balance.to_string().parse::<f64>().unwrap_or(0.0);
        if amount < copy_notional {
            bail!("not enough balance / allowance");
        }
        Ok(())
    }
 
    pub async fn check_geoblock(&self) -> Result<()> {
        info!("🕒 [t={}] Checking geographic eligibility...", current_time_ms());
        let url = "https://polymarket.com/api/geoblock";
        let resp = self.http.get(url).send().await?;
        if !resp.status().is_success() {
            warn!("Geoblock check failed (status {}), proceeding with caution.", resp.status());
            return Ok(());
        }
  
        let data: Value = resp.json().await?;
        let is_blocked = data.get("blocked").and_then(|v| v.as_bool()).unwrap_or(false);
        let country = data.get("country").and_then(|v| v.as_str()).unwrap_or("Unknown");
        let region = data.get("region").and_then(|v| v.as_str()).unwrap_or("");
  
        if is_blocked {
            let msg = format!("GEOGRAPHICALLY BLOCKED: Trading is restricted in {} {}", country, region);
            error!("{}", msg);
            bail!(msg);
        }
  
        info!("🕒 [t={}] Geographic check passed: {} {}", current_time_ms(), country, region);
        Ok(())
    }
  
    async fn ensure_approvals(&self) -> Result<()> {
        let clob = self
            .clob
            .read()
            .await
            .as_ref()
            .cloned()
            .ok_or_else(|| anyhow!("trader not initialized"))?;
 
        let _ = clob
            .update_balance_allowance(
                UpdateBalanceAllowanceRequest::builder()
                    .asset_type(AssetType::Collateral)
                    .signature_type(SignatureType::Eoa)
                    .build(),
            )
            .await;
        if self.config.polymarket_geo_token.is_none() {
            warn!("POLYMARKET_GEO_TOKEN is not set. This may fail in geo-restricted regions.");
        }
        Ok(())
    }
 
    pub async fn ws_auth(&self) -> Option<(String, String, String)> {
        self.creds.read().await.as_ref().map(|c| {
            (
                c.key().to_string(),
                c.secret().expose_secret().to_owned(),
                c.passphrase().expose_secret().to_owned(),
            )
        })
    }
}
 
fn to_f64(v: &Value) -> Option<f64> {
    if let Some(n) = v.as_f64() {
        return Some(n);
    }
    if let Some(s) = v.as_str() {
        return s.parse::<f64>().ok();
    }
    None
}
  
fn current_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
