// Textured 2D quads of the native display-list renderer: final output
// conversion and premultiplied artwork. Each draw carries its material's
// constants.
import resonance_presentation::native_frame::{draws, get_world_from_local, view};

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @if(VERTEX_COLORS) @location(5) color: vec4<f32>,
};
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) instance: u32,
    // Opaque white when the mesh has no colors, as Bevy's mesh2d leaves them.
    @location(2) color: vec4<f32>,
};

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world = get_world_from_local(vertex.instance_index) * vec4<f32>(vertex.position, 1.0);
    out.position = view.clip_from_world * world;
    out.uv = vertex.uv;
    out.instance = vertex.instance_index;
    out.color = vec4<f32>(1.0);
    @if(VERTEX_COLORS)
    out.color = vertex.color;
    return out;
}

@group(1) @binding(0) var source: texture_2d<f32>;
@group(1) @binding(1) var source_sampler: sampler;
// Only the output entry point's layout has the overlay.
@group(1) @binding(2) var overlay: texture_2d<f32>;
@group(1) @binding(3) var overlay_sampler: sampler;

// title_output.wesl: fades in encoded space, then decode for the sRGB target.
@fragment
fn output(in: VertexOutput) -> @location(0) vec4<f32> {
    let brightness = draws[in.instance].params;
    // Sample before branching: the parameters vary per draw.
    let center = textureSample(source, source_sampler, in.uv).rgb;
    let ui = textureSample(overlay, overlay_sampler, in.uv);
    var encoded = clamp(center * brightness.x + brightness.y, vec3(0.0), vec3(1.0));
    if brightness.z > 0.5 { encoded = vec3(0.0); }
    // The pause overlay is premultiplied and encoded, and covers scene fades.
    if brightness.w > 0.5 { encoded = ui.rgb + encoded * (1.0 - ui.a); }
    let linear = select(encoded / 12.92,
        pow((encoded + 0.055) / 1.055, vec3<f32>(2.4)), encoded > vec3<f32>(0.04045));
    return vec4<f32>(linear, 1.0);
}

// title_text.wesl: normal text plus additive pulse, premultiplied.
@fragment
fn text(in: VertexOutput) -> @location(0) vec4<f32> {
    let opacity_pulse = draws[in.instance].params;
    let texel = textureSample(source, source_sampler, in.uv);
    return vec4<f32>(texel.rgb * texel.a * (opacity_pulse.x + opacity_pulse.y),
                     texel.a * opacity_pulse.x);
}

// menu_backdrop.wesl: the frozen field image under a cover color, opaque
// while the menu shows it.
@fragment
fn backdrop(in: VertexOutput) -> @location(0) vec4<f32> {
    let cover = draws[in.instance].params;
    let color = mix(textureSample(source, source_sampler, in.uv).rgb, cover.rgb, cover.a);
    // The battle shatter fades its fragments through vertex alpha.
    return vec4<f32>(color, in.color.a);
}

// title_glow.wesl: texture times vertex color with fourfold RGB gain.
@fragment
fn glow(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(source, source_sampler, in.uv) * in.color;
    return vec4<f32>(min(color.rgb * 4.0, vec3<f32>(1.0)), color.a);
}

// field_water.wesl: the mirrored scene sampled in screen space, tinted. The
// scene size the material was made for rides in the draw's spare ids.
@fragment
fn water(in: VertexOutput) -> @location(0) vec4<f32> {
    let draw = draws[in.instance];
    let dimensions = vec2<f32>(bitcast<f32>(draw.ids.z), bitcast<f32>(draw.ids.w));
    let uv = in.position.xy / dimensions;
    let reflected = textureSample(source, source_sampler, vec2<f32>(1. - uv.x, uv.y));
    return vec4<f32>(min(reflected.rgb * draw.params.rgb, vec3<f32>(1.)), draw.params.a);
}

// model_preview.wesl: the preview camera's premultiplied image, faded.
@fragment
fn composite(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(source, source_sampler, in.uv) * draws[in.instance].params.x;
}

struct BlitOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// One triangle covering the target, as Bevy's fullscreen vertex stage.
@vertex
fn fullscreen(@builtin(vertex_index) index: u32) -> BlitOutput {
    let uv = vec2<f32>(f32(index >> 1u), f32(index & 1u)) * 2.0;
    return BlitOutput(vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0), uv);
}

// Copy the scene kept before UI under the overlay pass.
@fragment
fn blit(in: BlitOutput) -> @location(0) vec4<f32> {
    return textureSampleLevel(source, source_sampler, in.uv, 0.0);
}
