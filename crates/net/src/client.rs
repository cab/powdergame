use std::{
    net::SocketAddr,
    sync::{Arc, RwLock},
};

use super::Result;
use crate::protocol::{BufferResult, ClientProtocolPacket, ReliableBuffer, ServerProtocolPacket};
use serde::{de::DeserializeOwned, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, warn};

#[cfg(target_arch = "wasm32")]
mod wasm {

    use std::net::SocketAddr;

    use gloo_events::EventListener;
    use js_sys::Uint8Array;
    use std::sync::Arc;
    use tokio::sync::oneshot;
    use tracing::debug;
    use wasm_bindgen::JsCast;
    use web_sys::{BinaryType, MessageEvent, RtcDataChannel, RtcPeerConnection, WebSocket};

    #[derive(Debug)]
    pub(super) struct ReliableTransport {
        websocket: Option<WebSocket>,
        on_message: Option<EventListener>,
        on_open: Option<EventListener>,
        on_error: Option<EventListener>,
        on_close: Option<EventListener>,
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
                on_open: None,
                on_close: None,
                on_error: None,
            }
        }

        pub fn incoming(&self) -> impl Iterator<Item = Vec<u8>> + '_ {
            self.incoming_rx.try_iter()
        }

        pub fn process(&mut self) {}

        pub fn send(&mut self, data: &[u8]) -> bool {
            if let Some(websocket) = self.websocket.as_ref() {
                websocket.send_with_u8_array(data).unwrap();
                true
            } else {
                false
            }
        }

        pub async fn connect(&mut self, addr: SocketAddr) {
            let websocket = WebSocket::new(&format!("ws://{}/connect", addr)).unwrap();
            websocket.set_binary_type(BinaryType::Arraybuffer);
            let (ready_tx, ready_rx) = oneshot::channel::<()>();
            let on_open = EventListener::once(&websocket, "open", {
                move |e| {
                    debug!("websocket connected");
                    ready_tx.send(()).unwrap();
                }
            });
            let on_close = EventListener::new(&websocket, "close", {
                move |e| {
                    debug!("websocket closed");
                }
            });
            let on_error = EventListener::new(&websocket, "error", {
                move |e| {
                    debug!("websocket error");
                }
            });
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
            self.on_open = Some(on_open);
            self.on_error = Some(on_error);
            self.on_close = Some(on_close);
            ready_rx.await;
        }
    }

    #[derive(Debug)]
    pub(super) struct UnreliableTransport {
        peer: Arc<RtcPeerConnection>,
        channel: RtcDataChannel,
        on_error: EventListener,
        on_open: EventListener,
        on_message: EventListener,
        on_ice_candidate: EventListener,
        on_ice_connection_state_change: EventListener,
        ready_rx: Option<oneshot::Receiver<()>>,
        incoming_tx: crossbeam_channel::Sender<Vec<u8>>,
        incoming_rx: crossbeam_channel::Receiver<Vec<u8>>,
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
    OutgoingPacket: std::fmt::Debug + Serialize + Send + Sync + 'static,
    IncomingPacket: std::fmt::Debug + DeserializeOwned + Send + Sync + 'static,
{
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(ClientInner::new())),
        }
    }

    pub async fn connect(&self, addr: SocketAddr) -> Result<()> {
        if let Ok(mut inner) = self.inner.write() {
            inner.connect(addr).await
        } else {
            warn!("TODO");
            panic!();
        }
    }

    pub fn send_reliable(&self, packet: OutgoingPacket) {
        if let Ok(mut inner) = self.inner.try_write() {
            inner.send_reliable_user(packet);
        } else {
            warn!("TODO");
            panic!();
        }
    }

    pub fn process(&self) {
        if let Ok(mut inner) = self.inner.try_write() {
            inner.process();
        }
    }

    pub async fn recv(&self) -> impl Iterator<Item = IncomingPacket> + '_ {
        let inner = self.inner.read().unwrap();
        inner.recv().await.collect::<Vec<_>>().into_iter()
    }
}

#[derive(Debug)]
enum ProtocolOrUser<T> {
    Protocol(ClientProtocolPacket),
    User(T),
}

impl<T> ProtocolOrUser<T>
where
    T: Serialize,
{
    fn encode(&self) -> Vec<u8> {
        match self {
            ProtocolOrUser::Protocol(packet) => packet.encode(),
            ProtocolOrUser::User(packet) => bincode::serialize(packet).unwrap(),
        }
    }
}

#[derive(Debug)]
struct ClientInner<OutgoingPacket, IncomingPacket> {
    reliable_buffer: ReliableBuffer<ProtocolOrUser<OutgoingPacket>>,
    reliable_transport: ReliableTransport,
    incoming_tx: crossbeam_channel::Sender<IncomingPacket>,
    incoming_rx: crossbeam_channel::Receiver<IncomingPacket>,
}

impl<OutgoingPacket, IncomingPacket> ClientInner<OutgoingPacket, IncomingPacket>
where
    OutgoingPacket: std::fmt::Debug + Serialize + Send + Sync + 'static,
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
        self.reliable_transport.connect(addr).await;
        Ok(())
    }

    fn process(&mut self) {
        let transport = &mut self.reliable_transport;
        self.reliable_buffer.process(move |packet| {
            debug!("processing reliable buffer: {:?}", packet);
            if transport.send(&packet.encode()) {
                BufferResult::Sent
            } else {
                BufferResult::NotSent
            }
        });

        self.reliable_transport.process();
        use bincode::Options;
        let bincoder = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .reject_trailing_bytes();
        let packets = self
            .reliable_transport
            .incoming()
            .into_iter()
            .collect::<Vec<_>>();
        for packet in packets {
            if let Some(packet) = bincoder.deserialize::<IncomingPacket>(&packet).ok() {
                debug!("got this: {:?}", packet);
            } else if let Some(packet) = bincoder.deserialize::<ServerProtocolPacket>(&packet).ok()
            {
                debug!("got server protocol packet: {:?}", packet);
                match packet {
                    ServerProtocolPacket::ConnectChallenge { challenge, .. } => {
                        self.send_reliable_protocol(ClientProtocolPacket::Connect { challenge })
                    }
                }
            }
        }
    }

    fn send_user(&self, packet: OutgoingPacket) {}

    fn send_reliable_protocol(&mut self, packet: ClientProtocolPacket) {
        self.reliable_buffer.add(ProtocolOrUser::Protocol(packet));
    }

    fn send_reliable_user(&mut self, packet: OutgoingPacket) {
        self.reliable_buffer.add(ProtocolOrUser::User(packet));
    }

    async fn recv(&self) -> impl Iterator<Item = IncomingPacket> + '_ {
        self.incoming_rx.try_iter()
    }
}
