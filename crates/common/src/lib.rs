use serde::{Deserialize, Serialize};

// server -> client
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum ServerPacket {}

impl ServerPacket {
    pub fn encode(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }
}

// client -> server
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum ClientPacket {
    SetName { name: String },
}

impl ClientPacket {
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        bincode::deserialize(bytes).ok()
    }

    pub fn encode(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }
}
