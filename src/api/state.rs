use crate::config::Config;
use libp2p::multiaddr::Protocol;
use libp2p::Multiaddr;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

pub type SharedNetworkState = Arc<RwLock<NetworkSnapshot>>;

#[derive(Debug, Clone, Serialize)]
pub struct NetworkSnapshot {
    pub local_peer_id: String,
    pub role: String,
    pub listen: String,
    pub bootstrap_peers: Vec<BootstrapPeerRow>,
    pub peers: BTreeMap<String, PeerRow>,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BootstrapPeerRow {
    pub multiaddr: String,
    pub peer_id: Option<String>,
    pub connected: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PeerRow {
    pub peer_id: String,
    pub connected: bool,
    pub discovered_via: BTreeSet<String>,
    pub last_rtt_ms: Option<u64>,
}

pub fn new_shared_network_state(config: &Config, local_peer_id: String) -> SharedNetworkState {
    Arc::new(RwLock::new(NetworkSnapshot::new(config, local_peer_id)))
}

impl NetworkSnapshot {
    pub fn new(config: &Config, local_peer_id: String) -> Self {
        let bootstrap_peers = config
            .bootstrap_peers
            .iter()
            .map(|ma| BootstrapPeerRow {
                multiaddr: ma.clone(),
                peer_id: peer_id_from_multiaddr_str(ma),
                connected: false,
            })
            .collect();

        Self {
            local_peer_id,
            role: config.role.to_string(),
            listen: config.listen.clone(),
            bootstrap_peers,
            peers: BTreeMap::new(),
            updated_at_ms: now_ms(),
        }
    }

    pub fn set_connected(&mut self, peer_id: String, connected: bool) {
        let entry = self.peers.entry(peer_id.clone()).or_insert_with(|| PeerRow {
            peer_id: peer_id.clone(),
            connected,
            discovered_via: BTreeSet::new(),
            last_rtt_ms: None,
        });
        entry.connected = connected;
        self.refresh_bootstrap_connected_flags();
        self.touch();
    }

    pub fn mark_discovered(&mut self, peer_id: String, via: &'static str) {
        let entry = self.peers.entry(peer_id.clone()).or_insert_with(|| PeerRow {
            peer_id: peer_id.clone(),
            connected: false,
            discovered_via: BTreeSet::new(),
            last_rtt_ms: None,
        });
        entry.discovered_via.insert(via.to_string());
        self.touch();
    }

    pub fn set_rtt_ms(&mut self, peer_id: String, rtt_ms: u64) {
        let entry = self.peers.entry(peer_id.clone()).or_insert_with(|| PeerRow {
            peer_id: peer_id.clone(),
            connected: false,
            discovered_via: BTreeSet::new(),
            last_rtt_ms: None,
        });
        entry.last_rtt_ms = Some(rtt_ms);
        self.touch();
    }

    fn refresh_bootstrap_connected_flags(&mut self) {
        for bp in &mut self.bootstrap_peers {
            bp.connected = bp
                .peer_id
                .as_ref()
                .and_then(|pid| self.peers.get(pid))
                .map(|p| p.connected)
                .unwrap_or(false);
        }
    }

    fn touch(&mut self) {
        self.updated_at_ms = now_ms();
    }
}

fn peer_id_from_multiaddr_str(multiaddr: &str) -> Option<String> {
    let addr = multiaddr.parse::<Multiaddr>().ok()?;
    for p in addr.iter() {
        if let Protocol::P2p(peer_id) = p {
            return Some(peer_id.to_string());
        }
    }
    None
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

