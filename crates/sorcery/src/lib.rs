use sdfu::SDF;
use ultraviolet::Vec3;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn calculate_sdf() -> Result<Box<[f32]>, JsValue> {
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

    let voxels = encode(sdf);
    Ok(voxels.into_boxed_slice())
}

fn encode<S>(sdf: S) -> Vec<f32>
where
    S: SDF<f32, Vec3>,
{
    (0..64)
        .flat_map(|x| (0..64).map(move |y| (x, y)))
        .flat_map(|(x, y)| (0..64).map(move |z| Vec3::new(x as f32, y as f32, z as f32)))
        .map(|location| sdf.dist(location))
        .collect()
}

fn ray_march<S>(sdf: S, ray_origin: Vec3, ray_direction: Vec3) -> Vec3
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
            return Vec3::new(1.0, 0.0, 0.0);
        }

        if total_distance_traveled > MAXIMUM_TRACE_DISTANCE {
            break;
        }
        total_distance_traveled += distance_to_closest;
    }
    return Vec3::zero();
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
