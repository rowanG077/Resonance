import bevy_sprite_render::mesh2d::functions;
struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @if(GLYPH_PATTERN)
    @location(3) pattern_uv: vec2<f32>,
    @location(4) color: vec4<f32>,
};
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @if(MULTISAMPLED_UI)
    @interpolate(perspective, sample)
    @location(0) world_position: vec4<f32>,
    @if(!(MULTISAMPLED_UI))
    @location(0) world_position: vec4<f32>,
    @if(MULTISAMPLED_UI)
    @interpolate(perspective, sample)
    @location(2) uv: vec2<f32>,
    @if(!(MULTISAMPLED_UI))
    @location(2) uv: vec2<f32>,
    @if(GLYPH_PATTERN && MULTISAMPLED_UI)
    @interpolate(perspective, sample)
    @location(3) pattern_uv: vec2<f32>,
    @if(GLYPH_PATTERN && !(MULTISAMPLED_UI))
    @location(3) pattern_uv: vec2<f32>,
    @if(MULTISAMPLED_UI)
    @interpolate(perspective, sample)
    @location(4) color: vec4<f32>,
    @if(!(MULTISAMPLED_UI))
    @location(4) color: vec4<f32>,
};
@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    out.world_position = functions::mesh2d_position_local_to_world(
        functions::get_world_from_local(vertex.instance_index), vec4(vertex.position, 1.0));
    out.position = functions::mesh2d_position_world_to_clip(out.world_position);
    out.uv = vertex.uv;
    @if(GLYPH_PATTERN)
    out.pattern_uv = vertex.pattern_uv;
    out.color = vertex.color;
    return out;
}
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var source: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var source_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var frame_mask: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var frame_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var color_mask: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var color_sampler: sampler;
struct CoverageQuad {
    rect: vec4<f32>,
    uv: vec4<f32>,
    properties: vec4<f32>,
};
struct Coverage {
    count: vec4<u32>,
    quads: array<CoverageQuad, 64>,
};
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var<uniform> coverage: Coverage;
@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let point = vec2(in.world_position.x + 320.0, 240.0 - in.world_position.y);
    for (var i = 0u; i < coverage.count.x; i += 1u) {
        let quad = coverage.quads[i];
        let lower = min(quad.rect.xy, quad.rect.zw);
        let upper = max(quad.rect.xy, quad.rect.zw);
        if all(point >= lower) && all(point < upper) {
            let uv = mix(quad.uv.xy, quad.uv.zw, (point - quad.rect.xy) / (quad.rect.zw - quad.rect.xy));
            var alpha = 0.0;
            if quad.properties.x > 1.5 {
                alpha = 1.0;
            } else if quad.properties.x < 0.5 {
                alpha = textureSampleLevel(frame_mask, frame_sampler, uv, 0.0).a;
            } else {
                alpha = textureSampleLevel(color_mask, color_sampler, uv, 0.0).a;
            }
            if alpha * quad.properties.y >= 0.5 / 255.0 {
                discard;
            }
        }
    }
    var raster = in.color;
    var sampled = textureSample(source, source_sampler, in.uv);
    // Apply the channel swap to texture and raster inputs.
    @if(RED_CHANNEL) {
        sampled = sampled.rrra;
        raster = raster.rrra;
    }
    @if(OPAQUE_IMAGE)
    sampled.a = 1.0;
    let base = sampled * raster;
    // Composite the translucent pattern over the colored glyph in one draw.
    @if(GLYPH_PATTERN) {
        let pattern = textureSample(source, source_sampler, in.pattern_uv);
        let pattern_alpha = sampled.a * (64.0 / 255.0);
        let base_alpha = base.a * (1.0 - pattern_alpha);
        let alpha = pattern_alpha + base_alpha;
        let color = (pattern.rgb * pattern_alpha + base.rgb * base_alpha) / max(alpha, 0.000001);
        return vec4(color, alpha);
    }

    @else
    return base;
}
