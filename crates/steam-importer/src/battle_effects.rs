//! Steam's wider effect records stop at the shared effect compiler boundary.
use crate::{audio_effects, model, monsters, output, pe, preview_model, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_effect::{
        EffectOperation,
        declaration::Declaration,
        document::{Artwork, Definition},
    },
    battle_model::document::Model,
};
use std::{collections::BTreeMap, path::Path};

struct Canonical {
    bytes: Vec<u8>,
    sounds: Vec<u16>,
    strides: Vec<u8>,
}

fn short(value: usize) -> Result<[u8; 2]> {
    Ok(u16::try_from(value)
        .context("effect bank exceeds canonical offset range")?
        .to_be_bytes())
}

fn actor(bytes: &[u8]) -> Result<Vec<u8>> {
    ensure!(bytes.len() == 360, "truncated Steam effect actor");
    ensure!(
        pe::word(bytes, 28)? == 0,
        "expanded particle flags are not converted"
    );
    let mut row = vec![0; 352];
    row[..20].copy_from_slice(&bytes[..20]);
    row[20..24].copy_from_slice(&bytes[24..28]);
    row[24..].copy_from_slice(&bytes[32..]);
    for at in (8..18)
        .step_by(2)
        .chain((24..40).step_by(2))
        .chain(std::iter::once(136))
    {
        row[at..at + 2].reverse();
    }
    for at in std::iter::once(20)
        .chain((52..136).step_by(4))
        .chain((152..352).step_by(4))
    {
        // Model playback occupies four byte fields; notices occupy a string union.
        if row[0] == 3 && at == 212 || row[0] == 25 && at >= 176 {
            continue;
        }
        row[at..at + 4].reverse();
    }
    Ok(row)
}

/// Modifier targets address the unexpanded declaration. Floating point operands
/// use whole words; the remaining operands are independent halfwords.
fn modifier(bytes: &[u8], start: usize) -> Result<Vec<u8>> {
    let mut at = start;
    let mut result = Vec::new();
    loop {
        let opcode = pe::half(bytes, at)?;
        if opcode == 0xffff {
            result.extend(opcode.to_be_bytes());
            return Ok(result);
        }
        let count = match opcode {
            0 | 2..=6 | 10..=12 | 21 | 23..=25 => 4,
            1 | 22 => 8,
            7 | 8 | 13..=15 | 18 | 26 | 27 | 30 => 6,
            9 => 12,
            _ => anyhow::bail!("Steam particle modifier {opcode} is not converted"),
        };
        let mut words = (0..count)
            .map(|index| pe::half(bytes, at + index * 2))
            .collect::<Result<Vec<_>>>()?;
        words[1] = match opcode {
            21 => 0x14,
            22 => 0x5c,
            26 | 27 => 0x40,
            _ => match words[1] {
                0..=0x13 | 0x7ff8..=0x7fff => words[1],
                0x20..=0x167 => words[1] - 8,
                target => anyhow::bail!("Steam particle destination {target:#x} is not converted"),
            },
        };
        let pairs: &[usize] = match opcode {
            1 => &[2, 4, 6],
            9 => &[2, 4, 6, 8],
            7 | 8 | 13..=15 | 21 | 26 | 27 | 30 => &[2],
            _ => &[],
        };
        for &pair in pairs {
            words.swap(pair, pair + 1);
        }
        for word in words {
            result.extend(word.to_be_bytes());
        }
        at += count * 2;
    }
}

fn canonical(bytes: &[u8]) -> Result<Canonical> {
    ensure!(
        bytes.len() >= 40 && bytes.starts_with(b"ef1\0"),
        "invalid Steam effect bank"
    );
    let offsets = (8..32)
        .step_by(4)
        .map(|at| pe::word(bytes, at).map(|n| n as usize))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        offsets.iter().all(|&at| (40..=bytes.len()).contains(&at)),
        "effect section outside bank"
    );
    let pool = |index| -> &[u8] {
        let start = offsets[index];
        let end = offsets
            .iter()
            .copied()
            .filter(|&at| at > start)
            .min()
            .unwrap_or(bytes.len());
        &bytes[start..end]
    };
    let data = |index| {
        if offsets[index] == offsets[4] {
            &[][..]
        } else {
            pool(index)
        }
    };
    let actors = if offsets[1..].contains(&offsets[0]) {
        &[][..]
    } else {
        data(0)
    };
    ensure!(
        actors.len().is_multiple_of(360),
        "misaligned Steam effect actor pool"
    );
    let commands = data(1);
    ensure!(
        commands.len().is_multiple_of(8),
        "misaligned Steam effect commands"
    );
    let uv = data(3);
    ensure!(
        uv.len().is_multiple_of(12),
        "misaligned Steam effect UV pool"
    );
    let mut result = bytes[..20].to_vec();
    result[8..20].fill(0);
    let position = short(result.len())?;
    result[8..10].copy_from_slice(&position);
    let mut strides = Vec::new();
    for row in actors.chunks_exact(360) {
        strides.push(row[6]);
        result.extend(actor(row)?);
    }
    let modifier_start = result.len();
    let mut edits = BTreeMap::new();
    for row in commands.chunks_exact(8) {
        let pointer = pe::word(row, 4)? as usize;
        if pointer != 0 && (row[2] < 252 || row[2] == 253) && !edits.contains_key(&pointer) {
            ensure!(
                (offsets[2]..offsets[2] + data(2).len()).contains(&pointer),
                "effect modifier outside pool"
            );
            let offset = result.len();
            result.extend(modifier(data(2), pointer - offsets[2])?);
            edits.insert(pointer, offset);
        }
    }
    result[12..14].copy_from_slice(&short(modifier_start)?);
    let command_start = result.len();
    result[10..12].copy_from_slice(&short(command_start)?);
    let mut sounds = Vec::new();
    for row in commands.chunks_exact(8) {
        result.extend(pe::half(row, 0)?.to_be_bytes());
        let value = pe::word(row, 4)?;
        let (argument, operand) = match row[2] {
            252 => {
                let sound = audio_effects::id(value as u16)?;
                let index = if let Some(index) = sounds.iter().position(|&old| old == sound) {
                    index
                } else {
                    sounds.push(sound);
                    sounds.len() - 1
                };
                (
                    u8::try_from(index).context("too many effect sounds")?,
                    u16::from(row[3]),
                )
            }
            253 | 0..=251 if value != 0 => (row[3], u16::try_from(edits[&(value as usize)])?),
            _ => {
                let value = if (value as i32) < 0 {
                    i16::try_from(value as i32).context("expanded negative effect operand")? as u16
                } else {
                    u16::try_from(value).context("expanded effect command operand")?
                };
                (row[3], value)
            }
        };
        result.extend([row[2], argument]);
        result.extend(operand.to_be_bytes());
    }
    let position = short(result.len())?;
    result[14..16].copy_from_slice(&position);
    for row in uv.chunks_exact(12) {
        result.extend_from_slice(&row[..2]);
        for at in (4..12).step_by(2) {
            result.extend(pe::half(row, at)?.to_be_bytes());
        }
    }
    let position = short(result.len())?;
    result[16..18].copy_from_slice(&position);
    for index in 0..usize::from(bytes[4]) {
        let root = pe::half(pool(4), index * 2)? as i16;
        let root = usize::try_from(root).context("negative effect program root")?;
        ensure!(
            root.is_multiple_of(8) && root < commands.len(),
            "effect root outside commands"
        );
        result.extend(short(root / 8 * 6)?);
    }
    let position = short(result.len())?;
    result[18..20].copy_from_slice(&position);
    for index in 0..usize::from(bytes[5]) {
        let root = usize::from(pe::half(pool(5), index * 2)?);
        ensure!(
            root.is_multiple_of(12) && root < uv.len(),
            "effect UV root outside pool"
        );
        result.extend(short(root / 12 * 10)?);
    }
    Ok(Canonical {
        bytes: result,
        sounds,
        strides,
    })
}

fn decode(
    bytes: &[u8],
    bones: &[String],
    mut bind: impl FnMut(u8, &mut Declaration) -> Result<()>,
) -> Result<Definition> {
    let source = canonical(bytes)?;
    let mut definition =
        resonance_importer::decode_effect_bank(&source.bytes, bones, |actor, declaration| {
            bind(source.strides[usize::from(actor)], declaration)
        })?;
    for event in definition.programs.iter_mut().flatten() {
        if let EffectOperation::Sound { id, .. } = &mut event.operation {
            *id = *source
                .sounds
                .get(usize::from(*id))
                .context("missing normalized effect sound")?;
        }
    }
    Ok(definition)
}

fn models(
    source: &Source,
    library: &Path,
    package: &[u8],
    owner: &str,
) -> Result<BTreeMap<u8, Model>> {
    let metadata = monsters::metadata(package)?;
    let count = usize::from(metadata[0x1f4]);
    ensure!(count <= 6, "enemy effect model count exceeds directory");
    let mut attached = BTreeMap::<u8, Vec<u16>>::new();
    if metadata[0x1f2] != 0 {
        let rows =
            preview_model::member(package, 0x1d8)?.context("missing attached effect models")?;
        for row in rows
            .get(..usize::from(metadata[0x1f2]) * 40)
            .context("truncated attached model table")?
            .chunks_exact(40)
        {
            if row[1] != 0 {
                attached.entry(row[0]).or_default().push(u16::from(row[1]));
            }
        }
    }
    let mut models = BTreeMap::new();
    for slot in 0..count {
        let Some(body) = preview_model::reference(package, 0x180 + slot * 4)? else {
            continue;
        };
        let outline = preview_model::reference(package, 0x198 + slot * 4)?.unwrap_or_default();
        let mut motions = Vec::new();
        if let Some(motion) = preview_model::reference(package, 0x1b0 + slot * 4)? {
            motions.push((0, motion));
        }
        if let Some(slots) = attached.get(&(slot as u8)) {
            for &motion in slots {
                ensure!(motion < 80, "attached effect motion exceeds directory");
                if let Some(name) =
                    preview_model::reference(package, 0x20 + usize::from(motion) * 4)?
                {
                    if !motions.iter().any(|(slot, _)| *slot == motion) {
                        motions.push((motion, name));
                    }
                }
            }
        }
        let (mut preview, _) = preview_model::cook(
            source,
            library,
            [&body, &outline],
            &format!("{owner}/models/model-{slot}"),
            &motions,
        )?;
        for part in &mut preview.parts {
            part.animation = None;
            part.scene.autoplay = false;
        }
        models.insert(
            slot as u8,
            Model {
                attachment_nodes: Default::default(),
                independent_layers: Default::default(),
                layers: preview.parts,
            },
        );
    }
    Ok(models)
}

/// Programs and models use the ordinary enemy-local effect document. Atlas
/// images are a separate remaining media owner; missing images are not invented.
pub(crate) fn cook_enemies(
    source: &Source,
    library: &Path,
    selected: Option<&[u16]>,
) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let count = monsters::count(&image)?;
    ensure!(
        selected.is_none_or(|ids| ids.iter().all(|&id| usize::from(id) < count)),
        "enemy effect selection exceeds catalogue"
    );
    let mut failures = Vec::new();
    let mut prepared = 0;
    let mut model_count = 0;
    for id in 0..count as u16 {
        if selected.is_some_and(|ids| !ids.contains(&id)) {
            continue;
        }
        let result = (|| -> Result<usize> {
            let bytes = source.require(monsters::package(&image, id)?)?;
            let mut definition = if let Some(bank) = preview_model::member(&bytes, 0x1cc)? {
                let body = source.require(
                    &preview_model::reference(&bytes, 0x18)?
                        .context("missing enemy effect owner body")?,
                )?;
                let bones = model::Rig::read(&body)?.joint_names();
                decode(bank, &bones, |_, _| Ok(()))?
            } else {
                Definition {
                    programs: Vec::new(),
                    template_names: Vec::new(),
                    templates: Vec::new(),
                    artwork: None,
                }
            };
            let owner = format!("battle/enemies/{}/effects", monsters::symbol(id));
            let models = models(source, library, &bytes, &owner)?;
            let count = models.len();
            definition.artwork = Some(Artwork::from_models(BTreeMap::new(), models)?);
            output::json(&library.join(&owner).join("effects.json"), &definition)?;
            Ok(count)
        })();
        match result {
            Ok(count) => {
                prepared += 1;
                model_count += count;
            }
            Err(error) => {
                eprintln!("  Enemy {id} effects: {error:#}");
                failures.push(id);
            }
        }
    }
    eprintln!(
        "  Enemy effects: {prepared} program banks and {model_count} model slots; sprite atlases remain unported"
    );
    ensure!(
        failures.is_empty(),
        "enemy effect banks remain unconverted: {failures:?}"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Language, model::Rig, preview_model, source::Source};
    use std::path::PathBuf;

    fn fixture(actor: &[u8], edits: &[u8], commands: &[[u8; 8]]) -> Vec<u8> {
        let modifier = 40 + actor.len();
        let command = modifier + edits.len();
        let uv = command + commands.len() * 8;
        let mut bank = vec![0; uv + 2];
        bank[..4].copy_from_slice(b"ef1\0");
        bank[4] = 1;
        for (index, offset) in [40, command, modifier, uv, uv, uv + 2]
            .into_iter()
            .enumerate()
        {
            bank[8 + index * 4..12 + index * 4].copy_from_slice(&(offset as u32).to_le_bytes());
        }
        bank[40..modifier].copy_from_slice(actor);
        bank[modifier..command].copy_from_slice(edits);
        for (index, row) in commands.iter().enumerate() {
            bank[command + index * 8..command + (index + 1) * 8].copy_from_slice(row);
        }
        bank
    }

    #[test]
    fn sound_only_bank_keeps_wide_sound_ids_without_an_actor_pool() -> Result<()> {
        let mut sound = [0, 0, 252, 2, 0, 0, 0, 0];
        sound[4..8].copy_from_slice(&310u32.to_le_bytes());
        let bank = fixture(&[], &[], &[sound, [1, 0, 254, 0, 0, 0, 0, 0]]);
        let definition = decode(&bank, &[], |_, _| Ok(()))?;
        assert!(definition.templates.is_empty());
        assert!(matches!(
            definition.programs[0][0].operation,
            EffectOperation::Sound {
                id: 822,
                priority: 2
            }
        ));
        let round_trip: Definition = serde_json::from_slice(&serde_json::to_vec(&definition)?)?;
        assert_eq!(round_trip.programs.len(), 1);
        Ok(())
    }

    #[test]
    fn model_byte_fields_and_relocated_float_edits_reach_shared_declarations() -> Result<()> {
        let mut actor = vec![0; 360];
        actor[0] = 3;
        actor[2] = 2;
        actor[6] = 16;
        actor[24..28].copy_from_slice(&0x404u32.to_le_bytes());
        actor[58] = 255;
        for at in [184, 188, 192] {
            actor[at..at + 4].copy_from_slice(&1f32.to_le_bytes());
        }
        actor[220] = 4;
        actor[221] = 3;
        let mut edits = vec![7, 0, 0x3c, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255];
        edits[4..8].copy_from_slice(&2f32.to_le_bytes());
        let mut spawn = [0; 8];
        spawn[3] = 250;
        spawn[4..8].copy_from_slice(&400u32.to_le_bytes());
        let bank = fixture(&actor, &edits, &[spawn, [1, 0, 254, 0, 0, 0, 0, 0]]);
        let mut stride = None;
        let definition = decode(&bank, &["root".into(), "EF00_ARM".into()], |value, _| {
            stride = Some(value);
            Ok(())
        })?;
        assert_eq!(stride, Some(16));
        let Declaration::ModelParticle { binding, .. } = &definition.templates[0] else {
            anyhow::bail!("model declaration changed kind")
        };
        assert_eq!((binding.slot, binding.animation), (4, Some(3)));
        let EffectOperation::Spawn { birth, .. } = &definition.programs[0][0].operation else {
            anyhow::bail!("missing model birth")
        };
        assert_eq!(birth.origin_bone, Some(1));
        assert_eq!(
            birth.offset[0],
            Some(resonance_content::battle_effect::ValueRange::fixed(2.))
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; audits the complete enemy effect owner"]
    fn installed_enemy_effect_owner_cooks_all_banks_and_models() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let library = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        cook_enemies(&source, &library, None)
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY with enemy body motions; cooks seven contact-model banks"]
    fn installed_enemy_effect_models_unblock_shared_hitbox_baking() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let library = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let ids = [0, 1, 6, 134, 135, 139, 246];
        cook_enemies(&source, &library, Some(&ids))?;
        let image = pe::Image::parse(&source.executable)?;
        let usual = source.require("btlusual.pac")?;
        let package = crate::package::Package::parse(&usual)?;
        let common = package.member(8)?.context("missing common actions")?;
        let mut paths = 0;
        let mut prepared_models = 0;
        for id in ids {
            let definition: Definition =
                serde_json::from_slice(&std::fs::read(library.join(format!(
                    "battle/enemies/{}/effects/effects.json",
                    monsters::symbol(id)
                )))?)?;
            for value in definition
                .artwork
                .context("missing cooked effect models")?
                .models
                .into_values()
            {
                let model: Model = serde_json::from_value(value)?;
                model.prepare(&mut |path| Ok(std::fs::read(library.join(path))?.into()))?;
                prepared_models += 1;
            }
            let bytes = source.require(monsters::package(&image, id)?)?;
            let tables = crate::battle_enemies::actions(&bytes, common, &library, id)?;
            let models = tables
                .projectiles
                .values()
                .filter(|shot| !shot.definition.motion.contact_path.is_empty())
                .collect::<Vec<_>>();
            ensure!(!models.is_empty(), "enemy {id} has no baked model hitboxes");
            for shot in models {
                let points = &shot.definition.motion.contact_path;
                ensure!(
                    points.iter().all(|row| !row.is_empty()
                        && row.iter().flatten().all(|value| value.is_finite())),
                    "invalid baked contact path for enemy {id}"
                );
                paths += 1;
            }
        }
        eprintln!(
            "Seven enemy effect owners prepare {prepared_models} shared models and bake {paths} model-based hitbox paths"
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM; audits Steam effect bank conversion"]
    fn installed_enemy_effect_banks_use_shared_compiler() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let image = pe::Image::parse(&source.executable)?;
        let mut failures = BTreeMap::<String, Vec<u16>>::new();
        let mut prepared = 0;
        for id in 0..crate::monsters::count(&image)? as u16 {
            let filename =
                image.text(pe::word(image.slice(0x8e5b50 + u32::from(id) * 4, 4)?, 0)?)?;
            let bytes = source.require(filename)?;
            let Some(bank) = preview_model::member(&bytes, 0x1cc)? else {
                continue;
            };
            let model = source
                .require(&preview_model::reference(&bytes, 0x18)?.context("missing enemy body")?)?;
            let bones = Rig::read(&model)?.joint_names();
            let result = decode(bank, &bones, |_, _| Ok(()));
            match result {
                Ok(definition) => {
                    let json = serde_json::to_vec(&definition)?;
                    let round_trip: Definition = serde_json::from_slice(&json)?;
                    assert_eq!(round_trip.programs.len(), usize::from(bank[4]));
                    prepared += 1;
                }
                Err(error) => {
                    failures.entry(format!("{error:#}")).or_default().push(id);
                }
            }
        }
        eprintln!("Steam effect banks compiled: {prepared}; remaining: {failures:#?}");
        ensure!(failures.is_empty(), "enemy effect conversion is incomplete");
        Ok(())
    }
}
