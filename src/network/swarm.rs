use async_std::task;
use futures::StreamExt;
use libp2p::{
    PeerId, development_transport, identity, mdns,
    swarm::{SwarmBuilder, SwarmEvent},
};

use crate::config::Config;

pub async fn start_swarm(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from(id_keys.public());
    println!("PeerId: {peer_id}");

    let transport = development_transport(id_keys).await?;

    let behaviour = mdns::async_io::Behaviour::new(mdns::Config::default(), peer_id)?;

    let executor = |fut| {
        let _ = task::spawn(fut);
    };

    let mut swarm = SwarmBuilder::with_executor(transport, behaviour, peer_id, executor).build();

    swarm.listen_on(config.p2p_listen_addr.parse()?)?;

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => {
                println!("Listening on {address}");
            }
            SwarmEvent::Behaviour(event) => {
                println!("mDNS event: {:?}", event);
            }
            _ => {}
        }
    }
}
