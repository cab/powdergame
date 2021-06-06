use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tracing::debug;

// #[derive(Debug, Clone, Deserialize, Serialize)]
// pub(crate) struct AckMessage<T> {
//     message: T,
// }

// impl<T> AckMessage<T> {
//     pub fn new(message: T) -> Self {
//         Self { message }
//     }

//     pub fn decode(bytes: &[u8]) -> Option<Self> {
//         bincode::deserialize(bytes).ok()
//     }

//     pub fn encode(&self) -> Vec<u8> {
//         bincode::serialize(self).unwrap()
//     }
// }

// used to avoid having the same bytes as a user packet
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProtocolMarker {
    version: u16,
}

impl ProtocolMarker {
    pub(crate) fn new() -> Self {
        Self { version: 0 }
    }
}

// server -> client

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerProtocolPacket {
    inner: ServerProtocolPacketInner,
    marker: ProtocolMarker,
}

impl ServerProtocolPacket {
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        bincode::deserialize(bytes).ok()
    }

    pub fn encode(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }
}

impl From<ServerProtocolPacket> for ServerProtocolPacketInner {
    fn from(packet: ServerProtocolPacket) -> Self {
        packet.inner
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) enum ServerProtocolPacketInner {
    AckRequest { packet: Vec<u8>, id: AckId },
    Ack { id: AckId },
    ConnectChallenge { challenge: String },
    Welcome {},
}

impl ServerProtocolPacketInner {
    pub(crate) fn into_packet(self) -> ServerProtocolPacket {
        ServerProtocolPacket::from(self)
    }
}

impl From<ServerProtocolPacketInner> for ServerProtocolPacket {
    fn from(inner: ServerProtocolPacketInner) -> Self {
        Self {
            inner,
            marker: ProtocolMarker::new(),
        }
    }
}

// client -> server
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) enum ClientProtocolPacket {
    AckRequest { packet: Vec<u8>, id: AckId },
    Ack { id: AckId },
    Connect { challenge: String },
}

impl ClientProtocolPacket {
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        bincode::deserialize(bytes).ok()
    }

    pub fn encode(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, Hash, PartialEq, Eq)]
pub(crate) struct AckId(u32);

impl AckId {
    fn new(id: u32) -> Self {
        Self(id)
    }
}

#[derive(Debug)]
struct Sent<T> {
    value: T,
    sent_at: instant::Instant,
}

#[derive(Debug, Copy, Clone)]
pub(crate) enum BufferResult {
    Attempted,
    Sent,
    NotSent,
}

#[derive(Debug)]
pub(crate) struct ReliableBuffer<T> {
    pending: Vec<(AckId, T)>,
    sent: HashMap<AckId, Sent<T>>,
    next_ack_id: u32,
}

impl<T> ReliableBuffer<T>
where
    T: Clone,
{
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            sent: HashMap::new(),
            next_ack_id: 0,
        }
    }

    fn next_ack_id(&mut self) -> AckId {
        let id = self.next_ack_id;
        self.next_ack_id += 1;
        AckId::new(id)
    }

    pub fn ack(&mut self, id: &AckId) {
        self.sent.remove(id);
        debug!("{:?} was acked", id);
    }

    pub fn process(&mut self, mut f: impl FnMut(&T, AckId) -> BufferResult) {
        let mut not_sent = Vec::new();
        let now = instant::Instant::now();
        let max_delta = std::time::Duration::from_millis(300);

        for (ack_id, sent) in &self.sent {
            if now - sent.sent_at >= max_delta {
                debug!("sending {:?} again", ack_id);
                self.pending.push((*ack_id, sent.value.clone()));
            }
        }

        let pending = self.pending.drain(..).collect::<Vec<_>>();
        for (ack_id, value) in pending {
            debug!("sending {:?}", ack_id);
            let sent = f(&value, ack_id);
            match sent {
                BufferResult::NotSent => {
                    not_sent.push((ack_id, value));
                }
                BufferResult::Attempted => {
                    // we'll need to verify with the server that this was sent
                    let sent_at = instant::Instant::now();
                    self.sent.insert(ack_id, Sent { value, sent_at });
                }
                BufferResult::Sent => {
                    // no need to verify (e.g. TCP was used)
                }
            }
        }
        self.pending = not_sent;
    }

    pub fn add(&mut self, packet: T) {
        let ack_id = self.next_ack_id();
        self.pending.push((ack_id, packet));
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClientId(u32);

impl ClientId {
    pub(crate) fn new(id: u32) -> Self {
        Self(id)
    }
}
