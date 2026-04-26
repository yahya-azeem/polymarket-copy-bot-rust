
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

#[derive(Debug, Clone, Default)]
pub struct PositionState {
    pub token_id: String,
    pub market: String,
    pub outcome: String,
    pub shares: f64,
    pub cost_basis: f64, // total USDC spent to acquire (for accurate avg_price)
    pub avg_price: f64,
}

#[derive(Debug, Clone)]
pub struct CopyExecutionResult {
    pub order_id: String,
    pub copy_notional: f64,
    pub copy_shares: f64,
    pub price: f64,
    pub side: String,
}

#[derive(Default, Clone)]
pub struct Stats {
    pub trades_detected: u64,
    pub trades_copied: u64,
    pub trades_failed: u64,
    pub total_volume: f64,
}
