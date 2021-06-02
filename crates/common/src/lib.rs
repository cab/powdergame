pub mod app;
pub mod events;
mod gameloop;
pub mod net;
pub mod world;

use serde::{Deserialize, Serialize};
use world::Cell;

// server -> client
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum ServerPacket {
    ConnectChallenge { challenge: String },
    SetCells { cells: Vec<Cell> },
}

impl ServerPacket {
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        bincode::deserialize(bytes).ok()
    }

    pub fn encode(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }
}

// client -> server
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum ClientPacket {
    Connect(),
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
