mod net;
mod world;

use bevy_ecs::prelude::*;
use clap::Arg;

use game_common::{app::App, world::Tick, ClientPacket, ServerPacket};
use gnet::protocol::ClientId;
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{debug, info, trace};

use crate::world::WorldPlugin;

// #[tokio::main(flavor = "current_thread")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use tracing_subscriber::layer::SubscriberExt;
    tracing::subscriber::set_global_default(
        tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer())
            .with(
                tracing_subscriber::EnvFilter::try_new("game_server=debug,gnet=debug")
                    .expect("todo"),
            ),
    )
    .unwrap();

    let matches = clap::App::new("echo_server")
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

    let webrtc_public_addr = matches
        .value_of("public")
        .unwrap()
        .parse()
        .expect("could not parse advertised public WebRTC data address/port");

    let session_listen_addr = matches
        .value_of("http")
        .unwrap()
        .parse()
        .expect("could not parse HTTP address/port");

    let (server_broadcast_tx, server_broadcast_rx) = mpsc::unbounded_channel();
    let (server_tx, server_tx_rx) = mpsc::unbounded_channel();
    let (server_rx_tx, server_rx) = mpsc::unbounded_channel();

    let gameloop = tokio::spawn(async move {
        let mut app = setup_ecs(server_broadcast_tx, server_tx, server_rx);
        debug!("starting game loop");
        tick(move || {
            app.update();
        })
        .await;
    });

    let server: JoinHandle<anyhow::Result<()>> = tokio::spawn(async move {
        let mut server = gnet::server::Server::<ServerPacket, ClientPacket>::new(
            gnet::server::ServerConfig {
                http_listen_addr: session_listen_addr,
                webrtc_listen_addr,
                webrtc_public_addr,
            },
            server_broadcast_rx,
            server_tx_rx,
            server_rx_tx,
        )
        .await;
        server.listen().await;
        // debug!("starting server");
        // server
        //     .listen(webrtc_listen_addr, public_webrtc_addr, session_listen_addr)
        //     .await;
        Ok(())
    });

    tokio::select! {
        _ = server => {
            info!("httpserver stopped");
        }
        _ = gameloop => {
            info!("game loop stopped");
        }
    }

    Ok(())
}

async fn tick<U>(mut update: U)
where
    U: FnMut(),
{
    let delta = std::time::Duration::from_millis(16);
    let max_update_delta = delta * 10;
    let mut current_time = std::time::Instant::now();
    let mut accumulator = std::time::Duration::new(0, 0);
    loop {
        let new_time = std::time::Instant::now();
        let frame_duration = new_time.duration_since(current_time);
        current_time = new_time;

        accumulator += frame_duration;

        while accumulator >= delta {
            update();
            accumulator -= delta;
            if accumulator >= max_update_delta {
                accumulator = max_update_delta;
            }
        }
        tokio::task::yield_now().await;
        tokio::time::sleep(delta).await; // TODO keep this for dev?
    }
}

fn setup_ecs(
    server_broadcast_tx: mpsc::UnboundedSender<ServerPacket>,
    server_tx: mpsc::UnboundedSender<(ClientId, ServerPacket)>,
    server_rx: mpsc::UnboundedReceiver<(ClientId, ClientPacket)>,
) -> App {
    debug!("setting up ecs");
    App::builder()
        .insert_resource(Tick::zero())
        .insert_resource(server_broadcast_tx)
        .insert_resource(server_tx)
        .insert_resource(server_rx)
        .add_plugin(WorldPlugin)
        .add_system(update_tick.system())
        .build()
}

fn update_tick(mut tick: ResMut<Tick>) {
    trace!("server tick");
    tick.increment_self();
}
