use std::collections::HashMap;

use serde_json::Value;

use crate::monitor::Trade;

#[derive(Debug, Clone, Default)]
pub struct PositionState {
    pub token_id: String,
    pub market: String,
    pub outcome: String,
    pub shares: f64,
    pub notional: f64,
    pub avg_price: f64,
}

#[derive(Debug, Default)]
pub struct PositionTracker {
    positions: HashMap<String, PositionState>,
}

impl PositionTracker {
    pub fn load_from_data_api_positions(&mut self, positions: &[Value]) -> (usize, usize) {
        let mut loaded = 0usize;
        let mut skipped = 0usize;

        for pos in positions {
            let token_id = get_string(pos, &["asset_id", "token_id", "tokenId", "assetId"]);
            let Some(token_id) = token_id else {
                skipped += 1;
                continue;
            };

            let market = get_string(pos, &["condition_id", "conditionId", "market", "market_id"])
                .unwrap_or_default();
            let outcome = get_string(pos, &["outcome", "side"]).unwrap_or_else(|| "YES".to_owned());
            let shares = get_number(pos, &["size", "quantity", "shares", "balance", "position"]);
            let notional = get_number(pos, &["usdcValue", "notional", "usdc", "value", "collateral"]);
            let avg_price = get_number(pos, &["avgPrice", "averagePrice", "entryPrice", "price"]);
            let avg_price = if avg_price > 0.0 {
                avg_price
            } else if shares > 0.0 {
                (notional / shares).abs()
            } else {
                0.0
            };

            self.positions.insert(
                token_id.clone(),
                PositionState {
                    token_id,
                    market,
                    outcome,
                    shares: shares.max(0.0),
                    notional: notional.max(0.0),
                    avg_price,
                },
            );
            loaded += 1;
        }

        (loaded, skipped)
    }

    pub fn record_fill(
        &mut self,
        trade: &Trade,
        notional: f64,
        shares: f64,
        side: &str,
        price: f64,
    ) {
        let existing = self
            .positions
            .get(&trade.token_id)
            .cloned()
            .unwrap_or(PositionState {
                token_id: trade.token_id.clone(),
                market: trade.market.clone(),
                outcome: trade.outcome.clone(),
                shares: 0.0,
                notional: 0.0,
                avg_price: price,
            });

        let sign = if side == "BUY" { 1.0 } else { -1.0 };
        let next_shares = existing.shares + (shares * sign);
        let next_notional = existing.notional + (notional * sign);
        let next_avg = if next_shares > 0.0 {
            (next_notional / next_shares).abs()
        } else {
            0.0
        };

        self.positions.insert(
            trade.token_id.clone(),
            PositionState {
                token_id: trade.token_id.clone(),
                market: trade.market.clone(),
                outcome: trade.outcome.clone(),
                shares: next_shares.max(0.0),
                notional: next_notional.max(0.0),
                avg_price: next_avg,
            },
        );
    }

    pub fn get_notional(&self, token_id: &str) -> f64 {
        self.positions.get(token_id).map(|p| p.notional).unwrap_or(0.0)
    }

    pub fn get_position(&self, token_id: &str) -> Option<&PositionState> {
        self.positions.get(token_id)
    }
}

fn get_string(v: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(s) = v.get(*key).and_then(|x| x.as_str()) {
            let t = s.trim();
            if !t.is_empty() {
                return Some(t.to_owned());
            }
        }
    }
    None
}

fn get_number(v: &Value, keys: &[&str]) -> f64 {
    for key in keys {
        if let Some(raw) = v.get(*key) {
            if let Some(n) = raw.as_f64() {
                return n;
            }
            if let Some(s) = raw.as_str() {
                if let Ok(n) = s.parse::<f64>() {
                    return n;
                }
            }
        }
    }
    0.0
}
