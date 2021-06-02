// this cfg is temporary
#[cfg(target_arch = "wasm32")]
pub mod client;
pub mod protocol;
#[cfg(not(target_arch = "wasm32"))]
pub mod server;

#[derive(Debug, thiserror::Error)]
pub enum Error {}

pub type Result<T> = std::result::Result<T, Error>;
