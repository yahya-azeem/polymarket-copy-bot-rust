use std::str::FromStr;
use std::sync::Arc;

use alloy::signers::local::PrivateKeySigner;
use alloy::primitives::Address;
use anyhow::{Result, anyhow, bail};
use polymarket_client_sdk::POLYGON;
use polymarket_client_sdk::auth::Credentials;
use polymarket_client_sdk::auth::{Normal, state::Authenticated};
use polymarket_client_sdk::auth::Signer as _;
use polymarket_client_sdk::clob::types::request::{
    BalanceAllowanceRequest, OrderBookSummaryRequest, UpdateBalanceAllowanceRequest,
    OrdersRequest,
};
use polymarket_client_sdk::clob::types::{Amount, AssetType, OrderType, Side, SignatureType};
use std::sync::atomic::{AtomicU8, Ordering};

const SIG_TYPE_AUTO: u8 = 0;
const SIG_TYPE_EOA: u8 = 1;
const SIG_TYPE_PROXY: u8 = 2;
const SIG_TYPE_GNOSIS_SAFE: u8 = 3;
use polymarket_client_sdk::clob::{Client as ClobClient, Config as ClobConfig};
use polymarket_client_sdk::types::{Decimal, U256};
use reqwest::Client;
use serde_json::Value;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::config::Config;
use crate::types::{Trade, CopyExecutionResult};
use crate::positions::PositionTracker;
use crate::utils::current_time_ms;

const DATA_API_POSITIONS: &str = "https://data-api.polymarket.com/positions";
const CLOB_HOST: &str = "https://clob.polymarket.com";
const SIM_BASE: &str = "https://api.polysimulator.com/v1";

type AuthClient = ClobClient<Authenticated<Normal>>;



pub struct TradeExecutor {
    config: Config,
    signer: Arc<RwLock<Option<PrivateKeySigner>>>,
    clob: Arc<RwLock<Option<AuthClient>>>,
    _creds: Arc<RwLock<Option<Credentials>>>,
    http: Client,
    detected_sig_type: AtomicU8,
}

impl TradeExecutor {
    pub async fn new(config: Config) -> Result<Self> {
        let signer = if !config.simulation_mode {
            Some(PrivateKeySigner::from_str(&config.private_key)?.with_chain_id(Some(POLYGON)))
        } else {
            None
        };

        // Initialize HTTP client with Geo Token if available
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(ref token) = config.polymarket_geo_token {
            let clean_token = token.trim_matches('\'').trim_matches('"');
            let ascii_token: String = clean_token.chars().filter(|c| c.is_ascii()).collect();
            let cookie_val = format!("polymarket_geo_token={}", ascii_token);
            
            if let Ok(v) = reqwest::header::HeaderValue::from_str(&cookie_val) {
                headers.insert(reqwest::header::COOKIE, v);
            }
        }

        let http = Client::builder()
            .default_headers(headers)
            .build()?;

        Ok(Self {
            config,
            signer: Arc::new(RwLock::new(signer)),
            clob: Arc::new(RwLock::new(None)),
            _creds: Arc::new(RwLock::new(None)),
            http,
            detected_sig_type: AtomicU8::new(SIG_TYPE_AUTO),
        })
    }

    pub async fn initialize(&self) -> Result<()> {
        if self.config.simulation_mode {
            info!("Simulation Mode Enabled. Skipping Polymarket CLOB authentication.");
            return Ok(());
        }

        let signer_guard = self.signer.read().await;
        let signer = signer_guard
            .as_ref()
            .ok_or_else(|| anyhow!("signer missing in live mode"))?;

        let unauth = ClobClient::new(
            CLOB_HOST,
            ClobConfig::builder().use_server_time(true).build(),
        )?;

        // 🔍 DISCOVERY: Auto-detect signature type using Gamma Profile API
        let mut final_sig_type = self.get_signature_type();
        let mut final_proxy: Option<Address> = None;

        if self.config.polymarket_signature_type == "AUTO" {
            let addr = format!("{:?}", signer.address()).trim_matches('"').to_lowercase();
            let url = format!("https://gamma-api.polymarket.com/public-profile?address={}", addr);
            
            if let Ok(resp) = self.http.get(url).send().await {
                if resp.status().is_success() {
                    if let Ok(profile) = resp.json::<Value>().await {
                        if let Some(proxy) = profile.get("proxyWallet").and_then(|v| v.as_str()) {
                            if !proxy.is_empty() && proxy != "0x0000000000000000000000000000000000000000" {
                                info!("🎯 AUTO-DETECTED Gnosis Safe Proxy: {}", proxy);
                                final_sig_type = SignatureType::GnosisSafe;
                                final_proxy = Some(Address::from_str(proxy.trim())?);
                                self.detected_sig_type.store(SIG_TYPE_GNOSIS_SAFE, Ordering::Relaxed);
                            }
                        }
                    }
                }
            }
            
            if final_sig_type != SignatureType::GnosisSafe {
                // If no proxy found, fall back to EOA
                self.detected_sig_type.store(SIG_TYPE_EOA, Ordering::Relaxed);
                final_sig_type = SignatureType::Eoa;
            }
        }

        // Now initialize CLOB once with the correct settings
        let creds = unauth
            .create_or_derive_api_key(signer, None) // Nonce is None
            .await
            .map_err(|e| anyhow!("failed to derive/create api key: {e}"))?;

        let mut builder = unauth
            .authentication_builder(signer)
            .credentials(creds.clone())
            .signature_type(final_sig_type);
        
        // If we have a detected proxy, use it as the funder (Safe address)
        if let Some(proxy_addr) = final_proxy {
            builder = builder.funder(proxy_addr);
        }

        let auth = builder.authenticate().await?;

        let _ = auth.api_keys().await?;
        info!("API credentials initialized ({:?}).", final_sig_type);

        {
            *self._creds.write().await = Some(creds);
            *self.clob.write().await = Some(auth);
        }

        // On-chain check (Diagnostic)
        if let Ok((usdc, usdce)) = self.get_onchain_balances().await {
            info!("💰 On-Chain Assets: {:.2} USDC (Native) | {:.2} USDC.e (Bridged)", usdc, usdce);
        }

        // Geoblock check only at init
        self.check_geoblock().await?;
        self.ensure_approvals().await?;
        Ok(())
    }

    pub async fn get_onchain_balances(&self) -> Result<(f64, f64)> {
        use alloy::primitives::Address;
        use alloy::providers::{Provider, ProviderBuilder};
        
        let rpc_url = self.config.rpc_url.parse()?;
        let provider = ProviderBuilder::new().connect_http(rpc_url);
        
        let signer_addr_str = self.get_address().await;
        let signer_addr: Address = signer_addr_str.trim_matches('"').parse()?;
        
        // Native USDC (Polygon)
        let usdc_addr: Address = "0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359".parse()?;
        // Bridged USDC.e (Polygon) - Polymarket uses this
        let usdce_addr: Address = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174".parse()?;

        // IERC20::balanceOf(address) = 0x70a08231
        let mut data = Vec::with_capacity(36);
        data.extend_from_slice(&[0x70, 0xa0, 0x82, 0x31]);
        data.extend_from_slice(&[0u8; 12]);
        data.extend_from_slice(signer_addr.as_slice());

        let mut usdc_val = 0.0;
        let mut usdce_val = 0.0;

        if let Ok(res) = provider.call(alloy_rpc_types_eth::TransactionRequest::default().to(usdc_addr).input(data.clone().into())).await {
            if res.len() >= 32 {
                let val = alloy::primitives::U256::from_be_slice(&res[0..32]);
                usdc_val = val.to::<u128>() as f64 / 1_000_000.0;
            }
        }

        if let Ok(res) = provider.call(alloy_rpc_types_eth::TransactionRequest::default().to(usdce_addr).input(data.into())).await {
            if res.len() >= 32 {
                let val = alloy::primitives::U256::from_be_slice(&res[0..32]);
                usdce_val = val.to::<u128>() as f64 / 1_000_000.0;
            }
        }

        Ok((usdc_val, usdce_val))
    }

    pub async fn calculate_copy_size(&self, original_size: f64, target_wallet: &str) -> f64 {
        let t = &self.config.trading;
        let mut size = if t.use_sizing_model {
            match self
                .compute_sizing_model_notional(original_size, target_wallet)
                .await
            {
                Ok(v) => v,
                Err(err) => {
                    warn!(
                        "Sizing model unavailable for {}, falling back to POSITION_MULTIPLIER: {err}",
                        target_wallet
                    );
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
        (shares * 100.0).round() / 100.0
    }

    async fn compute_sizing_model_notional(
        &self,
        target_position_size: f64,
        target_wallet: &str,
    ) -> Result<f64> {
        let now = current_time_ms();
        info!("🕒 [t={}] Refreshing fresh LIVE balance for sizing...", now);

        let your_balance = self.get_your_balance_usdc().await?;
        info!(
            "💰 [t={}] Fresh Cash: {:.4} USDC",
            current_time_ms(),
            your_balance
        );

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

        // Proportional sizing: scale whale's trade by (our_balance / whale_balance)
        // This ALWAYS scales DOWN because we're smaller than the whale.
        let proportional = (your_balance / target_balance) * target_position_size;

        // Literal: use whale's exact dollar amount (only for small trades we can afford)
        let literal = target_position_size;

        // SIZING RULES:
        // 1. Default: use proportional (which scales down)
        // 2. Literal ONLY if: trade is small (<$15), AND we can comfortably afford it
        // 3. NEVER scale UP beyond the whale's actual dollar amount
        let chosen = if self.config.trading.prefer_literal_whale_size
            && literal <= 15.0
            && literal <= your_balance * self.config.trading.max_percent_of_balance
        {
            info!(
                "🕒 [t={}] Using LITERAL (small trade ${:.2} ≤ $15, within budget)",
                current_time_ms(), literal
            );
            literal
        } else {
            info!(
                "🕒 [t={}] Using PROPORTIONAL mirroring (scaled down)",
                current_time_ms()
            );
            proportional
        };

        // Hard cap: NEVER exceed the whale's actual trade size
        let mut final_size = chosen.min(target_position_size);

        // Safety: also cap at X% of your own wallet balance
        let wallet_cap = your_balance * self.config.trading.max_percent_of_balance;
        if final_size > wallet_cap {
            info!(
                "Sizing: capping trade to {:.2} USDC ({}% of wallet)",
                wallet_cap,
                self.config.trading.max_percent_of_balance * 100.0
            );
            final_size = wallet_cap;
        }

        info!(
            "Sizing model | wallet={} your_bal={:.2} target_bal={:.2} target_sz={:.2} chosen={:.2}",
            target_wallet, your_balance, target_balance, target_position_size, final_size
        );
        Ok(final_size)
    }

    pub async fn get_your_balance_usdc(&self) -> Result<f64> {
        if self.config.simulation_mode {
            let api_key = self
                .config
                .polysimulator_api_key
                .as_ref()
                .ok_or_else(|| anyhow!("SIM API KEY missing"))?;
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

        let preferred_sig_type = self.get_signature_type();
        let mut eoa_bal_val = 0.0;
        let mut proxy_bal_val = 0.0;
        let mut safe_bal_val = 0.0;
        let mut data_api_val = 0.0;

        // 🔍 Check EOA
        if let Ok(bal) = clob.balance_allowance(BalanceAllowanceRequest::builder().asset_type(AssetType::Collateral).signature_type(SignatureType::Eoa).build()).await {
            eoa_bal_val = bal.balance.to_string().parse::<f64>().unwrap_or(0.0) / 1_000_000.0;
        }

        // 🔍 Check Proxy
        if let Ok(bal) = clob.balance_allowance(BalanceAllowanceRequest::builder().asset_type(AssetType::Collateral).signature_type(SignatureType::Proxy).build()).await {
            proxy_bal_val = bal.balance.to_string().parse::<f64>().unwrap_or(0.0) / 1_000_000.0;
        }

        // 🔍 Check GnosisSafe
        if let Ok(bal) = clob.balance_allowance(BalanceAllowanceRequest::builder().asset_type(AssetType::Collateral).signature_type(SignatureType::GnosisSafe).build()).await {
            safe_bal_val = bal.balance.to_string().parse::<f64>().unwrap_or(0.0) / 1_000_000.0;
        }

        // 🔍 Check Data API
        let addr = self.get_address().await;
        let addr_clean = addr.trim_matches('"').to_lowercase();
        let url = "https://data-api.polymarket.com/value";
        if let Ok(resp) = self.http.get(url).query(&[("user", &addr_clean)]).send().await {
            if resp.status().is_success() {
                if let Ok(value) = resp.json::<Value>().await {
                    data_api_val = if let Some(arr) = value.as_array() {
                        arr.get(0).and_then(|row| row.get("value")).and_then(to_f64)
                    } else {
                        value.get("value").and_then(to_f64).or_else(|| to_f64(&value))
                    }.unwrap_or(0.0);
                }
            }
        }

        debug!("💰 [DEBUG] Balances: EOA={:.2} | Proxy={:.2} | Safe={:.2} | DataAPI={:.2}", 
            eoa_bal_val, proxy_bal_val, safe_bal_val, data_api_val);

        // 🔍 Highest Balance Detection
        let mut best_bal = eoa_bal_val;
        let mut best_type = SIG_TYPE_EOA;

        if proxy_bal_val > best_bal {
            best_bal = proxy_bal_val;
            best_type = SIG_TYPE_PROXY;
        }
        if safe_bal_val > best_bal {
            best_bal = safe_bal_val;
            best_type = SIG_TYPE_GNOSIS_SAFE;
        }
        if data_api_val > best_bal {
            best_bal = data_api_val;
            best_type = SIG_TYPE_GNOSIS_SAFE; // Data API balance usually implies Gnosis Safe
        }

        // 🎯 Selection logic
        let (final_bal, detected) = if self.config.polymarket_signature_type == "AUTO" {
            (best_bal, best_type)
        } else {
            // Manual Override: Honor the .env setting
            match preferred_sig_type {
                SignatureType::Eoa => (eoa_bal_val, SIG_TYPE_EOA),
                SignatureType::Proxy => (proxy_bal_val, SIG_TYPE_PROXY),
                SignatureType::GnosisSafe => (safe_bal_val, SIG_TYPE_GNOSIS_SAFE),
                _ => (eoa_bal_val, SIG_TYPE_EOA), // Catch-all for SDK updates
            }
        };

        if self.config.polymarket_signature_type == "AUTO" && self.detected_sig_type.load(Ordering::Relaxed) == SIG_TYPE_AUTO {
            self.detected_sig_type.store(detected, Ordering::Relaxed);
            info!("🎯 AUTO-DETECTED Signature Type: {:?}", self.get_signature_type());
        }

        debug!("💰 Final Calculated Exchange Balance: {:.2} USDC", final_bal);
        Ok(final_bal)
    }

    async fn get_target_balance_usdc(&self, target_wallet: &str) -> Result<f64> {
        let t_start = current_time_ms();
        info!(
            "🕒 [t={}] Fetching target balance ({})",
            t_start, target_wallet
        );
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
            arr.get(0)
                .and_then(|row| row.get("value"))
                .and_then(to_f64)
        } else {
            value
                .get("value")
                .and_then(to_f64)
                .or_else(|| to_f64(&value))
        }
        .unwrap_or(0.0);

        info!(
            "🕒 [t={}] Target Value: {:.4} USDC",
            current_time_ms(),
            target_val
        );
        Ok(target_val)
    }

    pub async fn execute_copy_trade(
        &self,
        original_trade: &Trade,
        copy_notional_override: Option<f64>,
    ) -> Result<CopyExecutionResult> {
        if self.config.simulation_mode {
            return self
                .execute_simulation_trade(original_trade, copy_notional_override)
                .await;
        }

        // No per-trade geoblock check — only checked at init (Bug #10 fix)

        let copy_notional = if let Some(v) = copy_notional_override {
            v
        } else {
            self.calculate_copy_size(original_trade.size_usdc, &original_trade.original_target_wallet)
                .await
        };
        self.validate_balance_or_shares(
            &original_trade.side,
            copy_notional,
            &original_trade.token_id,
        )
        .await?;

        let your_balance = self.get_your_balance_usdc().await?;
        let token_id = U256::from_str(&original_trade.token_id)?;
        let clob = self
            .clob
            .read()
            .await
            .as_ref()
            .cloned()
            .ok_or_else(|| anyhow!("trader not initialized"))?;

        info!(
            "🕒 [t={}] Fetching LIVE Order Book for token {}",
            current_time_ms(),
            original_trade.token_id
        );
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

        info!(
            "🕒 [t={}] Best Match Price: {}",
            current_time_ms(),
            best_price
        );

        let mut order_type = self.config.trading.order_type.clone();
        
        // AUTO/ADAPTIVE LOGIC: Choose best order type based on balance
        if order_type == "AUTO" || order_type == "LIMIT" {
            if your_balance >= self.config.trading.auto_market_threshold {
                if order_type == "AUTO" || order_type == "LIMIT" {
                    info!("🚀 Balance is ${:.2} (>= ${:.2}). Using FOK for guaranteed execution.", 
                        your_balance, self.config.trading.auto_market_threshold);
                    order_type = "FOK".to_string();
                }
            } else if order_type == "AUTO" {
                order_type = "LIMIT".to_string();
            }
        }
        
        let order_type_str = order_type.as_str();
        let whale_price = original_trade.price;
        
        // PRICE CEILING: For non-LIMIT orders, reject if order book price is way worse than whale's price
        let best_f = best_price.to_string().parse::<f64>().unwrap_or(0.0);
        if order_type_str != "LIMIT" && original_trade.side == "BUY" && whale_price > 0.0 {
            let max_acceptable = (whale_price * 1.20).min(0.99); // max 20% above whale's price
            if best_f > max_acceptable {
                bail!(
                    "Price too far from whale's entry: best_ask={:.4} vs whale={:.4} (max={:.4}). Skipping to protect from overpay.",
                    best_f, whale_price, max_acceptable
                );
            }
        }

        let mut px = if order_type_str == "LIMIT" {
            // For LIMIT orders, we base the price on the WHALE'S entry, not the current bad book price
            Decimal::from_str(&whale_price.to_string())?
        } else {
            best_price
        };

        if original_trade.side == "BUY" {
            px *= Decimal::from_str(&(1.0 + self.config.trading.slippage_tolerance).to_string())?;
        } else {
            px *= Decimal::from_str(&(1.0 - self.config.trading.slippage_tolerance).to_string())?;
        }
        let px_f = px.to_string().parse::<f64>().unwrap_or(original_trade.price);
        let px_f = px_f.clamp(0.01, 0.99);

        // MARKET-AWARE ROUNDING (Tick Size Fix)
        // We infer the required precision from the Order Book's best price
        let book_precision = get_precision(best_f).max(2).min(4);
        let price_str = format!("{:.*}", book_precision, px_f);
        let price = Decimal::from_str(&price_str)?;

        let copy_shares = self.calculate_shares_for_notional(copy_notional, px_f);

        let side = if original_trade.side == "BUY" {
            Side::Buy
        } else {
            Side::Sell
        };

        let signer_guard = self.signer.read().await;
        let signer = signer_guard
            .as_ref()
            .ok_or_else(|| anyhow!("signer missing in live mode"))?;

        let _sig_type = self.get_signature_type();
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
            info!(
                "🕒 [t={}] Order signed, submitting to Polymarket CLOB...",
                current_time_ms()
            );
            clob.post_order(signed).await?
        };

        info!(
            "🕒 [t={}] Order Result: success={}",
            current_time_ms(),
            post.success
        );

        if !post.success {
            let msg = post
                .error_msg
                .unwrap_or_else(|| "unknown post order error".to_owned());
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

    async fn execute_simulation_trade(
        &self,
        original_trade: &Trade,
        copy_notional_override: Option<f64>,
    ) -> Result<CopyExecutionResult> {
        let copy_notional = if let Some(v) = copy_notional_override {
            v
        } else {
            self.calculate_copy_size(
                original_trade.size_usdc,
                &original_trade.original_target_wallet,
            )
            .await
        };

        let api_key = self
            .config
            .polysimulator_api_key
            .as_ref()
            .ok_or_else(|| anyhow!("SIM API KEY missing"))?;
        let url = format!("{}/trade", SIM_BASE);

        let payload = serde_json::json!({
            "market_id": original_trade.market,
            "outcome": original_trade.outcome,
            "side": original_trade.side,
            "quantity_usdc": copy_notional,
            "original_tx_hash": original_trade.tx_hash
        });

        info!(
            "🕒 [t={}] Submitting SIMULATION trade to PolySimulator...",
            current_time_ms()
        );
        let resp = self
            .http
            .post(url)
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
        let order_id = res
            .get("order_id")
            .and_then(|v| v.as_str())
            .unwrap_or("sim-order")
            .to_owned();
        let fill_price = res
            .get("fill_price")
            .and_then(|v| v.as_f64())
            .unwrap_or(original_trade.price);

        info!(
            "🕒 [t={}] Simulation Order Success: order_id={}",
            current_time_ms(),
            order_id
        );

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
            let api_key = self
                .config
                .polysimulator_api_key
                .as_ref()
                .ok_or_else(|| anyhow!("SIM API KEY missing"))?;
            let url = format!("{}/portfolio", SIM_BASE);
            let resp = self.http.get(url).bearer_auth(api_key).send().await?;
            if !resp.status().is_success() {
                return Ok(Vec::new());
            }
            let res: Value = resp.json().await?;
            return Ok(res
                .get("positions")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default());
        }

        let addr = self.get_address().await;
        let addr_clean = addr.trim_matches('"').to_lowercase();
        if addr_clean.is_empty() || addr_clean == "simulation/none" {
            return Ok(Vec::new());
        }

        self.get_positions_for_user(&addr_clean).await
    }

    pub async fn get_positions_for_user(&self, addr: &str) -> Result<Vec<Value>> {
        let addr_clean = addr.trim_matches('"').to_lowercase();
        let resp = self
            .http
            .get(DATA_API_POSITIONS)
            .query(&[("user", addr_clean), ("limit", "500".to_owned())])
            .send()
            .await?;
            
        if !resp.status().is_success() {
            return Ok(Vec::new());
        }
        let rows: Vec<Value> = resp.json().await.unwrap_or_default();
        Ok(rows)
    }

    pub async fn get_address(&self) -> String {
        let signer_guard = self.signer.read().await;
        signer_guard
            .as_ref()
            .map(|s| format!("{:?}", s.address()))
            .unwrap_or_else(|| "Simulation/None".to_owned())
    }

    async fn validate_balance_or_shares(
        &self,
        side: &str,
        copy_notional: f64,
        token_id: &str,
    ) -> Result<()> {
        info!(
            "🕒 [t={}] Validating {} requirements...",
            current_time_ms(),
            side
        );
        if self.config.simulation_mode {
            return Ok(()); // Handled by PolySimulator server
        }

        // For Proxy accounts, CLOB balance check returns 0 for EOA.
        // Skip pre-trade validation; the CLOB will reject if truly insufficient.
        let sig_type = self.get_signature_type();
        if matches!(sig_type, SignatureType::Proxy | SignatureType::GnosisSafe) {
            info!("Proxy account — skipping pre-trade balance validation");
            return Ok(());
        }

        if side == "SELL" {
            let positions = self.get_positions().await?;
            info!(
                "🕒 [t={}] User positions fetched and reconciled",
                current_time_ms()
            );
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
            let needed = self.calculate_shares_for_notional(copy_notional, 0.5);
            if shares < needed.min(1.0) {
                bail!(
                    "insufficient shares to sell (have={:.4}, need>={:.4})",
                    shares,
                    needed
                );
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
                    .signature_type(sig_type)
                    .build(),
            )
            .await?;
        let raw_amount = bal.balance.to_string().parse::<f64>().unwrap_or(0.0);
        let amount = raw_amount / 1_000_000.0; // USDC has 6 decimal places on Polygon
        
        if amount < copy_notional {
            error!("💰 BALANCE ALERT: Your Polymarket exchange balance is {:.2} USDC.", amount);
            bail!(
                "not enough balance in Polymarket exchange (have={:.2}, need={:.2})",
                amount,
                copy_notional
            );
        }
        Ok(())
    }

    pub async fn get_order_book(
        &self,
        token_id: &str,
    ) -> Result<polymarket_client_sdk::clob::types::response::OrderBookSummaryResponse> {
        let clob = self
            .clob
            .read()
            .await
            .as_ref()
            .cloned()
            .ok_or_else(|| anyhow!("trader not initialized"))?;
        clob.order_book(
            &OrderBookSummaryRequest::builder()
                .token_id(U256::from_str(token_id)?)
                .build(),
        )
        .await
        .map_err(|e| anyhow!("failed to fetch order book: {e}"))
    }

    pub async fn cancel_stale_orders(&self) -> Result<usize> {
        if self.config.simulation_mode {
            return Ok(0);
        }

        let clob = self
            .clob
            .read()
            .await
            .as_ref()
            .cloned()
            .ok_or_else(|| anyhow!("trader not initialized"))?;

        // 1. Fetch all open orders for the authenticated account
        let orders_resp = clob.orders(&OrdersRequest::builder().build(), None).await?;
        let orders = orders_resp.data; 
        
        let now = current_time_ms();
        let timeout_ms = (self.config.trading.order_timeout_minutes * 60 * 1000) as i64;
        let mut cancelled = 0;

        for order in orders {
            let created_ms = order.created_at.timestamp_millis();
            
            if now - created_ms > timeout_ms {
                info!("🕒 [t={}] Cancelling stale order {} (age: {} mins)", 
                    now, order.id, (now - created_ms) / 60000);
                
                let res = clob.cancel_order(&order.id).await;
                if let Ok(resp) = res {
                    cancelled += resp.canceled.len();
                }
            }
        }

        if cancelled > 0 {
            info!("✅ Cancelled {} stale orders to free up balance", cancelled);
        }
        Ok(cancelled)
    }

    fn calculate_adaptive_tpsl(&self, your_balance: f64) -> (Option<f64>, Option<f64>) {
        if your_balance < self.config.trading.auto_market_threshold {
            // Tier 1: Small Account Protection (<$100)
            // Tight TP/SL to survive the variance.
            (Some(0.20), Some(0.10))
        } else if your_balance < 1000.0 {
            // Tier 2: Medium Account Growth ($100 - $1000)
            // Loosened limits to ride bigger whale trends.
            (Some(0.50), Some(0.25))
        } else {
            // Tier 3: Whale-Mirror Mode (>$1000)
            // No independent TP/SL. Follow the whale's conviction exactly.
            (None, None)
        }
    }

    pub async fn check_profit_taking(
        &self,
        positions_tracker: Arc<tokio::sync::Mutex<PositionTracker>>,
        target_token: Option<String>,
    ) -> Result<usize> {
        let balance = self.get_your_balance_usdc().await.unwrap_or(0.0);
        let (tp, sl) = self.calculate_adaptive_tpsl(balance);
        
        if tp.is_none() && sl.is_none() {
            // In Whale-Mirror mode, we don't do independent PnL exits.
            return Ok(0);
        }

        let to_sell = {
            let guard = positions_tracker.lock().await;
            let mut sell_list = Vec::new();
            
            let all_positions = guard.get_all();
            let check_list = if let Some(ref target) = target_token {
                all_positions.into_iter().filter(|p| p.token_id == *target).collect::<Vec<_>>()
            } else {
                all_positions
            };

            for pos in check_list {
                if pos.shares <= 0.0001 {
                    continue;
                }

                // Fetch best bid to see what we can sell for
                if let Ok(book) = self.get_order_book(&pos.token_id).await {
                    let bid = book.bids.first().map(|s| s.price.to_string().parse::<f64>().unwrap_or(0.0)).unwrap_or(0.0);
                    let entry = pos.avg_price;
                    if entry <= 0.0 {
                        continue;
                    }

                    let pnl = (bid - entry) / entry;
                    if let Some(tp_val) = tp {
                        if pnl >= tp_val {
                            info!(
                                "💰 ADAPTIVE TAKE PROFIT hit (Balance=${:.2}): {} pnl={:.2}% (entry={:.4}, bid={:.4})",
                                balance, pos.token_id, pnl * 100.0, entry, bid
                            );
                            sell_list.push(pos.clone());
                            continue;
                        }
                    }
                    
                    if let Some(sl_val) = sl {
                        if pnl <= -sl_val {
                            info!(
                                "🛑 ADAPTIVE STOP LOSS hit (Balance=${:.2}): {} pnl={:.2}% (entry={:.4}, bid={:.4})",
                                balance, pos.token_id, pnl * 100.0, entry, bid
                            );
                            sell_list.push(pos.clone());
                        }
                    }
                }
            }
            sell_list
        };

        let mut total_sold = 0;
        for pos in to_sell {
            let trade = Trade {
                tx_hash: format!("tp-sl-{}", current_time_ms()),
                timestamp_ms: current_time_ms(),
                market: pos.market.clone(),
                token_id: pos.token_id.clone(),
                side: "SELL".to_string(),
                price: pos.avg_price,
                size_usdc: pos.shares * pos.avg_price,
                outcome: pos.outcome.clone(),
                original_target_wallet: "SELF".to_string(),
            };

            // Force sell the entire position notional
            if let Ok(_) = self
                .execute_copy_trade(&trade, Some(pos.shares * pos.avg_price))
                .await
            {
                total_sold += 1;
            }
        }

        Ok(total_sold)
    }

    async fn check_geoblock(&self) -> Result<()> {
        info!(
            "🕒 [t={}] Checking geographic eligibility...",
            current_time_ms()
        );
        let url = "https://polymarket.com/api/geoblock";
        let resp = self.http.get(url).send().await?;
        if !resp.status().is_success() {
            warn!(
                "Geoblock check failed (status {}), proceeding with caution.",
                resp.status()
            );
            return Ok(());
        }

        let data: Value = resp.json().await?;
        let is_blocked = data
            .get("blocked")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let country = data
            .get("country")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown");
        let region = data.get("region").and_then(|v| v.as_str()).unwrap_or("");

        if is_blocked {
            let msg = format!(
                "GEOGRAPHICALLY BLOCKED: Trading is restricted in {} {}",
                country, region
            );
            error!("{}", msg);
            bail!(msg);
        }

        info!(
            "🕒 [t={}] Geographic check passed: {} {}",
            current_time_ms(),
            country,
            region
        );
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

        let sig_type = self.get_signature_type();
        info!("🕒 [t={}] Syncing Polymarket balance (sig_type={:?})...", current_time_ms(), sig_type);
        
        let res = clob
            .update_balance_allowance(
                UpdateBalanceAllowanceRequest::builder()
                    .asset_type(AssetType::Collateral)
                    .signature_type(sig_type)
                    .build(),
            )
            .await;

        match res {
            Ok(_) => {
                info!("✅ Balance-sync call successful.");
            }
            Err(e) => {
                warn!("⚠️ Balance-sync call failed: {}", e);
                warn!("   (Note: This is usually fine if your account is already initialized, but could explain 0.00 balance if new.)");
            }
        }

        if self.config.polymarket_geo_token.is_none() {
            warn!("POLYMARKET_GEO_TOKEN is not set. This may fail in geo-restricted regions.");
        }
        Ok(())
    }

    pub fn get_signature_type(&self) -> SignatureType {
        let detected = self.detected_sig_type.load(Ordering::Relaxed);
        if self.config.polymarket_signature_type == "AUTO" && detected != SIG_TYPE_AUTO {
            return match detected {
                SIG_TYPE_EOA => SignatureType::Eoa,
                SIG_TYPE_PROXY => SignatureType::Proxy,
                _ => SignatureType::GnosisSafe,
            };
        }

        match self.config.polymarket_signature_type.as_str() {
            "PROXY" => SignatureType::Proxy,
            "GNOSIS_SAFE" => SignatureType::GnosisSafe,
            _ => SignatureType::Eoa,
        }
    }
}

fn get_precision(val: f64) -> usize {
    let s = format!("{:.6}", val);
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() < 2 {
        return 0;
    }
    let decimals = parts[1].trim_end_matches('0');
    decimals.len()
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
