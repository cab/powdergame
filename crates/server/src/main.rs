use std::net::SocketAddr;

use clap::{App, Arg};
use game_common::ClientPacket;
use hyper::{
    header::{self, HeaderValue},
    server::conn::AddrStream,
    service::{make_service_fn, service_fn},
    Body, Method, Response, Server, StatusCode,
};
use tracing::{debug, info, trace, warn};
use webrtc_unreliable::{MessageType, Server as RtcServer};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use tracing_subscriber::layer::SubscriberExt;
    tracing::subscriber::set_global_default(
        tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer())
            .with(tracing_subscriber::EnvFilter::try_new("game_server=debug").expect("todo")),
    )
    .unwrap();

    let matches = App::new("echo_server")
        .arg(
            Arg::with_name("data")
                .long("data")
                .takes_value(true)
                .required(true)
                .help("listen on the specified address/port for UDP WebRTC data channels"),
        )
        .arg(
            Arg::with_name("public")
                .long("public")
                .takes_value(true)
                .required(true)
                .help("advertise the given address/port as the public WebRTC address/port"),
        )
          .arg(
            Arg::with_name("http")
                .long("http")
                .takes_value(true)
                .required(true)
                .help("listen on the specified address/port for incoming HTTP (session reqeusts and test page"),
        )
        .get_matches();

    let webrtc_listen_addr = matches
        .value_of("data")
        .unwrap()
        .parse()
        .expect("could not parse WebRTC data address/port");

    let public_webrtc_addr = matches
        .value_of("public")
        .unwrap()
        .parse()
        .expect("could not parse advertised public WebRTC data address/port");

    let session_listen_addr = matches
        .value_of("http")
        .unwrap()
        .parse()
        .expect("could not parse HTTP address/port");

    debug!("starting");
    let mut server = GameServer::new(webrtc_listen_addr, public_webrtc_addr).await?;
    server.listen(session_listen_addr).await;
    Ok(())
}

struct GameServer {
    rtc: RtcServer,
}

#[derive(Debug, thiserror::Error)]
enum Error {
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

impl GameServer {
    pub async fn new(listen_addr: SocketAddr, public_addr: SocketAddr) -> Result<Self> {
        debug!(
            "creating server, listening on {:?} and advertised on {:?}",
            listen_addr, public_addr
        );
        let rtc = RtcServer::new(listen_addr, public_addr).await?;
        Ok(Self { rtc })
    }

    pub async fn listen(&mut self, session_listen_addr: SocketAddr) {
        let session_endpoint = self.rtc.session_endpoint();
        let make_svc = make_service_fn(move |addr_stream: &AddrStream| {
            let session_endpoint = session_endpoint.clone();
            let remote_addr = addr_stream.remote_addr();
            async move {
                Ok::<_, Error>(service_fn(move |req| {
                    let mut session_endpoint = session_endpoint.clone();
                    async move {
                        if req.method() == Method::OPTIONS {
                            debug!("options");
                            Response::builder().with_cors_headers().body(Body::empty())
                        } else if req.uri().path() == "/new_session" && req.method() == Method::POST
                        {
                            info!("WebRTC session request from {}", remote_addr);
                            match session_endpoint.http_session_request(req.into_body()).await {
                                Ok(mut resp) => {
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
        });

        tokio::spawn(async move {
            debug!("listening to http on {:?}", session_listen_addr);
            Server::bind(&session_listen_addr)
                .serve(make_svc)
                .await
                .expect("HTTP session server has died");
        });

        loop {
            let received = match self.rtc.recv().await {
                Ok(received) => {
                    if received.message_type != MessageType::Binary {
                        unimplemented!("bad message");
                    }
                    if let Some(packet) = ClientPacket::decode(received.message.as_ref()) {
                        debug!("received {:?} from {:?}", received.remote_addr, packet);
                        Some((received.remote_addr, packet))
                    } else {
                        // invalid packet
                        None
                    }
                }
                Err(err) => {
                    warn!("could not receive RTC message: {:?}", err);
                    None
                }
            };

            // if let Some((remote_addr, packet)) = received {
            //     if let Err(err) = self
            //         .rtc
            //         .send(&message_buf, message_type, &remote_addr)
            //         .await
            //     {
            //         warn!("could not send message to {}: {:?}", remote_addr, err);
            //     }
            // }
        }
    }
}
