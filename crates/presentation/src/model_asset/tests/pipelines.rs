//! Compare explicit prewarming with the pipelines selected by real draws.
use bevy::{
    core_pipeline::{core_2d::Transparent2d, core_3d::Transparent3d},
    material::{
        MaterialProperties,
        key::{ErasedMaterialPipelineKey, ErasedMeshPipelineKey},
    },
    mesh::MeshVertexBufferLayoutRef,
    pbr::{
        MeshPipelineKey, PreparedMaterial, RenderMaterialInstances, RenderMeshInstances,
        ViewKeyCache, alpha_mode_pipeline_key,
    },
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems, erased_render_asset::ErasedRenderAssets,
        mesh::RenderMesh, render_asset::RenderAssets, render_phase::ViewSortedRenderPhases,
        render_resource::CachedRenderPipelineId,
    },
    sprite_render::{
        Mesh2dPipelineKey, PreparedMaterial2d, RenderMaterial2dInstances, RenderMesh2dInstances,
        alpha_mode_pipeline_key_2d,
    },
};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

#[derive(Resource, Clone, Default)]
struct Probe(Arc<Mutex<State>>);
#[derive(Default)]
struct State {
    seen: HashSet<(bool, CachedRenderPipelineId)>,
    prepared: HashSet<(bool, CachedRenderPipelineId)>,
    failures: Vec<String>,
}
struct Job {
    flat: bool,
    actual: CachedRenderPipelineId,
    key: ErasedMaterialPipelineKey,
    layout: MeshVertexBufferLayoutRef,
    properties: Arc<MaterialProperties>,
    shared: bool,
    mesh: AssetId<Mesh>,
    material: bevy::asset::UntypedAssetId,
    samples: u32,
    view_bits: u64,
}

pub(crate) fn install(app: &mut App) {
    let probe = Probe::default();
    app.insert_resource(probe.clone());
    app.sub_app_mut(RenderApp)
        .insert_resource(probe)
        .add_systems(Render, observe.in_set(RenderSystems::Cleanup));
}

pub(crate) fn check(world: &World) {
    let state = world.resource::<Probe>().0.lock().unwrap();
    assert!(
        state.failures.is_empty(),
        "explicit pipeline mismatch: {:?}",
        state.failures
    );
    for flat in [false, true] {
        let count = state.seen.iter().filter(|(kind, _)| *kind == flat).count();
        assert!(
            count > 0,
            "no {}D pipelines checked",
            if flat { 2 } else { 3 }
        );
        println!(
            "Explicit specialization matched {count} {}D pipelines",
            if flat { 2 } else { 3 }
        );
        let prepared = state
            .prepared
            .iter()
            .filter(|(kind, _)| *kind == flat)
            .count();
        assert!(prepared > 0, "no cached preparation view checked");
        println!(
            "Cached view specialization matched {prepared} {}D pipelines",
            if flat { 2 } else { 3 }
        );
    }
}

fn observe(world: &mut World) {
    let probe = world.resource::<Probe>().clone();
    let mut state = probe.0.lock().unwrap();
    let mut jobs = Vec::new();
    // Fresh temporary targets can legitimately render one extraction with
    // Bevy's fallback format. Cached field views promise the scene format;
    // the general API comparison below still checks those transient draws.
    let scene_format = crate::renderer::scene_color_format(
        world.get_resource::<bevy::render::renderer::RenderAdapterInfo>(),
    );
    {
        let views = world.resource::<ViewKeyCache>();
        let meshes = world.resource::<RenderAssets<RenderMesh>>();
        let instances = world.resource::<RenderMeshInstances>();
        let bindings = world.resource::<RenderMaterialInstances>();
        let materials = world.resource::<ErasedRenderAssets<PreparedMaterial>>();
        for (view, phase) in &world.resource::<ViewSortedRenderPhases<Transparent3d>>().0 {
            let Some(view) = views.get(view) else {
                continue;
            };
            for item in phase.items.values() {
                if state.seen.contains(&(false, item.pipeline)) {
                    continue;
                }
                let entity = item.entity.1;
                let Some(binding) = bindings.instances.get(&entity) else {
                    continue;
                };
                let Some(instance) = instances.render_mesh_queue_data(entity) else {
                    continue;
                };
                let Some(mesh) = meshes.get(instance.mesh_asset_id()) else {
                    continue;
                };
                let Some(material) = materials.get(binding.asset_id) else {
                    continue;
                };
                let mut material_bits: MeshPipelineKey =
                    material.properties.mesh_pipeline_key_bits.downcast();
                material_bits.insert(alpha_mode_pipeline_key(
                    material.properties.alpha_mode,
                    &Msaa::from_samples(view.msaa_samples()),
                ));
                let mesh_key =
                    *view | MeshPipelineKey::from_bits_retain(mesh.key_bits.bits()) | material_bits;
                state.seen.insert((false, item.pipeline));
                jobs.push(Job {
                    flat: false,
                    shared: binding.asset_id.type_id()
                        == std::any::TypeId::of::<crate::materials::TitleSurface>()
                        && view.contains(MeshPipelineKey::DISTANCE_FOG)
                        && view.target_format() == scene_format,
                    mesh: instance.mesh_asset_id(),
                    material: binding.asset_id,
                    samples: view.msaa_samples(),
                    view_bits: view.bits(),
                    actual: item.pipeline,
                    key: ErasedMaterialPipelineKey {
                        type_id: binding.asset_id.type_id(),
                        mesh_key: ErasedMeshPipelineKey::new(mesh_key),
                        material_key: material.properties.material_key.clone(),
                    },
                    layout: mesh.layout.clone(),
                    properties: material.properties.clone(),
                });
            }
        }
    }
    {
        let views = world.resource::<bevy::sprite_render::ViewKeyCache>();
        let meshes = world.resource::<RenderAssets<RenderMesh>>();
        let instances = world.resource::<RenderMesh2dInstances>();
        let bindings = world.resource::<RenderMaterial2dInstances>();
        let materials = world.resource::<ErasedRenderAssets<PreparedMaterial2d>>();
        for (view, phase) in &world.resource::<ViewSortedRenderPhases<Transparent2d>>().0 {
            let Some(view) = views.get(&view.main_entity) else {
                continue;
            };
            for item in phase.items.values() {
                if state.seen.contains(&(true, item.pipeline)) {
                    continue;
                }
                let entity = item.entity.1;
                let Some(binding) = bindings.get(&entity) else {
                    continue;
                };
                let Some(instance) = instances.get(&entity) else {
                    continue;
                };
                let Some(mesh) = meshes.get(instance.mesh_asset_id) else {
                    continue;
                };
                let Some(material) = materials.get(*binding) else {
                    continue;
                };
                let mut material_bits: Mesh2dPipelineKey =
                    material.properties.mesh_pipeline_key_bits.downcast();
                material_bits.insert(alpha_mode_pipeline_key_2d(material.properties.alpha_mode));
                let mesh_key = *view
                    | Mesh2dPipelineKey::from_bits_retain(mesh.key_bits.bits())
                    | material_bits;
                state.seen.insert((true, item.pipeline));
                jobs.push(Job {
                    flat: true,
                    shared: [
                        std::any::TypeId::of::<crate::field_ui::Surface>(),
                        std::any::TypeId::of::<crate::menu_backdrop::Material>(),
                    ]
                    .contains(&binding.type_id())
                        && view.target_format() == scene_format,
                    mesh: instance.mesh_asset_id,
                    material: *binding,
                    samples: view.msaa_samples(),
                    view_bits: view.bits(),
                    actual: item.pipeline,
                    key: ErasedMaterialPipelineKey {
                        type_id: binding.type_id(),
                        mesh_key: ErasedMeshPipelineKey::new(mesh_key),
                        material_key: material.properties.material_key.clone(),
                    },
                    layout: mesh.layout.clone(),
                    properties: material.properties.clone(),
                });
            }
        }
    }
    for job in jobs {
        if job.shared {
            match crate::gpu_prepare::audit_field_pipeline(
                world,
                job.mesh,
                job.material,
                job.flat,
                job.samples,
            ) {
                Ok(Some(id)) if id == job.actual => {
                    state.prepared.insert((job.flat, id));
                }
                other => state.failures.push(format!(
                    "cached {}D view differs from live view (live={:#x}, cached={}): expected {:?}, got {other:?}",
                    if job.flat { 2 } else { 3 }, job.view_bits, crate::gpu_prepare::audit_field_view(world, job.flat, job.samples), job.actual
                )),
            }
        }
        let Some(specialize) = job.properties.base_specialize else {
            continue;
        };
        match specialize(world, job.key, &job.layout, &job.properties) {
            Ok(expected) if expected == job.actual => {}
            result => state.failures.push(format!(
                "{}D actual={:?}, explicit={result:?}",
                if job.flat { 2 } else { 3 },
                job.actual
            )),
        }
    }
}
