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
use tracing::{info, warn};

use crate::config::Config;
use crate::monitor::Trade;

const DATA_API_POSITIONS: &str = "https://data-api.polymarket.com/positions";
const CLOB_HOST: &str = "https://clob.polymarket.com";

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
    signer: PrivateKeySigner,
    clob: Arc<RwLock<Option<AuthClient>>>,
    creds: Arc<RwLock<Option<Credentials>>>,
    http: Client,
}

impl TradeExecutor {
    pub async fn new(config: Config) -> Result<Self> {
        let signer = PrivateKeySigner::from_str(&config.private_key)?.with_chain_id(Some(POLYGON));
        Ok(Self {
            config,
            signer,
            clob: Arc::new(RwLock::new(None)),
            creds: Arc::new(RwLock::new(None)),
            http: Client::new(),
        })
    }

    pub async fn initialize(&self) -> Result<()> {
        let unauth = ClobClient::new(
            CLOB_HOST,
            ClobConfig::builder().use_server_time(true).build(),
        )?;

        let creds = unauth
            .create_or_derive_api_key(&self.signer, None)
            .await
            .map_err(|e| anyhow!("failed to derive/create api key: {e}"))?;

        let auth = unauth
            .authentication_builder(&self.signer)
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

        self.ensure_approvals().await?;
        Ok(())
    }

    pub async fn calculate_copy_size(&self, original_size: f64) -> f64 {
        let t = &self.config.trading;
        let mut size = if t.use_sizing_model {
            match self.compute_sizing_model_notional(original_size).await {
                Ok(v) => v,
                Err(err) => {
                    warn!("Sizing model unavailable, falling back to POSITION_MULTIPLIER: {err}");
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

    async fn compute_sizing_model_notional(&self, target_position_size: f64) -> Result<f64> {
        let your_balance = self.get_your_balance_usdc().await?;
        let target_balance = if let Some(v) = self.config.trading.target_balance_override {
            v
        } else {
            self.get_target_balance_usdc().await?
        };

        if your_balance <= 0.0 {
            bail!("your_balance is <= 0");
        }
        if target_balance <= 0.0 {
            bail!("target_balance is <= 0");
        }

        let ratio = your_balance / target_balance;
        let mirrored = ratio * target_position_size * self.config.trading.sizing_multiplier;
        info!(
            "Sizing model | your_balance={} target_balance={} ratio={} target_size={} multiplier={} mirrored={}",
            your_balance,
            target_balance,
            ratio,
            target_position_size,
            self.config.trading.sizing_multiplier,
            mirrored
        );
        Ok(mirrored)
    }

    async fn get_your_balance_usdc(&self) -> Result<f64> {
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

    async fn get_target_balance_usdc(&self) -> Result<f64> {
        let url = "https://data-api.polymarket.com/value";
        let resp = self
            .http
            .get(url)
            .query(&[("user", self.config.target_wallet.to_lowercase())])
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("target value endpoint returned {}", resp.status());
        }

        let value: Value = resp.json().await?;
        if let Some(arr) = value.as_array() {
            for row in arr {
                if let Some(v) = row.get("value").and_then(to_f64) {
                    return Ok(v);
                }
            }
        }
        if let Some(v) = value.get("value").and_then(to_f64) {
            return Ok(v);
        }
        if let Some(v) = to_f64(&value) {
            return Ok(v);
        }
        bail!("unable to parse target wallet balance from value endpoint");
    }

    pub async fn execute_copy_trade(
        &self,
        original_trade: &Trade,
        copy_notional_override: Option<f64>,
    ) -> Result<CopyExecutionResult> {
        let copy_notional = if let Some(v) = copy_notional_override {
            v
        } else {
            self.calculate_copy_size(original_trade.size_usdc).await
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

        let book = clob
            .order_book(&OrderBookSummaryRequest::builder().token_id(token_id).build())
            .await?;
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
            let signed = clob.sign(&self.signer, order).await?;
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
            let signed = clob.sign(&self.signer, order).await?;
            clob.post_order(signed).await?
        };

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

    pub async fn get_positions(&self) -> Result<Vec<Value>> {
        let wallet = self.signer.address().to_string().to_lowercase();
        let resp = self
            .http
            .get(DATA_API_POSITIONS)
            .query(&[("user", wallet), ("limit", "500".to_owned())])
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
        if side == "SELL" {
            let positions = self.get_positions().await?;
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
