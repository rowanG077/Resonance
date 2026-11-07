//! Steam's wider effect records stop at the shared effect compiler boundary.
use crate::{
    audio_effects, effect_atlas, model, monsters, output, pe, preview_model, source::Source,
};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_effect::{
        EffectOperation,
        declaration::Declaration,
        document::{Artwork, Definition},
        visual::ParticleTexture,
    },
    battle_model::document::Model,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

mod embedded_outline;
mod fixed_atlas;
mod gatekeeper_artwork;
mod lightning_artwork;
mod model_artwork;
mod model_color;
mod opacity;
mod rates;
mod resident;
mod stat_up_style;
pub(crate) use model_color::normalize as normalize_model_color;
pub(crate) use opacity::bind as bind_scene_opacity;
pub(crate) use resident::cook as cook_resident;

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
fn modifier(bytes: &[u8], start: usize) -> Result<(Vec<u8>, u32)> {
    let mut at = start;
    let mut result = Vec::new();
    let mut render_flags = 0;
    loop {
        let opcode = pe::half(bytes, at)?;
        if opcode == 0xffff {
            result.extend(opcode.to_be_bytes());
            return Ok((result, render_flags));
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
        if opcode == 21 {
            // These constructor ORs select an already supported static visual
            // and actor ordering. Intern a declaration variant at the call
            // site rather than adding mutable pipeline state to the runtime.
            const STATIC_RENDER_FLAGS: u32 = 0x80000 | 0x2000;
            let flags = pe::word(bytes, at + 4)?;
            render_flags |= flags & STATIC_RENDER_FLAGS;
            let retained = (flags & !STATIC_RENDER_FLAGS).to_le_bytes();
            words[2] = u16::from_le_bytes(retained[..2].try_into()?);
            words[3] = u16::from_le_bytes(retained[2..].try_into()?);
        }
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
    let mut actor_rows = actors
        .chunks_exact(360)
        .map(<[u8]>::to_vec)
        .collect::<Vec<_>>();
    let mut variants = BTreeMap::new();
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
            let (bytes, flags) = modifier(data(2), pointer - offsets[2])?;
            result.extend(bytes);
            edits.insert(pointer, (offset, flags));
        }
    }
    result[12..14].copy_from_slice(&short(modifier_start)?);
    let command_start = result.len();
    result[10..12].copy_from_slice(&short(command_start)?);
    let mut sounds = Vec::new();
    for row in commands.chunks_exact(8) {
        result.extend(pe::half(row, 0)?.to_be_bytes());
        let value = pe::word(row, 4)?;
        let mut command = row[2];
        if value != 0 && (command < 252 || command == 253) {
            let flags = edits[&(value as usize)].1;
            if flags != 0 {
                ensure!(
                    command < 252,
                    "static render flags cannot change a retained particle"
                );
                command = if let Some(&variant) = variants.get(&(command, flags)) {
                    variant
                } else {
                    ensure!(
                        actor_rows.len() < 252,
                        "too many effect declaration variants"
                    );
                    let mut row = actor_rows
                        .get(usize::from(command))
                        .context("modifier actor exceeds pool")?
                        .clone();
                    let combined = pe::word(&row, 24)? | flags;
                    row[24..28].copy_from_slice(&combined.to_le_bytes());
                    let variant = actor_rows.len() as u8;
                    actor_rows.push(row);
                    variants.insert((command, flags), variant);
                    variant
                };
            }
        }
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
            253 | 0..=251 if value != 0 => (row[3], u16::try_from(edits[&(value as usize)].0)?),
            _ => {
                let value = if (value as i32) < 0 {
                    i16::try_from(value as i32).context("expanded negative effect operand")? as u16
                } else {
                    u16::try_from(value).context("expanded effect command operand")?
                };
                (row[3], value)
            }
        };
        result.extend([command, argument]);
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
    if bytes[5] == 0 {
        // Keep an empty optional UV-root section distinct from the following
        // actor pool. The legacy reader uses shared starts to detect empty pools.
        result.extend([0; 2]);
    }
    // Only section starts and modifier pointers use the GC decoder's u16
    // addresses. Put the large actor pool last so a bank may exceed 64 KiB
    // without changing any binary reader or cooked/runtime representation.
    let position = short(result.len())?;
    result[8..10].copy_from_slice(&position);
    let mut strides = Vec::new();
    for row in &actor_rows {
        strides.push(row[6]);
        result.extend(actor(row)?);
    }
    Ok(Canonical {
        bytes: result,
        sounds,
        strides,
    })
}

pub(crate) fn decode(
    bytes: &[u8],
    bones: &[String],
    bind: impl FnMut(u8, &mut Declaration) -> Result<()>,
) -> Result<Definition> {
    decode_including(bytes, bones, &[], bind)
}

pub(crate) fn decode_including(
    bytes: &[u8],
    bones: &[String],
    include: &[u8],
    mut bind: impl FnMut(u8, &mut Declaration) -> Result<()>,
) -> Result<Definition> {
    let source = canonical(bytes)?;
    let mut definition = resonance_importer::decode_effect_bank(
        &source.bytes,
        bones,
        include,
        |actor, declaration| bind(source.strides[usize::from(actor)], declaration),
    )?;
    rates::normalize(&mut definition);
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
    let mut wings = BTreeSet::new();
    if metadata[0x1f2] != 0 {
        let rows =
            preview_model::member(package, 0x1d8)?.context("missing attached effect models")?;
        for row in rows
            .get(..usize::from(metadata[0x1f2]) * 40)
            .context("truncated attached model table")?
            .chunks_exact(40)
        {
            if row[3] & 0x40 != 0 {
                ensure!(
                    usize::from(row[0]) < count && row[3] == 0x70,
                    "layered wing attachment source recipe changed"
                );
                wings.insert(row[0]);
            }
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
        let model_owner = format!("{owner}/models/model-{slot}");
        let layered_wing = wings.contains(&(slot as u8));
        let (mut layers, mut native) = if layered_wing {
            // These source-declared additive wing layers have no outline or
            // character depth pass. Other effect models retain their existing
            // source-shell classification and rendering recipe.
            ensure!(
                outline.is_empty()
                    && matches!(
                        body.to_ascii_lowercase().as_str(),
                        "bh00_obj003.tomdlb_d" | "bh22_obj004.tomdlb_d" | "ba03_obj001.tomdlb_d"
                    ),
                "layered wing source body or outline changed"
            );
            let (scene, native) =
                preview_model::part(source, library, &body, &model_owner, "body", &motions)?;
            (
                vec![resonance_content::model_preview::PreviewPart {
                    scene,
                    animation: None,
                    attached_to: None,
                    additive: false,
                    uv_offsets: Vec::new(),
                }],
                native,
            )
        } else {
            let (preview, native) =
                preview_model::cook(source, library, [&body, &outline], &model_owner, &motions)?;
            (preview.parts, native)
        };
        normalize_model_color(source, library, &body, &layers[0].scene, &mut native)?;
        if !layered_wing
            && outline.is_empty()
            && embedded_outline::present_with_clips(&native, library, &layers[0].scene)?
        {
            layers.retain(|part| part.scene.outline_color.is_none());
        }
        if layered_wing {
            ensure!(
                outline.is_empty()
                    && native.materials.textures.len() == 1
                    && native.materials.records.iter().all(|material| {
                        matches!(material.textures.as_slice(), [slot] if slot.kind == 0 && slot.texture == 0)
                    }),
                "layered wing source no longer has one body color atlas"
            );
            model_artwork::wing_color_slot(&mut layers[0].scene)?;
        }
        for part in &mut layers {
            part.animation = None;
            part.scene.autoplay = false;
        }
        models.insert(
            slot as u8,
            Model {
                attachment_nodes: Default::default(),
                independent_layers: Default::default(),
                layers,
            },
        );
    }
    Ok(models)
}

/// Programs, sprite atlases and models use the ordinary enemy-local effect document.
pub(crate) fn cook_enemies(
    source: &Source,
    library: &Path,
    selected: Option<&[u16]>,
) -> Result<()> {
    let image = pe::Image::parse(&source.executable)?;
    let atlases = effect_atlas::Directory::read(&image)?;
    let residents = effect_atlas::residents(source, &image)?;
    let count = monsters::count(&image)?;
    ensure!(
        selected.is_none_or(|ids| ids.iter().all(|&id| usize::from(id) < count)),
        "enemy effect selection exceeds catalogue"
    );
    let mut failures = Vec::new();
    let mut prepared = 0;
    let mut model_count = 0;
    let mut image_count = 0;
    for id in 0..count as u16 {
        if selected.is_some_and(|ids| !ids.contains(&id)) {
            continue;
        }
        let result = (|| -> Result<(usize, usize)> {
            let bytes = source.require(monsters::package(&image, id)?)?;
            let mut definition = if let Some(bank) = preview_model::member(&bytes, 0x1cc)? {
                let body = source.require(
                    &preview_model::reference(&bytes, 0x18)?
                        .context("missing enemy effect owner body")?,
                )?;
                let bones = model::Rig::read(&body)?.joint_names();
                decode(bank, &bones, |stride, declaration| {
                    if let Declaration::Particle { visual, .. } = declaration
                        && let ParticleTexture::Atlas { palette_set, .. } = &mut visual.texture
                    {
                        *palette_set = stride;
                    }
                    Ok(())
                })?
            } else {
                Definition {
                    programs: Vec::new(),
                    template_names: Vec::new(),
                    templates: Vec::new(),
                    artwork: None,
                }
            };
            if id == 44 {
                let bank = preview_model::member(&bytes, 0x1cc)?
                    .context("missing Living Armor effect bank")?;
                stat_up_style::normalize(id, bank, &mut definition)?;
            }
            let dome = gatekeeper_artwork::Dome::read(id, &bytes, &definition)?;
            let owner = format!("battle/enemies/{}/effects", monsters::symbol(id));
            let slots = definition
                .templates
                .iter()
                .filter_map(|declaration| declaration.particle_visual().ok()?.texture.slot())
                .collect::<BTreeSet<_>>();
            let mut textures = BTreeMap::new();
            for slot in slots {
                let name = if let Some(name) = residents.get(&slot) {
                    name.clone()
                } else {
                    let field = match slot {
                        2 => 0x1d0,
                        14 => 0x1d4,
                        _ => anyhow::bail!("unbound enemy effect texture slot {slot}"),
                    };
                    preview_model::reference(&bytes, field)?
                        .with_context(|| format!("missing enemy effect atlas {slot}"))?
                };
                let texture = effect_atlas::cook_with_pixels(
                    source,
                    library,
                    &mut definition,
                    &name,
                    atlases.get(&name),
                    &owner,
                    slot,
                    |member, palette, pixels| {
                        if let Some(dome) = &dome {
                            dome.normalize(slot, member, palette, pixels);
                        }
                    },
                )?;
                textures.insert(slot, vec![texture]);
            }
            lightning_artwork::bind(
                source,
                library,
                &image,
                &atlases,
                id,
                &mut definition,
                &mut textures,
            )?;
            let images = textures
                .values()
                .flatten()
                .map(|texture| texture.images.len())
                .sum();
            let models = models(source, library, &bytes, &owner)?;
            let count = models.len();
            definition.artwork = Some(Artwork::from_models(textures, models)?);
            crate::battle_enemies::bind_carried_effects(
                source,
                library,
                &bytes,
                &monsters::symbol(id),
                &mut definition,
            )?;
            bind_scene_opacity(&mut definition)?;
            output::json(&library.join(&owner).join("effects.json"), &definition)?;
            Ok((count, images))
        })();
        match result {
            Ok((count, images)) => {
                prepared += 1;
                model_count += count;
                image_count += images;
            }
            Err(error) => {
                eprintln!("  Enemy {id} effects: {error:#}");
                failures.push(id);
            }
        }
    }
    eprintln!(
        "  Enemy effects: {prepared} program banks, {model_count} model slots and {image_count} sprite images"
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
    fn correlated_scratch_addition_keeps_one_draw_and_scales_discrete_steps() -> Result<()> {
        let mut actor = vec![0; 360];
        actor[0] = 5;
        actor[2] = 255;
        actor[58] = 255;
        let mut edits = Vec::new();
        edits.extend(11u16.to_le_bytes());
        edits.extend(0x7ff8u16.to_le_bytes());
        edits.extend(300u16.to_le_bytes());
        edits.extend(0u16.to_le_bytes());
        for (opcode, target, ignored_literal) in [(8u16, 0x7ff8u16, 60f32), (7, 0xb8, 0.)] {
            edits.extend(opcode.to_le_bytes());
            edits.extend(target.to_le_bytes());
            edits.extend(ignored_literal.to_le_bytes());
            edits.extend(0x7ff8u16.to_le_bytes());
            edits.extend(0u16.to_le_bytes());
        }
        edits.extend(u16::MAX.to_le_bytes());
        let mut spawn = [0; 8];
        spawn[4..8].copy_from_slice(&400u32.to_le_bytes());
        let definition = decode(
            &fixture(&actor, &edits, &[spawn, [1, 0, 254, 0, 0, 0, 0, 0]]),
            &[],
            |_, _| Ok(()),
        )?;
        let EffectOperation::Spawn { birth, .. } = &definition.programs[0][0].operation else {
            panic!("size birth")
        };
        assert_eq!(birth.random_draws, 1);
        assert!(birth.sums.is_empty());
        let range = birth.size[0].context("missing doubled scratch range")?;
        assert!((range.step - 0.2).abs() < f32::EPSILON);
        assert_eq!(range.draw, Some(0));
        let original = resonance_content::battle_effect::ValueRange {
            min: -29.9,
            max: 29.9,
            step: 0.1,
            draw: Some(0),
            reverse: false,
        };
        for unit in [0., 0.1, 0.5, 0.999] {
            assert!((range.sample(unit) - 2. * original.sample(unit)).abs() < 0.0001);
        }
        birth.validate()?;
        Ok(())
    }

    #[test]
    fn retained_angle_addition_updates_later_birth_scratch_without_a_runtime_register() -> Result<()>
    {
        let mut actor = vec![0; 360];
        actor[0] = 14;
        actor[2] = 255;
        actor[24..28].copy_from_slice(&0x40000u32.to_le_bytes());
        actor[58] = 255;
        let mut edits = Vec::new();
        for target in [0x68u16, 0x7ffb] {
            edits.extend(8u16.to_le_bytes());
            edits.extend(target.to_le_bytes());
            edits.extend((-30f32).to_le_bytes());
            edits.extend(0u32.to_le_bytes());
        }
        edits.extend(u16::MAX.to_le_bytes());
        let copy = 400 + edits.len() as u32;
        edits.extend(7u16.to_le_bytes());
        edits.extend(0x68u16.to_le_bytes());
        edits.extend(0f32.to_le_bytes());
        edits.extend(0x7ffbu16.to_le_bytes());
        edits.extend(0u16.to_le_bytes());
        edits.extend(u16::MAX.to_le_bytes());
        let mut delayed = [2, 0, 253, 0, 0, 0, 0, 0];
        delayed[4..8].copy_from_slice(&400u32.to_le_bytes());
        let mut spawn = [3, 0, 0, 0, 0, 0, 0, 0];
        spawn[4..8].copy_from_slice(&copy.to_le_bytes());
        let definition = decode(
            &fixture(
                &actor,
                &edits,
                &[[0; 8], delayed, spawn, [4, 0, 254, 0, 0, 0, 0, 0]],
            ),
            &[],
            |_, _| Ok(()),
        )?;
        let EffectOperation::SetMotion { motion, .. } = &definition.programs[0][1].operation else {
            anyhow::bail!("missing delayed roll")
        };
        assert_eq!(motion.angles_add, Some([0., 0., -30.]));
        let EffectOperation::Spawn { birth, .. } = &definition.programs[0][2].operation else {
            anyhow::bail!("missing later birth")
        };
        assert_eq!(
            birth.angles[2],
            Some(resonance_content::battle_effect::ValueRange::fixed(-30.))
        );
        Ok(())
    }

    #[test]
    fn retained_angular_velocity_addition_keeps_its_axis_and_prior_assignment() -> Result<()> {
        let mut actor = vec![0; 360];
        actor[0] = 14;
        actor[2] = 255;
        actor[24..28].copy_from_slice(&0x40000u32.to_le_bytes());
        actor[58] = 255;
        let mut edits = Vec::new();
        for (opcode, value) in [(7u16, 2.5f32), (8, -2.5)] {
            edits.extend(opcode.to_le_bytes());
            edits.extend(0x70u16.to_le_bytes());
            edits.extend(value.to_le_bytes());
            edits.extend(0u32.to_le_bytes());
        }
        edits.extend(u16::MAX.to_le_bytes());
        let mut delayed = [2, 0, 253, 0, 0, 0, 0, 0];
        delayed[4..8].copy_from_slice(&400u32.to_le_bytes());
        let definition = decode(
            &fixture(
                &actor,
                &edits,
                &[[0; 8], delayed, [3, 0, 254, 0, 0, 0, 0, 0]],
            ),
            &[],
            |_, _| Ok(()),
        )?;
        let EffectOperation::SetMotion { motion, .. } = &definition.programs[0][1].operation else {
            panic!("delayed motion")
        };
        assert_eq!(motion.angular_velocity, [None, Some(2.5), None]);
        assert_eq!(motion.angular_velocity_add, Some([0., -2.5, 0.]));
        Ok(())
    }

    #[test]
    fn constructor_render_flags_select_an_interned_static_declaration() -> Result<()> {
        let mut actor = vec![0; 360];
        actor[0] = 5;
        actor[2] = 255;
        actor[58] = 255;
        let mut edits = Vec::new();
        for flags in [0x80000u32, 0x2000] {
            edits.extend(21u16.to_le_bytes());
            edits.extend(0x18u16.to_le_bytes());
            edits.extend(flags.to_le_bytes());
        }
        edits.extend(u16::MAX.to_le_bytes());
        let mut changed = [0; 8];
        changed[4..8].copy_from_slice(&400u32.to_le_bytes());
        let bank = fixture(
            &actor,
            &edits,
            &[[0; 8], changed, changed, [1, 0, 254, 0, 0, 0, 0, 0]],
        );
        let normalized = canonical(&bank)?;
        assert_eq!(normalized.strides.len(), 2);
        let definition = decode(&bank, &[], |_, _| Ok(()))?;
        let particles = definition.programs[0]
            .iter()
            .map(|event| {
                let EffectOperation::Spawn { particle, .. } = event.operation else {
                    panic!("particle constructor");
                };
                particle
            })
            .collect::<Vec<_>>();
        assert_ne!(particles[0], particles[1]);
        assert_eq!(particles[1], particles[2]);
        for (particle, enabled) in [(particles[0], true), (particles[1], false)] {
            let declaration = &definition.templates[usize::from(particle)];
            assert_eq!(declaration.particle_visual()?.depth_test, enabled);
            assert_eq!(declaration.particle_visual()?.after_actor, !enabled);
            assert_eq!(declaration.template()?.draw_after_target, !enabled);
        }
        Ok(())
    }

    #[test]
    fn square_band_controller_and_primary_only_color_flag_keep_the_common_contract() -> Result<()> {
        let mut actor = vec![0; 360];
        actor[0] = 20;
        actor[2] = 255;
        actor[58] = 255;
        actor[24..28].copy_from_slice(&0x8000_0008u32.to_le_bytes());
        actor[188..192].copy_from_slice(&4f32.to_le_bytes());
        actor[192..196].copy_from_slice(&300f32.to_le_bytes());
        let bank = fixture(&actor, &[], &[[0; 8], [1, 0, 254, 0, 0, 0, 0, 0]]);
        let definition = decode(&bank, &[], |_, _| Ok(()))?;
        let declaration = &definition.templates[0];
        assert_eq!(
            declaration.particle_visual()?.geometry,
            resonance_content::battle_effect::visual::ParticleShape::SquareBand
        );
        assert!(declaration.template()?.brighten_primary_only);
        assert!(matches!(
            declaration.template()?.state.geometry,
            resonance_content::battle_effect::ParticleGeometry::Size {
                value: [0., 4., 300.],
                ..
            }
        ));
        Ok(())
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
    fn large_actor_pool_keeps_legacy_addresses_small_without_truncation() -> Result<()> {
        let mut actors = vec![0; 200 * 360];
        for row in actors.chunks_exact_mut(360) {
            row[0] = 22;
            row[16..18].copy_from_slice(&8u16.to_le_bytes());
            row[24..28].copy_from_slice(&0x1234u32.to_le_bytes());
        }
        let bank = fixture(
            &actors,
            &[],
            &[[0, 0, 199, 0, 0, 0, 0, 0], [1, 0, 254, 0, 0, 0, 0, 0]],
        );
        let normalized = canonical(&bank)?;
        assert!(normalized.bytes.len() > usize::from(u16::MAX));
        let definition = decode(&bank, &[], |_, _| Ok(()))?;
        assert!(matches!(
            definition.programs[0][0].operation,
            EffectOperation::Shake {
                duration: 8,
                amplitude: 0x1234
            }
        ));
        Ok(())
    }

    #[test]
    fn unavailable_program_does_not_publish_its_valid_sound_prefix() -> Result<()> {
        let mut sound = [0, 0, 252, 2, 0, 0, 0, 0];
        sound[4..8].copy_from_slice(&310u32.to_le_bytes());
        let mut bank = fixture(&[], &[], &[sound]);
        let definition = decode(&bank, &[], |_, _| Ok(()))?;
        assert!(matches!(
            definition.programs[0].as_slice(),
            [resonance_content::battle_effect::ScheduledEvent {
                at: 0,
                operation: EffectOperation::Unsupported { .. }
            }]
        ));
        bank[8..12].fill(0);
        assert!(decode(&bank, &[], |_, _| Ok(())).is_err());
        Ok(())
    }

    #[test]
    fn no_draw_controller_retains_its_shared_particle_timeline() -> Result<()> {
        let mut actor = vec![0; 360];
        actor[0] = 14;
        actor[2] = 255;
        actor[16..18].copy_from_slice(&6u16.to_le_bytes());
        actor[58] = 255;
        let bank = fixture(&actor, &[], &[[0; 8], [6, 0, 254, 0, 0, 0, 0, 0]]);
        let definition = decode(&bank, &[], |_, _| Ok(()))?;
        assert!(
            matches!(definition.templates[0], Declaration::Particle { ref template, ref visual }
            if template.lifetime == Some(7) && matches!(visual.geometry, resonance_content::battle_effect::visual::ParticleShape::NoDraw))
        );
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
}
