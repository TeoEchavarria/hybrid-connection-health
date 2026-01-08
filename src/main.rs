mod config;
mod p2p;

use anyhow::Result;
use p2p::swarm::{build_swarm, run_swarm};
use tracing::info;
use tokio::signal;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_max_level(tracing::Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    // Parse CLI args
    let config = config::parse_args();
    info!("Starting P2P Node with Role: {}", config.role);

    // Build Swarm
    let swarm = build_swarm(&config).await?;

    // Run Swarm loop with graceful shutdown
    tokio::select! {
        res = run_swarm(swarm, config) => {
            if let Err(e) = res {
                tracing::error!("Swarm error: {:?}", e);
            }
        }
        _ = signal::ctrl_c() => {
            info!("Received Ctrl+C, shutting down...");
        }
    }

    Ok(())
}
