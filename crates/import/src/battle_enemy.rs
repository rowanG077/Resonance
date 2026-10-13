//! Decode enemy decisions, including model contacts required by their projectiles.
use crate::read::u16 as half;
use anyhow::{Context, Result, ensure};
use resonance_content::battle_enemy::{Definition, Presentation};
use resonance_content::battle_voice::TimedVoice;
use std::{fs, path::Path};

/// Refresh every enemy's behavior/profile metadata without converting its artwork.
pub fn publish_all(extracted: &Path, output: &Path) -> Result<Vec<String>> {
    publish(extracted, output, false)
}

/// Refresh every enemy against body and effect descriptors already seeded into output.
/// Artwork is retained; newly declared palette windows are published as needed.
pub fn publish_cached(extracted: &Path, output: &Path) -> Result<Vec<String>> {
    publish(extracted, output, true)
}

fn publish(extracted: &Path, output: &Path, cached: bool) -> Result<Vec<String>> {
    let sources = crate::source_assets::Sources::read(extracted)?;
    let usual = fs::read(extracted.join("files").join(&sources.usual))?;
    let archive = extracted.join("files").join(&sources.enemy);
    let voices = crate::battle_voice::read(&usual)?;
    let module = crate::rel::Rel::read(&extracted.join("files").join(&sources.module))?;
    let guard_bonus = crate::battle_recoil::guard_defaults(&module)?[0];
    let mut paths = Vec::new();
    for id in 0..resonance_content::monster::MONSTER_COUNT {
        let bytes = crate::source_assets::enemy_package(&archive, &usual, id as u16)?;
        let definition = read(&bytes, id as u8, &voices, guard_bonus)?;
        let model_path = resonance_content::battle_model::enemy_path(id as u8);
        if cached || output.join(&model_path).is_file() {
            let mut model: resonance_content::battle_model::Enemy = serde_json::from_slice(
                &fs::read(output.join(&model_path))
                    .with_context(|| format!("missing cached enemy body {id}"))?,
            )?;
            ensure!(
                model.source_sha256 == definition.source_sha256,
                "cached enemy {id} body differs from source"
            );
            if cached {
                paths.push(
                    refresh_effects(&bytes, id as u8, &mut model, output)
                        .with_context(|| format!("refresh cached enemy {id} effect bank"))?,
                );
            }
            let motions = crate::battle_model::publish_enemy_carried_motions(
                &bytes,
                &definition.actions,
                &mut model,
                output,
            )?;
            if cached || !motions.is_empty() {
                crate::write_atomic(&output.join(&model_path), &serde_json::to_vec(&model)?)?;
                paths.push(model_path);
                paths.extend(motions);
            }
        }
        let path = resonance_content::battle_enemy::path(id as u8);
        crate::write_atomic(&output.join(&path), &serde_json::to_vec(&definition)?)?;
        paths.push(path);
    }
    Ok(paths)
}

fn refresh_effects(
    bytes: &[u8],
    enemy: u8,
    model: &mut resonance_content::battle_model::Enemy,
    output: &Path,
) -> Result<String> {
    let path = resonance_content::battle_model::enemy_effects_path(enemy);
    let mut bank: resonance_content::battle_effect::SourceBank =
        serde_json::from_slice(&fs::read(output.join(&path))?)?;
    ensure!(
        bank.art
            .as_ref()
            .is_some_and(|art| art.source_sha256 == model.source_sha256),
        "cached enemy effect artwork differs from source"
    );
    let members = crate::model_preview::PointerMembers::new(bytes, 0x18..0x1e8)?;
    if let Some(source) = members.model(0x1cc)? {
        let (mut fresh, strides) =
            crate::battle_effect::read_enemy_with_palettes(source, &model.body.skeleton)?;
        ensure!(
            fresh.source_sha256 == bank.source_sha256 && fresh.actors.len() == bank.actors.len(),
            "cached enemy effect declarations differ from source"
        );
        let mut art = bank
            .art
            .take()
            .context("cached enemy effect artwork is missing")?;
        let mut textures = std::collections::BTreeMap::new();
        for (field, slot) in [(0x1d0, 2), (0x1d4, 14)] {
            if let Some(bytes) = members.model(field)? {
                textures.insert(
                    slot,
                    crate::battle_effect::art::cached_bank_textures(
                        bytes,
                        &fresh,
                        &strides,
                        slot,
                        art.textures
                            .get(&slot)
                            .context("cached enemy texture slot is missing")?,
                        output,
                    )
                    .with_context(|| format!("enemy texture slot {slot}"))?,
                );
            }
        }
        crate::battle_effect::art::bind_palettes(&mut fresh, &strides, &mut textures)?;
        for (slot, textures) in textures {
            let textures: Vec<_> = textures.into_iter().map(|texture| texture.effect).collect();
            for image in textures.iter().flat_map(|texture| &texture.images) {
                if !art.files.contains_key(&image.path) {
                    art.files.insert(
                        image.path.clone(),
                        resonance_content::field_preload::File {
                            sha256: crate::media::hash_file(&output.join(&image.path))?,
                            bytes: fs::metadata(output.join(&image.path))?.len(),
                            roles: [resonance_content::field_preload::Role::Texture].into(),
                        },
                    );
                }
            }
            art.textures.insert(slot, textures);
        }
        model.files.extend(art.files.clone());
        fresh.art = Some(art);
        bank = fresh;
    } else {
        ensure!(
            bank.source_sha256 == model.source_sha256
                && bank.actors.is_empty()
                && bank.programs.is_empty(),
            "cached enemy has commands without an authored effect bank"
        );
    }
    let encoded = serde_json::to_vec(&bank)?;
    let entry = model
        .files
        .get_mut(&path)
        .context("cached enemy body omits its effect bank")?;
    entry.sha256 = crate::digest(&encoded);
    entry.bytes = encoded.len() as u64;
    crate::write_atomic(&output.join(&path), &encoded)?;
    Ok(path)
}

pub(crate) fn read(
    bytes: &[u8],
    enemy: u8,
    voices: &crate::battle_voice::Source,
    default_guard_bonus: u8,
) -> Result<Definition> {
    ensure!(bytes.starts_with(b"em8\0"), "invalid enemy package");
    let metadata = bytes
        .get(usize::from(half(bytes, 4)?)..)
        .context("missing enemy profile")?;
    let members = crate::model_preview::PointerMembers::new(bytes, 0x18..0x1e8)?;
    let projectiles = members
        .model(0x1c8)?
        .map(|member| {
            let start = crate::read::u32(bytes, 0x1c8)? as usize;
            crate::battle_projectile::read_enemy_member(member, start, &members)
        })
        .transpose()?
        .unwrap_or_default();
    let strategy = enemy_strategy(bytes)?;
    let mut actions = crate::battle_action::read(bytes, &projectiles)?;
    if enemy == 165 {
        use resonance_content::battle_action::{EnemyEvent, EnemyExecution};
        for row in &mut actions.rows {
            if let Some(EnemyExecution::Attack(attack) | EnemyExecution::Physical { attack, .. }) =
                &mut row.execution
            {
                for (_, event) in &mut attack.events {
                    if let EnemyEvent::Defeat { rewards } = event {
                        *rewards = false;
                    }
                }
            }
        }
    }
    let policy = usize::from(half(bytes, 12)?);
    match bytes.get(policy + 8) {
        Some(1) => ensure!(
            matches!(enemy, 105 | 165),
            "unprepared retained-body enemy {enemy}"
        ),
        Some(2) => ensure!(enemy == 191, "unprepared staged enemy {enemy}"),
        Some(4) => ensure!(
            matches!(enemy, 211..=214),
            "unprepared linked enemy {enemy}"
        ),
        _ => {}
    }
    if bytes.get(policy + 8) == Some(&13) {
        actions.policy.behavior = Some(guardian_parts(enemy)?);
    }
    let target_strategy =
        match resonance_content::battle_action::TargetPolicy::try_from(strategy[0]) {
            Ok(policy) => Some(policy),
            Err(error) => {
                actions
                    .policy
                    .unsupported_reason
                    .get_or_insert_with(|| error.to_string());
                None
            }
        };
    let (entry_row, guard_recovery_bonus) = guard_policy(strategy[2], default_guard_bonus)?;
    let mut profile =
        crate::battle_profile::read(metadata, voices, crate::battle_voice::ENEMY_ROLES)?;
    let presentation = presentation(bytes, metadata, &mut profile, voices)?;
    let motion_count = crate::read::u32(bytes, 0x14)?.max(31) as usize;
    ensure!(
        motion_count <= (0x160 - 0x20) / 4,
        "enemy motion count overlaps attachments"
    );
    let mut null_motions = std::collections::BTreeSet::new();
    for clip in 0..motion_count {
        if crate::read::u32(bytes, 0x20 + clip * 4)? == 0 {
            null_motions.insert(clip as u16);
        }
    }
    Ok(Definition {
        source_sha256: crate::digest(bytes),
        null_motions,
        profile,
        projectiles,
        actions,
        target_strategy,
        entry_row,
        guard_recovery_bonus,
        presentation,
    })
}

fn guardian_parts(enemy: u8) -> Result<resonance_content::battle_action::EnemyBehavior> {
    const WING_SCALE: f32 = 0.5;
    const BACK_ARMS: [&str; 2] = ["Bone21_BArm_L", "Bone21_BArm_R"];
    const FRONT_ARMS: [&str; 2] = ["Bone11_FArm_L", "Bone11_FArm_R"];
    let hidden: Vec<_> = match enemy {
        208 => BACK_ARMS.to_vec(),
        209 => FRONT_ARMS.to_vec(),
        210 => BACK_ARMS.into_iter().chain(FRONT_ARMS).collect(),
        _ => anyhow::bail!("Guardian parts on an incompatible enemy {enemy}"),
    };
    Ok(
        resonance_content::battle_action::EnemyBehavior::ModelParts {
            bone_scales: std::iter::once(("kk06_Hane".to_owned(), WING_SCALE))
                .chain(hidden.into_iter().map(|name| (name.to_owned(), 0.)))
                .collect(),
            hidden_weapons: if enemy == 210 {
                vec![]
            } else {
                (0..4).collect()
            },
        },
    )
}

fn presentation(
    bytes: &[u8],
    metadata: &[u8],
    profile: &mut resonance_content::battle_profile::Profile,
    voices: &crate::battle_voice::Source,
) -> Result<Option<Presentation>> {
    let policy = usize::from(half(bytes, 12)?);
    let selector = *bytes
        .get(policy + 8)
        .context("missing enemy presentation policy")?;
    if !matches!(
        selector,
        6 | 8 | 10 | 12 | 13 | 15 | 16 | 18 | 20 | 21 | 22 | 23 | 24 | 25 | 26 | 27
    ) {
        return Ok(None);
    }
    let timed = |sound| -> Result<TimedVoice> {
        Ok(TimedVoice {
            sound,
            ticks: sound
                .map(|sound| voices.duration(sound))
                .transpose()?
                .unwrap_or(0),
        })
    };
    let defeat_override = crate::battle_voice::sound(half(metadata, 0xf4)?);
    if let Some(sound) = defeat_override {
        profile.voices.get_or_insert_with(Default::default).defeat = Some(sound);
    }
    let defeat = timed(profile.voices.as_ref().and_then(|voices| voices.defeat))?;
    let opening = (selector == 20)
        .then(|| timed(crate::battle_voice::sound(half(metadata, 0xf6)?)))
        .transpose()?;
    Ok(Some(Presentation { opening, defeat }))
}

fn guard_policy(
    value: u8,
    default_bonus: u8,
) -> Result<(resonance_content::battle_enemy::EntryRow, u8)> {
    use resonance_content::battle_enemy::EntryRow;
    let row = match value {
        0 => EntryRow::Random,
        1 => EntryRow::Front,
        2 => EntryRow::Middle,
        3 => EntryRow::Back,
        _ => anyhow::bail!("invalid enemy entry row {value}"),
    };
    let bonus = if value == 0 {
        default_bonus
    } else {
        resonance_content::battle_recoil::strategy_guard_recovery(value)
    };
    Ok((row, bonus))
}

fn enemy_strategy(bytes: &[u8]) -> Result<[u8; 3]> {
    let start = usize::from(half(bytes, 6)?);
    Ok(bytes
        .get(start..start + 3)
        .context("truncated enemy strategy")?
        .try_into()
        .unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guard_policy_decodes_placement_and_resolves_actual_recovery_bonus() -> Result<()> {
        use resonance_content::battle_enemy::EntryRow;
        for (value, row, bonus) in [
            (0, EntryRow::Random, 7),
            (1, EntryRow::Front, 0),
            (2, EntryRow::Middle, 5),
            (3, EntryRow::Back, 25),
        ] {
            assert_eq!(guard_policy(value, 7)?, (row, bonus));
        }
        assert!(guard_policy(4, 7).is_err());
        Ok(())
    }
}
