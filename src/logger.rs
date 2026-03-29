use std::fs;
use std::path::Path;
use chrono::{Duration, Utc};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
 
pub fn init() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,reqwest=warn"));
 
    let log_dir = "logs";
    let file_appender = RollingFileAppender::new(Rotation::DAILY, log_dir, "copybot.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
 
    // Leak the guard to keep it alive for the duration of the program.
    // tracing-appender needs the guard to stay in scope to flush logs.
    Box::leak(Box::new(_guard));
 
    cleanup_old_logs(log_dir, 7);
 
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_target(false).compact())
        .with(tracing_subscriber::fmt::layer().with_writer(non_blocking).with_ansi(false).compact())
        .init();
}
 
fn cleanup_old_logs(dir: &str, days: i64) {
    let path = Path::new(dir);
    if !path.exists() {
        return;
    }
 
    let threshold = Utc::now() - Duration::days(days);
 
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(metadata) = entry.metadata() {
                if let Ok(modified) = metadata.modified() {
                    let modified_time: chrono::DateTime<Utc> = modified.into();
                    if modified_time < threshold {
                        let _ = fs::remove_file(entry.path());
                    }
                }
            }
        }
    }
}
