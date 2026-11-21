// Frame-level bindings of the native display-list renderer. One uniform/storage
// blob is uploaded per frame; draws select their record with the instance index.
struct NativeView {
    clip_from_world: mat4x4<f32>,
    view_from_world: mat4x4<f32>,
};
struct NativeFog {
    base_color: vec4<f32>,
    be: vec3<f32>,
};
struct NativeDraw {
    // Affine world_from_local, transposed to three rows like Bevy's mesh record.
    world_from_local: mat3x4<f32>,
    // Material record row, first joint matrix, mesh tag bits, reserved.
    ids: vec4<u32>,
    // Material-specific constants: output brightness, text pulse.
    params: vec4<f32>,
};
@group(0) @binding(0) var<uniform> view: NativeView;
@group(0) @binding(1) var<uniform> fog: NativeFog;
@group(0) @binding(2) var<storage, read> draws: array<NativeDraw>;
@group(0) @binding(3) var<storage, read> joints: array<mat4x4<f32>>;
// The draw's material record row, set per draw when the device has
// immediates. Read from here instead of a flat varying, it is uniform across
// the draw, so the GPU compiler can evaluate the material's loads and uniform
// math once per draw instead of once per fragment.
struct NativeImmediate {
    slot: u32,
};
@if(NATIVE_IMMEDIATE_SLOT)
var<immediate> native_immediate: NativeImmediate;

fn get_world_from_local(instance: u32) -> mat4x4<f32> {
    let affine = draws[instance].world_from_local;
    return transpose(mat4x4<f32>(affine[0], affine[1], affine[2], vec4<f32>(0.0, 0.0, 0.0, 1.0)));
}
fn position_world_to_clip(world: vec3<f32>) -> vec4<f32> {
    let clip = view.clip_from_world * vec4(world, 1.0);
    return clip;
}
fn position_world_to_view(world: vec3<f32>) -> vec3<f32> {
    let position = view.view_from_world * vec4(world, 1.0);
    return position.xyz;
}
fn mesh2d_position_local_to_world(world_from_local: mat4x4<f32>, position: vec4<f32>) -> vec4<f32> {
    return world_from_local * position;
}
fn mesh2d_position_world_to_clip(position: vec4<f32>) -> vec4<f32> {
    return view.clip_from_world * position;
}
