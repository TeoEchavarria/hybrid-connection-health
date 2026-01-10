use super::{
    behaviour::{NodeBehaviour, NodeBehaviourEvent},
    protocol::{Op, OpCodec, OpProtocol, Msg},
};
use crate::config::{Config, Role};
use anyhow::{Context, Result};
use futures::StreamExt;
use libp2p::{
    core::upgrade,
    identify, kad, ping,
    mdns,
    noise,
    request_response::{self, ProtocolSupport},
    swarm::SwarmEvent,
    tcp,
    yamux,
    Multiaddr, PeerId, Swarm, Transport,
};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tracing::{info, error, warn};
use uuid::Uuid;

/// Tracks dial attempts to prevent dial loops
struct DialState {
    last_dial: HashMap<PeerId, Instant>,
    cooldown: Duration,
    bootstrap_attempted: bool,
}

impl DialState {
    fn new() -> Self {
        Self {
            last_dial: HashMap::new(),
            cooldown: Duration::from_secs(30),
            bootstrap_attempted: false,
        }
    }
    
    fn can_dial(&mut self, peer_id: &PeerId) -> bool {
        if let Some(last) = self.last_dial.get(peer_id) {
            if last.elapsed() < self.cooldown {
                return false;
            }
        }
        self.last_dial.insert(*peer_id, Instant::now());
        true
    }
}

pub async fn build_swarm(config: &Config) -> Result<Swarm<NodeBehaviour>> {
    let id_keys = config.identity_keypair.clone();
    let peer_id = PeerId::from(id_keys.public());
    info!("🆔 Local PeerId: {}", peer_id);

    // NOTE: Relay support is not wired up yet in this repo. We still read this
    // config so it's not silently ignored.
    if config.enable_relay {
        warn!("Relay is enabled in config, but relay transport/behaviour is not configured yet; ignoring enable_relay=true for now.");
    }

    let tcp_transport = tcp::tokio::Transport::new(tcp::Config::default().nodelay(true));
    
    let transport = tcp_transport
        .upgrade(upgrade::Version::V1)
        .authenticate(noise::Config::new(&id_keys).context("Failed to create noise config")?)
        .multiplex(yamux::Config::default())
        .boxed();

    // Identify behaviour
    let identify = identify::Behaviour::new(identify::Config::new(
        "/hybrid-connection-health/1.0.0".to_string(),
        id_keys.public(),
    ));

    // mDNS for LAN discovery
    let mdns = if config.enable_mdns {
        let mut mdns_config = mdns::Config::default();
        mdns_config.query_interval = Duration::from_secs(5);
        mdns::tokio::Behaviour::new(mdns_config, peer_id)?
    } else {
        // Create disabled mDNS (will still be in the behaviour, just won't discover)
        warn!("mDNS disabled in configuration");
        let mdns_config = mdns::Config::default();
        mdns::tokio::Behaviour::new(mdns_config, peer_id)?
    };

    // Kademlia DHT
    let kad = if config.enable_kad {
        let mut kad_config = kad::Config::default();
        kad_config.set_query_timeout(Duration::from_secs(60));
        let store = kad::store::MemoryStore::new(peer_id);
        let mut kad_behaviour = kad::Behaviour::with_config(peer_id, store, kad_config);
        
        // Set Kademlia mode based on role
        if matches!(config.role, Role::Gateway) {
            kad_behaviour.set_mode(Some(kad::Mode::Server));
            info!("📡 Kademlia mode: Server (Gateway)");
        } else {
            kad_behaviour.set_mode(Some(kad::Mode::Client));
            info!("📡 Kademlia mode: Client");
        }
        
        kad_behaviour
    } else {
        warn!("Kademlia DHT disabled in configuration");
        let store = kad::store::MemoryStore::new(peer_id);
        kad::Behaviour::new(peer_id, store)
    };

    // Ping behaviour
    let ping = ping::Behaviour::new(ping::Config::new());

    // RequestResponse
    let protocols = std::iter::once((OpProtocol, ProtocolSupport::Full));
    let request_response = request_response::Behaviour::<OpCodec>::new(
        protocols,
        request_response::Config::default(),
    );

    let behaviour = NodeBehaviour {
        identify,
        mdns,
        kad,
        ping,
        request_response,
    };

    let mut swarm = Swarm::new(
        transport,
        behaviour,
        peer_id,
        libp2p::swarm::Config::with_tokio_executor()
            .with_idle_connection_timeout(Duration::from_secs(300)), // Keep connections alive for 5 minutes
    );

    swarm.listen_on(config.listen.parse()?)?;

    // Dial bootstrap peers for DHT
    if config.enable_kad {
        for bootstrap_addr in &config.bootstrap_peers {
            match bootstrap_addr.parse::<Multiaddr>() {
                Ok(addr) => {
                    info!("🔗 Dialing bootstrap peer: {}", bootstrap_addr);
                    if let Err(e) = swarm.dial(addr.clone()) {
                        error!("Failed to dial bootstrap peer {}: {:?}", bootstrap_addr, e);
                    }
                    
                    // Extract peer ID and add to Kademlia
                    if let Some(libp2p::multiaddr::Protocol::P2p(peer_id_hash)) = 
                        addr.iter().find(|p| matches!(p, libp2p::multiaddr::Protocol::P2p(_))) 
                    {
                        swarm.behaviour_mut().kad.add_address(&peer_id_hash, addr);
                    }
                }
                Err(e) => error!("Invalid bootstrap multiaddr '{}': {:?}", bootstrap_addr, e),
            }
        }
    }

    // Optional manual dial from CLI (legacy support)
    if let Some(dial_addr) = &config.dial {
        match dial_addr.parse::<Multiaddr>() {
            Ok(addr) => {
                info!("🔗 Dialing manual peer from CLI: {}", dial_addr);
                if let Err(e) = swarm.dial(addr) {
                    error!("Failed to dial CLI peer: {:?}", e);
                }
            }
            Err(e) => error!("Invalid multiaddr in manual dial: {:?}", e),
        }
    }

    // Dial peers from config file (legacy support)
    for peer_addr in &config.peers {
        match peer_addr.parse::<Multiaddr>() {
            Ok(addr) => {
                info!("🔗 Dialing config peer: {}", peer_addr);
                if let Err(e) = swarm.dial(addr) {
                    error!("Failed to dial peer {}: {:?}", peer_addr, e);
                }
            }
            Err(e) => error!("Invalid multiaddr in config peers list '{}': {:?}", peer_addr, e),
        }
    }

    Ok(swarm)
}

use crate::api::SharedNetworkState;
use crate::broker::handler::BrokerHandler;
use std::sync::Arc;

pub async fn run_swarm(
    mut swarm: Swarm<NodeBehaviour>,
    config: Config,
    network_state: SharedNetworkState,
    broker_handler: Option<Arc<BrokerHandler>>,
) -> Result<()> {
    let mut dial_state = DialState::new();
    let mut discovered_via_mdns: HashSet<PeerId> = HashSet::new();
    let mut discovered_via_kad: HashSet<PeerId> = HashSet::new();
    let start_time = Instant::now();
    let discovery_timeout = Duration::from_secs(config.discovery_timeout_secs);
    
    // Health check interval
    let mut health_check_interval = tokio::time::interval(Duration::from_secs(10));
    
    // DHT maintenance interval (random walks)
    let mut dht_maintenance_interval = tokio::time::interval(Duration::from_secs(60));

    info!("🚀 Starting P2P swarm event loop...");

    loop {
        tokio::select! {
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::NewListenAddr { address, .. } => {
                        info!("🎧 Listening on {:?}", address);
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                        info!("✅ Connection established with {} ({})", peer_id, endpoint.get_remote_address());

                        // Update shared network snapshot
                        {
                            let mut snap = network_state.write().await;
                            snap.set_connected(peer_id.to_string(), true);
                        }
                        
                        // Add peer to Kademlia and trigger bootstrap when we have an active connection
                        // This ensures bootstrap works regardless of startup order
                        if config.enable_kad {
                            // Add the peer's endpoint address to Kademlia routing table
                            swarm.behaviour_mut().kad.add_address(&peer_id, endpoint.get_remote_address().clone());
                            
                            // Trigger Kademlia bootstrap if not attempted yet
                            // Wait a brief moment if we just started (to let identify exchange addresses)
                            // but bootstrap immediately if we've been running for a bit
                            if !dial_state.bootstrap_attempted {
                                let should_bootstrap_now = start_time.elapsed() > Duration::from_secs(2);
                                
                                if should_bootstrap_now {
                                    info!("🌐 Bootstrapping Kademlia DHT after connection established...");
                                    if let Err(e) = swarm.behaviour_mut().kad.bootstrap() {
                                        warn!("Kademlia bootstrap failed (will retry later): {:?}", e);
                                    } else {
                                        dial_state.bootstrap_attempted = true;
                                    }
                                }
                            }
                        }
                        
                        // Legacy: send OpSubmit if Client role
                        if let Role::Client = config.role {
                             let op = Op {
                                 op_id: Uuid::new_v4().to_string(),
                                 actor_id: swarm.local_peer_id().to_string(),
                                 kind: "UpsertNote".into(),
                                 entity: "note:123".into(),
                                 payload_json: "{}".into(),
                                 created_at_ms: 1234567890,
                             };
                             info!("📤 Sending OpSubmit to connected peer {}", peer_id);
                             swarm.behaviour_mut().request_response.send_request(&peer_id, Msg::OpSubmit { op });
                        }
                    }
                    SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                        warn!("❌ Connection closed with {}: {:?}", peer_id, cause);

                        // Update shared network snapshot
                        {
                            let mut snap = network_state.write().await;
                            snap.set_connected(peer_id.to_string(), false);
                        }
                    }
                    
                    // Identify events
                    SwarmEvent::Behaviour(NodeBehaviourEvent::Identify(event)) => {
                        match *event {
                            identify::Event::Received { peer_id, info, .. } => {
                                info!("🔍 Identified peer {}: {} protocols, observed_addr={:?}", 
                                      peer_id, info.protocols.len(), info.observed_addr);
                                
                                // Add peer's listen addresses to Kademlia and swarm
                                for addr in info.listen_addrs {
                                    swarm.behaviour_mut().kad.add_address(&peer_id, addr.clone());
                                    swarm.add_peer_address(peer_id, addr);
                                }
                                
                                // Trigger Kademlia bootstrap after first successful identify
                                // This is a fallback in case ConnectionEstablished didn't trigger it
                                // We no longer require the 5-second delay since we have better timing in ConnectionEstablished
                                if config.enable_kad && !dial_state.bootstrap_attempted {
                                    info!("🌐 Bootstrapping Kademlia DHT after identify...");
                                    if let Err(e) = swarm.behaviour_mut().kad.bootstrap() {
                                        error!("Kademlia bootstrap failed: {:?}", e);
                                    } else {
                                        dial_state.bootstrap_attempted = true;
                                    }
                                }
                            }
                            identify::Event::Sent { .. } => {}
                            identify::Event::Pushed { .. } => {}
                            identify::Event::Error { peer_id, error, .. } => {
                                warn!("Identify error with {}: {:?}", peer_id, error);
                            }
                        }
                    }
                    
                    // mDNS events
                    SwarmEvent::Behaviour(NodeBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                        for (peer_id, multiaddr) in list {
                            info!("📡 mDNS Discovered: {} at {}", peer_id, multiaddr);
                            discovered_via_mdns.insert(peer_id);

                            {
                                let mut snap = network_state.write().await;
                                snap.mark_discovered(peer_id.to_string(), "mdns");
                            }
                            
                            swarm.add_peer_address(peer_id, multiaddr.clone());
                            if config.enable_kad {
                                swarm.behaviour_mut().kad.add_address(&peer_id, multiaddr);
                            }
                            
                            // Symmetric auto-dial (no role restriction)
                            if !swarm.is_connected(&peer_id) && dial_state.can_dial(&peer_id) {
                                info!("📞 Auto-dialing mDNS peer: {}", peer_id);
                                let _ = swarm.dial(peer_id);
                            }
                        }
                    }
                    SwarmEvent::Behaviour(NodeBehaviourEvent::Mdns(mdns::Event::Expired(list))) => {
                        for (peer_id, _multiaddr) in list {
                            info!("⏱️  mDNS Expired: {}", peer_id);
                        }
                    }
                    
                    // Kademlia events
                    SwarmEvent::Behaviour(NodeBehaviourEvent::Kad(kad::Event::OutboundQueryProgressed { result, .. })) => {
                        match result {
                            kad::QueryResult::Bootstrap(Ok(kad::BootstrapOk { peer, .. })) => {
                                info!("✅ Kademlia bootstrap success with peer: {}", peer);
                            }
                            kad::QueryResult::Bootstrap(Err(e)) => {
                                error!("❌ Kademlia bootstrap error: {:?}", e);
                            }
                            kad::QueryResult::GetClosestPeers(Ok(ok)) => {
                                info!("🔍 Found {} closest peers via Kademlia", ok.peers.len());
                                for peer_info in &ok.peers {
                                    discovered_via_kad.insert(peer_info.peer_id);
                                }
                            }
                            _ => {}
                        }
                    }
                    SwarmEvent::Behaviour(NodeBehaviourEvent::Kad(kad::Event::RoutingUpdated { peer, addresses, .. })) => {
                        info!("🗺️  Kademlia routing updated: {} with {} addresses", peer, addresses.len());
                        discovered_via_kad.insert(peer);

                        {
                            let mut snap = network_state.write().await;
                            snap.mark_discovered(peer.to_string(), "kad");
                        }
                        
                        // Auto-dial if not connected (symmetric)
                        if !swarm.is_connected(&peer) && dial_state.can_dial(&peer) {
                            info!("📞 Auto-dialing peer from Kademlia routing table: {}", peer);
                            let _ = swarm.dial(peer);
                        }
                    }
                    
                    // Ping events
                    SwarmEvent::Behaviour(NodeBehaviourEvent::Ping(ping::Event { peer, result, .. })) => {
                        match result {
                            Ok(rtt) => {
                                {
                                    let mut snap = network_state.write().await;
                                    snap.set_rtt_ms(peer.to_string(), rtt.as_millis() as u64);
                                }
                                // Don't log every ping to reduce noise
                                if rtt.as_millis() > 500 {
                                    warn!("🏓 High latency ping from {}: {:?}", peer, rtt);
                                }
                            }
                            Err(e) => {
                                warn!("Ping failure with {}: {:?}", peer, e);
                            }
                        }
                    }
                    
                    // RequestResponse events
                    SwarmEvent::Behaviour(NodeBehaviourEvent::RequestResponse(request_response::Event::Message { peer, message, .. })) => {
                       match message {
                           request_response::Message::Request { request, channel, .. } => {
                               match request {
                                   Msg::OpSubmit { op } => {
                                       info!("📥 Received OpSubmit from {}: {:?}", peer, op);
                                       
                                       let ack = Msg::OpAck { 
                                           op_id: op.op_id, 
                                           ok: true, 
                                           msg: "Processed".into() 
                                       };
                                       
                                       info!("📤 Sending OpAck to {}", peer);
                                       let _ = swarm.behaviour_mut().request_response.send_response(channel, ack);
                                   },
                                   Msg::SubmitBooking { correlation_id, booking, notify } => {
                                       // Only process if Gateway role and broker handler available
                                       if matches!(config.role, Role::Gateway) {
                                           if let Some(ref handler) = broker_handler {
                                               info!("📥 Received SubmitBooking from {}: correlation_id={}", peer, correlation_id);
                                               
                                               // Handle booking submission
                                               match handler.handle_submit_booking(correlation_id.clone(), booking, notify).await {
                                                   Ok(ack) => {
                                                       info!("📤 Sending BookingAck to {}: correlation_id={}", peer, correlation_id);
                                                       let _ = swarm.behaviour_mut().request_response.send_response(channel, ack);
                                                   },
                                                   Err(e) => {
                                                       error!("Failed to handle booking submission: {:?}", e);
                                                       // Send error ACK
                                                       let error_ack = Msg::BookingAck {
                                                           correlation_id,
                                                           status: "error".to_string(),
                                                       };
                                                       let _ = swarm.behaviour_mut().request_response.send_response(channel, error_ack);
                                                   }
                                               }
                                           } else {
                                               warn!("Received SubmitBooking but broker handler not available");
                                               let error_ack = Msg::BookingAck {
                                                   correlation_id,
                                                   status: "error".to_string(),
                                               };
                                               let _ = swarm.behaviour_mut().request_response.send_response(channel, error_ack);
                                           }
                                       } else {
                                           warn!("Received SubmitBooking but node is not a Gateway");
                                           let error_ack = Msg::BookingAck {
                                               correlation_id,
                                               status: "error".to_string(),
                                           };
                                           let _ = swarm.behaviour_mut().request_response.send_response(channel, error_ack);
                                       }
                                   },
                                   _ => info!("Received other request from {}", peer),
                               }
                           }
                           request_response::Message::Response { response, .. } => {
                                match response {
                                    Msg::OpAck { op_id, ok, msg } => {
                                        info!("📬 Received OpAck from {}: op_id={} ok={} msg={}", peer, op_id, ok, msg);
                                    }
                                    Msg::BookingAck { correlation_id, status } => {
                                        info!("📬 Received BookingAck from {}: correlation_id={} status={}", peer, correlation_id, status);
                                    }
                                    _ => info!("Received other response from {}", peer),
                                }
                           }
                       }
                    }
                    SwarmEvent::Behaviour(NodeBehaviourEvent::RequestResponse(request_response::Event::ResponseSent { .. })) => {
                        // Response sent confirmation
                    }
                    SwarmEvent::Behaviour(NodeBehaviourEvent::RequestResponse(request_response::Event::OutboundFailure { peer, error, .. })) => {
                        error!("Outbound failure for peer {:?}: {:?}", peer, error);
                    }
                    SwarmEvent::Behaviour(NodeBehaviourEvent::RequestResponse(request_response::Event::InboundFailure { peer, error, .. })) => {
                         error!("Inbound failure for peer {:?}: {:?}", peer, error);
                    }
                    // End of primary event handlers
                    _ => {}
                }
            }
            
            _ = health_check_interval.tick() => {
                let connected = swarm.connected_peers().count();
                let uptime = start_time.elapsed();
                
                info!("💚 Discovery health: connected={}, mdns_discovered={}, kad_discovered={}, uptime={:?}",
                      connected, discovered_via_mdns.len(), discovered_via_kad.len(), uptime);
                
                // Warning if no peers discovered
                if uptime > discovery_timeout && connected == 0 {
                    error!("⚠️  No peers discovered after {:?}. Check bootstrap_peers config and network connectivity.", discovery_timeout);
                    
                    if config.bootstrap_peers.is_empty() && !config.enable_mdns {
                        error!("💡 Hint: Both mDNS and bootstrap_peers are disabled/empty. Enable at least one discovery method.");
                    }
                }
            }
            
            _ = dht_maintenance_interval.tick() => {
                // Periodic random DHT walk to keep routing table fresh
                if config.enable_kad && dial_state.bootstrap_attempted {
                    let random_peer = PeerId::random();
                    swarm.behaviour_mut().kad.get_closest_peers(random_peer);
                }
            }
        }
    }
}

pub async fn run_test_submission(mut swarm: Swarm<NodeBehaviour>, dial_addr: String, timeout_secs: u64) -> Result<()> {
    // 1. Dial the target
    let addr: Multiaddr = dial_addr.parse()?;
    info!("Test: Dialing {}...", addr);
    swarm.dial(addr.clone())?;
    
    let target_peer = match addr.iter().find(|p| matches!(p, libp2p::multiaddr::Protocol::P2p(_))) {
        Some(libp2p::multiaddr::Protocol::P2p(peer_id)) => Some(peer_id),
        _ => None,
    };

    let mut op_sent = false;
    let expected_op_id = Uuid::new_v4().to_string();
    let timeout = Duration::from_secs(timeout_secs);
    let start_time = Instant::now();

    loop {
        if start_time.elapsed() > timeout {
            anyhow::bail!("Test timed out after {} seconds", timeout_secs);
        }

        let event = tokio::select! {
             e = swarm.select_next_some() => e,
             _ = tokio::time::sleep(Duration::from_millis(100)) => continue,
        };

        match event {
            SwarmEvent::NewListenAddr { address, .. } => {
                info!("Test: Listening on {:?}", address);
            }
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                info!("Test: Connected to {}", peer_id);
                if let Some(tp) = target_peer {
                     if tp != peer_id {
                         continue;
                     }
                }
                
                if !op_sent {
                     let op = Op {
                         op_id: expected_op_id.clone(),
                         actor_id: swarm.local_peer_id().to_string(),
                         kind: "TestOp".into(),
                         entity: "test".into(),
                         payload_json: "{}".into(),
                         created_at_ms: 123456,
                     };
                     info!("Test: Sending OpSubmit to {}", peer_id);
                     swarm.behaviour_mut().request_response.send_request(&peer_id, Msg::OpSubmit { op });
                     op_sent = true;
                }
            }
             SwarmEvent::Behaviour(NodeBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                for (peer_id, multiaddr) in list {
                    info!("Test: mDNS Discovered: {} at {}", peer_id, multiaddr);
                    swarm.add_peer_address(peer_id, multiaddr);
                    if let Some(tp) = target_peer {
                        if tp == peer_id && !swarm.is_connected(&peer_id) {
                             let _ = swarm.dial(peer_id);
                        }
                    }
                }
            }
             SwarmEvent::Behaviour(NodeBehaviourEvent::RequestResponse(request_response::Event::Message { peer, message, .. })) => {
                match message {
                     request_response::Message::Response { response, .. } => {
                         match response {
                             Msg::OpAck { op_id, ok, msg } => {
                                 info!("Test: Received ACK from {}: op_id={} ok={} msg={}", peer, op_id, ok, msg);
                                 if op_id == expected_op_id && ok {
                                     info!("Test PASSED: Valid ACK received.");
                                     return Ok(());
                                 } else {
                                     anyhow::bail!("Test FAILED: Invalid ACK (id mismatch or ok=false)");
                                 }
                             }
                             _ => {}
                         }
                    }
                    _ => {}
                }
             }
             SwarmEvent::Behaviour(NodeBehaviourEvent::RequestResponse(request_response::Event::OutboundFailure { error, .. })) => {
                 error!("Test: Outbound failure: {:?}", error);
                 if op_sent {
                      anyhow::bail!("Test FAILED: Outbound failure after send");
                 }
             }
             _ => {}
        }
    }
}
