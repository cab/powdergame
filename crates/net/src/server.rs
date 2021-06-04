use std::{
    collections::HashMap, convert::Infallible, marker::PhantomData, net::SocketAddr, sync::Arc,
};

use futures::{FutureExt, SinkExt, StreamExt};
use serde::{de::DeserializeOwned, Serialize};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};
use warp::{
    ws::{Message, WebSocket},
    Filter,
};
use webrtc_unreliable::{Server as RtcServer, SessionEndpoint};

use crate::protocol::{
    ClientId, ClientProtocolPacket, ProtocolMarker, ReliableBuffer, ServerProtocolPacket,
};

struct ReliableTransport {
    inner: Inner,
}

type Inner = Arc<RwLock<ReliableTransportInner>>;

#[derive(Debug)]
enum ReliableEvent {
    NewClient { id: ClientId, challenge: String },
}

impl ReliableTransport {
    pub fn new(listen_addr: SocketAddr) -> Self {
        Self {
            inner: Arc::new(RwLock::new(ReliableTransportInner::new(listen_addr))),
        }
    }

    async fn set_session_endpoint(&mut self, endpoint: SessionEndpoint) {
        let mut inner = self.inner.write().await;
        inner.set_session_endpoint(endpoint);
    }

    async fn incoming(&self) -> crossbeam_channel::Receiver<(ClientId, Vec<u8>)> {
        self.inner.read().await.incoming_rx.clone()
    }

    async fn outgoing(&self) -> crossbeam_channel::Sender<(ClientId, Vec<u8>)> {
        self.inner.read().await.outgoing_tx.clone()
    }

    async fn events(&self) -> crossbeam_channel::Receiver<ReliableEvent> {
        self.inner.read().await.events_rx.clone()
    }

    pub async fn listen(&mut self) {
        async fn rtc_callback<S, B>(
            req: S,
            inner: Inner,
        ) -> Result<warp::reply::Response, warp::Rejection>
        where
            B: bytes::Buf,
            S: futures::Stream<Item = Result<B, warp::Error>>,
        {
            use futures::TryStreamExt;
            use warp::Reply;

            let mut inner = inner.write().await;

            if let Some(mut endpoint) = inner.session_endpoint.as_mut() {
                match endpoint
                    .http_session_request(req.map_ok(|mut buf| buf.copy_to_bytes(buf.remaining())))
                    .await
                {
                    Ok(resp) => Ok(warp::reply::with_header(
                        resp,
                        warp::hyper::header::ACCESS_CONTROL_ALLOW_ORIGIN,
                        "*",
                    )
                    .into_response()),
                    Err(_) => Err(warp::reject::custom(NotReady)), // TODO
                }
            } else {
                Err(warp::reject::custom(NotReady))
            }
        }

        let inner = self.inner.clone();
        let inner = warp::any().map(move || inner.clone());

        let connect = warp::path("connect")
            .and(warp::ws())
            .and(inner.clone())
            .map(|ws: warp::ws::Ws, inner| {
                ws.on_upgrade(move |socket| client_connected(socket, inner))
            });

        let rtc = warp::post()
            .and(warp::path("rtc"))
            .and(warp::body::stream())
            .and(inner)
            .and_then(rtc_callback);
        // .and_then(move |body, inner: Inner| async move {
        //     let inner = inner.write().await;

        //     if let Some(endpoint) = inner.session_endpoint.as_ref() {
        //         let req = endpoint.http_session_request(body.map_ok(|mut buf| buf.to_bytes()));
        //         Ok("hi".to_string())
        //     } else {
        //         Err(warp::reject::custom(NotReady))
        //     }
        // });

        let routes = connect.or(rtc);

        tokio::spawn(async move { loop {} });

        let http_listen_addr = self.inner.read().await.listen_addr;
        debug!("listening for websockets on {:?}", http_listen_addr);
        warp::serve(routes).run(http_listen_addr).await;
    }
}

#[derive(Debug)]
struct NotReady;

impl warp::reject::Reject for NotReady {}

async fn client_connected(ws: WebSocket, inner: Inner) {
    let (mut user_ws_tx, mut user_ws_rx) = ws.split();

    let (tx, mut rx) = mpsc::unbounded_channel();

    let client_id = inner.write().await.register_client(tx.clone());
    debug!("client connected: {:?}", client_id);
    let challenge = "challenge_1".to_string();
    inner
        .read()
        .await
        .events_tx
        .send(ReliableEvent::NewClient {
            id: client_id,
            challenge: challenge.clone(),
        })
        .unwrap();

    tokio::task::spawn(async move {
        for message in rx.recv().await {
            user_ws_tx.send(Message::binary(message)).await.unwrap();
        }
    });
    tx.send(
        ServerProtocolPacket::ConnectChallenge {
            challenge,
            marker: ProtocolMarker::new(),
        }
        .encode(),
    )
    .unwrap();

    while let Some(result) = user_ws_rx.next().await {
        let packet = match result {
            Ok(msg) => msg.into_bytes(),
            Err(e) => {
                warn!("websocket error: {}", e);
                break;
            }
        };
        inner
            .read()
            .await
            .incoming_tx
            .send((client_id, packet))
            .unwrap();
    }

    debug!("client disconnected");
}

struct ReliableTransportInner {
    listen_addr: SocketAddr,
    next_client_id: u32,
    session_endpoint: Option<SessionEndpoint>,
    connections: HashMap<ClientId, mpsc::UnboundedSender<Vec<u8>>>,
    incoming_tx: crossbeam_channel::Sender<(ClientId, Vec<u8>)>,
    incoming_rx: crossbeam_channel::Receiver<(ClientId, Vec<u8>)>,
    outgoing_tx: crossbeam_channel::Sender<(ClientId, Vec<u8>)>,
    outgoing_rx: crossbeam_channel::Receiver<(ClientId, Vec<u8>)>,
    events_tx: crossbeam_channel::Sender<ReliableEvent>,
    events_rx: crossbeam_channel::Receiver<ReliableEvent>,
}

impl ReliableTransportInner {
    fn new(listen_addr: SocketAddr) -> Self {
        let (incoming_tx, incoming_rx) = crossbeam_channel::unbounded();
        let (outgoing_tx, outgoing_rx) = crossbeam_channel::unbounded();
        let (events_tx, events_rx) = crossbeam_channel::unbounded();
        Self {
            next_client_id: 1,
            session_endpoint: None,
            connections: HashMap::new(),
            listen_addr,
            incoming_rx,
            incoming_tx,
            events_rx,
            events_tx,
            outgoing_rx,
            outgoing_tx,
        }
    }

    fn set_session_endpoint(&mut self, endpoint: SessionEndpoint) {
        self.session_endpoint = Some(endpoint);
    }

    pub fn register_client(&mut self, tx: mpsc::UnboundedSender<Vec<u8>>) -> ClientId {
        let id = self.next_client_id();
        self.connections.insert(id, tx);
        id
    }

    fn next_client_id(&mut self) -> ClientId {
        let id = self.next_client_id;
        self.next_client_id += 1;
        ClientId::new(id)
    }
}

struct UnreliableTransport {
    rtc: RtcServer,
    incoming_tx: crossbeam_channel::Sender<(SocketAddr, Vec<u8>)>,
    incoming_rx: crossbeam_channel::Receiver<(SocketAddr, Vec<u8>)>,
}

impl UnreliableTransport {
    async fn new(listen_addr: SocketAddr, public_addr: SocketAddr) -> Self {
        let mut rtc = RtcServer::new(listen_addr, public_addr).await.unwrap();
        let (incoming_tx, incoming_rx) = crossbeam_channel::unbounded();
        Self {
            rtc,
            incoming_rx,
            incoming_tx,
        }
    }

    pub fn session_endpoint(&self) -> SessionEndpoint {
        self.rtc.session_endpoint()
    }

    pub fn incoming(&self) -> crossbeam_channel::Receiver<(SocketAddr, Vec<u8>)> {
        self.incoming_rx.clone()
    }

    async fn listen(&mut self) {
        loop {
            if let Ok(recv) = self.rtc.recv().await {
                let bytes = recv.message.as_ref().to_vec();
                let addr = recv.remote_addr;
                self.incoming_tx.send((addr, bytes)).unwrap();
            }
        }
    }
}

pub struct ServerConfig {
    pub http_listen_addr: SocketAddr,
    pub webrtc_listen_addr: SocketAddr,
    pub webrtc_public_addr: SocketAddr,
}

pub struct Server<OutgoingPacket, IncomingPacket> {
    config: ServerConfig,
    reliable_buffers: HashMap<ClientId, ReliableBuffer<OutgoingPacket>>,
    incoming_packet_type: PhantomData<IncomingPacket>,
    reliable_transport: Option<ReliableTransport>,
    unreliable_transport: Option<UnreliableTransport>,
}

impl<OutgoingPacket, IncomingPacket> Server<OutgoingPacket, IncomingPacket> {
    pub async fn new(config: ServerConfig) -> Self {
        let reliable_transport = ReliableTransport::new(config.http_listen_addr.clone());
        let unreliable_transport = UnreliableTransport::new(
            config.webrtc_listen_addr.clone(),
            config.webrtc_public_addr.clone(),
        )
        .await;
        Self {
            config,
            reliable_buffers: HashMap::new(),
            incoming_packet_type: PhantomData,
            reliable_transport: Some(reliable_transport),
            unreliable_transport: Some(unreliable_transport),
        }
    }

    pub async fn listen(&mut self) {
        let mut unreliable_transport = self.unreliable_transport.take().unwrap();
        let mut transport = self.reliable_transport.take().unwrap();
        transport
            .set_session_endpoint(unreliable_transport.session_endpoint())
            .await;
        let reliable_rx = transport.incoming().await;
        let reliable_tx = transport.outgoing().await;
        let reliable_events_rx = transport.events().await;
        let reliable = tokio::spawn(async move {
            transport.listen().await;
        });
        let unreliable_rx = unreliable_transport.incoming();
        let unreliable = tokio::spawn(async move {
            unreliable_transport.listen().await;
        });
        let process = tokio::spawn(async move {
            let mut processor = Processor::new();
            use bincode::Options;
            let bincoder = bincode::DefaultOptions::new()
                .with_fixint_encoding()
                .reject_trailing_bytes();
            loop {
                for event in reliable_events_rx.try_iter() {
                    debug!("got reliable event {:?}", event);
                    match event {
                        ReliableEvent::NewClient { id, challenge } => {
                            processor.register_reliable_client(id, challenge);
                        }
                    }
                }
                for (client_id, packet) in reliable_rx.try_iter() {
                    debug!("got reliable data {:?}", client_id);
                }
                for (addr, packet) in unreliable_rx.try_iter() {
                    if let Some(client_id) = processor.client_id(&addr) {
                    } else if let Some(ClientProtocolPacket::Connect { challenge }) =
                        bincoder.deserialize::<ClientProtocolPacket>(&packet).ok()
                    {
                        debug!(
                            ?challenge,
                            ?addr,
                            "got unreliable transport client connect packet",
                        );
                        if let Some(client_id) =
                            processor.register_unreliable_client(&challenge, addr)
                        {
                            debug!(
                                ?client_id,
                                "associated unreliable connection to reliable connection"
                            );
                        } else {
                            // TODO
                        }
                    }
                }
            }
        });
        tokio::select! {
            _ = reliable => {
                info!("reliable transport stopped");
            }
            _ = unreliable => {
                info!("unreliable transport stopped");
            }
            _ = process => {
                info!("processing stopped");
            }
        }
    }
}

#[derive(Debug)]
struct Processor {
    challenge_to_client: HashMap<String, ClientId>,
    addr_to_client: HashMap<SocketAddr, ClientId>,
}

impl Processor {
    fn new() -> Self {
        Self {
            challenge_to_client: HashMap::new(),
            addr_to_client: HashMap::new(),
        }
    }

    fn client_id(&self, addr: &SocketAddr) -> Option<ClientId> {
        self.addr_to_client.get(addr).copied()
    }

    fn register_unreliable_client(
        &mut self,
        challenge: &str,
        addr: SocketAddr,
    ) -> Option<ClientId> {
        let client_id = self.challenge_to_client.get(challenge)?;
        self.addr_to_client.insert(addr, *client_id);
        Some(*client_id)
    }

    fn register_reliable_client(&mut self, client_id: ClientId, challenge: String) {
        self.challenge_to_client.insert(challenge, client_id);
    }
}
