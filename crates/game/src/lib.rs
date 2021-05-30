mod render;
mod world;

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

    debug!("setting up ecs");
    let world = bevy_ecs::world::World::new();

    let event_loop = EventLoop::new();
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
                renderer.render();
            }
            _ => (),
        }
    });

    Ok(())
}