use futures::{FutureExt, StreamExt};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use warp::ws::{Message, WebSocket};
use warp::Filter;
use tracing::debug;
use futures::SinkExt;

use crate::protocol::ClientId;
use crate::protocol::ReliableBuffer;
use crate::protocol::ServerProtocolPacket;
use crate::protocol::ProtocolMarker;

pub struct Server<OutgoingPacket, IncomingPacket> {
    inner: Inner<OutgoingPacket, IncomingPacket>,
}

type Inner<OutgoingPacket, IncomingPacket> =
    Arc<RwLock<ServerInner<OutgoingPacket, IncomingPacket>>>;

impl<OutgoingPacket, IncomingPacket> Server<OutgoingPacket, IncomingPacket>
where
    OutgoingPacket: Send + Sync + 'static,
    IncomingPacket: Send + Sync + 'static,
{
    pub fn new(config: ServerConfig) -> Self {
        Self {
            inner: Arc::new(RwLock::new(ServerInner::new(config))),
        }
    }

    pub async fn listen(&mut self) {
        let inner = self.inner.clone();
        let inner = warp::any().map(move || inner.clone());

        let connect =
            warp::path("connect")
                .and(warp::ws())
                .and(inner)
                .map(|ws: warp::ws::Ws, inner| {
                    ws.on_upgrade(move |socket| client_connected(socket, inner))
                });
        let routes = connect;
        let http_listen_addr = self.inner.read().await.config.http_listen_addr;
        debug!("listening for websockets on {:?}", http_listen_addr);
        warp::serve(routes)
            .run(http_listen_addr)
            .await;
    }
}

async fn client_connected<OutgoingPacket, IncomingPacket>(
    ws: WebSocket,
    inner: Inner<OutgoingPacket, IncomingPacket>,
) {
    debug!("client connected");
    let (mut user_ws_tx, mut user_ws_rx) = ws.split();

    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::task::spawn(async move {
        for message in rx.recv().await {
            user_ws_tx.send(message).await.unwrap();
        }
    });
    tx.send(Message::binary(ServerProtocolPacket::ConnectChallenge {
        challenge: "challenge_1".to_string(),
        marker: ProtocolMarker::new(),
    }.encode()));
}

struct ServerInner<OutgoingPacket, IncomingPacket> {
    config: ServerConfig,
    reliable_buffers: HashMap<ClientId, ReliableBuffer<OutgoingPacket>>,
    incoming_packet_type: PhantomData<IncomingPacket>,
}

impl<OutgoingPacket, IncomingPacket> ServerInner<OutgoingPacket, IncomingPacket> {
    fn new(config: ServerConfig) -> Self {
        Self {
            config,
            reliable_buffers: HashMap::new(),
            incoming_packet_type: PhantomData,
        }
    }
}

pub struct ServerConfig {
    pub http_listen_addr: SocketAddr,
    pub webrtc_listen_addr: SocketAddr,
    pub webrtc_public_addr: SocketAddr,
}
