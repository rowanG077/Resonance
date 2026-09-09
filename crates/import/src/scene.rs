use crate::read::{f32 as f32_at, u32 as u32_at};
use crate::{animation, digest, geometry, glow, write_atomic};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    CameraKey, SceneClip, SceneMaterial, ScenePart, TextureBinding, TextureWrap, TitleGlow,
    TitleScene,
};
use std::{
    fs,
    io::{Cursor, Read},
    path::Path,
};

fn vector(bytes: &[u8], offset: usize) -> Result<[f32; 3]> {
    Ok([
        f32_at(bytes, offset)?,
        f32_at(bytes, offset + 4)?,
        f32_at(bytes, offset + 8)?,
    ])
}
fn section(map: &[u8], index: usize) -> Result<&[u8]> {
    let count = u32_at(map, 0)? as usize;
    ensure!(count <= 256 && index < count, "invalid map section count");
    let start = u32_at(map, 4 + index * 4)? as usize;
    ensure!(start >= 4 + count * 4, "missing map section {index}");
    let mut end = map.len();
    for i in index + 1..count {
        let next = u32_at(map, 4 + i * 4)? as usize;
        if next != 0 {
            end = next;
            break;
        }
    }
    map.get(start..end).context("invalid map section range")
}
fn camera(bytes: &[u8]) -> Result<Vec<CameraKey>> {
    ensure!(bytes.get(..4) == Some(b"CAMM"), "expected camera track");
    let count = u32_at(bytes, 8)? as usize;
    let optional = u32_at(bytes, 4)? as usize;
    ensure!(
        count > 1 && count < 10000 && optional != 0,
        "invalid camera track"
    );
    (0..count)
        .map(|i| {
            let at = 12 + i * 36;
            let target = optional + 12 + i * 20;
            let time = f32_at(bytes, at)?;
            ensure!(
                f32_at(bytes, target)? == time,
                "camera tracks have different times"
            );
            Ok(CameraKey {
                time,
                position: vector(bytes, at + 4)?,
                target: vector(bytes, target + 4)?,
            })
        })
        .collect()
}

pub fn cook(source: &Path, executable: &Path, output: &Path, ktx: &Path) -> Result<TitleScene> {
    let bytes = fs::read(source)?;
    let dol = fs::read(executable)?;
    let mut cabinet = cab::Cabinet::new(Cursor::new(&bytes))?;
    let mut map = Vec::new();
    cabinet
        .read_file("TIT_T00.BIN")?
        .take(64 * 1024 * 1024)
        .read_to_end(&mut map)?;
    let mut parts = Vec::new();
    let mut feather = None;
    let mut reflection = None;
    let mut landing = None;
    let mut landing_loop = None;
    for index in [0, 2, 17, 18, 20, 21, 22] {
        let (part, gltf, binary) = cook_part(
            PartSource {
                name: &format!("title-scene/{index:02}"),
                source: section(&map, index)?,
                resource: index as u16,
                draw_order: [0, 20, 21, 17, 18, 22, 2]
                    .iter()
                    .position(|p| *p == index)
                    .unwrap() as u32,
                depth_write: index != 2,
                translation: [0., 0., if index == 17 { 5. } else { 0. }],
                autoplay: if matches!(index, 0 | 2) {
                    Some(section(&map, index + 1)?)
                } else {
                    None
                },
                animation_slots: if [17, 18, 21].contains(&index) {
                    &[12, 36]
                } else if [20, 22].contains(&index) {
                    &[12]
                } else {
                    &[]
                },
                clip_prefix: &format!("title-{index}"),
                extra_clips: &[],
                texture_animations: if index == 2 {
                    crate::texture_animation::cook(&dol)?
                } else {
                    Vec::new()
                },
            },
            output,
            ktx,
        )?;
        if index == 17 {
            feather = Some(glow::positions(
                &gltf,
                &binary,
                "Fz_Bone01",
                glam::Vec3::Z * 5.,
                0,
                730,
            )?);
            let points = glow::positions(&gltf, &binary, "Dummy", glam::Vec3::Z * 5., 0, 730)?;
            ensure!(
                points.iter().all(|p| *p == points[0]),
                "landing attachment unexpectedly moves"
            );
            landing = Some(points[0]);
            landing_loop = Some(glow::positions(
                &gltf,
                &binary,
                "Dummy",
                glam::Vec3::Z * 5.,
                1,
                360,
            )?);
        }
        if index == 18 {
            reflection = Some(glow::positions(
                &gltf,
                &binary,
                "Rf_Fez_Ref_120",
                glam::Vec3::ZERO,
                0,
                730,
            )?);
        }
        parts.push(part);
    }
    let (texture, source_sha256) = glow::texture(
        &source
            .parent()
            .context("map directory")?
            .parent()
            .context("files directory")?
            .join("effect.cab"),
        output,
        ktx,
    )?;
    let script_bytes = section(&map, 6)?;
    symphonia_script::Program::decode(script_bytes)?;
    let script_path = "title/events.ssb";
    write_atomic(&output.join(script_path), script_bytes)?;
    let (listing, _) = symphonia_script::scenario::disassemble(script_bytes)?;
    write_atomic(
        &output.join("intermediate/title/events.ssasm"),
        listing.as_bytes(),
    )?;
    Ok(TitleScene {
        script: resonance_content::ScriptAsset {
            path: script_path.into(),
            sha256: digest(script_bytes),
        },
        source_sha256: digest(&bytes),
        code_source_sha256: digest(&dol),
        parts,
        glow: TitleGlow {
            texture,
            source_sha256,
            feather: feather.context("missing feather")?,
            reflection: reflection.context("missing reflection")?,
            landing: landing.context("missing landing")?,
            landing_loop: landing_loop.context("missing landing loop")?,
        },
        cameras: vec![camera(section(&map, 16)?)?, camera(section(&map, 23)?)?],
        fov_degrees: 27.,
    })
}

pub(crate) struct PartSource<'a> {
    pub name: &'a str,
    pub source: &'a [u8],
    pub resource: u16,
    pub draw_order: u32,
    pub depth_write: bool,
    pub translation: [f32; 3],
    pub autoplay: Option<&'a [u8]>,
    pub animation_slots: &'a [usize],
    pub clip_prefix: &'a str,
    pub extra_clips: &'a [SourceClip<'a>],
    pub texture_animations: Vec<resonance_content::TextureAnimation>,
}

pub(crate) struct SourceClip<'a> {
    pub slot: u16,
    pub bytes: &'a [u8],
    pub resource: Option<u32>,
}

/// Compile source geometry and materials into ordinary runtime assets.
/// The extra glTF and vertex data are available for offline attachment baking.
pub(crate) fn cook_part(
    spec: PartSource<'_>,
    output: &Path,
    ktx: &Path,
) -> Result<(ScenePart, serde_json::Value, Vec<u8>)> {
    let name = spec.name;
    let intermediate = output.join("intermediate").join(name);
    let manifest = geometry::export_section(spec.source, &intermediate)?;
    let mut gltf: serde_json::Value =
        serde_json::from_slice(&fs::read(intermediate.join("scene.gltf"))?)?;
    // Keep non-triangle objects in the import diagnostics; never hand
    // zero-length accessors to the runtime loader.
    let empty: Vec<usize> = gltf["meshes"]
        .as_array()
        .context("meshes")?
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            m["primitives"].as_array().unwrap().iter().all(|p| {
                let accessor = p["indices"].as_u64().unwrap() as usize;
                gltf["accessors"][accessor]["count"].as_u64() == Some(0)
            })
        })
        .map(|(i, _)| i)
        .collect();
    let mut unsupported = Vec::new();
    for node in gltf["nodes"].as_array_mut().context("nodes")? {
        if node["mesh"]
            .as_u64()
            .is_some_and(|m| empty.contains(&(m as usize)))
        {
            unsupported.push(node["name"].clone());
            node.as_object_mut().unwrap().remove("mesh");
        } else if let Some(old) = node["mesh"].as_u64() {
            node["mesh"] = serde_json::json!(
                old as usize - empty.iter().filter(|i| **i < old as usize).count()
            );
        }
    }
    let meshes = gltf["meshes"].as_array_mut().context("meshes")?;
    let mut i = 0;
    meshes.retain(|_| {
        let keep = !empty.contains(&i);
        i += 1;
        keep
    });
    if !unsupported.is_empty() {
        eprintln!(
            "Scene part {name}: {} objects have no triangle geometry",
            unsupported.len()
        );
        write_atomic(
            &intermediate.join("unsupported-effects.json"),
            &serde_json::to_vec_pretty(&unsupported)?,
        )?;
    }
    let mut textures = Vec::new();
    for texture in &manifest.textures {
        let path = format!("{name}/texture_{:03}.ktx2", texture.index);
        let destination = output.join(&path);
        fs::create_dir_all(destination.parent().context("texture directory")?)?;
        crate::texture::cook(ktx, &intermediate.join(&texture.image), &destination)?;
        textures.push(path);
    }
    // Compile texture/vertex-color combination modes into material recipes.
    let templates = gltf["materials"]
        .as_array()
        .context("materials array")?
        .clone();
    let mut materials = Vec::new();
    // Draw backdrop, actors in creation order, then lights.
    let part_order = spec.draw_order;
    let mut cooked_materials = Vec::new();
    for object in manifest
        .objects
        .iter()
        .filter(|o| !empty.contains(&o.index))
    {
        let mode = object
            .tev_modes
            .first()
            .copied()
            .context("missing scene material mode")?;
        ensure!(
            [1, 4, 5, 0x11].contains(&mode),
            "unsupported scene material mode {mode:#x}"
        );
        let binding = |stage: usize| -> Result<TextureBinding> {
            let command = *object
                .texture_commands
                .get(stage)
                .context("missing material texture stage")?;
            ensure!(
                ((command >> 13) & 7) as usize == stage,
                "unexpected texture stage"
            );
            let wrap = |v| -> Result<TextureWrap> {
                match v {
                    0 => Ok(TextureWrap::Clamp),
                    1 => Ok(TextureWrap::Repeat),
                    2 => Ok(TextureWrap::Mirror),
                    _ => anyhow::bail!("invalid texture wrap {v}"),
                }
            };
            let min = (command >> 24) & 15;
            let mag = command >> 28;
            ensure!(min <= 5 && mag <= 1, "unsupported texture filter");
            Ok(TextureBinding {
                texture: (command & 0x1fff) as usize,
                wrap_u: wrap((command >> 16) & 15)?,
                wrap_v: wrap((command >> 20) & 15)?,
                nearest_min: [0, 2, 4].contains(&min),
                nearest_mag: mag == 0,
            })
        };
        let mut material = templates
            .get(object.material)
            .context("missing material template")?
            .clone();
        let color = if mode == 5 { None } else { Some(binding(0)?) };
        let multiply = if mode == 0x11 {
            Some(binding(1)?)
        } else {
            None
        };
        let blend = material["alphaMode"] == "BLEND" || multiply.is_some();
        let mesh_index = object.index - empty.iter().filter(|i| **i < object.index).count();
        for primitive in gltf["meshes"][mesh_index]["primitives"]
            .as_array_mut()
            .context("primitives")?
        {
            primitive["material"] = serde_json::json!(materials.len());
            if mode == 4 {
                primitive["attributes"]
                    .as_object_mut()
                    .unwrap()
                    .remove("COLOR_0");
            }
            ensure!(
                multiply.is_none() || primitive["attributes"].get("TEXCOORD_1").is_some(),
                "multiply material needs secondary UVs"
            );
        }
        material["name"] = serde_json::json!(object.name);
        material["pbrMetallicRoughness"]
            .as_object_mut()
            .context("material")?
            .remove("baseColorTexture");
        material["extensions"] = serde_json::json!({"KHR_materials_unlit": {}});
        material.as_object_mut().unwrap().remove("extras");
        cooked_materials.push(material);
        ensure!(
            object.draw_order < 65536,
            "too many authored draws in scene part"
        );
        materials.push(SceneMaterial {
            color,
            multiply,
            blend,
            depth_write: spec.depth_write,
            cull: resonance_content::CullFace::Back,
            draw_order: part_order * 65536 + object.draw_order,
        });
    }
    gltf["materials"] = serde_json::json!(cooked_materials);
    gltf["extensionsUsed"] = serde_json::json!(["KHR_materials_unlit"]);
    for key in ["images", "textures", "samplers", "extras"] {
        gltf.as_object_mut().unwrap().remove(key);
    }
    gltf["buffers"][0]
        .as_object_mut()
        .context("buffer")?
        .remove("uri");
    let mesh = format!("{name}/scene.glb");
    let mut binary = fs::read(intermediate.join("scene.bin"))?;
    let mut clips = Vec::new();
    let autoplay = spec.autoplay.is_some();
    if let Some(animation) = spec.autoplay {
        // Map sections 1 and 3 hold backdrop and light groups.
        let source = spec.source;
        let model = manifest.model_offset.context("field group has no model")?;
        write_atomic(&intermediate.join("default-animation.bin"), animation)?;
        let duration_seconds = animation::bake_with_model(
            animation,
            &source[model..],
            &mut gltf,
            &mut binary,
            &format!("field-{}", spec.resource),
        )?;
        clips.push(SceneClip {
            resource_slot: 0,
            duration_seconds,
            animation_resource: None,
            secondary_pose_nodes: Vec::new(),
        });
    }
    if !spec.animation_slots.is_empty() {
        let source = spec.source;
        let model = manifest
            .model_offset
            .context("animated title part has no model")?;
        let schedule = spec.animation_slots;
        for (clip, slot) in schedule.iter().enumerate() {
            let offset = u32_at(source, *slot)? as usize;
            let duration_seconds = animation::bake(
                source,
                offset,
                model,
                &mut gltf,
                &mut binary,
                &format!("{}-{clip}", spec.clip_prefix),
            )?;
            clips.push(SceneClip {
                resource_slot: *slot as u16,
                duration_seconds,
                animation_resource: None,
                secondary_pose_nodes: Vec::new(),
            });
        }
    }
    for clip in spec.extra_clips {
        let model = manifest
            .model_offset
            .context("animated part has no model")?;
        let duration_seconds = animation::bake_with_model(
            clip.bytes,
            &spec.source[model..],
            &mut gltf,
            &mut binary,
            &format!("{}-{}", spec.clip_prefix, clip.slot),
        )?;
        clips.push(SceneClip {
            resource_slot: clip.slot,
            duration_seconds,
            animation_resource: clip.resource,
            secondary_pose_nodes: serde_json::from_value(
                gltf["animations"]
                    .as_array()
                    .context("cooked animations")?
                    .last()
                    .context("cooked clip")?["extras"]["secondary_pose_nodes"]
                    .clone(),
            )?,
        });
    }
    let mut json = serde_json::to_vec(&gltf)?;
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    while binary.len() % 4 != 0 {
        binary.push(0);
    }
    let length = 12 + 8 + json.len() + 8 + binary.len();
    let mut glb = Vec::with_capacity(length);
    for v in [
        0x46546C67u32,
        2,
        length as u32,
        json.len() as u32,
        0x4E4F534A,
    ] {
        glb.extend(v.to_le_bytes());
    }
    glb.extend(json);
    for v in [binary.len() as u32, 0x004E4942] {
        glb.extend(v.to_le_bytes());
    }
    glb.extend(&binary);
    write_atomic(&output.join(&mesh), &glb)?;
    let part = ScenePart {
        resource: spec.resource,
        mesh,
        textures,
        materials,
        appearance: None,
        outline_color: None,
        secondary_motion: Vec::new(),
        clips,
        autoplay,
        texture_animations: spec.texture_animations,
        translation: spec.translation,
        bone_names: manifest
            .model_nodes
            .iter()
            .map(|n| n.name.clone())
            .collect(),
        material_nodes: manifest
            .objects
            .iter()
            .filter(|object| !empty.contains(&object.index))
            .map(|object| {
                manifest
                    .model_nodes
                    .iter()
                    .filter(|node| node.object_index == object.source_index)
                    .map(|node| node.index as u16)
                    .collect()
            })
            .collect(),
    };
    Ok((part, gltf, binary))
}
