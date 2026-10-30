#define_import_path resonance::effect_depth

#ifdef MULTISAMPLED_DEPTH
@group(0) @binding(4) var scene_depth: texture_depth_multisampled_2d;
#else
@group(0) @binding(4) var scene_depth: texture_depth_2d;
#endif

// Keep fractional visibility at silhouettes instead of treating a single
// covered MSAA sample as a fully opaque pixel. Reverse-Z comparisons retain
// strict Greater tests for ripples/captured meshes and inclusive screen copies.
fn depth_passes(z: f32, depth: f32, comparison: u32) -> f32 {
    if comparison == 0u { return select(0., 1., z > depth); }
    if comparison == 1u { return select(0., 1., z >= depth); }
    return select(0., 1., z <= depth);
}
fn depth_coverage(pixel: vec2<i32>, z: f32, comparison: u32) -> f32 {
#ifdef MULTISAMPLED_DEPTH
    var coverage = 0.;
    let samples = textureNumSamples(scene_depth);
    for (var sample = 0u; sample < samples; sample += 1u) {
        coverage += depth_passes(z, textureLoad(scene_depth, pixel, i32(sample)), comparison);
    }
    return coverage / f32(samples);
#else
    return depth_passes(z, textureLoad(scene_depth, pixel, 0), comparison);
#endif
}
