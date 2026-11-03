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
    use crate::Language;
    use resonance_content::battle_effect::EffectOperation;
    use std::path::PathBuf;

    #[test]
    #[ignore = "requires installed Steam and cooked library; refreshes only Sacred Light after native color-rate normalization"]
    fn installed_sacred_light_preserves_native_color_envelope_and_emission_times() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let image = pe::Image::parse(&source.executable)?;
        // Both complete add-and-clamp blocks operate on EBX+8*EAX+0x20;
        // both subtract-and-clamp blocks do likewise. EAX only advances after
        // the second subtraction, so these are repeated updates, not endpoints.
        ensure!(
            image.slice(0x48bf03, 104)? == image.slice(0x48bf6b, 104)?
                && image.slice(0x48bff5, 96)? == image.slice(0x48c055, 96)?
                && image.slice(0x48bf03, 9)?
                    == [0x0f, 0xb6, 0x4b, 0x30, 0x66, 0x01, 0x4c, 0xc3, 0x20]
                && image.slice(0x48bff5, 9)?
                    == [0x0f, 0xb6, 0x4b, 0x34, 0x66, 0x29, 0x4c, 0xc3, 0x20]
                && image.slice(0x48c0b5, 4)? == [0x40, 0x83, 0xf8, 0x02],
            "native repeated RGBA update contract changed"
        );
        let name = image.text(pe::word(image.slice(0x8e98a8 + 263 * 4, 4)?, 0)?)?;
        ensure!(
            name.eq_ignore_ascii_case("BTLmagic_063.pac"),
            "Sacred Light source changed"
        );
        let package = source.require(name)?;
        let native = member(&package, 4)?.context("Sacred Light has no effects")?;
        let actors = pe::word(native, 8)? as usize;
        let decoded = battle_effects::decode(native, &[], |_, _| Ok(()))?;
        for (index, (brighten, until, fade, from, rgb)) in [
            (8, 16, 8, 124, 64),
            (2, 64, 4, 168, 32),
            (2, 64, 4, 138, 32),
        ]
        .into_iter()
        .enumerate()
        {
            let row = &native[actors + index * 360..][..360];
            ensure!(
                row[0] == 3
                    && row[51] == brighten
                    && row[55] == fade
                    && row[56] == until
                    && row[57] == from,
                "Sacred Light native envelope changed"
            );
            let template = decoded.templates[index].template()?;
            ensure!(
                template.state.colors == [[rgb, rgb, rgb, 0], [0; 4]]
                    && template.state.brighten == [0, 0, 0, brighten * 2]
                    && template.fade == [0, 0, 0, fade * 2]
                    && template.state.brighten_until == Some(u32::from(until))
                    && template.fade_from == u32::from(from),
                "rate conversion changed initial colors or native envelope timing: actor {index}, colors {:?}, brighten {:?}/{:?}, fade {:?}/{}",
                template.state.colors,
                template.state.brighten,
                template.state.brighten_until,
                template.fade,
                template.fade_from
            );
        }
        let starts = decoded.programs[1]
            .iter()
            .filter_map(|event| match event.operation {
                EffectOperation::Spawn { particle, .. } if particle < 3 => {
                    Some((event.at, particle))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        ensure!(
            starts == [(0, 1), (30, 2), (60, 0)],
            "native model emission timestamps changed"
        );
        let atlases = effect_atlas::Directory::read(&image)?;
        let residents = effect_atlas::residents(&source, &image)?;
        let mappings: Mappings =
            serde_json::from_slice(&fs::read(root.join(document::TECHNIQUES))?)?;
        let owner = format!("battle/scenes/{}", mappings.target(263)?);
        cook_one(&source, &root, &image, &atlases, &residents, 263, &owner)?;
        let cooked = resonance_content::battle_effect::document::read(
            &format!("{owner}/effects.json"),
            &resonance_content::diagnostics::Diagnostics::new(true),
            |path| Ok(fs::read(root.join(path))?.into()),
        )?;
        for member in 0..cooked.programs.len() {
            cooked.program(member)?;
        }
        for declaration in &decoded.templates[..3] {
            ensure!(
                cooked.actors.contains(declaration),
                "published model envelope differs from decoded source"
            );
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires installed Steam and cooked library; republishes only four summon attachment owners"]
    fn installed_summon_attachment_markers_keep_native_indices_and_gc_prefixes() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let image = pe::Image::parse(&source.executable)?;
        let atlases = effect_atlas::Directory::read(&image)?;
        let residents = effect_atlas::residents(&source, &image)?;
        for (id, name, required) in [
            (286, "summon-wind-0286", &["KK00_BE19_00"][..]),
            (290, "summon-light-0290", &["KK00_BE13_00", "KK01_BE13_01"]),
            (
                293,
                "summon-origin-0293",
                &[
                    "KK00_WEP0_L",
                    "KK01_WEP0_R",
                    "KK02_WEP1_L",
                    "KK03_WEP1_R",
                    "KK04_WEP2",
                    "KK05_EFE1_L",
                    "KK06_EFE1_R",
                ],
            ),
            (292, "summon-birth-0292", &["KK00_TUE"]),
        ] {
            let owner = format!("battle/scenes/{name}");
            let pointer = pe::word(image.slice(0x8e98a8 + id * 4, 4)?, 0)?;
            let package = source.require(image.text(pointer)?)?;
            let body = reference(&package, 12)?.context("summon has no primary model")?;
            let native = crate::model::Rig::read(&source.require(body)?)?;
            cook_one(
                &source, &root, &image, &atlases, &residents, id as u16, &owner,
            )?;
            let path = format!("{owner}/effects.json");
            let bank = resonance_content::battle_effect::document::read(
                &path,
                &resonance_content::diagnostics::Diagnostics::new(true),
                |path| Ok(fs::read(root.join(path))?.into()),
            )?;
            let art = bank.art.context("summon artwork is absent")?;
            let model = art
                .models
                .get(&0)
                .context("summon primary model is absent")?;
            let bones = &model.rig.skeleton.bones;
            ensure!(
                bones.len() == native.nodes.len(),
                "summon attachment normalization changed source indices"
            );
            for (bone, node) in bones.iter().zip(&native.nodes) {
                ensure!(
                    bone.parent == node.parent,
                    "summon attachment normalization changed hierarchy"
                );
            }
            for &raw in required {
                let index = native
                    .nodes
                    .iter()
                    .position(|node| node.name == raw)
                    .with_context(|| format!("missing native summon marker {raw}"))?;
                ensure!(
                    bones[index].name == format!("kk{}", &raw[2..]),
                    "summon marker {raw} lost its source slot, suffix or index"
                );
            }
            eprintln!(
                "Published {owner}: {} source-backed attachment markers",
                required.len()
            );
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM; checks the native current-axe contract without publication"]
    fn installed_crimson_effect_retains_the_current_equipped_axe_dependency() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let image = pe::Image::parse(&source.executable)?;
        let pointer = pe::word(image.slice(0x8e98a8 + 5 * 4, 4)?, 0)?;
        let package = source.require(image.text(pointer)?)?;
        let bank = member(&package, 4)?.context("missing Crimson effect bank")?;
        crimson_equipped_axe(&image, bank)?;
        ensure!(
            reference(&package, 12 + 14 * 4)?.is_none(),
            "axe became local artwork"
        );
        let effects = battle_effects::decode_including(bank, &[], &[], |_, _| Ok(()))?;
        ensure!(
            effects
                .templates
                .iter()
                .filter(
                    |declaration| matches!(declaration, Declaration::ModelParticle { binding, .. }
                    if binding.slot == 14)
                )
                .count()
                == 2,
            "cooked axe dependency changed"
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; cooks only Crimson Devastation twice"]
    fn installed_crimson_scene_preserves_external_axe_across_owner_refreshes() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let image = pe::Image::parse(&source.executable)?;
        let atlases = effect_atlas::Directory::read(&image)?;
        let residents = effect_atlas::residents(&source, &image)?;
        let owner = "battle/skills/crimson-devastation";
        cook_one(&source, &root, &image, &atlases, &residents, 5, owner)?;
        let path = root.join(owner).join("effects.json");
        let before = fs::read(&path)?;
        cook_one(&source, &root, &image, &atlases, &residents, 5, owner)?;
        ensure!(
            before == fs::read(path)?,
            "Crimson owner refresh changed effects"
        );
        let effects: resonance_content::battle_effect::document::Definition =
            serde_json::from_slice(&before)?;
        ensure!(
            !effects
                .artwork
                .context("Crimson artwork is absent")?
                .models
                .contains_key(&14),
            "owner refresh replaced the current axe with a static scene model"
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; cooks only Grand Cross and Divine Judgment"]
    fn installed_added_angel_spells_publish_shared_pattern_recipes() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let image = pe::Image::parse(&source.executable)?;
        let mappings: Mappings =
            serde_json::from_slice(&fs::read(root.join(document::TECHNIQUES))?)?;
        let atlases = effect_atlas::Directory::read(&image)?;
        let residents = effect_atlas::residents(&source, &image)?;
        for id in [271, 281] {
            cook_one(
                &source,
                &root,
                &image,
                &atlases,
                &residents,
                id,
                &format!("battle/scenes/{}", mappings.target(u32::from(id))?),
            )?;
        }
        let library = resonance_content::library::Library::open(&root, &[])?;
        for (id, count) in [(271, 3), (281, 19)] {
            let (scene, _) = document::capture(
                &mut library.snapshot(),
                document::Kind::Technique,
                id,
                &resonance_content::diagnostics::Diagnostics::new(true),
                &|| false,
            )?;
            let battle_scene::SpellRecipe::FixedPattern(recipe) =
                scene.spell_recipe.context("missing added angel recipe")?;
            ensure!(
                recipe.contacts.len() == count,
                "added angel contact schedule changed"
            );
            if id == 271 {
                ensure!(
                    matches!(recipe.origin, battle_scene::PatternOrigin::SceneTargetRoot),
                    "Grand Cross lost retained root"
                );
            }
            if id == 281 {
                ensure!(
                    recipe
                        .cutin
                        .as_ref()
                        .is_some_and(|cutin| cutin.catalogue == 144 && cutin.tick == 62),
                    "Divine Judgment lost portrait"
                );
            }
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; refreshes only three visually reviewed effect owners"]
    fn installed_effect_models_publish_gc_range_rgb_without_changing_opacity() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let image = pe::Image::parse(&source.executable)?;
        // The arena renderer checks all RGB channels before model submission;
        // the branch bypasses draw 0x57c090 but retains animation/update below.
        ensure!(
            image.slice(0x42a680, 27)?
                == [
                    0x80, 0xbe, 0x04, 0x07, 0, 0, 0, 0x75, 0x12, 0x80, 0xbe, 0x05, 0x07, 0, 0, 0,
                    0x75, 0x09, 0x80, 0xbe, 0x06, 0x07, 0, 0, 0, 0x74, 0x23
                ],
            "native black-scenery draw guard changed"
        );
        let atlases = effect_atlas::Directory::read(&image)?;
        let residents = effect_atlas::residents(&source, &image)?;
        let mappings: Mappings =
            serde_json::from_slice(&fs::read(root.join(document::TECHNIQUES))?)?;
        for id in [202, 205, 206] {
            let name = image.text(pe::word(image.slice(0x8e98a8 + u32::from(id) * 4, 4)?, 0)?)?;
            let package = source.require(name)?;
            let body = reference(&package, 12)?.context("missing reviewed effect model")?;
            let descriptor = source.require(body)?;
            let dependencies = crate::package::dependencies(&descriptor)?;
            let native =
                crate::model::Model::read(&descriptor, &source.require(&dependencies[0])?)?;
            let stem = body.rsplit_once('.').context("missing model suffix")?.0;
            let (tints, _, _) = crate::model::material_parameters(
                &source.require(&format!("{stem}.mxd"))?,
                &native.materials,
            )?;
            ensure!(
                native
                    .meshes
                    .iter()
                    .flat_map(|m| &m.colors)
                    .all(|c| c[..3].iter().all(|v| (0.49..0.51).contains(v))),
                "reviewed native effect RGB range changed"
            );
            let expected = native
                .meshes
                .iter()
                .filter(|m| !m.indices.is_empty())
                .flat_map(|m| {
                    let opacity = tints[usize::from(m.material)][3];
                    m.colors.iter().map(move |c| [1., 1., 1., c[3] * opacity])
                })
                .collect::<Vec<_>>();
            let owner = format!("battle/scenes/{}", mappings.target(u32::from(id))?);
            cook_one(&source, &root, &image, &atlases, &residents, id, &owner)?;
            let glb = fs::read(root.join(&owner).join("models/model-0/meshes/body.glb"))?;
            let json_length = u32::from_le_bytes(glb[12..16].try_into()?) as usize;
            let doc: serde_json::Value = serde_json::from_slice(&glb[20..20 + json_length])?;
            let binary = &glb[28 + json_length..];
            let mut actual = Vec::new();
            for mesh in doc["meshes"].as_array().context("missing meshes")? {
                for primitive in mesh["primitives"]
                    .as_array()
                    .context("missing primitives")?
                {
                    let accessor = &doc["accessors"][primitive["attributes"]["COLOR_0"]
                        .as_u64()
                        .context("missing colors")?
                        as usize];
                    ensure!(
                        accessor["componentType"] == 5126 && accessor["type"] == "VEC4",
                        "unexpected color encoding"
                    );
                    let view = &doc["bufferViews"][accessor["bufferView"]
                        .as_u64()
                        .context("missing color view")?
                        as usize];
                    let start = (view["byteOffset"].as_u64().unwrap_or(0)
                        + accessor["byteOffset"].as_u64().unwrap_or(0))
                        as usize;
                    let stride = view["byteStride"].as_u64().unwrap_or(16) as usize;
                    for i in 0..accessor["count"].as_u64().context("missing color count")? as usize
                    {
                        actual.push(std::array::from_fn::<_, 4, _>(|c| {
                            f32::from_le_bytes(
                                binary[start + i * stride + c * 4..][..4]
                                    .try_into()
                                    .unwrap(),
                            )
                        }));
                    }
                }
            }
            ensure!(
                actual == expected,
                "effect model RGB normalization changed native opacity or vertex order"
            );
            let diagnostics = resonance_content::diagnostics::Diagnostics::new(true);
            let bank = resonance_content::battle_effect::document::read(
                &format!("{owner}/effects.json"),
                &diagnostics,
                |path| Ok(fs::read(root.join(path))?.into()),
            )?;
            for program in 0..bank.programs.len() {
                bank.program(program)?;
            }
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; refreshes only Dreaded Wave and verifies native opacity coverage"]
    fn installed_dreaded_wave_scene_sampler_preserves_native_alpha_binding() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let image = pe::Image::parse(&source.executable)?;
        let atlases = effect_atlas::Directory::read(&image)?;
        let residents = effect_atlas::residents(&source, &image)?;
        let name = image.text(pe::word(image.slice(0x8e98a8 + 224 * 4, 4)?, 0)?)?;
        ensure!(
            name.eq_ignore_ascii_case("BTLmagic_024.pac"),
            "Dreaded Wave source package changed"
        );
        let package = source.require(name)?;
        let native = battle_effects::decode(
            member(&package, 4)?.context("missing Dreaded Wave effects")?,
            &[],
            |stride, declaration| {
                if let Declaration::Particle { visual, .. } = declaration
                    && let ParticleTexture::Atlas { palette_set, .. } = &mut visual.texture
                {
                    *palette_set = stride;
                }
                Ok(())
            },
        )?;
        let index = native
            .templates
            .iter()
            .position(|declaration| {
                declaration
                    .particle_visual()
                    .is_ok_and(|visual| visual.scene_sample.is_some())
            })
            .context("missing native Dreaded Wave sampler")?;
        let declaration = &native.templates[index];
        ensure!(
            declaration.particle_visual()?.scene_sample == Some([12, 12])
                && declaration.template()?.state.palettes == [0, 1],
            "Dreaded Wave opacity selectors changed"
        );
        let stride = declaration.particle_visual()?.texture.palette_set();
        ensure!(stride == 32, "Dreaded Wave native palette stride changed");
        let resource = residents.get(&0).context("missing resident source atlas")?;
        let descriptor = crate::texture::Descriptor::read(&source.require(resource)?)?;
        let pixels = descriptor.decode(&source.require(&descriptor.payload)?)?;
        let table = atlases.get(resource);
        let palette = if table.is_some_and(|table| table.rows.iter().any(|row| row.palette == 2)) {
            2
        } else {
            u16::MAX
        };
        let expected = effect_atlas::frames::Plan::read(&native, index)?.image(
            &pixels.levels[0],
            table,
            palette,
        )?;
        let mappings: Mappings =
            serde_json::from_slice(&fs::read(root.join(document::TECHNIQUES))?)?;
        let owner = format!("battle/scenes/{}", mappings.target(224)?);
        cook_one(&source, &root, &image, &atlases, &residents, 224, &owner)?;
        let diagnostics = resonance_content::diagnostics::Diagnostics::new(true);
        let cooked = resonance_content::battle_effect::document::read(
            &format!("{owner}/effects.json"),
            &diagnostics,
            |path| Ok(fs::read(root.join(path))?.into()),
        )?;
        for member in 0..cooked.programs.len() {
            cooked.program(member)?;
        }
        let declaration = cooked
            .actors
            .iter()
            .find(|declaration| {
                declaration
                    .particle_visual()
                    .is_ok_and(|visual| visual.scene_sample.is_some())
            })
            .context("missing cooked scene sampler")?;
        let ParticleTexture::Atlas {
            slot,
            dual,
            palette_set,
        } = declaration.particle_visual()?.texture
        else {
            anyhow::bail!("scene sampler lost its atlas")
        };
        ensure!(dual, "scene sampler lost separate opacity binding");
        let textures = &cooked.art.context("missing cooked artwork")?.textures[&slot];
        ensure!(
            textures.len() == 2,
            "scene sampler needs two prepared texture bindings"
        );
        let page =
            textures[1].palette_image(palette_set, declaration.template()?.state.palettes[1])?;
        let alpha = &textures[1].images[page];
        let actual = resonance_asset_writer::texture::ktx2(&fs::read(root.join(&alpha.path))?)?;
        ensure!(
            actual[0] == expected,
            "scene sampler changed native RGBA coverage or its UV mapping"
        );
        ensure!(
            actual[0].rgba.chunks_exact(4).any(|pixel| pixel[3] == 0)
                && actual[0].rgba.chunks_exact(4).any(|pixel| pixel[3] > 0),
            "scene sampler opacity lost native coverage"
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; category cook only"]
    fn installed_stored_scene_category_cooks_all_owners() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        cook(&source, &root)?;
        let scenes: Mappings = serde_json::from_slice(&fs::read(root.join(document::TECHNIQUES))?)?;
        let skills: Mappings = serde_json::from_slice(&fs::read(root.join(document::SKILLS))?)?;
        ensure!(
            scenes.iter().count() == 100 && skills.iter().count() == 7,
            "stored scene mapping count changed"
        );
        for (category, mappings) in [("scenes", &scenes), ("skills", &skills)] {
            for (_, name) in mappings.iter() {
                ensure!(
                    root.join("battle")
                        .join(category)
                        .join(name)
                        .join("scene.json")
                        .is_file(),
                    "missing stored owner {category}/{name}"
                );
            }
        }
        ensure!(
            root.join("battle/skills/packages/package-009/scene.json")
                .is_file(),
            "missing direct callback skill package"
        );
        eprintln!(
            "Stored category refresh:100 scene owners +8 physical skill owners;7 logical skill mappings"
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; cooks only Time Stop, optionally over RESONANCE_TEST_ASSETS"]
    fn installed_time_stop_scene_loads_its_discrete_clock_hand_program() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let base = std::env::var_os("RESONANCE_TEST_ASSETS")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.clone());
        let image = pe::Image::parse(&source.executable)?;
        cook_one(
            &source,
            &root,
            &image,
            &effect_atlas::Directory::read(&image)?,
            &effect_atlas::residents(&source, &image)?,
            266,
            "battle/scenes/time-stop-0266",
        )?;
        let library = if base == root {
            resonance_content::library::Library::open(&root, &[])?
        } else {
            resonance_content::library::Library::open_paths(&base, &[root])?
        };
        let (scene, bank) = document::capture(
            &mut library.snapshot(),
            document::Kind::Technique,
            266,
            &resonance_content::diagnostics::Diagnostics::new(true),
            &|| false,
        )?;
        ensure!(scene.duration == 310, "Time Stop lost its native lifetime");
        let Some(resonance_content::battle_scene::SpellRecipe::FixedPattern(pattern)) =
            scene.spell_recipe.as_ref()
        else {
            anyhow::bail!("Time Stop has no executable scene recipe");
        };
        ensure!(
            pattern.lifetime == 310
                && pattern.opponent_stop_ticks == 310
                && pattern.contacts.is_empty(),
            "Time Stop lost its timed opponent-stop benefit"
        );
        let steps = bank
            .program(1)?
            .iter()
            .filter_map(|event| match &event.operation {
                resonance_content::battle_effect::EffectOperation::SetMotion { motion, .. } => {
                    Some((event.at, motion.angles_add))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        ensure!(
            steps
                == (1..=12)
                    .map(|step| (step * 20, Some([0., 0., -30.])))
                    .collect::<Vec<_>>(),
            "Time Stop lost its native clock-hand steps"
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; cooks only Final Fury"]
    fn installed_final_fury_scene_keeps_correlated_random_programs() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let image = pe::Image::parse(&source.executable)?;
        let physical = pe::word(image.slice(0x8d84b0 + 200 * 96 + 0x58, 4)?, 0)? + 1;
        ensure!(physical == 8, "Final Fury stored package binding changed");
        let mappings: Mappings = serde_json::from_slice(&fs::read(root.join(document::SKILLS))?)?;
        let owner = format!("battle/skills/{}", mappings.target(200)?);
        cook_one(
            &source,
            &root,
            &image,
            &effect_atlas::Directory::read(&image)?,
            &effect_atlas::residents(&source, &image)?,
            physical.try_into()?,
            &owner,
        )?;
        let library = resonance_content::library::Library::open(&root, &[])?;
        let (_, bank) = document::capture(
            &mut library.snapshot(),
            document::Kind::Skill,
            200,
            &resonance_content::diagnostics::Diagnostics::new(true),
            &|| false,
        )?;
        for program in [1, 2] {
            bank.program(program)
                .with_context(|| format!("Final Fury skill200 program{program}"))?;
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; cooks only Wyrm Wrath"]
    fn installed_wyrm_wrath_keeps_its_second_hits_armor_damage() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let base = std::env::var_os("RESONANCE_TEST_ASSETS")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.clone());
        let mappings: Mappings =
            serde_json::from_slice(&fs::read(base.join(document::TECHNIQUES))?)?;
        let owner = format!("battle/scenes/{}", mappings.target(323)?);
        let image = pe::Image::parse(&source.executable)?;
        cook_one(
            &source,
            &root,
            &image,
            &effect_atlas::Directory::read(&image)?,
            &effect_atlas::residents(&source, &image)?,
            323,
            &owner,
        )?;
        let library = if base == root {
            resonance_content::library::Library::open(&root, &[])?
        } else {
            resonance_content::library::Library::open_paths(&base, &[root])?
        };
        let (scene, _) = document::capture(
            &mut library.snapshot(),
            document::Kind::Technique,
            323,
            &resonance_content::diagnostics::Diagnostics::new(true),
            &|| false,
        )?;
        for member in [1, 2] {
            let hit = scene
                .projectiles
                .get(member)
                .context("missing Wyrm Wrath projectile")?
                .hit_variant(1)?;
            ensure!(
                hit.armor_damage == 8 && hit.effect.is_none(),
                "Wyrm Wrath lost its native armor damage or changed contact artwork"
            );
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires native Steam and current library; cooks only Tetraspell298"]
    fn installed_tetraspell_publishes_its_native_five_contact_recipe() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let image = pe::Image::parse(&source.executable)?;
        let owner = "battle/scenes/tetraspell-0298";
        cook_one(
            &source,
            &root,
            &image,
            &effect_atlas::Directory::read(&image)?,
            &effect_atlas::residents(&source, &image)?,
            298,
            owner,
        )?;
        let library = resonance_content::library::Library::open(&root, &[])?;
        let (scene, bank) = document::capture(
            &mut library.snapshot(),
            document::Kind::Technique,
            298,
            &resonance_content::diagnostics::Diagnostics::new(true),
            &|| false,
        )?;
        ensure!(
            scene.effects == format!("{owner}/effects.json"),
            "Tetraspell lost its own artwork bank"
        );
        ensure!(scene.duration == 316, "Tetraspell native lifetime changed");
        let battle_scene::SpellRecipe::FixedPattern(pattern) = scene
            .spell_recipe
            .as_ref()
            .context("Tetraspell has no recipe")?;
        ensure!(
            pattern.lifetime == 316 && pattern.opponent_stop_ticks == 316,
            "Tetraspell lost its native lifetime or opponent stop"
        );
        ensure!(
            pattern
                .contacts
                .iter()
                .map(|row| (row.tick, row.projectile, row.hit_variant))
                .collect::<Vec<_>>()
                == [
                    (16, 1, 0),
                    (80, 2, 1),
                    (150, 3, 2),
                    (160, 1, 0),
                    (220, 4, 3)
                ],
            "Tetraspell contacts were aliased or retimed"
        );
        pattern.validate()?;
        scene.spell_recipe.as_ref().unwrap().validate(&scene)?;
        bank.program(1)?;
        Ok(())
    }
    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; cooks four dynamic scene owners"]
    fn installed_dynamic_scenes_keep_fairy_paths_and_live_sacred_light() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let image = pe::Image::parse(&source.executable)?;
        let atlases = effect_atlas::Directory::read(&image)?;
        let residents = effect_atlas::residents(&source, &image)?;
        let mappings: Mappings =
            serde_json::from_slice(&fs::read(root.join(document::TECHNIQUES))?)?;
        for id in [263, 279] {
            let owner = format!("battle/scenes/{}", mappings.target(id)?);
            cook_one(
                &source, &root, &image, &atlases, &residents, id as u16, &owner,
            )?;
        }
        let skills: Mappings = serde_json::from_slice(&fs::read(root.join(document::SKILLS))?)?;
        for (physical, logical) in [(6, 249), (7, 153)] {
            let owner = format!("battle/skills/{}", skills.target(logical)?);
            cook_one(
                &source, &root, &image, &atlases, &residents, physical, &owner,
            )?;
            let effects: resonance_content::battle_effect::document::Definition =
                serde_json::from_slice(&fs::read(root.join(&owner).join("effects.json"))?)?;
            ensure!(
                effects
                    .templates
                    .iter()
                    .all(|declaration| !matches!(declaration, Declaration::Unsupported { .. })),
                "Holy Binds retained unsupported native declarations"
            );
        }
        let library = resonance_content::library::Library::open(&root, &[])?;
        let diagnostics = resonance_content::diagnostics::Diagnostics::new(true);
        let (sacred, _) = document::capture(
            &mut library.snapshot(),
            document::Kind::Technique,
            263,
            &diagnostics,
            &|| false,
        )?;
        let battle_scene::SpellRecipe::FixedPattern(recipe) =
            sacred.spell_recipe.context("missing Sacred Light recipe")?;
        ensure!(
            recipe.contacts.len() == 1
                && recipe.contacts[0].aim == battle_scene::PatternAim::Live
                && recipe.contacts[0].effect_member == 0,
            "Sacred Light lost live aim or gained duplicate artwork"
        );
        let (fairy, _) = document::capture(
            &mut library.snapshot(),
            document::Kind::Technique,
            279,
            &diagnostics,
            &|| false,
        )?;
        ensure!(
            fairy.effect_paths.len() == 4,
            "Fairy Circle lost emitter anchors"
        );
        for (slot, path) in fairy.effect_paths.iter().enumerate() {
            ensure!(
                path.member == 3 + slot as u16 && path.positions.len() == 361,
                "Fairy Circle path lifetime changed"
            );
            ensure!(
                path.positions[0] == [0.; 3] && path.positions[1] != path.positions[2],
                "Fairy Circle emitter no longer moves"
            );
        }
        Ok(())
    }
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
    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY"]
    fn installed_stored_scenes_cook_and_load_shared_documents() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        cook(&source, &root)?;
        let library = resonance_content::library::Library::open(&root, &[])?;
        let diagnostics = resonance_content::diagnostics::Diagnostics::new(true);
        for (kind, path) in [
            (document::Kind::Technique, document::TECHNIQUES),
            (document::Kind::Skill, document::SKILLS),
        ] {
            let mappings: Mappings = serde_json::from_slice(&fs::read(root.join(path))?)?;
            for (id, _) in mappings.iter() {
                document::capture(
                    &mut library.snapshot(),
                    kind,
                    id.try_into()?,
                    &diagnostics,
                    &|| false,
                )?;
            }
        }
        Ok(())
    }
}
