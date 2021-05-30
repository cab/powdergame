mod render;

use tracing::{debug, trace, warn};
use wasm_bindgen::prelude::*;

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

pub fn start_internal(canvas: web_sys::HtmlCanvasElement) -> Result<(), Error> {
    debug!("creating renderer");
    let renderer = render::Renderer::new(canvas)?;
    renderer.render();
    Ok(())
}
