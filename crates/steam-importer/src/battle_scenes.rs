//! Stored Steam spell, skill and combination packages into shared scene owners.
use crate::{
    audio_effects, battle_effects, battle_normal::canonical, battle_projectiles, effect_atlas,
    output, pe, preview_model, source::Source, ui_images,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_effect::{declaration::Declaration, document::Artwork, visual::ParticleTexture},
    battle_model::document::Model,
    battle_projectile::DamageKind,
    battle_scene::{
        self, SceneProjectile,
        document::{self, Definition},
    },
    library::Mappings,
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

mod actions;
mod settings;
pub(crate) fn unison_points(image: &pe::Image<'_>, native: u16) -> Result<Vec<[f32; 3]>> {
    settings::unison_points::read(image, native)
}

pub(super) fn member(bytes: &[u8], field: usize) -> Result<Option<&[u8]>> {
    ensure!(
        bytes.len() >= 0x1b0 && (4..0x1b0).contains(&field) && field.is_multiple_of(4),
        "invalid scene pointer directory"
    );
    let start = pe::word(bytes, field)? as usize;
    if start == 0 {
        return Ok(None);
    }
    ensure!(
        (0x1b0..bytes.len()).contains(&start),
        "scene member pointer exceeds package"
    );
    let end = (4..0x1b0)
        .step_by(4)
        .map(|at| pe::word(bytes, at).map(|v| v as usize))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|&at| at > start)
        .min()
        .unwrap_or(bytes.len());
    Ok(Some(
        bytes
            .get(start..end)
            .context("scene member exceeds package")?,
    ))
}

fn reference(bytes: &[u8], field: usize) -> Result<Option<&str>> {
    member(bytes, field)?
        .map(|bytes| pe::string(bytes, 0))
        .transpose()
}

fn projectiles(bytes: &[u8], image: &pe::Image<'_>, id: u16) -> Result<Vec<SceneProjectile>> {
    let Some(rows) = member(bytes, 0x18c)? else {
        return Ok(Vec::new());
    };
    let parameters = member(bytes, 0x190)?.context("scene contacts have no parameters")?;
    let start = pe::word(parameters, 0)? as usize;
    let end = pe::word(parameters, 4)? as usize;
    let rules = parameters
        .get(start..end)
        .context("scene contacts exceed parameters")?;
    ensure!(rules.len().is_multiple_of(28), "misaligned scene contacts");
    let length = rows.len() / 400 * 400;
    ensure!(
        length > 0 && (length == rows.len() || length.next_multiple_of(32) == rows.len()),
        "misaligned scene projectile member"
    );
    let reset = f64::from_le_bytes(image.slice(0x882b38, 8)?.try_into()?) as f32;
    rows[..length]
        .chunks_exact(400)
        .enumerate()
        .map(|(index, row)| {
            let mut canonical_row = battle_projectiles::canonical_record(row)?;
            // Skill-local banks are the same stored-scene scope in the cooked format.
            if id < 200 {
                for at in [0x5c, 0x5e, 0x8c] {
                    if canonical_row[at] == 8 {
                        canonical_row[at] = 6;
                    }
                }
            }
            let projectile = resonance_importer::decode_projectile(&canonical_row, reset)?;
            let kind = match row[17] {
                0 => DamageKind::Slash,
                1 => DamageKind::Thrust,
                2 => DamageKind::Magic,
                _ => anyhow::bail!("unknown scene damage kind"),
            };
            let mut hit_errors = BTreeMap::new();
            let hits = rules
                .chunks_exact(28)
                .enumerate()
                .map(|(index, rule)| {
                    let decoded = (|| -> Result<_> {
                        let mut rule = canonical(rule, 28, &[(0, 2), (8, 4), (14, 2), (16, 2)])?;
                        if id < 200 && rule[24] == 8 {
                            rule[24] = 6;
                        }
                        let mut hit = resonance_importer::decode_spell_hit(&rule, row[19])?;
                        hit.kind = kind;
                        hit.sound = hit.sound.map(audio_effects::id).transpose()?;
                        Ok(hit)
                    })();
                    match decoded {
                        Ok(hit) => Some(hit),
                        Err(error) => {
                            hit_errors.insert(index, format!("{error:#}"));
                            None
                        }
                    }
                })
                .collect::<Vec<_>>();
            let selected = if id == 213 {
                index
            } else if id == 286 && index >= 2 {
                index - 1
            } else if id == 215 && index == 1
                || matches!(id, 206 | 207 | 219 | 230..=232 | 253 | 283) && index == 2
            {
                1
            } else {
                usize::from(row[18])
            };
            let selected = if matches!(id, 305..=308 | 311) && hits.len() == 1 && selected == 1 {
                0
            } else {
                selected
            };
            let hit = hits.get(selected).copied().flatten();
            let unsupported_reason = if hit.is_none() && !hits.is_empty() {
                Some(format!(
                    "scene contact {selected}: {}",
                    hit_errors
                        .get(&selected)
                        .map(String::as_str)
                        .unwrap_or("absent rule")
                ))
            } else {
                None
            };
            Ok(SceneProjectile {
                projectile,
                hit,
                hits,
                hit_errors,
                unsupported_reason,
            })
        })
        .collect()
}

fn crimson_equipped_axe(image: &pe::Image<'_>, bank: &[u8]) -> Result<()> {
    // Native skill 254 (shared catalogue 261) loads Presea's current primary
    // weapon into common model 14. Encounter preparation supplies that dynamic
    // dependency through the same equipped-weapon binding as Colette's chakram.
    ensure!(
        pe::word(image.slice(0x8d84b0 + 254 * 96 + 0x58, 4)?, 0)? + 1 == 5
            && image.slice(0x444786, 2)? == [0xb1, 0x07]
            && image.slice(0x4447c5, 6)? == [0x8d, 0xb2, 0xb0, 0x27, 0x01, 0x00]
            && image.slice(0x4447cb, 5)? == [0xe8, 0xb0, 0x88, 0x03, 0x00]
            && image.slice(0x47d0c9, 3)? == [0x80, 0xf9, 0x07],
        "Crimson Devastation current-axe callback changed"
    );
    let actors = pe::word(bank, 8)? as usize;
    for index in [41, 42] {
        let row = bank
            .get(actors + index * 360..actors + (index + 1) * 360)
            .context("missing Crimson Devastation axe particle")?;
        ensure!(
            row[0] == 3 && row[2] == 0 && row[220] == 14,
            "Crimson Devastation current-axe particle changed"
        );
    }
    Ok(())
}

fn cook_one(
    source: &Source,
    library: &Path,
    image: &pe::Image<'_>,
    atlases: &effect_atlas::Directory,
    residents: &BTreeMap<u8, String>,
    id: u16,
    owner: &str,
) -> Result<()> {
    let pointer = pe::word(image.slice(0x8e98a8 + u32::from(id) * 4, 4)?, 0)?;
    let bytes = source.require(image.text(pointer)?)?;
    let bank = member(&bytes, 4)?.context("stored scene has no effects")?;
    let mut effects = battle_effects::decode_including(bank, &[], &[], |stride, declaration| {
        if let Declaration::Particle { visual, .. } = declaration
            && let ParticleTexture::Atlas { palette_set, .. } = &mut visual.texture
        {
            *palette_set = stride;
        }
        Ok(())
    })?;
    let mut models = BTreeMap::new();
    for slot in 0..16 {
        let Some(body) = reference(&bytes, 12 + slot * 4)? else {
            continue;
        };
        let mut motions = Vec::new();
        for motion in 0..4 {
            if let Some(name) = reference(&bytes, 0x8c + slot * 16 + motion * 4)? {
                motions.push((motion as u16, name.to_owned()));
            }
        }
        let label = format!("{owner}/models/model-{slot}");
        let (scene, mut native) =
            preview_model::part(source, library, body, &label, "body", &motions)?;
        battle_effects::normalize_model_color(source, library, body, &scene, &mut native)?;
        let mut layers = vec![resonance_content::model_preview::PreviewPart {
            scene,
            animation: None,
            attached_to: None,
            additive: false,
            uv_offsets: Vec::new(),
        }];
        if let Some(outline) = reference(&bytes, 0x4c + slot * 4)? {
            let (mut scene, _) =
                preview_model::part(source, library, outline, &label, "outline", &motions)?;
            crate::actor_model::style_outline(&mut scene);
            layers.push(resonance_content::model_preview::PreviewPart {
                scene,
                animation: None,
                attached_to: None,
                additive: false,
                uv_offsets: Vec::new(),
            });
        }
        models.insert(
            slot as u8,
            Model {
                layers,
                attachment_nodes: Default::default(),
                independent_layers: Default::default(),
            },
        );
    }
    if id == 5 {
        crimson_equipped_axe(image, bank)?;
        ensure!(
            !models.contains_key(&14),
            "current axe overlaps a local scene model"
        );
    }
    let slots = effects
        .templates
        .iter()
        .filter_map(|declaration| declaration.particle_visual().ok()?.texture.slot())
        .collect::<BTreeSet<_>>();
    let mut textures = BTreeMap::new();
    for slot in slots {
        let resource = if slot == 6 {
            reference(&bytes, 8)?.context("scene atlas is absent")?
        } else {
            residents
                .get(&slot)
                .context("unbound scene resident atlas")?
        };
        let texture = effect_atlas::cook(
            source,
            library,
            &mut effects,
            resource,
            atlases.get(resource),
            owner,
            slot,
        )?;
        textures.insert(slot, vec![texture]);
    }
    effects.artwork = Some(Artwork::from_models(textures, models)?);
    battle_effects::bind_scene_opacity(&mut effects)?;
    let path = format!("{owner}/effects.json");
    output::json(&library.join(&path), &effects)?;
    let parameters = member(&bytes, 0x190)?;
    let mut scene = Definition {
        effects: path,
        cutin: None,
        weapon: None,
        projectiles: projectiles(&bytes, image, id)?,
        actions: BTreeMap::new(),
        actions_error: None,
        duration: parameters
            .map(|p| pe::half(p, 16))
            .transpose()?
            .unwrap_or(0),
        stage_color: None,
        camera: None,
        strike_points: Vec::new(),
        recovery_radius: None,
        spell_recipe: None,
        effect_paths: Vec::new(),
    };
    settings::bind(image, id, &mut scene)?;
    if id >= 300 {
        match parameters.map(actions::read).transpose() {
            Ok(Some(actions)) => scene.actions = actions,
            Ok(None) => {}
            Err(error) => scene.actions_error = Some(format!("{error:#}")),
        }
        for (variant, action) in &scene.actions {
            if let battle_scene::SceneAction::Unsupported { reason } = action {
                eprintln!("  Stored scene {id} participant {variant} unavailable: {reason}");
            }
        }
        if let Some(reason) = &scene.actions_error {
            eprintln!("  Stored scene {id} participant table unavailable: {reason}");
        }
    }
    if let Some(name) = reference(&bytes, 0x194)?
        && name.to_ascii_lowercase().ends_with(".totexb_d")
    {
        scene.cutin = Some(battle_scene::Texture {
            images: vec![ui_images::cook(
                source,
                library,
                name,
                &format!("{owner}/cutin.ktx2"),
                false,
            )?],
            sampler: Default::default(),
        });
    }
    for field in [0x194, 0x198] {
        let Some(part) = member(&bytes, field)? else {
            continue;
        };
        let weapon = if (5..=7).contains(&pe::word(part, 0)?) {
            Some(crate::equipment::embedded(
                source,
                library,
                part,
                &format!("{owner}/weapon"),
            )?)
        } else if let name = pe::string(part, 0)?
            && name.to_ascii_lowercase().ends_with(".pac")
        {
            Some(
                crate::equipment::package(
                    source,
                    library,
                    name,
                    &format!("{owner}/weapon"),
                    resonance_content::battle_model::document::EquipmentKind::Weapon,
                    &[],
                )?
                .into(),
            )
        } else {
            None
        };
        if let Some(weapon) = weapon {
            ensure!(
                scene.weapon.is_none(),
                "stored scene has multiple temporary weapons"
            );
            scene.weapon = Some(weapon);
        }
    }
    output::json(&library.join(owner).join("scene.json"), &scene)?;
    Ok(())
}

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let atlases = effect_atlas::Directory::read(&image)?;
    let residents = effect_atlas::residents(source, &image)?;
    let scenes: Mappings = serde_json::from_slice(&fs::read(library.join(document::TECHNIQUES))?)?;
    let techniques: resonance_content::arte::document::Document =
        serde_json::from_slice(&fs::read(library.join(resonance_content::arte::PATH))?)?;
    let mut work = scenes
        .iter()
        .map(|(id, name)| Ok((u16::try_from(id)?, format!("battle/scenes/{name}"))))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut skills = BTreeMap::new();
    for id in 0..255u32 {
        let slot = pe::word(image.slice(0x8d84b0 + id * 96 + 0x58, 4)?, 0)?;
        if slot == 0 {
            continue;
        }
        let physical = u16::try_from(slot + 1)?;
        let logical = crate::techniques::logical_id(id);
        let name = techniques.mappings.target(logical)?.to_owned();
        let owner = work
            .entry(physical)
            .or_insert_with(|| format!("battle/skills/{name}"))
            .clone();
        skills.insert(
            logical,
            owner
                .strip_prefix("battle/skills/")
                .context("skill owner crosses category")?
                .to_owned(),
        );
    }
    // Some native callbacks load a package directly rather than through an
    // inventory arte row (including the last stored skill package).
    for physical in 2..=9u16 {
        if pe::word(image.slice(0x8e98a8 + u32::from(physical) * 4, 4)?, 0)? != 0 {
            work.entry(physical)
                .or_insert_with(|| format!("battle/skills/packages/package-{physical:03}"));
        }
    }
    let mut failures = Vec::new();
    for (id, owner) in work {
        let result = cook_one(source, library, &image, &atlases, &residents, id, &owner);
        if let Err(error) = result {
            eprintln!("  Stored scene {id} ({owner}): {error:#}");
            failures.push(id);
        }
    }
    output::json(&library.join(document::SKILLS), &Mappings::new(skills)?)?;
    ensure!(
        failures.is_empty(),
        "stored scenes remain unconverted: {failures:?}"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    #[ignore = "requires RESONANCE_TEST_LIBRARY with cooked ordinary/combination scene owners"]
    fn cooked_ordinary_and_combination_scenes_prepare_shared_artwork() -> Result<()> {
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let library = resonance_content::library::Library::open(&root, &[])?;
        let diagnostics = resonance_content::diagnostics::Diagnostics::new(true);
        for id in [201, 213, 251, 301, 321] {
            let (scene, bank) = document::capture(
                &mut library.snapshot(),
                document::Kind::Technique,
                id,
                &diagnostics,
                &|| false,
            )
            .with_context(|| format!("prepare stored scene {id}"))?;
            ensure!(
                !bank.programs.is_empty(),
                "stored scene lost its effect programs"
            );
            if id == 213 {
                ensure!(
                    !scene.projectiles.is_empty(),
                    "ordinary casting scene lost its contacts"
                );
            }
            if id == 301 {
                ensure!(
                    scene.weapon.is_some() && !scene.actions.is_empty(),
                    "combination scene lost weapon or participant tracks"
                );
            }
        }
        Ok(())
    }
}
