//! Field-only party meshes, read directly from their native resources.
use crate::{
    character_models, compression, dol, field_core, native_archive::read_named, text, world_gpl,
    world_motion, world_texture,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    AppearanceTextures, CullFace, SceneClip, ScenePart,
    field::document::{Model, ModelAlias, ModelTarget},
};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::Path,
};

fn wing_source(exe: &[u8]) -> Result<String> {
    let name = dol::slice(exe, 0x8017_e52c, 16)?;
    let end = name
        .iter()
        .position(|&byte| byte == 0)
        .context("unterminated wing filename")?;
    Ok(std::str::from_utf8(&name[..end])?.to_owned())
}

/// Native wing joint IDs for field-local motion bindings.
pub(crate) fn wing_animation_bindings(extracted: &Path, exe: &[u8]) -> Result<Vec<(u16, String)>> {
    let wings = read_named(&extracted.join("disc1/files"), &wing_source(exe)?)?;
    let body = field_core::optional_section(&wings, 0)?.context("wing body is missing")?;
    world_gpl::animation_bindings(body)
}

fn sheena_source(extracted: &Path, exe: &[u8]) -> Result<Vec<u8>> {
    // Resource 0x0002_010b is member 267 of the second resource group.
    let filename = text::pointer(exe, 0x801f_86c4)?;
    ensure!(
        !filename.contains(['/', '\\']),
        "invalid native group filename"
    );
    let path = extracted
        .join("disc1/files")
        .join(filename.to_ascii_lowercase());
    let mut archive = File::open(&path).with_context(|| format!("open {}", path.display()))?;
    let length = archive.metadata()?.len();
    let mut word = [0; 4];
    archive.read_exact(&mut word)?;
    let count = u32::from_be_bytes(word);
    ensure!(
        count > 267 && 4 + u64::from(count) * 8 <= length,
        "invalid native resource group"
    );
    archive.seek(SeekFrom::Start(4 + 267 * 8))?;
    let mut entry = [0; 8];
    archive.read_exact(&mut entry)?;
    let start = u64::from(u32::from_be_bytes(entry[..4].try_into()?));
    let size = u64::from(u32::from_be_bytes(entry[4..].try_into()?));
    ensure!(
        start >= 4 + u64::from(count) * 8 && size > 0 && start + size <= length,
        "Sheena field model exceeds resource group"
    );
    ensure!(
        size <= 16 * 1024 * 1024,
        "Sheena field model is unexpectedly large"
    );
    archive.seek(SeekFrom::Start(start))?;
    let mut bytes = vec![0; size as usize];
    archive.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn write_mesh(
    library: &Path,
    base: &str,
    package: &[u8],
    outline: bool,
) -> Result<(Vec<u8>, Vec<ScenePart>)> {
    let body = field_core::optional_section(package, 0)?.context("field model has no body")?;
    ensure!(
        dol::u32(body, 0x20)? == 0x005b_bc61,
        "invalid field body model"
    );
    let start = dol::u32(body, 0)? as usize;
    let end = dol::u32(body, 4)? as usize;
    let tpl = body
        .get(start..end)
        .context("field model palette exceeds body")?;
    ensure!(dol::u32(tpl, 0)? == 0x0020_af30, "missing field model TPL");
    let mut paths = Vec::new();
    let mut alpha = Vec::new();
    for index in 0..dol::u32(tpl, 4)? as usize {
        let path = format!("{base}/textures/body-texture-{index}.ktx2");
        let (texture, transparent) = world_texture::tpl_texture(tpl, index)?;
        write(library, &path, &texture)?;
        paths.push(path);
        alpha.push(transparent);
    }
    let (glb, mut part) = world_gpl::static_model(body, paths.clone(), alpha.clone())?;
    part.mesh = format!("{base}/meshes/body.glb");
    write(library, &format!("{base}/meshes/body.glb"), &glb)?;
    let mut parts = vec![part];
    if outline {
        let original =
            field_core::optional_section(package, 1)?.context("field model has no outline")?;
        let normalized = world_gpl::with_primary_palette(body, original)?;
        let (outline_glb, mut part) = world_gpl::static_model(&normalized, paths, alpha)?;
        part.mesh = format!("{base}/meshes/outline.glb");
        write(library, &part.mesh, &outline_glb)?;
        parts.push(part);
    }
    Ok((glb, parts))
}

fn clip(motion: String, slot: u16) -> SceneClip {
    SceneClip {
        motion,
        joints: BTreeMap::new(),
        resource_slot: slot,
        duration_seconds: 0.,
        animation_resource: None,
        pose_chains: Vec::new(),
    }
}

fn wing_model(library: &Path, package: &[u8], body_glb: &[u8], mut part: ScenePart) -> Result<()> {
    let body = field_core::optional_section(package, 0)?.context("wing body is missing")?;
    let bindings = world_gpl::animation_bindings(body)?;
    let base = "meshes/characters/colette/wings";
    let mut unique = BTreeMap::<Vec<u8>, String>::new();
    for index in 2..31 {
        let Some(native) = field_core::optional_section(package, index)? else {
            continue;
        };
        let slot = 4 + index as u16 * 4;
        let path = if let Some(path) = unique.get(native) {
            path.clone()
        } else {
            let expanded = if dol::u32(native, 0)? == 0x007b_7960 {
                native.to_vec()
            } else {
                compression::decode(native)?
            };
            let encoded = world_motion::encode_bound(&expanded, body_glb, &bindings)?;
            let label = if slot == 12 {
                "idle".to_owned()
            } else {
                format!("motion-{slot}")
            };
            let path = format!("{base}/animations/{label}.glb");
            write(library, &path, &encoded)?;
            unique.insert(native.to_vec(), path.clone());
            path
        };
        part.clips.push(clip(path, slot));
    }
    let document = Model {
        parts: vec![part],
        collision: None,
        hidden_geometry: Vec::new(),
    };
    write(
        library,
        &format!("{base}/model.json"),
        &resonance_asset_writer::json::encode(&document)?,
    )
}

fn sheena_model(
    library: &Path,
    package: &[u8],
    body_glb: &[u8],
    mut parts: Vec<ScenePart>,
) -> Result<()> {
    let body = field_core::optional_section(package, 0)?.context("Sheena field body is missing")?;
    let bindings = world_gpl::animation_bindings(body)?;
    let source_names = bindings
        .iter()
        .map(|(_, name)| name.clone())
        .collect::<Vec<_>>();
    let json_len = u32::from_le_bytes(
        body_glb
            .get(12..16)
            .context("short Sheena GLB")?
            .try_into()?,
    ) as usize;
    let body_json: serde_json::Value = serde_json::from_slice(
        body_glb
            .get(20..20 + json_len)
            .context("short Sheena GLB JSON")?,
    )?;
    let body_part = &mut parts[0];
    for material in &mut body_part.materials {
        if material.color.is_none() && material.multiply.is_none() {
            material.blend = false;
        }
    }
    body_part.appearance = Some(AppearanceTextures {
        eyes: body[0x0c].checked_sub(1).map(usize::from),
        mouth: body[0x0d].checked_sub(1).map(usize::from),
        variant: None,
        costume: None,
    });
    body_part.prepare_bones(body_glb)?;
    let dynamics = character_models::character_dynamics::bind(
        "shi00",
        &body_json,
        &source_names,
        &body_part.bone_names,
    )?;
    body_part.secondary_motion = dynamics.clone();
    let clips = [(12, "idle"), (36, "walk"), (40, "run")].map(|(slot, name)| {
        clip(
            format!("meshes/characters/sheena/animations/{name}.glb"),
            slot,
        )
    });
    body_part.clips.extend(clips.iter().cloned());
    let hidden_geometry = body_part
        .bone_names
        .iter()
        .filter(|name| name.starts_with("kk"))
        .cloned()
        .collect();
    let outline = &mut parts[1];
    outline.resource = 1;
    outline.outline_color = Some([0, 0, 0, 127]);
    outline.secondary_motion = dynamics;
    outline.clips.extend(clips);
    for material in &mut outline.materials {
        material.cull = CullFace::Front;
        material.blend = true;
        material.draw_order += 65536;
    }
    let document = Model {
        parts,
        collision: None,
        hidden_geometry,
    };
    write(
        library,
        "meshes/characters/sheena/alternate-field/model.json",
        &resonance_asset_writer::json::encode(&document)?,
    )
}

/// Entries to merge into the shared `meshes/mappings.json` at cutover.
pub fn attachment_mappings() -> BTreeMap<u32, ModelTarget> {
    BTreeMap::from([
        (
            131339,
            ModelTarget::Model("characters/sheena/alternate-field".into()),
        ),
        (
            131428,
            ModelTarget::Model("characters/colette/wings".into()),
        ),
        (
            0x2200_0000,
            ModelTarget::Alias(ModelAlias {
                name: "characters/colette/wings/equipped".into(),
                model: "characters/colette/wings".into(),
            }),
        ),
    ])
}

fn write(library: &Path, path: &str, bytes: &[u8]) -> Result<()> {
    crate::output::write(&library.join(path), bytes)
}

/// `selected` accepts `"wings"` or `"sheena"` for a focused conversion.
pub fn cook_character_attachments(
    extracted: &Path,
    library: &Path,
    selected: Option<&str>,
) -> Result<()> {
    ensure!(
        matches!(selected, None | Some("wings" | "sheena")),
        "unknown character attachment"
    );
    let exe = fs::read(extracted.join("disc1/sys/main.dol"))?;
    if selected.is_none_or(|name| name == "wings") {
        let wings = read_named(&extracted.join("disc1/files"), &wing_source(&exe)?)?;
        let (body_glb, parts) =
            write_mesh(library, "meshes/characters/colette/wings", &wings, false)?;
        wing_model(
            library,
            &wings,
            &body_glb,
            parts.into_iter().next().unwrap(),
        )?;
    }
    if selected.is_none_or(|name| name == "sheena") {
        let sheena = sheena_source(extracted, &exe)?;
        let (body_glb, parts) = write_mesh(
            library,
            "meshes/characters/sheena/alternate-field",
            &sheena,
            true,
        )?;
        sheena_model(library, &sheena, &body_glb, parts)?;
    }
    Ok(())
}
