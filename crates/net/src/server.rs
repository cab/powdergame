use crate::protocol::{ClientId, ProtocolMarker, ReliableBuffer, ServerProtocolPacket};
use futures::{FutureExt, SinkExt, StreamExt};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    collections::HashMap, convert::Infallible, marker::PhantomData, net::SocketAddr, sync::Arc,
};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};
use warp::{
    ws::{Message, WebSocket},
    Filter,
};
use webrtc_unreliable::{Server as RtcServer, SessionEndpoint};

struct ReliableTransport {
    inner: Inner,
}

type Inner = Arc<RwLock<ReliableTransportInner>>;

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
        let http_listen_addr = self.inner.read().await.listen_addr;
        debug!("listening for websockets on {:?}", http_listen_addr);
        warp::serve(routes).run(http_listen_addr).await;
    }
}

#[derive(Debug)]
struct NotReady;

impl warp::reject::Reject for NotReady {}

async fn client_connected(ws: WebSocket, inner: Inner) {
    let client_id = inner.write().await.next_client_id();
    debug!("client connected: {:?}", client_id);
    let (mut user_ws_tx, mut user_ws_rx) = ws.split();

    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::task::spawn(async move {
        for message in rx.recv().await {
            user_ws_tx.send(message).await.unwrap();
        }
    });
    tx.send(Message::binary(
        ServerProtocolPacket::ConnectChallenge {
            challenge: "challenge_1".to_string(),
            marker: ProtocolMarker::new(),
        }
        .encode(),
    ));

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
    incoming_tx: crossbeam_channel::Sender<(ClientId, Vec<u8>)>,
    incoming_rx: crossbeam_channel::Receiver<(ClientId, Vec<u8>)>,
}

impl ReliableTransportInner {
    fn new(listen_addr: SocketAddr) -> Self {
        let (incoming_tx, incoming_rx) = crossbeam_channel::unbounded();
        Self {
            next_client_id: 1,
            session_endpoint: None,
            listen_addr,
            incoming_rx,
            incoming_tx,
        }
    }

    fn set_session_endpoint(&mut self, endpoint: SessionEndpoint) {
        self.session_endpoint = Some(endpoint);
    }

    fn next_client_id(&mut self) -> ClientId {
        let id = self.next_client_id;
        self.next_client_id += 1;
        ClientId::new(id)
    }
}

struct UnreliableTransport {
    rtc: RtcServer,
}

impl UnreliableTransport {
    async fn new(listen_addr: SocketAddr, public_addr: SocketAddr) -> Self {
        let mut rtc = RtcServer::new(listen_addr, public_addr).await.unwrap();
        Self { rtc }
    }

    pub fn session_endpoint(&self) -> SessionEndpoint {
        self.rtc.session_endpoint()
    }

    async fn listen(&mut self) {}
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
        let unreliable_transport = self.unreliable_transport.take().unwrap();
        let mut transport = self.reliable_transport.take().unwrap();
        transport
            .set_session_endpoint(unreliable_transport.session_endpoint())
            .await;
        let reliable_rx = transport.incoming().await;
        let reliable = tokio::spawn(async move {
            transport.listen().await;
        });
        let process = tokio::spawn(async move {
            loop {
                for (client_id, packet) in reliable_rx.try_iter() {
                    debug!("got data {:?}", client_id);
                }
            }
        });
        tokio::select! {
            _ = reliable => {
                info!("reliable transport stopped");
            }
            _ = process => {
                info!("processing stopped");
            }
        }
    }
}
