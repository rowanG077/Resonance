//! Isolated native-source observations for the external replay tools.
use crate::{
    battle_effect_recipes::NativeBank, battle_enemy_media, battle_party_mappings,
    battle_party_model, character_models, dol, world_gpl, world_motion,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{animation::Motion, battle_effect::declaration::Declaration};
use std::{fs, path::Path};

/// Preserve native declaration indices, including unsupported unused entries.
pub fn inspect_effect_templates(bytes: &[u8]) -> Result<serde_json::Value> {
    let bank = NativeBank::read(bytes)?;
    let declarations = (0..bank.actors.len() / 352)
        .map(|index| {
            bank.simple_particle(index as u8)
                .unwrap_or_else(|error| Declaration::Unsupported {
                    reason: error.to_string(),
                })
        })
        .collect::<Vec<_>>();
    Ok(serde_json::from_slice(&serde_json::to_vec(&declarations)?)?)
}

/// Bind an observed enemy-package motion to its native receiving party body.
/// Evidence hashes belong to the calling oracle, never the cooked library.
pub fn bind_fixture_motion(
    extracted: &Path,
    package: &Path,
    package_clip: u16,
    character: u8,
    costume: u8,
    output: &Path,
) -> Result<serde_json::Value> {
    ensure!(
        (1..=9).contains(&character) && costume < 5,
        "invalid fixture costume"
    );
    let root = if extracted.join("sys/main.dol").is_file() {
        extracted.parent().context("extracted disc has no parent")?
    } else {
        extracted
    };
    let bytes = fs::read(package)?;
    let field = 0x20 + usize::from(package_clip) * 4;
    ensure!(field < 0x160, "fixture clip exceeds package motion table");
    let source = battle_enemy_media::member(&bytes, field)?.context("fixture motion absent")?;
    let offset = dol::u32(&bytes, field)? as usize;
    let native = battle_party_model::expanded_motion(source)?;
    let exe = fs::read(root.join("disc1/sys/main.dol"))?;
    let declared = battle_party_mappings::source_name(&exe, 0x801f_ace4, character, costume)?;
    let body_path = battle_party_mappings::native_path(&root.join("disc1/files"), &declared)?
        .context("fixture party body absent")?;
    let body = character_models::party_body(root, character, costume)?;
    let (paths, alpha, _) = character_models::texture_paths(
        character_models::palette(&body)?,
        "fixture",
        "body",
        |_, _| {},
    )?;
    let (glb, _) = world_gpl::static_model(&body, paths, alpha)?;
    let bindings = world_gpl::animation_bindings(&body)?;
    let encoded = world_motion::encode_bound(&native, &glb, &bindings)?;
    let motion = Motion::decode(&encoded)?;
    ensure!(!motion.tracks.is_empty(), "fixture motion binds no tracks");
    let duration = (0..usize::from(dol::u16(&native, 12)?)).try_fold(0f32, |end, index| {
        Ok::<_, anyhow::Error>(end.max(f32::from_bits(dol::u32(&native, 36 + index * 16)?)))
    })?;
    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, encoded)?;
    Ok(serde_json::json!({
        "character":character, "costume":costume, "package_clip":package_clip,
        "package":{"path":fs::canonicalize(package)?},
        "body":{"path":fs::canonicalize(body_path)?},
        "source_motion":{"offset":offset,"bytes":source.len()},
        "motion":{"path":fs::canonicalize(output)?},
        "duration_frames":duration, "tracks":motion.tracks.len(),
    }))
}
