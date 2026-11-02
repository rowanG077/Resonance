// Bevy 0.20.1 sprite_material.wesl with the DX12 instance-index workaround.
// Source: https://github.com/bevyengine/bevy/blob/v0.20.1/crates/bevy_sprite_render/src/sprite_mesh/sprite_material.wesl
// Workaround: https://github.com/gfx-rs/wgpu/issues/4564
/*
MIT License

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
*/
import package::{
    mesh2d::functions as mesh_functions,
    mesh2d::vertex_output::VertexOutput,
    mesh2d::view_bindings::view,
    mesh2d::vertex_input::{Vertex, decompress_vertex},
    mesh2d::bindings as mesh2d_bindings,
    sprite_mesh::bindings::material,
    sprite_mesh::functions as sprite_functions,
};
@if(BINDLESS)
import package::sprite_mesh::bindings::material_indices;

@if(TONEMAP_IN_SHADER)
import bevy_core_pipeline::tonemapping;
@if(SRGB_OUTPUT)
import bevy_render::color_operations::linear_to_srgb;
@if(OKLAB_OUTPUT)
import bevy_render::color_operations::linear_rgb_to_oklab;

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    // A mutable local prevents Naga's HLSL writer from adding base_instance
    // again at each use of an immutable alias (wgpu issue #4564).
    var instance_index = vertex.instance_index;

@if(BINDLESS)
    let slot = mesh2d_bindings::mesh[instance_index].material_bind_group_slot;
@if(BINDLESS)
    let vertex_scale = material[material_indices[slot].material].vertex_scale;
@else
    let vertex_scale = material.vertex_scale;
@if(BINDLESS)
    let vertex_offset = material[material_indices[slot].material].vertex_offset;
@else
    let vertex_offset = material.vertex_offset;

    var out: VertexOutput;
    let uncompressed_vertex = decompress_vertex(vertex, instance_index);
@if(VERTEX_UVS)
    out.uv = uncompressed_vertex.uv;

@if(VERTEX_POSITIONS)
    var world_from_local = mesh_functions::get_world_from_local(instance_index);
@if(VERTEX_POSITIONS)
    let position = vec4<f32>(uncompressed_vertex.position * vec3<f32>(vertex_scale, 1.0) + vec3<f32>(vertex_offset, 0.0), 1.0);

@if(VERTEX_POSITIONS)
    out.world_position = mesh_functions::mesh2d_position_local_to_world(
        world_from_local,
        position
    );
@if(VERTEX_POSITIONS)
    out.position = mesh_functions::mesh2d_position_world_to_clip(out.world_position);

@if(VERTEX_NORMALS)
    out.world_normal = mesh_functions::mesh2d_normal_local_to_world(uncompressed_vertex.normal, vertex.instance_index);

@if(VERTEX_TANGENTS)
    out.world_tangent = mesh_functions::mesh2d_tangent_local_to_world(
        world_from_local,
        uncompressed_vertex.tangent
    );

@if(VERTEX_COLORS)
    out.color = uncompressed_vertex.color;

    out.instance_index = instance_index;

    return out;
}

@fragment
fn fragment(
    mesh: VertexOutput,
) -> @location(0) vec4<f32> {
    var output_color = sprite_functions::sample_final_color(mesh.uv, mesh.instance_index);

@if(TONEMAP_IN_SHADER)
    output_color = tonemapping::tone_mapping(output_color, view.color_grading);

@if(SRGB_OUTPUT)
    output_color = vec4(linear_to_srgb(output_color.rgb), output_color.a);

@if(OKLAB_OUTPUT)
    output_color = vec4(linear_rgb_to_oklab(output_color.rgb), output_color.a);

    return output_color;
}
