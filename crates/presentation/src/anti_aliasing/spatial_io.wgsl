// Sample positions are measured on the GPU, so API-dependent sample ordering
// cannot silently select the wrong SMAA subpixel area table.
struct CalibrationVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(linear, sample) uv: vec2<f32>,
}
@vertex
fn calibration_vertex(@builtin(vertex_index) index: u32) -> CalibrationVertex {
    let uv = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    var out: CalibrationVertex;
    out.position = vec4(uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0);
    out.uv = uv;
    return out;
}
@fragment
fn calibration_fragment(in: CalibrationVertex, @builtin(sample_index) sample: u32) -> @location(0) vec4<f32> {
    return vec4(in.uv, f32(sample), 1.0);
}
#ifdef SPLIT_SAMPLES
@group(0) @binding(0) var samples: texture_multisampled_2d<f32>;
struct SamplePair { @location(0) first: vec4<f32>, @location(1) second: vec4<f32> }
@fragment
fn split_samples(@builtin(position) position: vec4<f32>) -> SamplePair {
    var out: SamplePair;
    out.first = textureLoad(samples, vec2<i32>(position.xy), 0);
    out.second = textureLoad(samples, vec2<i32>(position.xy), 1);
    return out;
}
#endif
