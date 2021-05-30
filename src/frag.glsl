precision mediump float;

uniform vec3 u_camera_position;
uniform vec3 u_camera_direction;

uniform sampler2D u_sampler;
uniform sampler2D u_sdf;

float distance_from_sphere(in vec3 p, in vec3 c, float r) {
    return length(p - c) - r;
}

float map_the_world(in vec3 p) {
    float sphere_0 = distance_from_sphere(p, vec3(0.0), 1.0);

    return sphere_0;
}

vec3 calculate_normal(in vec3 p) {
    const vec3 small_step = vec3(0.001, 0.0, 0.0);

    float gradient_x = map_the_world(p + small_step.xyy) - map_the_world(p - small_step.xyy);
    float gradient_y = map_the_world(p + small_step.yxy) - map_the_world(p - small_step.yxy);
    float gradient_z = map_the_world(p + small_step.yyx) - map_the_world(p - small_step.yyx);

    vec3 normal = vec3(gradient_x, gradient_y, gradient_z);

    return normalize(normal);
}

vec3 triplanarMap(vec3 surfacePos, vec3 normal, float scale) {
	// Take projections along 3 axes, sample texture values from each projection, and stack into a matrix
    mat3 triMapSamples = mat3(texture2D(u_sampler, surfacePos.yz * scale).xyz, texture2D(u_sampler, surfacePos.xz * scale).xyz, texture2D(u_sampler, surfacePos.xy * scale).xyz);

    // return texture2D(u_sampler, vec2(0.25, 0.25)).xyz;

	// Weight three samples by absolute value of normal components
    return triMapSamples * abs(normal);
}

vec3 map2d(vec3 surfacePos) {
    float textureFreq = 0.5;
    vec2 uv = textureFreq * surfacePos.xz;

    vec3 surfaceCol = texture2D(u_sampler, uv).xyz;
    return surfaceCol;
}

vec3 sampled(vec3 surfacePos, vec3 normal, float scale) {
  // return map2d(surfacePos);
  // return triplanarMap(surfacePos, normal, scale);
    return vec3(1.0, 0.0, 0.0);
}

vec3 ray_march(in vec3 ro, in vec3 rd) {
    float total_distance_traveled = 0.0;
    const int NUMBER_OF_STEPS = 32;
    const float MINIMUM_HIT_DISTANCE = 0.001;
    const float MAXIMUM_TRACE_DISTANCE = 1000.0;

    for(int i = 0; i < NUMBER_OF_STEPS; ++i) {
        vec3 current_position = ro + total_distance_traveled * rd;

        float distance_to_closest = map_the_world(current_position);

        if(distance_to_closest < MINIMUM_HIT_DISTANCE) {
            vec3 normal = calculate_normal(current_position);

            vec3 light_position = vec3(2.0, 0.0, 10.0);
            vec3 direction_to_light = normalize(current_position - light_position);

            float diffuse_intensity = max(0.0, dot(normal, direction_to_light));

            vec3 textureSample = sampled(current_position, normal, 1.0);

            return textureSample * diffuse_intensity;
        }

        if(total_distance_traveled > MAXIMUM_TRACE_DISTANCE) {
            break;
        }
        total_distance_traveled += distance_to_closest;
    }
    return vec3(0.0);
}

void main() {
    // TODO use actual canvas size
    vec2 vUv = vec2(gl_FragCoord.x / 1774.0, gl_FragCoord.y / 1330.5);
    vec2 uv = vUv * 2.0 - 1.0;

    vec3 ro = u_camera_position;
    vec3 rd = vec3(uv, 0.0) + u_camera_direction;

    vec3 shaded_color = ray_march(ro, rd);

    gl_FragColor = vec4(shaded_color, 1.0);
}