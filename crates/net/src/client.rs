use std::{
    net::SocketAddr,
    sync::{Arc, RwLock},
};

use serde::{de::DeserializeOwned, Serialize};
use tracing::{debug, warn};

use crate::protocol::{
    BufferResult, ClientProtocolPacket, ReliableBuffer, ServerProtocolPacket,
    ServerProtocolPacketInner,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Http(#[from] reqwest::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(target_arch = "wasm32")]
mod wasm {

    use std::{
        cell::{Cell, RefCell},
        net::SocketAddr,
        sync::Arc,
        time::Duration,
    };

    use crossbeam_channel::{Receiver, Sender};
    use gloo_events::EventListener;
    use gloo_timers::future::TimeoutFuture;
    use js_sys::Uint8Array;
    use serde::{Deserialize, Serialize};
    use tokio::sync::{mpsc, oneshot};
    use tracing::{debug, trace, warn};
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{
        BinaryType, MessageEvent, RtcConfiguration, RtcDataChannel, RtcDataChannelInit,
        RtcDataChannelType, RtcIceCandidate, RtcIceCandidateInit, RtcPeerConnection, RtcSdpType,
        RtcSessionDescription, RtcSessionDescriptionInit, WebSocket,
    };

    use super::{Error, Result};

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

    macro_rules! js_object {
	($($key:expr, $value:expr),+) => {
		{

			let o = js_sys::Object::new();
			$(
				{
					let k = JsValue::from_str($key);
					let v = JsValue::from($value);
          unsafe {
            js_sys::Reflect::set(&o, &k, &v).unwrap();
          }
				}
			)*
			o
		}
	};
}

    #[derive(Debug)]
    pub(super) struct UnreliableTransport {
        peer: Arc<RtcPeerConnection>,
        channel: RtcDataChannel,
        on_error: EventListener,
        http_client: reqwest::Client,
        on_open: EventListener,
        on_message: EventListener,
        on_ice_candidate: EventListener,
        on_ice_connection_state_change: EventListener,
        ready_rx: Option<oneshot::Receiver<()>>,
        incoming_tx: crossbeam_channel::Sender<Vec<u8>>,
        incoming_rx: crossbeam_channel::Receiver<Vec<u8>>,
    }

    impl UnreliableTransport {
        pub fn new() -> Self {
            let peer_configuration = {
                let mut config = RtcConfiguration::new();
                let urls = JsValue::from_serde(&["stun:stun.l.google.com:19302"]).unwrap();
                let server = js_object!("urls", urls);
                let ice_servers = js_sys::Array::new();
                ice_servers.push(&server);
                config.ice_servers(&ice_servers);
                config
            };
            let peer =
                Arc::new(RtcPeerConnection::new_with_configuration(&peer_configuration).unwrap());
            let on_ice_connection_state_change =
                EventListener::new(&peer, "iceconnectionstatechange", {
                    let peer = peer.clone();
                    move |e| {
                        trace!("ice state change: {:?}", peer.ice_connection_state());
                    }
                });
            let (ready_tx, ready_rx) = oneshot::channel::<()>();
            let mut channel_init = RtcDataChannelInit::new();
            channel_init.ordered(false);
            channel_init.max_retransmits(0);
            let channel = peer.create_data_channel_with_data_channel_dict("data", &channel_init);
            channel.set_binary_type(RtcDataChannelType::Arraybuffer);
            let http_client = reqwest::Client::new();
            let on_error = EventListener::new(&channel, "error", move |e| {
                warn!("channel error {:?}", e);
            });
            let on_open = EventListener::once(&channel, "open", {
                move |e| {
                    trace!("data channel opened");
                    ready_tx.send(());
                }
            });
            let on_message = EventListener::new(&channel, "message", {
                move |e| {
                    trace!("got message");
                }
            });
            let on_ice_candidate = EventListener::new(&peer, "icecandidate", move |e| {
                trace!("ice candidate event");
            });

            let (incoming_tx, incoming_rx) = crossbeam_channel::unbounded();
            Self {
                ready_rx: Some(ready_rx),
                peer,
                channel,
                http_client,
                on_error,
                on_open,
                on_ice_candidate,
                on_message,
                on_ice_connection_state_change,
                incoming_tx,
                incoming_rx,
            }
        }

        pub fn send(&self, data: &[u8]) {
            self.channel.send_with_u8_array(data).unwrap();
        }

        pub async fn connect(&mut self, addr: SocketAddr) -> Result<()> {
            debug!("creating peer offer");
            let offer = JsFuture::from(self.peer.create_offer()).await.unwrap();
            JsFuture::from(self.peer.set_local_description(&offer.unchecked_into()))
                .await
                .unwrap();
            let res = self
                .http_client
                .post(format!("http://{}/rtc", addr))
                .body(self.peer.local_description().unwrap().sdp())
                .send()
                .await?
                .json::<SessionResponse>()
                .await?;
            let description = {
                let mut init = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
                init.sdp(res.answer.get("sdp").unwrap().as_str().unwrap());
                init
            };
            let candidate = {
                let mut init = RtcIceCandidateInit::new(
                    res.candidate.get("candidate").unwrap().as_str().unwrap(),
                );
                init.sdp_m_line_index(
                    res.candidate
                        .get("sdpMLineIndex")
                        .unwrap()
                        .as_u64()
                        .map(|v| v as u16),
                );
                init.sdp_mid(res.candidate.get("sdpMid").unwrap().as_str());
                RtcIceCandidate::new(&init).unwrap()
            };
            JsFuture::from(self.peer.set_remote_description(&description))
                .await
                .unwrap();

            JsFuture::from(
                self.peer
                    .add_ice_candidate_with_opt_rtc_ice_candidate(Some(&candidate)),
            )
            .await
            .unwrap();
            self.ready_rx.take().unwrap().await;

            Ok(())
        }
    }

    #[derive(Debug, Deserialize, Serialize)]
    struct SessionResponse {
        answer: serde_json::Value,
        candidate: serde_json::Value,
    }
}

#[cfg(target_arch = "wasm32")]
use wasm::*;

#[cfg(not(target_arch = "wasm32"))]
mod native {

    use std::net::SocketAddr;

    use super::Result;

    #[derive(Debug)]
    pub(super) struct UnreliableTransport {}

    impl UnreliableTransport {
        pub fn new() -> Self {
            unimplemented!()
        }
        pub fn send(&self, _data: &[u8]) {
            unimplemented!()
        }
        pub async fn connect(&mut self, _addr: SocketAddr) -> Result<()> {
            unimplemented!()
        }
    }

    #[derive(Debug)]
    pub(super) struct ReliableTransport {}

    impl ReliableTransport {
        pub(super) fn new() -> Self {
            unimplemented!()
        }
        pub fn incoming(&self) -> impl Iterator<Item = Vec<u8>> + '_ {
            unimplemented!();
            vec![].into_iter()
        }
        pub fn process(&mut self) {
            unimplemented!()
        }
        pub fn send(&mut self, _data: &[u8]) -> bool {
            unimplemented!()
        }
        pub async fn connect(&mut self, _addr: SocketAddr) {
            unimplemented!()
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
use native::*;

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
    unreliable_transport: UnreliableTransport,
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
            unreliable_transport: UnreliableTransport::new(),
            reliable_buffer: ReliableBuffer::new(),
            incoming_rx,
            incoming_tx,
        }
    }

    pub async fn connect(&mut self, addr: SocketAddr) -> Result<()> {
        self.reliable_transport.connect(addr).await;
        self.unreliable_transport.connect(addr).await;
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
            if let Ok(packet) = bincoder.deserialize::<IncomingPacket>(&packet) {
                debug!("got this: {:?}", packet);
            } else if let Ok(packet) = bincoder.deserialize::<ServerProtocolPacket>(&packet) {
                debug!("got server protocol packet: {:?}", packet);
                let packet = packet.into();
                match packet {
                    ServerProtocolPacketInner::ConnectChallenge { challenge } => {
                        self.send_unreliable_protocol(ClientProtocolPacket::Connect { challenge })
                    }
                    ServerProtocolPacketInner::Welcome {} => {
                        debug!("welcomed");
                    }
                }
            }
        }
    }

    fn send_user(&self, _packet: OutgoingPacket) {}

    fn send_unreliable_protocol(&mut self, packet: ClientProtocolPacket) {
        self.unreliable_transport.send(&packet.encode());
    }

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
