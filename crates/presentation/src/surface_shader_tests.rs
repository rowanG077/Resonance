//! Compose the actual surface shader and Bevy imports without a render device.
use bevy::{
    pbr::{MAX_CASCADES_PER_LIGHT, MAX_DIRECTIONAL_LIGHTS, MAX_RECT_LIGHTS},
    prelude::*,
    render::render_resource::{DownlevelFlags, WgpuFeatures},
    shader::{ShaderCache, ShaderCacheError, ShaderDefVal},
};
use std::{fs, path::Path, process::Command};

#[test]
fn surface_shaders_validate_for_field_and_effect_materials() -> anyhow::Result<()> {
    // Resolve the locked dependency versions, including patched/local Bevy
    // checkouts, instead of copying shader declarations into this test.
    let metadata = Command::new(env!("CARGO"))
        .args(["metadata", "--offline", "--locked", "--format-version", "1"])
        .output()?;
    anyhow::ensure!(metadata.status.success(), "cargo metadata failed");
    let metadata: serde_json::Value = serde_json::from_slice(&metadata.stdout)?;
    let mut sources = Vec::new();
    fn collect(root: &Path, sources: &mut Vec<Shader>) -> anyhow::Result<()> {
        for entry in fs::read_dir(root)? {
            let path = entry?.path();
            if path.is_dir() {
                collect(&path, sources)?;
            } else if path.extension().is_some_and(|ext| ext == "wgsl") {
                sources.push(Shader::from_wgsl(
                    fs::read_to_string(&path)?,
                    path.display().to_string(),
                ));
            }
        }
        Ok(())
    }
    for package in metadata["packages"].as_array().unwrap() {
        if package["name"].as_str().unwrap().starts_with("bevy_") {
            let manifest = Path::new(package["manifest_path"].as_str().unwrap());
            collect(&manifest.parent().unwrap().join("src"), &mut sources)?;
        }
    }
    sources.push(Shader::from_wgsl(
        include_str!("surface_bindings.wgsl"),
        "surface_bindings.wgsl",
    ));
    sources.push(Shader::from_wgsl(
        include_str!("effect_color.wgsl"),
        "effect_color.wgsl",
    ));
    let mut assets = Assets::<Shader>::default();
    let mut cache = ShaderCache::new((), WgpuFeatures::all(), DownlevelFlags::all(), |_, _, _| {
        Ok(())
    });
    for shader in sources {
        let handle = assets.add(shader.clone());
        cache.set_shader(handle.id(), shader);
    }
    let shaders = [
        ("fragment", include_str!("title_surface.wgsl")),
        ("vertex", include_str!("title_surface_vertex.wgsl")),
        ("refraction", include_str!("field_refraction.wgsl")),
    ]
    .map(|(name, source)| {
        let shader = Shader::from_wgsl(source, name);
        let handle = assets.add(shader.clone());
        cache.set_shader(handle.id(), shader);
        (name, handle)
    });
    for bindless in [false, true] {
        for fog in [false, true] {
            for (lighting, particles, vertex_alpha) in [
                (false, false, false),
                (true, false, false),
                (false, true, false),
                (false, true, true),
            ] {
                let mut defs: Vec<ShaderDefVal> = [
                    ("MATERIAL_BIND_GROUP", 3),
                    ("MAX_DIRECTIONAL_LIGHTS", MAX_DIRECTIONAL_LIGHTS as u32),
                    ("MAX_CASCADES_PER_LIGHT", MAX_CASCADES_PER_LIGHT as u32),
                    ("MAX_RECT_LIGHTS", MAX_RECT_LIGHTS as u32),
                    ("AVAILABLE_STORAGE_BUFFER_BINDINGS", 8),
                    (
                        "REFRACTION_LIMIT",
                        resonance_content::effect::REFRACTION_LIMIT as u32,
                    ),
                ]
                .map(|(name, value)| ShaderDefVal::UInt(name.into(), value))
                .into();
                defs.extend(
                    [
                        "VERTEX_POSITIONS",
                        "VERTEX_NORMALS",
                        "VERTEX_UVS_A",
                        "VERTEX_UVS_B",
                        "VERTEX_COLORS",
                        "VERTEX_OUTPUT_INSTANCE_INDEX",
                    ]
                    .map(ShaderDefVal::from),
                );
                for (name, enabled) in [
                    ("BINDLESS", bindless),
                    ("DISTANCE_FOG", fog),
                    ("FIELD_LIGHTING", lighting),
                    ("CLAMP_COLOR", particles),
                    ("VERTEX_ALPHA", vertex_alpha),
                ] {
                    if enabled {
                        defs.push(name.into());
                    }
                }
                for (name, handle) in &shaders {
                    if let Err(error) = cache.get(0, handle.id(), &defs) {
                        let message = match error {
                            ShaderCacheError::ProcessShaderError(error) => {
                                error.emit_to_string(&cache.composer)
                            }
                            error => error.to_string(),
                        };
                        anyhow::bail!(
                            "{name}: bindless={bindless}, fog={fog}, lighting={lighting}, particles={particles}, vertex_alpha={vertex_alpha}: {message}"
                        );
                    }
                }
            }
        }
    }
    Ok(())
}
