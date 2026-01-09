use super::protocol::{OpCodec, Msg};
use libp2p::{
    identify, mdns, kad, ping,
    request_response,
    swarm::NetworkBehaviour,
};

#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "NodeBehaviourEvent")]
pub struct NodeBehaviour {
    pub identify: identify::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
    pub kad: kad::Behaviour<kad::store::MemoryStore>,
    pub ping: ping::Behaviour,
    pub request_response: request_response::Behaviour<OpCodec>,
}

#[derive(Debug)]
pub enum NodeBehaviourEvent {
    Identify(Box<identify::Event>),
    Mdns(mdns::Event),
    Kad(kad::Event),
    Ping(ping::Event),
    RequestResponse(request_response::Event<Msg, Msg>),
}

// From trait implementations for event conversions
impl From<identify::Event> for NodeBehaviourEvent {
    fn from(event: identify::Event) -> Self {
        NodeBehaviourEvent::Identify(Box::new(event))
    }
}

impl From<mdns::Event> for NodeBehaviourEvent {
    fn from(event: mdns::Event) -> Self {
        NodeBehaviourEvent::Mdns(event)
    }
}

impl From<kad::Event> for NodeBehaviourEvent {
    fn from(event: kad::Event) -> Self {
        NodeBehaviourEvent::Kad(event)
    }
}

impl From<ping::Event> for NodeBehaviourEvent {
    fn from(event: ping::Event) -> Self {
        NodeBehaviourEvent::Ping(event)
    }
}

impl From<request_response::Event<Msg, Msg>> for NodeBehaviourEvent {
    fn from(event: request_response::Event<Msg, Msg>) -> Self {
        NodeBehaviourEvent::RequestResponse(event)
    }
}
