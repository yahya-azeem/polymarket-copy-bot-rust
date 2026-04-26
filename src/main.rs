use anyhow::Result;
use tracing::info;

mod config;
mod logger;
mod monitor;
mod positions;
mod risk_manager;
mod trader;
mod utils;
mod websocket_monitor;
mod types;
mod bot;
mod leaderboard;

use config::Config;
use bot::PolymarketCopyBot;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    logger::init();
    let config = Config::from_env()?;
    config.validate()?;

    let mut bot = PolymarketCopyBot::new(config).await?;
    bot.initialize().await?;
    
    // Graceful Shutdown Handler
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    ctrlc::set_handler(move || {
        let _ = tx.blocking_send(());
    })?;

    info!("Bot is running. Press Ctrl+C to exit gracefully.");
    
    tokio::select! {
        res = bot.run() => {
            if let Err(e) = res {
                tracing::error!("Bot error: {e}");
            }
        }
        _ = rx.recv() => {
            info!("Shutting down gracefully...");
            // Cleanup happens here if needed
        }
    }

    Ok(())
}
