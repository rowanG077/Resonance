// Surface vertex stage of the native display-list renderer. It follows
// title_surface_vertex.wesl statement for statement, reading the draw record
// and joint palette of the frame blob instead of Bevy's mesh bindings.
import resonance_presentation::native_frame::{
    draws, joints, get_world_from_local, position_world_to_clip, position_world_to_view,
};
import resonance_presentation::surface_bindings::surface_data;
import resonance_presentation::surface_vertex::VertexOutput;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @if(VERTEX_NORMALS) @location(1) normal: vec3<f32>,
    @if(VERTEX_UVS_A) @location(2) uv: vec2<f32>,
    @if(VERTEX_UVS_B) @location(3) uv_b: vec2<f32>,
    @if(VERTEX_COLORS) @location(5) color: vec4<f32>,
    @if(SKINNED) @location(6) joint_indices: vec4<u32>,
    @if(SKINNED) @location(7) joint_weights: vec4<f32>,
    @if(SURFACE_LAYOUT) @location(8) channels: u32,
};

// Bevy's preprocessing derives the same inverse transpose from the affine model.
@if(VERTEX_NORMALS && !SKINNED && !WORLD_SURFACE)
fn normal_local_to_world(normal: vec3<f32>, world_from_local: mat4x4<f32>) -> vec3<f32> {
    if any(normal != vec3<f32>(0.0)) {
        let x = world_from_local[0].xyz;
        let y = world_from_local[1].xyz;
        let z = world_from_local[2].xyz;
        let tmp0 = cross(y, z);
        let tmp1 = cross(z, x);
        let tmp2 = cross(x, y);
        let inv_det = 1.0 / dot(z, tmp2);
        return normalize(mat3x3<f32>(tmp0 * inv_det, tmp1 * inv_det, tmp2 * inv_det) * normal);
    } else {
        return normal;
    }
}

@if(SKINNED)
fn skin_model(indexes: vec4<u32>, weights: vec4<f32>, base: u32) -> mat4x4<f32> {
    return weights.x * joints[base + indexes.x]
        + weights.y * joints[base + indexes.y]
        + weights.z * joints[base + indexes.z]
        + weights.w * joints[base + indexes.w];
}

@if(SKINNED && VERTEX_NORMALS && !WORLD_SURFACE)
fn skin_normals(world_from_local: mat4x4<f32>, normal: vec3<f32>) -> vec3<f32> {
    let a = world_from_local[0].xyz;
    let b = world_from_local[1].xyz;
    let c = world_from_local[2].xyz;
    let x = cross(b, c);
    let y = cross(c, a);
    let z = cross(a, b);
    let det = dot(c, z);
    return normalize(mat3x3<f32>(x / det, y / det, z / det) * normal);
}

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    out.channels = 0u;
    @if(SURFACE_LAYOUT)
    out.channels = vertex.channels;
    @else {
        @if(VERTEX_NORMALS) out.channels |= 1u;
        @if(VERTEX_UVS_A) out.channels |= 2u;
        @if(VERTEX_UVS_B) out.channels |= 4u;
        @if(VERTEX_COLORS) out.channels |= 8u;
    }
    let draw_ids = draws[vertex.instance_index].ids;
    let material_slot = draw_ids.x;
    out.material_slot = material_slot;
    let vertex_material = surface_data(material_slot);
    @if(WORLD_SURFACE)
    var material_alpha = 0.0;
    @if(WORLD_SURFACE)
    if (vertex_material.flags & 16u) != 0u {
        material_alpha = clamp(bitcast<f32>(draw_ids.z), 0.0, 1.0);
    }
    @if(WORLD_SURFACE)
    let vertex_tint = vertex_material.tint + vertex_material.tint_motion * material_alpha;
    @else
    let vertex_tint = vertex_material.tint;
    @if(WORLD_SURFACE)
    out.material_tint = vertex_tint;
    @if(WORLD_SURFACE && CLAMP_COLOR)
    out.material_alpha = material_alpha;
    @if(WORLD_SURFACE && !CLAMP_COLOR)
    let uv_offsets = vertex_material.uv_offsets + vertex_material.uv_motion * material_alpha;
    @if(!WORLD_SURFACE)
    let field_lighting = (vertex_material.flags & 8u) != 0u;
    if (vertex_material.flags & 4u) == 0u { out.channels &= ~8u; }

    let mesh_world_from_local = get_world_from_local(vertex.instance_index);
    @if(SKINNED)
    var world_from_local = skin_model(vertex.joint_indices, vertex.joint_weights, draw_ids.y);
    @else
    var world_from_local = mesh_world_from_local;

    @if(VERTEX_NORMALS && SKINNED && !WORLD_SURFACE)
    out.world_normal = skin_normals(world_from_local, vertex.normal);
    @if(VERTEX_NORMALS && !(SKINNED) && !WORLD_SURFACE)
    out.world_normal = normal_local_to_world(vertex.normal, mesh_world_from_local);

    out.world_position = world_from_local * vec4<f32>(vertex.position, 1.0);
    out.position = position_world_to_clip(out.world_position.xyz);
    @if(WORLD_SURFACE)
    out.world_position.w = -position_world_to_view(out.world_position.xyz).z;

    @if(VERTEX_UVS_A) {
        @if(!WORLD_SURFACE || CLAMP_COLOR) out.uv = vertex.uv;
        @else out.uv = vertex.uv * vertex_material.uv_scales.xy + uv_offsets.xy;
    }
    @if(VERTEX_UVS_B) {
        @if(!WORLD_SURFACE || CLAMP_COLOR) out.uv_b = vertex.uv_b;
        @else out.uv_b = vertex.uv_b * vertex_material.uv_scales.zw + uv_offsets.zw;
    }

    @if(VERTEX_COLORS)
    out.color = vertex.color;

    @if(VERTEX_NORMALS && !WORLD_SURFACE) {
        if field_lighting && (out.channels & 1u) != 0u {
            let material = surface_data(material_slot);
            let field_light = material.field_light;
            let direction = normalize(field_light.xyz - out.world_position.xyz);
            let diffuse = dot(out.world_normal, direction);
            // Signed diffuse light plus constant ambient select a texel in the
            // cooked toon ramp, with eight-bit rounding before the lookup.
            let illumination = clamp(field_light.w + round(field_light.w * diffuse), 0.0, 255.0);
            let coordinate = floor(field_light.w * (illumination + floor(illumination / 128.0)) / 256.0) / 255.0;
            @if(VERTEX_COLORS)
            if (out.channels & 8u) != 0u {
                out.color = vec4<f32>(floor(round(vertex.color.rgb * 255.0)
                    * material.ambient_color.rgb / 256.0) / 255.0, vertex.color.a);
            }
            out.world_normal = vec3<f32>(coordinate, 16.0 / 255.0, 0.0);
        }
    }

    @if(VERTEX_NORMALS && !WORLD_SURFACE) {
        let lighting = surface_data(material_slot).scenery_lighting;
        if !field_lighting && (out.channels & 1u) != 0u && lighting.ambient.w != 0.0 {
            var illumination = lighting.ambient.rgb;
            for (var index = 0u; index < 2u; index++) {
                let light = lighting.positions[index];
                let delta = light.xyz - out.world_position.xyz;
                let distance = length(delta);
                let diffuse = max(dot(out.world_normal, delta / max(distance, 0.0001)), 0.0);
                let attenuation = dot(lighting.attenuation.xyz, vec3<f32>(1.0, distance, distance * distance));
                illumination += lighting.color.rgb * (light.w * diffuse / max(attenuation, 0.0001));
            }
            out.world_normal = clamp(illumination, vec3<f32>(0.0), vec3<f32>(1.0));
        }
    }

    @if(VERTEX_ALPHA && VERTEX_COLORS) {
        let particle_material = surface_data(material_slot);
        if (out.channels & 8u) != 0u {
            let particle_tint = vec4<f32>(particle_material.ambient_color.rgb, round(vertex_tint.a * 255.0));
            out.color = floor(round(out.color * 255.0) * particle_tint / 256.0) / 255.0;
        }
    }

    return out;
}
