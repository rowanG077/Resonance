// Average linear radiance, not tonemapped pixels. No history is reused across
// game ticks; transparent geometry and overlays haven't been drawn yet.
#ifdef ACCUMULATE
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var history: texture_storage_2d<rgba32float, read_write>;
@group(0) @binding(2) var<uniform> settings: vec4<u32>;

@compute @workgroup_size(8, 8, 1)
fn accumulate(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= textureDimensions(source)) { return; }
    let pixel = vec2<i32>(id.xy);
    let color = textureLoad(source, pixel, 0).rgb;
    var previous = vec4(0.0);
    if settings.x > 1u {
        previous = textureLoad(history, pixel);
    }
    // Reject NaN/Inf before they poison a persistent mean and its neighbors.
    // Alpha stores the accepted sample count, so skipped samples don't darken
    // the result. The source is RGBA16F; every finite radiance fits this range.
    if all(color >= vec3(0.0)) && all(color <= vec3(65504.0)) {
        let count = previous.a + 1.0;
        textureStore(history, pixel, vec4(previous.rgb + (color - previous.rgb) / count, count));
    } else {
        textureStore(history, pixel, previous);
    }
}
#else
#import bevy_pbr::pbr_deferred_types::unpack_24bit_normal
#import bevy_pbr::utils::octahedral_decode

@group(0) @binding(0) var history: texture_2d<f32>;
@group(0) @binding(1) var gbuffer: texture_2d<u32>;
@group(0) @binding(2) var depth: texture_depth_2d;
@group(0) @binding(3) var destination: texture_storage_2d<rgba16float, write>;

fn albedo(gpixel: vec4<u32>) -> vec3<f32> {
    // Demodulate illumination to retain the HD texture's lettering and grain.
    return max(pow(unpack4x8unorm(gpixel.r).rgb, vec3(2.2)), vec3(0.04));
}
@compute @workgroup_size(8, 8, 1)
fn filter_room(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<i32>(textureDimensions(history));
    let pixel = vec2<i32>(id.xy);
    if any(pixel >= size) { return; }
    let center = textureLoad(history, pixel, 0);
    let z = textureLoad(depth, pixel, 0);
    if z == 0.0 {
        textureStore(destination, pixel, vec4(center.rgb, 1.0));
        return;
    }
    let g = textureLoad(gbuffer, pixel, 0);
    let normal = octahedral_decode(unpack_24bit_normal(g.a));
    var sum = vec3(0.0);
    var weights = 0.0;
    for (var y = -3; y <= 3; y++) {
        for (var x = -3; x <= 3; x++) {
            let q = clamp(pixel + vec2(x, y), vec2(0), size - 1);
            let other_z = textureLoad(depth, q, 0);
            let other_g = textureLoad(gbuffer, q, 0);
            let other_normal = octahedral_decode(unpack_24bit_normal(other_g.a));
            let normal_weight = pow(max(dot(normal, other_normal), 0.0), 32.0);
            let depth_weight = exp(-abs(other_z - z) / max(abs(z) * 0.01, 0.000001));
            let spatial_weight = exp(-f32(x*x + y*y) / 8.0);
            let weight = normal_weight * depth_weight * spatial_weight;
            sum += textureLoad(history, q, 0).rgb / albedo(other_g) * weight;
            weights += weight;
        }
    }
    textureStore(destination, pixel, vec4(sum / max(weights, 0.0001) * albedo(g), 1.0));
}
#endif
