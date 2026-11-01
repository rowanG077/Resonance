//! Compose the actual surface shader and Bevy imports without a render device.
use crate::test_support::validate_shader;
use bevy::{
    pbr::{MAX_CASCADES_PER_LIGHT, MAX_DIRECTIONAL_LIGHTS, MAX_RECT_LIGHTS},
    prelude::*,
    shader::{ShaderCache, ShaderCacheError, ShaderDefVal},
};
use std::{fs, path::Path, process::Command};

#[test]
fn surface_shaders_validate_for_field_and_effect_materials() -> anyhow::Result<()> {
    // Resolve the locked dependency versions, including patched/local Bevy
    // checkouts, instead of copying shader declarations into this test.
    // Fresh CI runners only cache dependencies for their own platform. Without
    // this filter, offline metadata also tries to download other targets' crates.
    let platform = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .args(["--print", "host-tuple"])
        .output()?;
    anyhow::ensure!(
        platform.status.success(),
        "rustc --print host-tuple failed: {}",
        String::from_utf8_lossy(&platform.stderr)
    );
    let platform = String::from_utf8(platform.stdout)?;
    let metadata = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--offline",
            "--locked",
            "--format-version",
            "1",
            "--filter-platform",
            platform.trim(),
        ])
        .output()?;
    anyhow::ensure!(
        metadata.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&metadata.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&metadata.stdout)?;
    let mut sources = Vec::new();
    fn collect(root: &Path, module: &str, sources: &mut Vec<Shader>) -> anyhow::Result<()> {
        for entry in fs::read_dir(root)? {
            let path = entry?.path();
            if path.is_dir() {
                collect(
                    &path,
                    &format!("{module}/{}", path.file_name().unwrap().to_str().unwrap()),
                    sources,
                )?;
            } else if path.extension().is_some_and(|ext| ext == "wesl") {
                sources.push(Shader::from_wesl(
                    fs::read_to_string(&path)?,
                    format!("{module}/{}", path.file_name().unwrap().to_str().unwrap()),
                ));
            }
        }
        Ok(())
    }
    for package in metadata["packages"].as_array().unwrap() {
        if package["name"].as_str().unwrap().starts_with("bevy_") {
            let manifest = Path::new(package["manifest_path"].as_str().unwrap());
            collect(
                &manifest.parent().unwrap().join("src"),
                &format!("embedded://{}", package["name"].as_str().unwrap()),
                &mut sources,
            )?;
        }
    }
    sources.push(Shader::from_wesl(
        include_str!("surface_bindings.wesl"),
        "embedded://resonance_presentation/surface_bindings.wesl",
    ));
    sources.push(Shader::from_wesl(
        include_str!("effect_color.wesl"),
        "embedded://resonance_presentation/effect_color.wesl",
    ));
    sources.push(Shader::from_wesl(
        include_str!("effect_depth.wesl"),
        "embedded://resonance_presentation/effect_depth.wesl",
    ));
    let mut assets = Assets::<Shader>::default();
    let mut cache = ShaderCache::new((), validate_shader);
    for shader in sources {
        let handle = assets.add(shader.clone());
        cache.set_shader(handle.id(), shader);
    }
    let ui = Shader::from_wesl(
        include_str!("field_ui.wesl"),
        "embedded://resonance_presentation/field_ui.wesl",
    );
    let ui_handle = assets.add(ui.clone());
    cache.set_shader(ui_handle.id(), ui);
    for multisampled in [false, true] {
        for effect in [
            None,
            Some("RED_CHANNEL"),
            Some("GLYPH_PATTERN"),
            Some("OPAQUE_IMAGE"),
        ] {
            let mut defs = vec![ShaderDefVal::UInt("MATERIAL_BIND_GROUP".into(), 2)];
            if multisampled {
                defs.push("MULTISAMPLED_UI".into());
            }
            if let Some(effect) = effect {
                defs.push(effect.into());
            }
            cache.get(0, ui_handle.id(), &defs).map_err(|error| {
                anyhow::anyhow!("UI multisampled={multisampled}, effect={effect:?}: {error:?}")
            })?;
        }
    }
    let shaders = [
        ("fragment", include_str!("title_surface.wesl")),
        ("vertex", include_str!("title_surface_vertex.wesl")),
        ("title_output", include_str!("title_output.wesl")),
        ("title_text", include_str!("title_text.wesl")),
        ("title_glow", include_str!("title_glow.wesl")),
        ("field_water", include_str!("field_water.wesl")),
        ("field_dissolve", include_str!("field_dissolve.wesl")),
        ("menu_backdrop", include_str!("menu_backdrop.wesl")),
        ("model_preview", include_str!("model_preview.wesl")),
        ("refraction", include_str!("field_refraction.wesl")),
        ("captured_mesh", include_str!("field_capture.wesl")),
        (
            "battle_scene_sample",
            include_str!("battle_view/scene_sample.wesl"),
        ),
    ]
    .map(|(name, source)| {
        let shader = Shader::from_wesl(
            source,
            format!("embedded://resonance_presentation/{name}.wesl"),
        );
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
                (true, true, true),
            ] {
                let mut defs: Vec<ShaderDefVal> = [
                    ("MATERIAL_BIND_GROUP", 3),
                    ("MAX_DIRECTIONAL_LIGHTS", MAX_DIRECTIONAL_LIGHTS as u32),
                    ("MAX_CASCADES_PER_LIGHT", MAX_CASCADES_PER_LIGHT as u32),
                    ("MAX_RECT_LIGHTS", MAX_RECT_LIGHTS as u32),
                    ("AVAILABLE_STORAGE_BUFFER_BINDINGS", 8),
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
                    if *name == "fragment" {
                        for flags in [
                            vec!["CONSTANT_COLOR"],
                            vec!["RED_CHANNEL"],
                            vec!["MULTIPLY_ALPHA_ONLY", "RED_CHANNEL"],
                            vec!["CLIP_BELOW_FLOOR"],
                        ] {
                            let mut effect = defs.clone();
                            effect.extend(flags.iter().copied().map(ShaderDefVal::from));
                            cache.get(0, handle.id(), &effect).map_err(|error| {
                                anyhow::anyhow!("surface effect {flags:?}: {error:?}")
                            })?;
                        }
                    }
                    if *name == "vertex" {
                        for flags in [
                            vec!["SKINNED"],
                            vec!["MORPH_TARGETS"],
                            vec!["SKINNED", "MORPH_TARGETS"],
                        ] {
                            let mut animated = defs.clone();
                            animated.extend(flags.iter().copied().map(ShaderDefVal::from));
                            cache.get(0, handle.id(), &animated).map_err(|error| {
                                anyhow::anyhow!("animated vertex {flags:?}: {error:?}")
                            })?;
                        }
                    }
                    if matches!(*name, "refraction" | "captured_mesh") {
                        let mut multisampled = defs.clone();
                        multisampled.push("MULTISAMPLED_DEPTH".into());
                        cache
                            .get(0, handle.id(), &multisampled)
                            .map_err(|error| anyhow::anyhow!("multisampled {name}: {error:?}"))?;
                    }
                    if let Err(error) = cache.get(0, handle.id(), &defs) {
                        let message = match error {
                            ShaderCacheError::ProcessShaderError(error) => error,
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
