// this cfg is temporary
#[cfg(target_arch = "wasm32")]
pub mod client;
pub mod protocol;
#[cfg(not(target_arch = "wasm32"))]
pub mod server;
