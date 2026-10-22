//! Isolated authored-motion binding for replay diagnostics.
use anyhow::{Context, Result, ensure};
use std::{fs, path::Path};

/// Bind a package's selected motion to a party body, publishing only the requested
/// fixture file. The receipt pins both source inputs and the resulting clip.
pub fn bind_party_motion(
    extracted: &Path,
    package: &Path,
    package_clip: u16,
    character: u8,
    costume: u8,
    output: &Path,
) -> Result<serde_json::Value> {
    let bytes = fs::read(package)?;
    let field = 0x20 + usize::from(package_clip) * 4;
    ensure!(
        field < 0x160,
        "fixture clip exceeds the package motion table"
    );
    let members = crate::model_preview::PointerMembers::new(&bytes, 0x18..0x1e8)?;
    let animation = members
        .model(field)?
        .context("fixture package motion is absent")?;
    let catalogue = crate::resource::read(&fs::read(extracted.join("sys/main.dol"))?)?;
    let declared = catalogue.party(crate::resource::PartyResource::Body, character, costume)?;
    let files = extracted.join("files");
    let body_path = files.join(crate::field_resources::resolve_path(&files, declared)?);
    let body_bytes = fs::read(&body_path)?;
    let body = crate::compression::payload(body_bytes.clone())?;
    let primary = crate::source_assets::section(&body, 0)?;
    let (_, resource) = crate::geometry::model_resource(primary)?;
    let model = crate::model::Model::parse(&resource[crate::geometry::skeleton_range(resource)?])?;
    let bindings = crate::animation::ModelBindings::from_model(&model);
    let motion = crate::animation::read_member(animation)?.motion(&bindings)?;
    ensure!(!motion.tracks.is_empty(), "fixture motion binds no tracks");
    motion.validate(&super::rig(primary, super::RigKind::Body)?.skeleton)?;
    let encoded = motion.encode()?;
    crate::write_atomic(output, &encoded)?;
    Ok(serde_json::json!({
        "character": character,
        "costume": costume,
        "package_clip": package_clip,
        "package": {"path": fs::canonicalize(package)?, "sha256": crate::digest(&bytes)},
        "source_motion_sha256": crate::digest(animation),
        "body": {"path": fs::canonicalize(body_path)?, "sha256": crate::digest(&body_bytes)},
        "motion": {"path": fs::canonicalize(output)?, "sha256": crate::digest(&encoded)},
        "duration_frames": motion.duration_frames,
        "tracks": motion.tracks.len(),
    }))
}
