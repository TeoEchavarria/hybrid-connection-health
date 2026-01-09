mod config;
mod p2p;
mod api;

use anyhow::Result;
use config::Commands;
use p2p::swarm::{build_swarm, run_swarm, run_test_submission};
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
    let (cli_args, config) = config::parse_args();

    match cli_args.command {
        Some(Commands::PeerId) => {
            let peer_id = libp2p::PeerId::from(config.identity_keypair.public());
            println!("{}", peer_id);
            return Ok(());
        }
        Some(Commands::TestSubmit { listen, dial, timeout_secs }) => {
            info!("Starting One-Shot Test: Submit Op -> Wait Ack");
            // Build swarm with persistent identity (from config) but override listen addr
            // We use the same config struct but maybe we should override listen in it?
            // Actually build_swarm uses config.listen.
            let mut test_config = config.clone();
            test_config.listen = listen;
            // dial is passed to run_test_submission, not used in build_swarm for initial dial here (though it could be)
            
            let swarm = build_swarm(&test_config).await?;
            run_test_submission(swarm, dial, timeout_secs).await?;
            info!("Test completed successfully.");
            return Ok(());
        }
        _ => {
            // Run mode (Default or Explicit)
            info!("Starting P2P Node with Role: {}", config.role);
            
            // Build Swarm
            let swarm = build_swarm(&config).await?;
            let local_peer_id = swarm.local_peer_id().to_string();
            let network_state = api::new_shared_network_state(&config, local_peer_id);

            // Iniciar API local en paralelo con el swarm
            let api_state = network_state.clone();
            let api_task = tokio::spawn(async {
                api::iniciar_api_local(api_state).await;
            });

            // Run Swarm loop with graceful shutdown
            tokio::select! {
                res = run_swarm(swarm, config, network_state) => {
                    if let Err(e) = res {
                        tracing::error!("Swarm error: {:?}", e);
                    }
                }
                _ = signal::ctrl_c() => {
                    info!("Received Ctrl+C, shutting down...");
                }
            }

            // Abort API task on shutdown
            api_task.abort();
        }
    }

    Ok(())
}
