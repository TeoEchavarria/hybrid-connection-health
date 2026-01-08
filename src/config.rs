use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Role {
    Client,
    Gateway,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub role: Role,
    pub local_api_port: u16,
    pub p2p_listen_addr: String,
}

impl Config {
    pub fn defaults_for(role: Role) -> Self {
        match role {
            Role::Gateway => Self {
                role,
                local_api_port: 7000,
                p2p_listen_addr: "/ip4/0.0.0.0/tcp/4001".to_string(),
            },
            Role::Client => Self {
                role,
                local_api_port: 7001,
                p2p_listen_addr: "/ip4/0.0.0.0/tcp/0".to_string(),
            },
        }
    }
}

/// CLI es *input*, no configuración final
#[derive(Debug, Parser)]
#[command(name = "hybrid-connection-health", version)]
pub struct Cli {
    #[arg(long, value_enum, default_value_t = Role::Client)]
    pub role: Role,

    #[arg(long)]
    pub local_api_port: Option<u16>,

    #[arg(long)]
    pub p2p_listen_addr: Option<String>,
}

impl Cli {
    pub fn into_config(self) -> Config {
        let mut cfg = Config::defaults_for(self.role);

        if let Some(p) = self.local_api_port {
            cfg.local_api_port = p;
        }
        if let Some(addr) = self.p2p_listen_addr {
            cfg.p2p_listen_addr = addr;
        }

        cfg
    }
}
