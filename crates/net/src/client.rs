use std::{
    net::SocketAddr,
    sync::{Arc, RwLock},
};

use super::Result;
use crate::protocol::{ReliableBuffer, ServerProtocolPacket};
use serde::de::DeserializeOwned;
use tokio::sync::mpsc;
use tracing::debug;

#[cfg(target_arch = "wasm32")]
mod wasm {

    use std::net::SocketAddr;

    use gloo_events::EventListener;
    use js_sys::Uint8Array;
    use tracing::debug;
    use wasm_bindgen::JsCast;
    use web_sys::{BinaryType, MessageEvent, WebSocket};

    #[derive(Debug)]
    pub(super) struct ReliableTransport {
        websocket: Option<WebSocket>,
        on_message: Option<EventListener>,
        incoming_tx: crossbeam_channel::Sender<Vec<u8>>,
        incoming_rx: crossbeam_channel::Receiver<Vec<u8>>,
    }

    impl ReliableTransport {
        pub(super) fn new() -> Self {
            let (incoming_tx, incoming_rx) = crossbeam_channel::unbounded();
            Self {
                incoming_rx,
                incoming_tx,
                websocket: None,
                on_message: None,
            }
        }

        pub fn incoming(&self) -> impl Iterator<Item = Vec<u8>> + '_ {
            self.incoming_rx.try_iter()
        }

        pub fn process(&mut self) {}

        pub fn connect(&mut self, addr: SocketAddr) {
            let websocket = WebSocket::new(&format!("ws://{}/connect", addr)).unwrap();
            websocket.set_binary_type(BinaryType::Arraybuffer);
            let on_message = EventListener::new(&websocket, "message", {
                let incoming_tx = self.incoming_tx.clone();
                move |event| {
                    let event = event.unchecked_ref::<MessageEvent>();
                    let data = Uint8Array::new(&event.data()).to_vec();
                    debug!("got message");
                    incoming_tx.send(data);
                }
            });
            self.websocket = Some(websocket);
            self.on_message = Some(on_message);
        }
    }
}

#[cfg(target_arch = "wasm32")]
use wasm::*;

type Inner<OutgoingPacket, IncomingPacket> =
    Arc<RwLock<ClientInner<OutgoingPacket, IncomingPacket>>>;

#[derive(Debug, Clone)]
pub struct Client<OutgoingPacket, IncomingPacket> {
    inner: Inner<OutgoingPacket, IncomingPacket>,
}

impl<OutgoingPacket, IncomingPacket> Client<OutgoingPacket, IncomingPacket>
where
    OutgoingPacket: Send + Sync + 'static,
    IncomingPacket: std::fmt::Debug + DeserializeOwned + Send + Sync + 'static,
{
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(ClientInner::new())),
        }
    }

    pub async fn connect(&self, addr: SocketAddr) -> Result<()> {
        let mut inner = self.inner.write().unwrap();
        inner.connect(addr).await
    }

    pub fn send_reliable(&self, packet: OutgoingPacket) {
        let mut inner = self.inner.write().unwrap();
        inner.send_reliable(packet);
    }

    pub fn process(&self) {
        let mut inner = self.inner.write().unwrap();
        inner.process();
    }

    pub async fn recv(&self) -> impl Iterator<Item = IncomingPacket> + '_ {
        let inner = self.inner.read().unwrap();
        inner.recv().await.collect::<Vec<_>>().into_iter()
    }
}

#[derive(Debug)]
struct ClientInner<OutgoingPacket, IncomingPacket> {
    reliable_buffer: ReliableBuffer<OutgoingPacket>,
    reliable_transport: ReliableTransport,
    incoming_tx: crossbeam_channel::Sender<IncomingPacket>,
    incoming_rx: crossbeam_channel::Receiver<IncomingPacket>,
}

impl<OutgoingPacket, IncomingPacket> ClientInner<OutgoingPacket, IncomingPacket>
where
    OutgoingPacket: Send + Sync + 'static,
    IncomingPacket: std::fmt::Debug + DeserializeOwned + Send + Sync + 'static,
{
    pub fn new() -> Self {
        let (incoming_tx, incoming_rx) = crossbeam_channel::unbounded();

        Self {
            reliable_transport: ReliableTransport::new(),
            reliable_buffer: ReliableBuffer::new(),
            incoming_rx,
            incoming_tx,
        }
    }

    pub async fn connect(&mut self, addr: SocketAddr) -> Result<()> {
        self.reliable_transport.connect(addr);
        Ok(())
    }

    fn process(&mut self) {
        self.reliable_buffer.process(|packet| {
            ();
            false
        });

        self.reliable_transport.process();
        use bincode::Options;
        let bincoder = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .reject_trailing_bytes();
        for packet in self.reliable_transport.incoming() {
            debug!("checking packet {:?}", packet);

            if let Some(packet) = bincoder.deserialize::<IncomingPacket>(&packet).ok() {
                debug!("got this: {:?}", packet);
            } else if let Some(packet) = bincoder.deserialize::<ServerProtocolPacket>(&packet).ok()
            {
                debug!("got server protocol packet: {:?}", packet);
            }
        }
    }

    fn send(&self, packet: OutgoingPacket) {}

    fn send_reliable(&mut self, packet: OutgoingPacket) {
        self.reliable_buffer.add(packet);
    }

    async fn recv(&self) -> impl Iterator<Item = IncomingPacket> + '_ {
        self.incoming_rx.try_iter()
    }
}
