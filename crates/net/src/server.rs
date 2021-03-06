use std::{collections::HashMap, marker::PhantomData, net::SocketAddr, sync::Arc};

use futures::{FutureExt, SinkExt, StreamExt};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, trace, warn};
use uuid::Uuid;
use warp::{
    ws::{Message, WebSocket},
    Filter,
};
use webrtc_unreliable::{Server as RtcServer, SessionEndpoint};

use crate::protocol::{
    ClientId, ClientProtocolPacket, ReliableBuffer, ServerProtocolPacket, ServerProtocolPacketInner,
};

struct ReliableTransport {
    inner: Inner,
    outgoing_tx: mpsc::Sender<(ClientId, Vec<u8>)>,
    outgoing_rx: Option<mpsc::Receiver<(ClientId, Vec<u8>)>>,
}

type Inner = Arc<RwLock<ReliableTransportInner>>;

#[derive(Debug)]
enum ReliableEvent {
    NewClient { id: ClientId, challenge: String },
    ClientDisconnected { id: ClientId },
}

impl ReliableTransport {
    pub fn new(listen_addr: SocketAddr, events_tx: mpsc::Sender<ReliableEvent>) -> Self {
        let (outgoing_tx, outgoing_rx) = mpsc::channel(32);

        Self {
            inner: Arc::new(RwLock::new(ReliableTransportInner::new(
                listen_addr,
                events_tx,
            ))),
            outgoing_rx: Some(outgoing_rx),
            outgoing_tx,
        }
    }

    async fn set_session_endpoint(&mut self, endpoint: SessionEndpoint) {
        let mut inner = self.inner.write().await;
        inner.set_session_endpoint(endpoint);
    }

    async fn incoming(&self) -> crossbeam_channel::Receiver<(ClientId, Vec<u8>)> {
        self.inner.read().await.incoming_rx.clone()
    }

    async fn outgoing(&self) -> mpsc::Sender<(ClientId, Vec<u8>)> {
        self.outgoing_tx.clone()
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

            if let Some(endpoint) = inner.session_endpoint.as_mut() {
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

        let mut outgoing = self.outgoing_rx.take().unwrap();
        let inner = self.inner.clone();
        let outgoing_sender = tokio::spawn(async move {
            while let Some((client_id, message)) = outgoing.recv().await {
                debug!("sending to {:?}", client_id);
                inner.write().await.send(&client_id, message);
            }
        });

        let http_listen_addr = self.inner.read().await.listen_addr;
        debug!("listening for websockets on {:?}", http_listen_addr);
        let http = warp::serve(routes).run(http_listen_addr);

        tokio::select! {
            _ = outgoing_sender => {
                debug!("outgoing stopped");
            }
            _ = http => {
                debug!("http stopped");
            }
        };
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
    let challenge = format!("{}", Uuid::new_v4());
    inner
        .read()
        .await
        .events_tx
        .send(ReliableEvent::NewClient {
            id: client_id,
            challenge: challenge.clone(),
        })
        .await
        .unwrap();

    let sender = tokio::task::spawn(async move {
        while let Some(message) = rx.recv().await {
            debug!(?client_id, "sending");
            user_ws_tx.send(Message::binary(message)).await.unwrap();
        }
        debug!("ws send loop done");
    });

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

    sender.abort();

    sender.await;

    inner.write().await.unregister(&client_id);

    inner
        .read()
        .await
        .events_tx
        .send(ReliableEvent::ClientDisconnected { id: client_id })
        .await
        .unwrap();
}

struct ReliableTransportInner {
    listen_addr: SocketAddr,
    next_client_id: u32,
    session_endpoint: Option<SessionEndpoint>,
    connections: HashMap<ClientId, mpsc::UnboundedSender<Vec<u8>>>,
    incoming_tx: crossbeam_channel::Sender<(ClientId, Vec<u8>)>,
    incoming_rx: crossbeam_channel::Receiver<(ClientId, Vec<u8>)>,
    events_tx: mpsc::Sender<ReliableEvent>,
}

impl ReliableTransportInner {
    fn new(listen_addr: SocketAddr, events_tx: mpsc::Sender<ReliableEvent>) -> Self {
        let (incoming_tx, incoming_rx) = crossbeam_channel::unbounded();
        Self {
            next_client_id: 1,
            session_endpoint: None,
            connections: HashMap::new(),
            listen_addr,
            incoming_rx,
            incoming_tx,
            events_tx,
        }
    }

    fn set_session_endpoint(&mut self, endpoint: SessionEndpoint) {
        self.session_endpoint = Some(endpoint);
    }

    pub fn send(&mut self, client_id: &ClientId, message: Vec<u8>) {
        if let Some(tx) = self.connections.get(client_id) {
            debug!("sending to {:?}: {:?}", client_id, self.connections.keys());
            tx.send(message).unwrap();
        }
    }

    pub fn register_client(&mut self, tx: mpsc::UnboundedSender<Vec<u8>>) -> ClientId {
        let id = self.next_client_id();
        self.connections.insert(id, tx);
        id
    }

    fn unregister(&mut self, client_id: &ClientId) {
        debug!("unregistering {:?}", client_id);
        self.connections.remove(client_id);
    }

    fn next_client_id(&mut self) -> ClientId {
        let id = self.next_client_id;
        self.next_client_id += 1;
        ClientId::new(id)
    }
}

struct UnreliableTransport {
    rtc: RtcServer,
    session_endpoint: SessionEndpoint,
    incoming_tx: mpsc::Sender<(SocketAddr, Vec<u8>)>,
    outgoing_rx: mpsc::Receiver<(SocketAddr, Vec<u8>)>,
}

impl UnreliableTransport {
    async fn new(
        listen_addr: SocketAddr,
        public_addr: SocketAddr,
        incoming_tx: mpsc::Sender<(SocketAddr, Vec<u8>)>,
        outgoing_rx: mpsc::Receiver<(SocketAddr, Vec<u8>)>,
    ) -> Self {
        let rtc = RtcServer::new(listen_addr, public_addr).await.unwrap();
        let session_endpoint = rtc.session_endpoint();
        // let rtc = Arc::new(RwLock::new(rtc));
        Self {
            rtc,
            session_endpoint,
            incoming_tx,
            outgoing_rx,
        }
    }

    pub fn session_endpoint(&self) -> SessionEndpoint {
        self.session_endpoint.clone()
    }

    async fn listen(&mut self) {
        enum Next {
            Recv(SocketAddr, Vec<u8>),
            Send(SocketAddr, Vec<u8>),
        }
        loop {
            let next = tokio::select! {
                Ok(recv) = self.rtc.recv() => {
                    let bytes = recv.message.as_ref().to_vec();
                    let addr = recv.remote_addr;
                    Next::Recv(addr, bytes)
                    // self.incoming_tx.send((addr, bytes)).await.unwrap();
                },
                Some((addr, data)) = self.outgoing_rx.recv() => {
                    trace!("sending via rtc to {:?}", addr);
                    Next::Send(addr, data)
                }
            };

            match next {
                Next::Recv(addr, data) => {
                    self.incoming_tx.send((addr, data)).await.unwrap();
                }
                Next::Send(addr, data) => {
                    let size = data.len();
                    // debug!("size: {:?}", size);
                    if size > 1600 {
                        panic!();
                    }
                    if let Err(e) = self
                        .rtc
                        .send(&data, webrtc_unreliable::MessageType::Binary, &addr)
                        .await
                    {
                        warn!("failed to send to {:?}", addr);
                    }
                }
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
    events_rx: mpsc::Receiver<ReliableEvent>,
    unreliable_incoming_rx: mpsc::Receiver<(SocketAddr, Vec<u8>)>,
    unreliable_outgoing_tx: mpsc::Sender<(SocketAddr, Vec<u8>)>,
    server_broadcast_rx: mpsc::UnboundedReceiver<OutgoingPacket>,
    server_rx: mpsc::UnboundedReceiver<(ClientId, OutgoingPacket)>,
    server_tx: mpsc::UnboundedSender<(ClientId, IncomingPacket)>,
}

impl<OutgoingPacket, IncomingPacket> Server<OutgoingPacket, IncomingPacket>
where
    OutgoingPacket: std::fmt::Debug + Send + Sync + Serialize,
    IncomingPacket: std::fmt::Debug + Send + Sync + DeserializeOwned,
{
    pub async fn new(
        config: ServerConfig,
        server_broadcast_rx: mpsc::UnboundedReceiver<OutgoingPacket>,
        server_rx: mpsc::UnboundedReceiver<(ClientId, OutgoingPacket)>,
        server_tx: mpsc::UnboundedSender<(ClientId, IncomingPacket)>,
    ) -> Self {
        let (events_tx, events_rx) = mpsc::channel(32);

        let reliable_transport = ReliableTransport::new(config.http_listen_addr, events_tx);
        let (incoming_tx, unreliable_incoming_rx) = mpsc::channel(32);
        let (unreliable_outgoing_tx, unreliable_outgoing_rx) = mpsc::channel(32);

        let unreliable_transport = UnreliableTransport::new(
            config.webrtc_listen_addr,
            config.webrtc_public_addr,
            incoming_tx,
            unreliable_outgoing_rx,
        )
        .await;
        Self {
            config,
            reliable_buffers: HashMap::new(),
            incoming_packet_type: PhantomData,
            reliable_transport: Some(reliable_transport),
            unreliable_transport: Some(unreliable_transport),
            events_rx,
            unreliable_incoming_rx,
            unreliable_outgoing_tx,
            server_broadcast_rx,
            server_rx,
            server_tx,
        }
    }

    pub async fn listen(&mut self) {
        let mut unreliable_transport = self.unreliable_transport.take().unwrap();
        let mut reliable_transport = self.reliable_transport.take().unwrap();
        reliable_transport
            .set_session_endpoint(unreliable_transport.session_endpoint())
            .await;
        let _reliable_rx = reliable_transport.incoming().await;
        let reliable_tx = reliable_transport.outgoing().await;
        let _reliable = tokio::spawn(async move {
            reliable_transport.listen().await;
        });
        let _unreliable = tokio::spawn(async move {
            unreliable_transport.listen().await;
        });
        {
            let mut processor = Processor::<OutgoingPacket, IncomingPacket>::new(
                reliable_tx,
                self.unreliable_outgoing_tx.clone(),
            );

            loop {
                tokio::select! {
                    Some(broadcast) = self.server_broadcast_rx.recv() => {
                        processor.broadcast(broadcast).await;
                    }
                    Some(event) = self.events_rx.recv() => {
                        debug!("got reliable event {:?}", event);
                        match event {
                            ReliableEvent::NewClient { id, challenge } => {
                                processor.register_reliable_client(id, challenge).await;
                            }
                            ReliableEvent::ClientDisconnected { id } => {
                                processor.unregister_client(&id);
                            }
                        }
                    }

                    Some((addr, packet)) = self.unreliable_incoming_rx.recv() => {
                        processor.process_packet(addr, packet).await;
                    }
                }
            }
        };
    }
}

#[derive(Debug)]
struct Processor<OutgoingPacket, IncomingPacket> {
    incoming_type: std::marker::PhantomData<IncomingPacket>,
    outgoing_type: std::marker::PhantomData<OutgoingPacket>,
    challenge_to_client: HashMap<String, ClientId>,
    addr_to_client: HashMap<SocketAddr, ClientId>,
    reliable_tx: mpsc::Sender<(ClientId, Vec<u8>)>,
    unreliable_tx: mpsc::Sender<(SocketAddr, Vec<u8>)>,
}

impl<OutgoingPacket, IncomingPacket> Processor<OutgoingPacket, IncomingPacket>
where
    IncomingPacket: std::fmt::Debug + Send + Sync + DeserializeOwned,
    OutgoingPacket: std::fmt::Debug + Send + Sync + Serialize,
{
    fn new(
        reliable_tx: mpsc::Sender<(ClientId, Vec<u8>)>,
        unreliable_tx: mpsc::Sender<(SocketAddr, Vec<u8>)>,
    ) -> Self {
        Self {
            incoming_type: std::marker::PhantomData,
            outgoing_type: std::marker::PhantomData,
            challenge_to_client: HashMap::new(),
            addr_to_client: HashMap::new(),
            reliable_tx,
            unreliable_tx,
        }
    }

    #[tracing::instrument(level = "debug", skip(self))]
    async fn broadcast(&self, packet: OutgoingPacket) {
        use bincode::Options;
        let bincoder = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .reject_trailing_bytes();
        let encoded = bincoder.serialize(&packet).unwrap();
        for addr in self.addr_to_client.keys() {
            self.unreliable_tx
                .send((addr.clone(), encoded.clone()))
                .await
                .unwrap();
        }
    }

    fn client_id(&self, addr: &SocketAddr) -> Option<ClientId> {
        self.addr_to_client.get(addr).copied()
    }

    #[tracing::instrument(level = "debug", skip(self, packet))]
    #[async_recursion::async_recursion]
    async fn process_packet(&mut self, addr: SocketAddr, packet: Vec<u8>) {
        use bincode::Options;
        let bincoder = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .reject_trailing_bytes();

        if let Ok(deserialized) = bincoder.deserialize::<IncomingPacket>(&packet) {
            debug!("got user packet");
        } else if let Ok(deserialized) = bincoder.deserialize::<ClientProtocolPacket>(&packet) {
            debug!(?deserialized);
            match deserialized {
                ClientProtocolPacket::Connect { challenge } => {
                    debug!(
                        ?challenge,
                        ?addr,
                        "got unreliable transport client connect packet",
                    );
                    if let Some(client_id) = self.register_unreliable_client(&challenge, addr) {
                        debug!(
                            ?client_id,
                            "associated unreliable connection to reliable connection"
                        );
                        self.reliable_tx
                            .send((
                                client_id,
                                ServerProtocolPacket::from(ServerProtocolPacketInner::Welcome {})
                                    .encode(),
                            ))
                            .await
                            .unwrap();
                    } else {
                        // TODO
                        panic!("no known client for challenge");
                    }
                }
                ClientProtocolPacket::Ack { id } => {
                    debug!("got ack");
                }
                ClientProtocolPacket::AckRequest { packet, id } => {
                    self.process_packet(addr, packet).await;
                    debug!(?id, "sending ack");
                    self.unreliable_tx
                        .send((
                            addr,
                            ServerProtocolPacketInner::Ack { id }.into_packet().encode(),
                        ))
                        .await
                        .unwrap();
                }
            }
        }
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

    async fn register_reliable_client(&mut self, client_id: ClientId, challenge: String) {
        self.challenge_to_client
            .insert(challenge.clone(), client_id);
        self.reliable_tx
            .send((
                client_id,
                ServerProtocolPacket::from(ServerProtocolPacketInner::ConnectChallenge {
                    challenge,
                })
                .encode(),
            ))
            .await
            .unwrap();
    }

    fn unregister_client(&mut self, client_id: &ClientId) {
        self.addr_to_client.retain(|_, v| v != client_id);
        self.challenge_to_client.retain(|_, v| v != client_id);
    }
}
