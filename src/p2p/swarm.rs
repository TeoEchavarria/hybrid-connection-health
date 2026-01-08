use super::{
    behaviour::{NodeBehaviour, NodeBehaviourEvent},
    protocol::{Op, OpCodec, OpProtocol, Msg},
};
use crate::config::{Config, Role};
use anyhow::{Context, Result};
use futures::StreamExt;
use libp2p::{
    core::upgrade,
    identity,
    mdns,
    noise,
    request_response::{self, ProtocolSupport},
    swarm::{SwarmEvent},
    tcp,
    yamux,
    PeerId, Swarm, Transport,
};
use tracing::{info, error};
use uuid::Uuid;

pub async fn build_swarm(config: &Config) -> Result<Swarm<NodeBehaviour>> {
    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from(id_keys.public());
    info!("Local PeerId: {}", peer_id);

    let tcp_transport = tcp::tokio::Transport::new(tcp::Config::default().nodelay(true));
    
    let transport = tcp_transport
        .upgrade(upgrade::Version::V1)
        .authenticate(noise::Config::new(&id_keys).context("Failed to create noise config")?)
        .multiplex(yamux::Config::default())
        .boxed();

    // mDNS
    let mut mdns_config = mdns::Config::default();
    // Use a shorter query interval to speed up discovery in demos
    mdns_config.query_interval = std::time::Duration::from_secs(5);
    let mdns = mdns::tokio::Behaviour::new(mdns_config, peer_id)?;

    // RequestResponse
    let protocols = std::iter::once((OpProtocol, ProtocolSupport::Full));
    let request_response = request_response::Behaviour::<OpCodec>::new(
        protocols,
        request_response::Config::default(),
    );

    let behaviour = NodeBehaviour {
        mdns,
        request_response,
    };

    let mut swarm = Swarm::new(
        transport,
        behaviour,
        peer_id,
        libp2p::swarm::Config::with_tokio_executor(),
    );

    swarm.listen_on(config.listen.parse()?)?;

    // Optional manual dial
    if let Some(dial_addr) = &config.dial {
        let addr: libp2p::Multiaddr = dial_addr.parse()?;
        swarm.dial(addr)?;
        info!("Dialing manual peer: {}", dial_addr);
    }

    Ok(swarm)
}

pub async fn run_swarm(mut swarm: Swarm<NodeBehaviour>, config: Config) -> Result<()> {
    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => {
                info!("Listening on {:?}", address);
            }
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                info!("Connection established with {}", peer_id);
                
                if let Role::Client = config.role {
                     let op = Op {
                         op_id: Uuid::new_v4().to_string(),
                         actor_id: swarm.local_peer_id().to_string(),
                         kind: "UpsertNote".into(),
                         entity: "note:123".into(),
                         payload_json: "{}".into(),
                         created_at_ms: 1234567890,
                     };
                     info!("Sending OpSubmit to connected peer {}", peer_id);
                     swarm.behaviour_mut().request_response.send_request(&peer_id, Msg::OpSubmit { op });
                }
            }
            SwarmEvent::Behaviour(NodeBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                for (peer_id, multiaddr) in list {
                    info!("mDNS Discovered: {} at {}", peer_id, multiaddr);
                    
                    // Add address to swarm so we can dial it if needed
                    swarm.add_peer_address(peer_id, multiaddr.clone());

                    // If we are client, ensure we are connected
                    if let Role::Client = config.role {
                         if !swarm.is_connected(&peer_id) {
                              let _ = swarm.dial(peer_id);
                         }
                    }
                }
            }
            SwarmEvent::Behaviour(NodeBehaviourEvent::Mdns(mdns::Event::Expired(list))) => {
                for (peer_id, _multiaddr) in list {
                    info!("mDNS Expired: {}", peer_id);
                }
            }
            SwarmEvent::Behaviour(NodeBehaviourEvent::RequestResponse(request_response::Event::Message { peer, message, .. })) => {
               match message {
                   request_response::Message::Request { request, channel, .. } => {
                       match request {
                           Msg::OpSubmit { op } => {
                               info!("Received OpSubmit from {}: {:?}", peer, op);
                               
                               // Both Gateway and Client (if peer-to-peer) logic:
                               // Respond with Ack
                               let ack = Msg::OpAck { 
                                   op_id: op.op_id, 
                                   ok: true, 
                                   msg: "Processed".into() 
                               };
                               
                               info!("Sending OpAck to {}", peer);
                               let _ = swarm.behaviour_mut().request_response.send_response(channel, ack);
                           },
                           _ => info!("Received other request from {}", peer),
                       }
                   }
                   request_response::Message::Response { response, .. } => {
                        match response {
                            Msg::OpAck { op_id, ok, msg } => {
                                info!("Received OpAck from {}: op_id={} ok={} msg={}", peer, op_id, ok, msg);
                            }
                            _ => info!("Received other response from {}", peer),
                        }
                   }
               }
            }
             SwarmEvent::Behaviour(NodeBehaviourEvent::RequestResponse(request_response::Event::ResponseSent { peer: _, .. })) => {
                // Confirm response sent
                // info!("Response sent to {}", peer);
            }
             SwarmEvent::Behaviour(NodeBehaviourEvent::RequestResponse(request_response::Event::OutboundFailure { peer, error, .. })) => {
                error!("Outbound failure for peer {:?}: {:?}", peer, error);
            }
            SwarmEvent::Behaviour(NodeBehaviourEvent::RequestResponse(request_response::Event::InboundFailure { peer, error, .. })) => {
                 error!("Inbound failure for peer {:?}: {:?}", peer, error);
            }
             _ => {}
        }
    }
}
