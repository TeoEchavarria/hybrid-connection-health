use clap::{Parser, Subcommand, ValueEnum};
use libp2p::identity;
use serde::Deserialize;
use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, ValueEnum, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
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
pub struct CliArgs {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Path to the identity file (keypair)
    #[arg(long, global = true)]
    pub identity_file: Option<PathBuf>,

    // --- Legacy args for backward compatibility/default "run" mode if no subcommand ---
    /// Role of the node: client or gateway
    #[arg(long, value_enum)]
    pub role: Option<Role>,

    /// Multiaddr to listen on
    #[arg(long)]
    pub listen: Option<String>,

    /// Optional peer to dial (multiaddr)
    #[arg(long)]
    pub dial: Option<String>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Run the agent in normal mode (default)
    Run {
        /// Role of the node: client or gateway
        #[arg(long, value_enum)]
        role: Option<Role>,

        /// Multiaddr to listen on
        #[arg(long)]
        listen: Option<String>,

        /// Optional peer to dial (multiaddr)
        #[arg(long)]
        dial: Option<String>,
    },
    /// Print the Peer ID derived from the identity file and exit
    PeerId,
    /// Run a one-shot P2P test (OpSubmit -> OpAck)
    TestSubmit {
        /// Multiaddr to listen on (e.g., /ip4/0.0.0.0/tcp/0)
        #[arg(long, default_value = "/ip4/0.0.0.0/tcp/0")]
        listen: String,

        /// Peer to dial (Multiaddr)
        #[arg(long)]
        dial: String,

        /// Timeout in seconds waiting for ACK
        #[arg(long, default_value = "10")]
        timeout_secs: u64,
    },
}

#[derive(Debug, Clone)]
pub struct Config {
    pub role: Role,
    pub listen: String,
    pub dial: Option<String>,
    pub peers: Vec<String>,
    pub identity_keypair: identity::Keypair,
    // Production peer discovery configuration
    pub bootstrap_peers: Vec<String>,
    pub enable_mdns: bool,
    pub enable_kad: bool,
    pub enable_relay: bool,
    pub discovery_timeout_secs: u64,
}

pub fn load_or_create_identity(path: &Path) -> identity::Keypair {
    if path.exists() {
        let mut file = fs::File::open(path).expect("Failed to open identity file");
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).expect("Failed to read identity file");
        
        match identity::Keypair::from_protobuf_encoding(&bytes) {
            Ok(kp) => return kp,
            Err(e) => {
                eprintln!("Failed to decode identity from file, creating new one: {:?}", e);
            }
        }
    }

    // Create new
    let keypair = identity::Keypair::generate_ed25519();
    let bytes = keypair.to_protobuf_encoding().expect("Failed to encode keypair");
    
    // Ensure parent dir exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("Failed to create identity file directory");
    }

    let mut file = fs::File::create(path).expect("Failed to create identity file");
    file.write_all(&bytes).expect("Failed to write identity file");
    
    keypair
}

pub fn parse_args() -> (CliArgs, Config) {
    let args = CliArgs::parse();
    
    // Load config from file if exists
    #[derive(Deserialize)]
    struct FileConfig {
        role: Option<Role>,
        listen: Option<String>,
        dial: Option<String>,
        #[serde(default)]
        peers: Vec<String>,
        #[serde(default)]
        bootstrap_peers: Vec<String>,
        enable_mdns: Option<bool>,
        enable_kad: Option<bool>,
        enable_relay: Option<bool>,
        discovery_timeout_secs: Option<u64>,
    }

    let file_config: Option<FileConfig> = if Path::new("config.toml").exists() {
        let content = fs::read_to_string("config.toml").expect("Failed to read config.toml");
        Some(toml::from_str(&content).expect("Failed to parse config.toml"))
    } else {
        None
    };

    // Determine Role, Listen, Dial based on args (Run subcommand or legacy top-level) or config file
    // Default values:
    let mut final_role = Role::Client;
    let mut final_listen = "/ip4/0.0.0.0/tcp/0".to_string();
    let mut final_dial = None;
    let mut final_peers = vec![];
    let mut final_bootstrap_peers = vec![];
    let mut final_enable_mdns = true;
    let mut final_enable_kad = true;
    let mut final_enable_relay = false;
    let mut final_discovery_timeout = 60;

    if let Some(cfg) = &file_config {
        if let Some(r) = &cfg.role { final_role = r.clone(); }
        if let Some(l) = &cfg.listen { final_listen = l.clone(); }
        final_dial = cfg.dial.clone();
        final_peers = cfg.peers.clone();
        final_bootstrap_peers = cfg.bootstrap_peers.clone();
        if let Some(mdns) = cfg.enable_mdns { final_enable_mdns = mdns; }
        if let Some(kad) = cfg.enable_kad { final_enable_kad = kad; }
        if let Some(relay) = cfg.enable_relay { final_enable_relay = relay; }
        if let Some(timeout) = cfg.discovery_timeout_secs { final_discovery_timeout = timeout; }
    }

    // Overrides from CLI
    match &args.command {
        Some(Commands::Run { role, listen, dial }) => {
            if let Some(r) = role { 
                final_role = r.clone(); 
            } else if let Some(r) = &args.role {
                // Fallback to top-level arg if subcommand arg is missing (parsing quirk?)
                final_role = r.clone();
            }

            if let Some(l) = listen { final_listen = l.clone(); }
            else if let Some(l) = &args.listen { final_listen = l.clone(); }

            if let Some(d) = dial { final_dial = Some(d.clone()); }
            else if let Some(d) = &args.dial { final_dial = Some(d.clone()); }
        }
        Some(Commands::PeerId) => {
            // No config needed for PeerId mainly, but we return a valid config anyway
        }
        Some(Commands::TestSubmit { listen, dial, .. }) => {
            final_role = Role::Client; // Tester acts as client
            final_listen = listen.clone();
            final_dial = Some(dial.clone());
        }
        None => {
            // Fallback: Check top-level args
            if let Some(r) = &args.role { final_role = r.clone(); }
            if let Some(l) = &args.listen { final_listen = l.clone(); }
            if let Some(d) = &args.dial { final_dial = Some(d.clone()); }
        }
    }

    // Identity handling
    let keypair = if let Some(path) = &args.identity_file {
        load_or_create_identity(path)
    } else {
        // If no file specified, generate ephemeral
        identity::Keypair::generate_ed25519()
    };

    let config = Config {
        role: final_role,
        listen: final_listen,
        dial: final_dial,
        peers: final_peers,
        identity_keypair: keypair,
        bootstrap_peers: final_bootstrap_peers,
        enable_mdns: final_enable_mdns,
        enable_kad: final_enable_kad,
        enable_relay: final_enable_relay,
        discovery_timeout_secs: final_discovery_timeout,
    };

    (args, config)
}
