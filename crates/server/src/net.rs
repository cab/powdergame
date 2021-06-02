use std::{
    borrow::BorrowMut,
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use crossbeam_channel::{Receiver, Sender};
use game_common::{ClientPacket, ServerPacket};
use hyper::{
    header::{self, HeaderValue},
    server::conn::AddrStream,
    service::{make_service_fn, service_fn},
    Body, Method, Response, Server, StatusCode,
};
use tokio::sync::RwLock;
use tokio::{net::TcpListener, sync::mpsc};
use tracing::{debug, info, trace, warn};
use webrtc_unreliable::{MessageType, Server as RtcServer, SessionEndpoint};

struct Client {
    remote_addr: SocketAddr,
}

#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct ClientId(u32);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

type Result<T> = std::result::Result<T, Error>;

trait CorsExt {
    fn with_cors_headers(self) -> Self;
}

impl CorsExt for hyper::http::response::Builder {
    fn with_cors_headers(mut self) -> Self {
        self.header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
    }
}

pub struct GameServer {
    clients: HashMap<ClientId, Client>,
    server_broadcast_tx: mpsc::UnboundedSender<ServerPacket>,
    server_broadcast_rx: mpsc::UnboundedReceiver<ServerPacket>,
    server_tx: mpsc::UnboundedSender<(ClientId, ServerPacket)>,
    server_rx: mpsc::UnboundedReceiver<(ClientId, ServerPacket)>,
    client_tx: mpsc::UnboundedSender<(ClientId, ClientPacket)>,
    client_rx: Option<mpsc::UnboundedReceiver<(ClientId, ClientPacket)>>,
}

impl GameServer {
    pub fn new() -> Self {
        let clients = HashMap::new();
        let (server_tx, server_rx) = mpsc::unbounded_channel();
        let (client_tx, client_rx) = mpsc::unbounded_channel();
        let (server_broadcast_tx, server_broadcast_rx) = mpsc::unbounded_channel();
        Self {
            clients,
            server_rx,
            server_tx,
            client_rx: Some(client_rx),
            client_tx,
            server_broadcast_rx,
            server_broadcast_tx,
        }
    }

    pub fn channels(
        &mut self,
    ) -> Option<(
        mpsc::UnboundedSender<ServerPacket>,
        mpsc::UnboundedSender<(ClientId, ServerPacket)>,
        mpsc::UnboundedReceiver<(ClientId, ClientPacket)>,
    )> {
        Some((
            self.server_broadcast_tx.clone(),
            self.server_tx.clone(),
            self.client_rx.take()?,
        ))
    }

    pub async fn listen(
        &mut self,
        listen_addr: SocketAddr,
        public_addr: SocketAddr,
        session_listen_addr: SocketAddr,
    ) -> Result<()> {
        debug!(
            "creating server, listening on {:?} and advertised on {:?}",
            listen_addr, public_addr
        );
        let mut rtc = RtcServer::new(listen_addr, public_addr).await?;

        let (server_event_tx, mut server_event_rx) = mpsc::unbounded_channel::<ServerEvent>();

        let session_endpoint = rtc.session_endpoint();
        let mut next_client_id = 0;
        let make_svc = make_service_fn({
            let server_event_tx = server_event_tx.clone();
            move |addr_stream: &AddrStream| {
                let session_endpoint = session_endpoint.clone();
                let remote_addr = addr_stream.remote_addr();
                let server_event_tx = server_event_tx.clone();
                async move {
                    Ok::<_, Error>(service_fn(move |req| {
                        let mut session_endpoint = session_endpoint.clone();
                        let server_event_tx = server_event_tx.clone();
                        async move {
                            if req.method() == Method::OPTIONS {
                                debug!("options");
                                Response::builder().with_cors_headers().body(Body::empty())
                            } else if req.uri().path() == "/new_session"
                                && req.method() == Method::POST
                            {
                                info!("WebRTC session request from {}", remote_addr);
                                match session_endpoint.http_session_request(req.into_body()).await {
                                    Ok(mut resp) => {
                                        server_event_tx.send(ServerEvent::AddClient(
                                            ClientId(next_client_id),
                                            remote_addr,
                                        ));
                                        next_client_id += 1;
                                        resp.headers_mut().insert(
                                            header::ACCESS_CONTROL_ALLOW_ORIGIN,
                                            HeaderValue::from_static("*"),
                                        );
                                        Ok(resp.map(Body::from))
                                    }
                                    Err(err) => {
                                        warn!("bad rtc session request: {:?}", err);
                                        Response::builder()
                                            .status(StatusCode::BAD_REQUEST)
                                            .body(Body::from(format!("error: {:?}", err)))
                                    }
                                }
                            } else {
                                Response::builder()
                                    .status(StatusCode::NOT_FOUND)
                                    .body(Body::from("not found"))
                            }
                        }
                    }))
                }
            }
        });

        tokio::spawn(async move {
            debug!("listening to http on {:?}", session_listen_addr);
            Server::bind(&session_listen_addr)
                .serve(make_svc)
                .await
                .expect("HTTP session server has died");
        });

        let mut clients = HashMap::<ClientId, Client>::new();
        let rtc = RwLock::new(rtc);
        let addr_to_client_id = Arc::new(Mutex::new(HashMap::<SocketAddr, ClientId>::new()));

        loop {}

        // tokio::spawn({
        //     let server_event_tx = server_event_tx.clone();
        //     let addr_to_client_id = addr_to_client_id.clone();
        //     async move {
        //         loop {
        //             let recv = {
        //                 let mut rtc = rtc.write().await;
        //                 if let Ok(recv) = rtc.recv().await {
        //                     if let Some(packet) = ClientPacket::decode(recv.message.as_ref()) {
        //                         Some((recv.remote_addr, packet))
        //                     } else {
        //                         None
        //                     }
        //                 } else {
        //                     None
        //                 }
        //             };
        //             if let Some((addr, packet)) = recv {
        //                 if let Some(client_id) = addr_to_client_id.lock().unwrap().get(&addr) {
        //                     server_event_tx.send(ServerEvent::Message(*client_id, packet));
        //                 } else {
        //                     match packet {
        //                         ClientPacket::Connect() => {
        //                             server_event_tx
        //                                 .send(ServerEvent::SendDirect(
        //                                     addr,
        //                                     ServerPacket::ConnectChallenge {
        //                                         challenge: "challenge".to_string(),
        //                                     },
        //                                 ))
        //                                 .unwrap();
        //                         }
        //                         _ => {
        //                             // ignore
        //                             // TODO: force disconnect
        //                         }
        //                     }
        //                 }
        //             }
        //         }
        //     }
        // });

        // loop {
        //     tokio::select! {
        //       recv = rtc.recv() => {
        //         if let Ok(received) = recv {
        //           if received.message_type != MessageType::Binary {
        //                   unimplemented!("bad message");
        //               }
        //               if let Some(packet) = ClientPacket::decode(received.message.as_ref()) {
        //                   debug!("received {:?} from {:?}", packet, received.remote_addr);
        //                   let data = (received.remote_addr, packet);
        //               }
        //         }
        //       }
        //       send = self.server_broadcast_rx.recv() => {
        //         if let Some(send) = send {
        //           trace!("broadcasting {:?}", send);
        //           let encoded = send.encode();
        //           for client in clients.values() {
        //             rtc.send(&encoded, MessageType::Binary, &client.remote_addr).await.unwrap();
        //           }
        //         }
        //       }
        //     }

        // if let Some((remote_addr, packet)) = received {
        //     if let Err(err) = self
        //         .rtc
        //         .send(&message_buf, message_type, &remote_addr)
        //         .await
        //     {
        //         warn!("could not send message to {}: {:?}", remote_addr, err);
        //     }
        // }
        // }

        Ok(())
    }
}

#[derive(Debug)]
enum ServerEvent {
    AddClient(ClientId, SocketAddr),
    Message(ClientId, ClientPacket),
    SendDirect(SocketAddr, ServerPacket),
}
