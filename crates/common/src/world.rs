use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub struct Tick(pub u32);

impl Tick {
    pub fn zero() -> Self {
        Self(0)
    }

    pub fn increment_self(&mut self) {
        self.0 = self.0.wrapping_add(1);
    }
}

#[derive(Debug, Copy, Clone, Deserialize, Serialize)]
pub enum Cell {
    Empty,
    Stone,
}
