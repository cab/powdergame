mod net;
mod render;
mod world;

use std::{net::SocketAddr, sync::Arc};

use game_common::{ClientPacket, ServerPacket};
use tracing::{debug, trace, warn};
use wasm_bindgen::prelude::*;
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Render(#[from] render::Error),
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn wasm_main() -> Result<(), wasm_bindgen::JsValue> {
    use tracing_subscriber::layer::SubscriberExt;
    console_error_panic_hook::set_once();
    tracing::subscriber::set_global_default(
        tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer())
            .with(tracing_subscriber::EnvFilter::try_new("debug").expect("todo"))
            .with(tracing_wasm::WASMLayer::new(
                tracing_wasm::WASMLayerConfig::default(),
            )),
    )
    .unwrap();
    debug!("starting");
    Ok(())
}

#[wasm_bindgen]
pub fn start(canvas: web_sys::HtmlCanvasElement) {
    start_internal(canvas).unwrap();
}

pub fn start_internal(mut canvas: web_sys::HtmlCanvasElement) -> Result<(), Error> {
    debug!("creating renderer");
    let renderer = render::Renderer::new(&mut canvas)?;

    debug!("setting up networking");
    let mut client = Arc::new(gnet::client::Client::<ClientPacket, ServerPacket>::new());

    wasm_bindgen_futures::spawn_local({
        let client = client.clone();
        async move {
            client.connect(([127, 0, 0, 1], 9000).into()).await.unwrap();
            client.send_reliable(ClientPacket::SetName {
                name: "conner".to_string(),
            });
            for message in client.recv().await {
                debug!("got message {:?}", message);
            }
        }
    });

    debug!("setting up ecs");
    let world = bevy_ecs::world::World::new();

    let event_loop = EventLoop::new();
    debug!("creating window");
    #[cfg(target_arch = "wasm32")]
    let window = {
        use winit::platform::web::WindowBuilderExtWebSys;
        WindowBuilder::new()
            .with_title("jsgame")
            .with_inner_size(winit::dpi::LogicalSize {
                height: canvas.height() / 2,
                width: canvas.width() / 2,
            })
            .with_canvas(Some(canvas))
            .build(&event_loop)
            .unwrap()
    };

    #[cfg(not(target_arch = "wasm32"))]
    let window = {
        WindowBuilder::new()
            .with_title("jsgame")
            .with_inner_size(winit::dpi::LogicalSize {
                height: canvas.height() / 2,
                width: canvas.width() / 2,
            })
            .build(&event_loop)
            .unwrap()
    };

    debug!("starting event loop");
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                window_id,
            } if window_id == window.id() => *control_flow = ControlFlow::Exit,
            Event::MainEventsCleared => {
                window.request_redraw();
            }
            Event::RedrawRequested(_) => {
                client.process();
                renderer.render();
            }
            _ => (),
        }
    });

    Ok(())
}
