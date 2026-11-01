// Adapted from Bevy 0.20.1's mesh.wesl, copyright the Bevy contributors,
// licensed under MIT OR Apache-2.0. Standard skinning/morph processing remains
// unchanged; field surfaces additionally interpolate the authored light ramp.
import bevy_pbr::render::{
    mesh_bindings::mesh,
    mesh_functions,
    skinning,
    morph::{morph_position, morph_normal, morph_tangent},
    forward_io::{Vertex, UncompressedVertex, VertexOutput, decompress_vertex},
    view_transformations::position_world_to_clip,
};
import resonance_presentation::surface_bindings::surface_data;

// The instance_index parameter must match vertex_in.instance_index. This is a work around for a wgpu dx12 bug.
// See https://github.com/gfx-rs/naga/issues/2416
@if(MORPH_TARGETS)
fn morph_vertex(vertex_in: UncompressedVertex, instance_index: u32) -> UncompressedVertex {
    var vertex = vertex_in;
    let first_vertex = mesh[instance_index].first_vertex_index;
    let vertex_index = vertex.index - first_vertex;
    let morph_descriptor_index = mesh[instance_index].morph_descriptor_index;

    let weight_count = bevy_pbr::render::morph::layer_count(morph_descriptor_index);
    for (var i: u32 = 0u; i < weight_count; i ++) {
        let weight = bevy_pbr::render::morph::weight_at(i, morph_descriptor_index);
        if weight == 0.0 {
            continue;
        }
        vertex.position += weight * morph_position(vertex_index, i, morph_descriptor_index);
        @if(VERTEX_NORMALS)
        vertex.normal += weight * morph_normal(vertex_index, i, morph_descriptor_index);
        @if(VERTEX_TANGENTS)
        vertex.tangent += vec4(weight * morph_tangent(vertex_index, i, morph_descriptor_index), 0.0);
    }
    return vertex;
}

@vertex
fn vertex(vertex_no_morph: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let uncompressed_vertex_no_morph = decompress_vertex(vertex_no_morph, vertex_no_morph.instance_index);

    @if(MORPH_TARGETS)
    var vertex = morph_vertex(uncompressed_vertex_no_morph, vertex_no_morph.instance_index);
    @else
    var vertex = uncompressed_vertex_no_morph;

    let mesh_world_from_local = mesh_functions::get_world_from_local(vertex_no_morph.instance_index);

    // Use vertex_no_morph.instance_index instead of vertex.instance_index to work around a wgpu dx12 bug.
    // See https://github.com/gfx-rs/naga/issues/2416 .
    @if(SKINNED)
    var world_from_local = skinning::skin_model(
        vertex.joint_indices,
        vertex.joint_weights,
        vertex_no_morph.instance_index
    );
    @else
    var world_from_local = mesh_world_from_local;

    @if(VERTEX_NORMALS && SKINNED)
    out.world_normal = skinning::skin_normals(world_from_local, vertex.normal);
    @if(VERTEX_NORMALS && !(SKINNED))
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        // Use vertex_no_morph.instance_index instead of vertex.instance_index to work around a wgpu dx12 bug.
        // See https://github.com/gfx-rs/naga/issues/2416
        vertex_no_morph.instance_index
    );

    @if(VERTEX_POSITIONS) {
        out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
        out.position = position_world_to_clip(out.world_position.xyz);
    }

    @if(VERTEX_UVS_A)
    out.uv = vertex.uv;
    @if(VERTEX_UVS_B)
    out.uv_b = vertex.uv_b;

    @if(VERTEX_TANGENTS)
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        vertex.tangent,
        // Use vertex_no_morph.instance_index instead of vertex.instance_index to work around a wgpu dx12 bug.
        // See https://github.com/gfx-rs/naga/issues/2416
        vertex_no_morph.instance_index
    );

    @if(VERTEX_COLORS)
    out.color = vertex.color;

    @if(FIELD_LIGHTING && VERTEX_NORMALS) {
        @if(BINDLESS)
        let slot = mesh[vertex_no_morph.instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
        @else
        let slot = 0u;
        let material = surface_data(slot);
        let field_light = material.field_light;
        let direction = normalize(field_light.xyz - out.world_position.xyz);
        let diffuse = dot(out.world_normal, direction);
        // Signed diffuse light plus constant ambient select a texel in the
        // cooked toon ramp, with eight-bit rounding before the lookup.
        let illumination = clamp(field_light.w + round(field_light.w * diffuse), 0.0, 255.0);
        let coordinate = floor(field_light.w * (illumination + floor(illumination / 128.0)) / 256.0) / 255.0;
        @if(VERTEX_COLORS)
        out.color = vec4<f32>(floor(round(vertex.color.rgb * 255.0)
            * material.ambient_color.rgb / 256.0) / 255.0, vertex.color.a);
        // This unlit material has no later use for the normal varying. Reusing it
        // keeps the vertex layout identical for skinned and ordinary scene meshes.
        out.world_normal = vec3<f32>(coordinate, 16.0 / 255.0, 0.0);
    }

    @if(!FIELD_LIGHTING && VERTEX_NORMALS) {
        @if(BINDLESS)
        let scenery_slot = mesh[vertex_no_morph.instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
        @else
        let scenery_slot = 0u;
        let lighting = surface_data(scenery_slot).scenery_lighting;
        if lighting.ambient.w != 0.0 {
            var illumination = lighting.ambient.rgb;
            for (var index = 0u; index < 2u; index++) {
                let light = lighting.positions[index];
                let delta = light.xyz - out.world_position.xyz;
                let distance = length(delta);
                let diffuse = max(dot(out.world_normal, delta / max(distance, 0.0001)), 0.0);
                let attenuation = dot(lighting.attenuation.xyz, vec3<f32>(1.0, distance, distance * distance));
                illumination += lighting.color.rgb * (light.w * diffuse / max(attenuation, 0.0001));
            }
            // Unlit scenery has no later use for its normal. Interpolate the
            // vertex illumination through that varying without another layout.
            out.world_normal = clamp(illumination, vec3<f32>(0.0), vec3<f32>(1.0));
        }
    }

    // Use vertex_no_morph.instance_index instead of vertex.instance_index to work around a wgpu dx12 bug.
    // See https://github.com/gfx-rs/naga/issues/2416
    @if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_no_morph.instance_index;

    @if(VERTEX_ALPHA && VERTEX_COLORS) {
        @if(BINDLESS)
        let particle_slot = mesh[vertex_no_morph.instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
        @else
        let particle_slot = 0u;
        let particle_material = surface_data(particle_slot);
        let opacity = particle_material.tint.a;
        let vertex_tint = vec4<f32>(particle_material.ambient_color.rgb, round(opacity * 255.0));
        out.color = floor(round(out.color * 255.0) * vertex_tint / 256.0) / 255.0;
    }

    @if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex_no_morph.instance_index, mesh_world_from_local[3]);

    return out;
}

@fragment
fn fragment(
    mesh: VertexOutput,
) -> @location(0) vec4<f32> {
    @if(VERTEX_COLORS)
    return mesh.color;
    @else
    return vec4<f32>(1.0, 0.0, 1.0, 1.0);
}
