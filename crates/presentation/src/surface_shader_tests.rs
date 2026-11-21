//! Compose the native frame's shaders and their Bevy imports without a render device.
use crate::test_support::validate_shader;
use bevy::{
    prelude::*,
    shader::{ShaderCache, ShaderCacheError, ShaderDefVal},
};
use std::{fs, path::Path, process::Command};

#[test]
fn native_frame_shaders_validate() -> anyhow::Result<()> {
    // Resolve the locked dependency versions, including patched/local Bevy
    // checkouts, instead of copying shader declarations into this test.
    // CI runs `cargo fetch --locked` first: metadata can need manifests beyond
    // the packages/features compiled by these tests. Filter the resolved graph
    // to the native platform while keeping shader validation itself offline.
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
        include_str!("surface_vertex.wesl"),
        "embedded://resonance_presentation/surface_vertex.wesl",
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
    let mut add = |path: &str, source: &'static str| {
        let shader = Shader::from_wesl(source, format!("embedded://resonance_presentation/{path}"));
        let handle = assets.add(shader.clone());
        cache.set_shader(handle.id(), shader);
        handle
    };
    add("native_frame.wesl", include_str!("native_frame.wesl"));
    let ui = add("field_ui.wesl", include_str!("field_ui.wesl"));
    let vertex = add(
        "native_surface_vertex.wesl",
        include_str!("native_surface_vertex.wesl"),
    );
    let fragment = add("title_surface.wesl", include_str!("title_surface.wesl"));
    let flat = add("native_flat.wesl", include_str!("native_flat.wesl"));
    let effects = [
        ("field_dissolve", include_str!("field_dissolve.wesl")),
        ("field_refraction", include_str!("field_refraction.wesl")),
        (
            "native_scene_sample",
            include_str!("native_scene_sample.wesl"),
        ),
    ]
    .map(|(name, source)| (name, add(&format!("{name}.wesl"), source)));
    // Every pipeline of the native frame adds this, as `native::render::defs`.
    let native = |names: &[&'static str]| {
        let mut defs: Vec<ShaderDefVal> = names.iter().copied().map(ShaderDefVal::from).collect();
        defs.push(ShaderDefVal::UInt("MATERIAL_BIND_GROUP".into(), 1));
        defs
    };
    for interpolation in [
        &[][..],
        &["CLAMP_UI_ATLAS"][..],
        &["CLAMP_UI_ATLAS", "CENTROID_UI"][..],
        &["MULTISAMPLED_UI"][..],
    ] {
        for effect in [
            None,
            Some("RED_CHANNEL"),
            Some("GLYPH_PATTERN"),
            Some("OPAQUE_IMAGE"),
        ] {
            let mut defs = native(interpolation);
            defs.extend(effect.map(ShaderDefVal::from));
            cache.get(0, ui.id(), &defs).map_err(|error| {
                anyhow::anyhow!("UI interpolation={interpolation:?}, effect={effect:?}: {error:?}")
            })?;
        }
    }
    for immediate in [false, true] {
        for (world, fog, skinned, layout) in [
            (false, false, false, false),
            (false, true, true, true),
            (false, false, false, true),
            (true, false, false, false),
            (true, true, false, false),
        ] {
            for material in [
                &[][..],
                &["CONSTANT_COLOR"][..],
                &["MULTIPLY_ALPHA_ONLY"][..],
                &["CLAMP_COLOR"][..],
                &["CLAMP_COLOR", "VERTEX_ALPHA"][..],
            ] {
                let mut defs = native(&[
                    "VERTEX_NORMALS",
                    "VERTEX_UVS_A",
                    "VERTEX_UVS_B",
                    "VERTEX_COLORS",
                ]);
                defs.extend(material.iter().copied().map(ShaderDefVal::from));
                for (name, enabled) in [
                    ("WORLD_SURFACE", world),
                    ("DISTANCE_FOG", fog),
                    ("SKINNED", skinned),
                    ("SURFACE_LAYOUT", layout),
                    ("NATIVE_IMMEDIATE_SLOT", immediate),
                ] {
                    if enabled {
                        defs.push(name.into());
                    }
                }
                for (stage, handle) in [("vertex", &vertex), ("fragment", &fragment)] {
                    cache.get(0, handle.id(), &defs).map_err(|error| {
                        anyhow::anyhow!(
                            "surface {stage}: immediate={immediate}, world={world}, fog={fog}, \
                             skinned={skinned}, layout={layout}, material={material:?}: {error:?}"
                        )
                    })?;
                }
            }
        }
    }
    // The flat quads: output, text, menu backdrop, model preview composite,
    // glow, water and blits, with and without vertex colors.
    for colors in [&[][..], &["VERTEX_COLORS"][..]] {
        cache
            .get(0, flat.id(), &native(colors))
            .map_err(|error| anyhow::anyhow!("flat quads {colors:?}: {error:?}"))?;
    }
    // Scene effects, single- and multisampled.
    for (name, handle) in &effects {
        for depth in [&[][..], &["MULTISAMPLED_DEPTH"][..]] {
            for masked in [false, true] {
                let mut defs: Vec<ShaderDefVal> =
                    depth.iter().copied().map(ShaderDefVal::from).collect();
                if *name == "field_refraction" {
                    defs.push("NATIVE_SCENE".into());
                }
                if *name == "native_scene_sample" {
                    defs.extend(
                        ["NATIVE_SCENE", "DEPTH_TEST", "VERTEX_COLORS"].map(ShaderDefVal::from),
                    );
                    if masked {
                        defs.push("MASKED_SCENE".into());
                    }
                }
                if let Err(error) = cache.get(0, handle.id(), &defs) {
                    let message = match error {
                        ShaderCacheError::ProcessShaderError(error) => error,
                        error => error.to_string(),
                    };
                    anyhow::bail!("{name} {depth:?}: {message}");
                }
            }
        }
    }
    Ok(())
}
