use anyhow::Result;
use serde::Deserialize;
use tracing::{info, warn};

#[derive(Debug, Deserialize)]
struct LeaderboardEntry {
    wallet: String,
    #[serde(rename = "traderName")]
    #[allow(dead_code)]
    trader_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LeaderboardResponse {
    entries: Vec<LeaderboardEntry>,
}

pub async fn fetch_top_whales(limit: usize) -> Result<Vec<String>> {
    let url = "https://www.polywhaler.com/api/leaderboard";
    let client = reqwest::Client::new();
    
    let resp = client.get(url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .send()
        .await?;
        
    if !resp.status().is_success() {
        warn!("Failed to fetch Polywhaler leaderboard: {}", resp.status());
        return Ok(Vec::new());
    }

    let data: LeaderboardResponse = resp.json().await?;
    let mut whales = Vec::new();
    
    for entry in data.entries.iter().take(limit) {
        let addr = entry.wallet.to_lowercase();
        if !addr.starts_with("0x") || addr.len() != 42 {
            continue;
        }
        whales.push(addr);
    }

    info!("Fetched {} whales from Polywhaler leaderboard", whales.len());
    Ok(whales)
}
