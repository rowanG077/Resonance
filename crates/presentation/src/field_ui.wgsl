#import bevy_sprite::mesh2d_vertex_output::VertexOutput
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var source: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var source_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var frame_mask: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var frame_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var color_mask: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var color_sampler: sampler;
struct CoverageQuad {
    rect: vec4<f32>,
    uv: vec4<f32>,
    properties: vec4<f32>,
};
struct Coverage {
    count: vec4<u32>,
    quads: array<CoverageQuad, 64>,
};
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var<uniform> coverage: Coverage;
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
    return textureSample(source, source_sampler, in.uv) * in.color;
}
