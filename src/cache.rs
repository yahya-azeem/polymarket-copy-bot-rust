use std::collections::HashSet;
use std::fs;
use std::path::Path;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

const CACHE_FILE: &str = "whale_cache.json";

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct WhaleCache {
    pub tracked_whales: HashSet<String>,
}

impl WhaleCache {
    pub fn load() -> Self {
        if !Path::new(CACHE_FILE).exists() {
            return Self::default();
        }

        match fs::read_to_string(CACHE_FILE) {
            Ok(content) => {
                match serde_json::from_str::<WhaleCache>(&content) {
                    Ok(cache) => {
                        info!("📂 Loaded {} whales from persistent cache.", cache.tracked_whales.len());
                        cache
                    }
                    Err(e) => {
                        warn!("Failed to parse whale cache: {e}. Starting fresh.");
                        Self::default()
                    }
                }
            }
            Err(e) => {
                warn!("Failed to read whale cache: {e}. Starting fresh.");
                Self::default()
            }
        }
    }

    pub fn save(&self) {
        match serde_json::to_string_pretty(self) {
            Ok(content) => {
                if let Err(e) = fs::write(CACHE_FILE, content) {
                    warn!("Failed to write whale cache to disk: {e}");
                }
            }
            Err(e) => warn!("Failed to serialize whale cache: {e}"),
        }
    }

    pub fn add_whales(&mut self, whales: Vec<String>) -> bool {
        let mut changed = false;
        for whale in whales {
            let whale_lower = whale.to_lowercase();
            if self.tracked_whales.insert(whale_lower) {
                changed = true;
            }
        }
        if changed {
            self.save();
        }
        changed
    }

    pub fn remove_whale(&mut self, whale: &str) -> bool {
        let whale_lower = whale.to_lowercase();
        if self.tracked_whales.remove(&whale_lower) {
            self.save();
            return true;
        }
        false
    }

    pub fn get_all(&self) -> Vec<String> {
        self.tracked_whales.iter().cloned().collect()
    }
}
