use super::protocol::{OpCodec, Msg};
use libp2p::{
    mdns,
    request_response,
    swarm::NetworkBehaviour,
};

#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "NodeBehaviourEvent")]
pub struct NodeBehaviour {
    pub mdns: mdns::tokio::Behaviour,
    pub request_response: request_response::Behaviour<OpCodec>,
}

#[derive(Debug)]
pub enum NodeBehaviourEvent {
    Mdns(mdns::Event),
    RequestResponse(request_response::Event<Msg, Msg>),
}

impl From<mdns::Event> for NodeBehaviourEvent {
    fn from(event: mdns::Event) -> Self {
        NodeBehaviourEvent::Mdns(event)
    }
}

impl From<request_response::Event<Msg, Msg>> for NodeBehaviourEvent {
    fn from(event: request_response::Event<Msg, Msg>) -> Self {
        NodeBehaviourEvent::RequestResponse(event)
    }
}
