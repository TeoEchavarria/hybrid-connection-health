use clap::{Parser, ValueEnum};
use std::fmt;

#[derive(Debug, Clone, ValueEnum)]
pub enum Role {
    Client,
    Gateway,
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Role::Client => write!(f, "client"),
            Role::Gateway => write!(f, "gateway"),
        }
    }
}

#[derive(Parser, Debug, Clone)]
#[command(name = "hybrid-connection-health")]
#[command(version = "1.0")]
#[command(about = "P2P Agent for hybrid connection health monitoring")]
pub struct Config {
    /// Role of the node: client or gateway
    #[arg(long, value_enum)]
    pub role: Role,

    /// Multiaddr to listen on
    #[arg(long, default_value = "/ip4/0.0.0.0/tcp/0")]
    pub listen: String,

    /// Optional peer to dial (multiaddr)
    #[arg(long)]
    pub dial: Option<String>,
}

pub fn parse_args() -> Config {
    Config::parse()
}
