use std::marker::PhantomData;

use serde::{Deserialize, Serialize};

// used to avoid having the same bytes as a user packet
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProtocolMarker {
    version: String,
}

impl ProtocolMarker {
    pub(crate) fn new() -> Self {
        Self {
            version: "v1".to_string(),
        }
    }
}

// server -> client
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum ServerProtocolPacket {
    ConnectChallenge {
        challenge: String,
        marker: ProtocolMarker,
    },
}

impl ServerProtocolPacket {
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        bincode::deserialize(bytes).ok()
    }

    pub fn encode(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }
}

// client -> server
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum ClientProtocolPacket {
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

#[derive(Debug)]
pub struct ReliableBuffer<T> {
    pending: Vec<T>,
    sent: Vec<Sent<T>>,
}

#[derive(Debug)]
struct Sent<T> {
    value: T,
    sent_at: instant::Instant,
}

// pub(crate) enum BufferResult {
//   Sent
// }

impl<T> ReliableBuffer<T> {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            sent: Vec::new(),
        }
    }

    pub fn process(&mut self, mut f: impl FnMut(&T) -> bool) {
        let mut not_sent = Vec::new();
        for value in self.pending.drain(..) {
            let sent = f(&value);
            if sent {
                let sent_at = instant::Instant::now();
                self.sent.push(Sent { value, sent_at })
            } else {
                not_sent.push(value);
            }
        }
        self.pending = not_sent;
    }

    pub fn add(&mut self, packet: T) {
        self.pending.push(packet);
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClientId(u32);
