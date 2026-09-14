// Adapted from Bevy 0.19.1, copyright the Bevy contributors.
// SPDX-License-Identifier: MIT OR Apache-2.0
// Camera-ray culling, bounded GPU batches and per-video-frame accumulation.
enable wgpu_ray_query;

#import bevy_core_pipeline::tonemapping::tonemapping_luminance as luminance
#import bevy_pbr::pbr_functions::calculate_F0
#import bevy_pbr::utils::{rand_f, rand_vec2f}
#import bevy_render::maths::{PI, orthonormalize}
#import bevy_render::view::View
#import bevy_solari::brdf::{evaluate_brdf, evaluate_and_sample_brdf, fresnel}
#import bevy_solari::sampling::{sample_random_light, random_emissive_light_pdf, ggx_vndf_pdf, power_heuristic}
#import bevy_solari::scene_bindings::{trace_ray, resolve_ray_hit_full, ResolvedRayHitFull, geometry_ids, transforms, load_vertices, transform_positions, RAY_T_MIN, RAY_T_MAX, MIRROR_ROUGHNESS_THRESHOLD}

@group(1) @binding(0) var accumulation_texture: texture_storage_2d<rgba32float, read_write>;
@group(1) @binding(1) var view_output: texture_storage_2d<rgba16float, write>;
@group(1) @binding(2) var<uniform> view: View;

@group(1) @binding(3) var<uniform> settings: vec4<u32>;

@compute @workgroup_size(8, 8, 1)
fn pathtrace(@builtin(global_invocation_id) global_id: vec3<u32>) {
    if any(global_id.xy >= vec2u(view.viewport.zw)) {
        return;
    }

    var color = vec3(0.0);
    if settings.x > 0u {
        color = textureLoad(accumulation_texture, global_id.xy).rgb;
    }
    for (var i = 0u; i < settings.y; i++) {
        let sample = trace_pixel(global_id.xy, settings.x + i);
        color = mix(color, sample, 1.0 / f32(settings.x + i + 1u));
    }
    textureStore(accumulation_texture, global_id.xy, vec4(color, f32(settings.x + settings.y)));
    textureStore(view_output, global_id.xy, vec4(color, 1.0));
}

fn trace_pixel(pixel: vec2<u32>, sample_index: u32) -> vec3<f32> {
    // Setup RNG
    let pixel_index = pixel.x + pixel.y * u32(view.viewport.z);
    let frame_index = sample_index * 5782582u;
    var rng = pixel_index + frame_index;

    // Shoot the first ray from the camera
    let pixel_center = vec2<f32>(pixel) + 0.5;
    let jitter = rand_vec2f(&rng) - 0.5;
    let pixel_uv = (pixel_center + jitter) / view.viewport.zw;
    let pixel_ndc = (pixel_uv * 2.0) - 1.0;
    let primary_ray_target = view.world_from_clip * vec4(pixel_ndc.x, -pixel_ndc.y, 1.0, 1.0);
    var ray_origin = view.world_position;
    var ray_direction = normalize((primary_ray_target.xyz / primary_ray_target.w) - ray_origin);
    var ray_t_min = 0.0;

    // Path trace
    var radiance = vec3(0.0);
    var throughput = vec3(1.0);
    var p_bounce = 0.0;
    var bounce = 0u;
    var camera_skipped = 0u;
    loop {
        let ray = trace_ray(ray_origin, ray_direction, ray_t_min, RAY_T_MAX, RAY_FLAG_NONE);
        if bounce == 0u && ray.kind != RAY_QUERY_INTERSECTION_NONE {
            // Only camera rays cull the shell's back faces and light proxies.
            let resolved = resolve_ray_hit_full(ray);
            let vertices = load_vertices(geometry_ids[ray.instance_index], ray.primitive_index);
            let positions = transform_positions(transforms[ray.instance_index], vertices);
            let normal = cross(positions[1] - positions[0], positions[2] - positions[0]);
            if dot(normal, ray_direction) >= 0.0 || any(resolved.material.emissive != vec3(0.0)) {
                camera_skipped += 1u;
                if camera_skipped >= 64u { break; }
                ray_t_min = distance(ray_origin, resolved.world_position) + RAY_T_MIN;
                continue;
            }
        }
        if ray.kind != RAY_QUERY_INTERSECTION_NONE {
            let ray_hit = resolve_ray_hit_full(ray);
            let wo = -ray_direction;

            // Emissive contribution
            var mis_weight = 1.0;
            if p_bounce != 0.0 { // Not first bounce
                let p_light = random_emissive_light_pdf(ray_hit);
                mis_weight = power_heuristic(p_bounce, p_light);
            }
            radiance += mis_weight * throughput * ray_hit.material.emissive;

            // Sample direct lighting, but only if the surface is not mirror-like
            // TODO: randomly choose to use NEE or not with probability proportional to roughness and metallicness
            let is_perfectly_specular = ray_hit.material.roughness <= MIRROR_ROUGHNESS_THRESHOLD && ray_hit.material.metallic > 0.9999;
            if !is_perfectly_specular {
                let direct_lighting = sample_random_light(ray_hit.world_position, ray_hit.world_normal, &rng);

                mis_weight = 1.0;
                if direct_lighting.brdf_rays_can_hit {
                    let pdf_of_bounce = brdf_pdf(wo, direct_lighting.wi, ray_hit);
                    mis_weight = power_heuristic(1.0 / direct_lighting.inverse_pdf, pdf_of_bounce);
                }

                let direct_lighting_brdf = evaluate_brdf(wo, direct_lighting.wi, ray_hit.world_normal, ray_hit.material);
                radiance += mis_weight * throughput * direct_lighting.radiance * direct_lighting.inverse_pdf * direct_lighting_brdf;
            }

            // Sample new ray direction from the material BRDF for next bounce and apply BRDF
            let next_bounce = evaluate_and_sample_brdf(wo, ray_hit.world_normal, ray_hit.material, &rng);
            if next_bounce.pdf == 0.0 { break; }
            ray_direction = next_bounce.wi;
            ray_origin = ray_hit.world_position + (ray_hit.geometric_world_normal * RAY_T_MIN);
            ray_t_min = RAY_T_MIN;
            p_bounce = next_bounce.pdf;
            throughput *= next_bounce.throughput;

            bounce += 1u;
            // Russian roulette keeps the complete multi-bounce estimator while
            // ensuring bright albedos do not produce excessively long paths.
            let p = min(luminance(throughput), 0.95);
            if rand_f(&rng) > p { break; }
            if p <= 0.0 { break; }
            throughput /= p;
        } else { break; }
    }

    // Camera exposure
    radiance *= view.exposure;

    // Reject non-finite arithmetic before it can poison a frozen-frame history.
    return select(vec3(0.0), clamp(radiance, vec3(0.0), vec3(65504.0)), radiance == radiance);
}

fn brdf_pdf(wo: vec3<f32>, wi: vec3<f32>, ray_hit: ResolvedRayHitFull) -> f32 {
    let NdotV = max(dot(ray_hit.world_normal, wo), 0.0001);
    let F0 = calculate_F0(ray_hit.material.base_color, ray_hit.material.metallic, vec3(ray_hit.material.reflectance));
    let df = 1.0 - luminance(fresnel(F0, NdotV));

    let matte = ray_hit.material.reflectance == 0.0 && ray_hit.material.metallic == 0.0;
    let diffuse_weight = select(mix(df, 0.0, ray_hit.material.metallic), 1.0, matte);
    let specular_weight = 1.0 - diffuse_weight;

    let TBN = orthonormalize(ray_hit.world_normal);
    let T = TBN[0];
    let B = TBN[1];
    let N = TBN[2];

    let wo_tangent = vec3(dot(wo, T), dot(wo, B), dot(wo, N));
    let wi_tangent = vec3(dot(wi, T), dot(wi, B), dot(wi, N));

    let diffuse_pdf = wi_tangent.z / PI;
    let specular_pdf = ggx_vndf_pdf(wo_tangent, wi_tangent, ray_hit.material.roughness);
    let pdf = (diffuse_weight * diffuse_pdf) + (specular_weight * specular_pdf);
    return pdf;
}
