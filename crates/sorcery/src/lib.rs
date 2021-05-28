use sdfu::SDF;
use tracing::{debug, trace, warn};
use tracing_subscriber::layer::SubscriberExt;
use ultraviolet::Vec3;
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn wasm_main() -> Result<(), wasm_bindgen::JsValue> {
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
    debug!("initialized wasm");
    Ok(())
}

#[wasm_bindgen]
pub fn create_sdf() -> Result<Box<[f32]>, JsValue> {
    let sdf = sdfu::Sphere::new(0.45)
        .subtract(sdfu::Box::new(Vec3::new(0.25, 0.25, 1.5)))
        .union_smooth(
            sdfu::Sphere::new(0.3).translate(Vec3::new(0.3, 0.3, 0.0)),
            0.1,
        )
        .union_smooth(
            sdfu::Sphere::new(0.3).translate(Vec3::new(-0.3, 0.3, 0.0)),
            0.1,
        )
        .subtract(sdfu::Box::new(Vec3::new(0.125, 0.125, 1.5)).translate(Vec3::new(-0.3, 0.3, 0.0)))
        .subtract(sdfu::Box::new(Vec3::new(0.125, 0.125, 1.5)).translate(Vec3::new(0.3, 0.3, 0.0)))
        .subtract(sdfu::Box::new(Vec3::new(1.5, 0.1, 0.1)).translate(Vec3::new(0.0, 0.3, 0.0)))
        .subtract(sdfu::Box::new(Vec3::new(0.2, 2.0, 0.2)))
        .translate(Vec3::new(0.0, 0.0, -1.0));

    let distances = encode(sdf);
    Ok(distances.into_boxed_slice())
}

fn encode<S>(sdf: S) -> Vec<f32>
where
    S: SDF<f32, Vec3>,
{
    let size = 128;
    let depth = 32;
    (0..depth)
        .flat_map(|z| {
            (0..size)
                .flat_map(move |y| (0..size).map(move |x| Vec3::new(x as f32, y as f32, z as f32)))
        })
        .map(|location| sdf.dist(location))
        .collect()
}

#[wasm_bindgen]
pub fn march() -> Result<Box<[f32]>, JsValue> {
    let sdf = sdfu::Sphere::new(0.2).translate(Vec3::new(0.75, 0.75, 0.0));

    let size = 64;
    let depth = 32;
    let width = 1.0;
    let height = 1.0;
    let center = Vec3::new(width / 2.0, height / 2.0, -1.0);
    let distances = (0..size)
        .flat_map(move |y| {
            (0..size).map(move |x| {
                Vec3::new(
                    (x as f32 / size as f32) * width,
                    (y as f32 / size as f32) * height,
                    1.0,
                )
            })
        })
        .map(|location| ray_march(sdf, center, location))
        .collect::<Vec<_>>();
    Ok(distances.into_boxed_slice())
}

fn ray_march<S>(sdf: S, ray_origin: Vec3, ray_direction: Vec3) -> f32
where
    S: SDF<f32, Vec3>,
{
    let mut total_distance_traveled = 0.0;
    let NUMBER_OF_STEPS = 32;
    let MINIMUM_HIT_DISTANCE = 0.001;
    let MAXIMUM_TRACE_DISTANCE = 1000.0;

    for _ in 0..NUMBER_OF_STEPS {
        let current_position = ray_origin + total_distance_traveled * ray_direction;
        let distance_to_closest = sdf.dist(current_position);
        // let distance_to_closest = distance_from_sphere(current_position, vec3(0.0), 1.0);

        if distance_to_closest < MINIMUM_HIT_DISTANCE {
            return total_distance_traveled;
        }

        if total_distance_traveled > MAXIMUM_TRACE_DISTANCE {
            break;
        }
        total_distance_traveled += distance_to_closest;
    }
    0.0
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
