mod config;
mod network;

use clap::Parser; // ← ESTO
use crate::config::Cli;
use crate::network::swarm::start_swarm;

#[async_std::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let config = cli.into_config();

    println!("Hybrid Connection Health Agent");
    println!("Config: {:?}", config);

    start_swarm(&config).await
}
