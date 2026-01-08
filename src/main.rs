use futures::StreamExt;
use libp2p::{
    PeerId, development_transport, identity, mdns,
    swarm::{SwarmBuilder, SwarmEvent},
};

#[async_std::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Hybrid Connection Health Agent");
    start_swarm().await
}

async fn start_swarm() -> Result<(), Box<dyn std::error::Error>> {
    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from(id_keys.public());
    println!("PeerId: {peer_id}");

    let transport = development_transport(id_keys).await?;

    let behaviour = mdns::async_io::Behaviour::new(mdns::Config::default(), peer_id)?;

    let executor = |fut| {
        let _ = async_std::task::spawn(fut);
    };

    let mut swarm = SwarmBuilder::with_executor(transport, behaviour, peer_id, executor).build();

    swarm.listen_on("/ip4/127.0.0.1/tcp/0".parse()?)?;

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
