// Shape tags and Parameters layout are shared with gpu.rs.
const IMAGE: u32 = 0u;
const ELLIPSE: u32 = 2u;
const RADIAL: u32 = 3u;
const CLEAN_IMAGE: u32 = 4u;
const MIN_COMPONENT_PIXELS: u32 = 6u;

struct Parameters {
    color: vec4<f32>,
    region: vec4<f32>,
    image_bounds: vec4<f32>,
    rect: vec4<f32>,
    radial: vec4<f32>,
    positions: vec4<f32>,
    stops: array<vec4<f32>, 4>,
    kind: vec4<u32>,
    corners: array<vec4<f32>, 2>,
}
@group(0) @binding(0) var source: texture_2d<f32>;
@group(1) @binding(0) var<storage, read> parameters: array<Parameters>;

// Remove isolated assembly specks. Connected costume and weapon silhouettes survive.
fn connected(origin: vec2<i32>) -> bool {
    let size = vec2<i32>(textureDimensions(source));
    var queue: array<vec2<i32>, MIN_COMPONENT_PIXELS>;
    queue[0] = origin;
    var count = 1u;
    var cursor = 0u;
    loop {
        if cursor >= count { break; }
        let at = queue[cursor];
        cursor += 1u;
        for (var dy = -1; dy <= 1; dy++) {
            for (var dx = -1; dx <= 1; dx++) {
                let next = at + vec2<i32>(dx, dy);
                if any(next < vec2<i32>(0)) || any(next >= size) { continue; }
                if textureLoad(source, next, 0).a <= 0.0 { continue; }
                var seen = false;
                for (var i = 0u; i < count; i++) {
                    if all(queue[i] == next) { seen = true; break; }
                }
                if !seen {
                    if count + 1u == MIN_COMPONENT_PIXELS { return true; }
                    queue[count] = next;
                    count += 1u;
                }
            }
        }
    }
    return false;
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) parameter: u32,
}
@vertex
fn vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let quad = index / 6u;
    let corner = array<u32, 6>(0u, 1u, 2u, 0u, 2u, 3u)[index % 6u];
    let p = parameters[quad];
    let xy = p.corners[corner / 2u];
    var out: VertexOutput;
    out.position = vec4<f32>(select(xy.xy, xy.zw, corner % 2u == 1u), 0.0, 1.0);
    out.uv = array<vec2<f32>, 4>(
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 0.0)
    )[corner];
    out.parameter = quad;
    return out;
}
@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = parameters[in.parameter];
    if p.kind.x == IMAGE || p.kind.x == CLEAN_IMAGE {
        let uv = p.region.xy + in.uv * p.region.zw;
        // Nearest sampling has an ambiguous tie on texel boundaries. Fractional
        // sprite reduction (e.g. 128 -> 96) hits these repeatedly. Resolve ties
        // toward the preceding texel, consistently across quads and mirroring;
        // otherwise floating-point interpolation can break one-pixel linework.
        const TEXEL_BOUNDARY_TOLERANCE: f32 = 1.0 / 1024.0;
        // Quantize within the sprite before adding its atlas origin. Texture
        // placement must not change a rotated silhouette or a one-pixel line.
        let size = vec2<i32>(p.image_bounds.zw);
        let pixel = clamp(
            vec2<i32>(ceil(uv * vec2<f32>(size) - TEXEL_BOUNDARY_TOLERANCE)) - 1,
            vec2<i32>(0), size - 1
        );
        let atlas_pixel = pixel + vec2<i32>(p.image_bounds.xy);
        let color = textureLoad(source, atlas_pixel, 0) * p.color.a;
        if p.kind.x == CLEAN_IMAGE && color.a > 0.0 {
            if !connected(atlas_pixel) { return vec4<f32>(0.0); }
        }
        return color;
    }
    if p.kind.x == RADIAL {
        let point = p.rect.xy + in.uv * p.rect.zw;
        let t = clamp((distance(point, p.radial.xy) - p.radial.z) / (p.radial.w - p.radial.z), 0.0, 1.0);
        var color = p.stops[p.kind.y - 1u];
        for (var i = 1u; i < p.kind.y; i++) {
            if t <= p.positions[i] {
                let u = clamp((t - p.positions[i-1u]) / (p.positions[i] - p.positions[i-1u]), 0.0, 1.0);
                color = mix(p.stops[i-1u], p.stops[i], u);
                break;
            }
        }
        return vec4<f32>(color.rgb * color.a, color.a);
    }
    var alpha = p.color.a;
    if p.kind.x == ELLIPSE {
        let distance = length((in.uv - 0.5) * 2.0);
        alpha *= clamp((1.0 - distance) / max(fwidth(distance), 0.0001) + 0.5, 0.0, 1.0);
    }
    return vec4<f32>(p.color.rgb * alpha, alpha);
}
