use std::collections::HashMap;

use serde_json::Value;

use crate::types::{PositionState, Trade};



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
            let outcome =
                get_string(pos, &["outcome", "side"]).unwrap_or_else(|| "YES".to_owned());
            let shares = get_number(pos, &["size", "quantity", "shares", "balance", "position"]);
            let notional =
                get_number(pos, &["usdcValue", "notional", "usdc", "value", "collateral"]);
            let avg_price =
                get_number(pos, &["avgPrice", "averagePrice", "entryPrice", "price"]);
            let avg_price = if avg_price > 0.0 {
                avg_price
            } else if shares > 0.0 {
                (notional / shares).abs()
            } else {
                0.0
            };

            // Cost basis = shares * avg_price (what was paid to acquire)
            let cost_basis = shares * avg_price;

            self.positions.insert(
                token_id.clone(),
                PositionState {
                    token_id,
                    market,
                    outcome,
                    shares: shares.max(0.0),
                    cost_basis: cost_basis.max(0.0),
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
                cost_basis: 0.0,
                avg_price: price,
            });

        if side == "BUY" {
            // Add shares, add cost basis
            let next_shares = existing.shares + shares;
            let next_cost = existing.cost_basis + notional;
            let next_avg = if next_shares > 0.0 {
                next_cost / next_shares
            } else {
                0.0
            };

            self.positions.insert(
                trade.token_id.clone(),
                PositionState {
                    token_id: trade.token_id.clone(),
                    market: trade.market.clone(),
                    outcome: trade.outcome.clone(),
                    shares: next_shares,
                    cost_basis: next_cost,
                    avg_price: next_avg,
                },
            );
        } else {
            // SELL: reduce shares, reduce cost basis proportionally (Bug #8 fix)
            let next_shares = (existing.shares - shares).max(0.0);
            // Reduce cost basis proportionally to shares sold
            let fraction_remaining = if existing.shares > 0.0 {
                next_shares / existing.shares
            } else {
                0.0
            };
            let next_cost = existing.cost_basis * fraction_remaining;
            let next_avg = if next_shares > 0.0 {
                next_cost / next_shares
            } else {
                0.0
            };

            self.positions.insert(
                trade.token_id.clone(),
                PositionState {
                    token_id: trade.token_id.clone(),
                    market: trade.market.clone(),
                    outcome: trade.outcome.clone(),
                    shares: next_shares,
                    cost_basis: next_cost,
                    avg_price: next_avg,
                },
            );
        }
    }

    pub fn get_notional(&self, token_id: &str) -> f64 {
        self.positions
            .get(token_id)
            .map(|p| p.cost_basis)
            .unwrap_or(0.0)
    }

    pub fn get_position(&self, token_id: &str) -> Option<&PositionState> {
        self.positions.get(token_id)
    }

    pub fn get_all(&self) -> Vec<PositionState> {
        self.positions.values().cloned().collect()
    }

    /// Returns all tracked token IDs (for seeding WebSocket subscriptions)
    #[allow(dead_code)]
    pub fn all_token_ids(&self) -> Vec<String> {
        self.positions.keys().cloned().collect()
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
