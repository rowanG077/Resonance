//! Native action tables become the same named techniques as the GC importer.
use crate::{audio_effects, battle_normal, output, package::Package, pe, source::Source};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_action::ActionHit,
    battle_martial::{Attack, Binding, Catalogue, HammerPattern, Member},
    battle_normal::Event,
    battle_scene::SceneAction,
};
use std::{collections::BTreeMap, path::Path};
mod falcon;
pub(crate) mod ordinary;

pub(crate) fn cook(source: &Source, library: &Path) -> Result<()> {
    let catalogue = read(source)?;
    let mut prepared = 0;
    for (&id, member) in &catalogue.members {
        if let Some(reason) = &member.unsupported_reason {
            eprintln!("  Action {id}: {reason}");
        }
        for (&variant, action) in &member.variants {
            match action {
                SceneAction::Prepared(_) => prepared += 1,
                SceneAction::Unsupported { reason } => {
                    eprintln!("  Action {id}:{variant}: {reason}");
                }
            }
        }
    }
    eprintln!(
        "  Martial actions: {} members, {prepared} prepared tracks",
        catalogue.members.len()
    );
    output::json(
        &library.join(resonance_content::battle_rules::MARTIAL_ATTACKS),
        &resonance_importer::martial_attack_document(&catalogue)?,
    )?;
    ordinary::cook(source, library, &catalogue)
}

pub(crate) fn read(source: &Source) -> Result<Catalogue> {
    let image = pe::Image::parse(&source.executable)?;
    let usual = source.require("btlusual.pac")?;
    let package = Package::parse(&usual)?;
    let directory = package.member(8)?.context("missing martial directory")?;
    let single =
        |address| -> Result<f32> { Ok(f32::from_le_bytes(image.slice(address, 4)?.try_into()?)) };
    let double = |address| -> Result<f32> {
        Ok(f64::from_le_bytes(image.slice(address, 8)?.try_into()?) as f32)
    };
    let input = resonance_importer::MartialAttackSource {
        projectiles: package.member(7)?.context("missing projectiles")?,
        // Colette's native callbacks supply the same launch recipes as GC.
        hammer_volley: [
            0.,
            single(0x72f8cc)?,
            single(0x882e10)?,
            single(0x882bd0)?,
            single(0x72f418)?,
        ],
        hammer_ring: [
            0.,
            single(0x88308c)?,
            single(0x8830b0)?,
            double(0x882fe8)?,
            double(0x8830a8)?,
            double(0x882e18)?,
            double(0x882d18)?,
            double(0x8829c8)?,
            double(0x882b50)?,
        ],
        steal: [0., single(0x882ea4)?, single(0x882bd0)?, single(0x72f418)?],
        voice: battle_normal::voice,
    };
    let count = pe::word(directory, 0)? as usize;
    ensure!(
        count <= directory.len().saturating_sub(4) / 4,
        "truncated martial directory"
    );
    let offsets = (0..count)
        .map(|index| pe::word(directory, 4 + index * 4).map(|value| value as usize))
        .collect::<Result<Vec<_>>>()?;
    let mut members = BTreeMap::new();
    for (index, &start) in offsets.iter().enumerate() {
        if start == 0 || start == directory.len() {
            continue;
        }
        let id = u16::try_from(index)?;
        let result = (|| -> Result<_> {
            ensure!(start >= 4 + count * 4, "martial member overlaps directory");
            let end = offsets
                .iter()
                .copied()
                .filter(|&at| at > start)
                .min()
                .unwrap_or(directory.len());
            let bytes = directory
                .get(start..end)
                .context("martial member outside directory")?;
            let end = pe::word(bytes, 0)? as usize;
            ensure!(
                end >= 16 && end <= bytes.len() && (end - 16).is_multiple_of(28),
                "invalid martial variants"
            );
            let mut variants = BTreeMap::new();
            for variant in 0..(end - 16) / 28 {
                if pe::half(bytes, 16 + variant * 28)? == 0 {
                    continue;
                }
                let prepared = (|| -> Result<Attack> {
                    if matches!(id, 132 | 134 | 135 | 149) {
                        let address = pe::word(image.slice(0x8e98d0 + u32::from(id) * 4, 4)?, 0)?;
                        let handlers = image.slice(address, 8)?;
                        ensure!(
                            pe::word(handlers, 0)? == 0x427dc0
                                && pe::word(handlers, 4)? == 0x427f20,
                            "added action handlers changed"
                        );
                    }
                    let normalized = canonical(bytes, id, variant)?;
                    let mut attack = resonance_importer::decode_martial_attack(
                        id,
                        variant,
                        &normalized,
                        &input,
                    )?;
                    if id == 140 {
                        falcon::normalize(&mut attack)?;
                    }
                    crate::battle_callbacks::attach(source, &image, id, bytes, &mut attack)?;
                    sounds(&mut attack)?;
                    Ok(attack)
                })();
                variants.insert(
                    u8::try_from(variant)?,
                    match prepared {
                        Ok(attack) => SceneAction::Prepared(Box::new(attack)),
                        Err(error) => SceneAction::Unsupported {
                            reason: format!("{error:#}"),
                        },
                    },
                );
            }
            Ok(variants)
        })();
        let (variants, unsupported_reason) = match result {
            Ok(variants) => (variants, None),
            Err(error) => (BTreeMap::new(), Some(format!("{error:#}"))),
        };
        members.insert(
            id,
            Member {
                variants,
                unsupported_reason,
            },
        );
    }
    let original = resonance_importer::martial_attack_bindings();
    let techniques = image.slice(0x8d84b0, 255 * 96)?;
    let learning = image.slice(0x8d82c0, 9 * 41)?;
    let mut bindings = BTreeMap::<u16, BTreeMap<u8, Binding>>::new();
    for (index, row) in learning.as_chunks::<41>().0.iter().enumerate() {
        let character = (index + 1) as u8;
        for &native in row
            .get(1..1 + usize::from(row[0]))
            .context("invalid martial learning list")?
        {
            let member = pe::half(techniques, usize::from(native) * 96)?;
            let Some(action) = members.get(&member) else {
                continue;
            };
            let catalogue = u16::from(native);
            let variant = if let Some(binding) = original
                .get(&catalogue)
                .and_then(|owners| owners.get(&character))
                .filter(|binding| binding.member == member)
            {
                binding.variant
            } else if action.variants.len() == 1 {
                *action.variants.keys().next().unwrap()
            } else {
                continue;
            };
            ensure!(
                action.variants.contains_key(&variant),
                "missing martial variant {member}:{variant}"
            );
            let catalogue = crate::techniques::logical_id(u32::from(catalogue)).try_into()?;
            bindings
                .entry(catalogue)
                .or_default()
                .insert(character, Binding { member, variant });
        }
    }
    Ok(Catalogue { members, bindings })
}

fn records(bytes: &mut [u8], width: usize, fields: &[(usize, usize)]) {
    // Sentinels only require the time halfword; unused trailing padding is kept.
    for row in bytes.chunks_mut(width) {
        for &(offset, size) in fields {
            if let Some(field) = row.get_mut(offset..offset + size) {
                field.reverse();
            }
        }
    }
}

pub(crate) fn canonical(bytes: &[u8], id: u16, variant: usize) -> Result<Vec<u8>> {
    let sections = [0, 4, 8, 12].map(|at| pe::word(bytes, at).map(|v| v as usize));
    let [rules, contacts, poses, commands] = sections;
    let [rules, contacts, poses, commands] = [rules?, contacts?, poses?, commands?];
    ensure!(
        16 <= rules
            && rules <= contacts
            && contacts <= poses
            && poses <= commands
            && commands <= bytes.len(),
        "invalid martial sections"
    );
    let mut result = bytes.to_vec();
    records(&mut result[..16], 4, &[(0, 4)]);
    // Startup effects occupy a word at +8; the shared reader uses its low half.
    records(
        &mut result[16..rules],
        28,
        &[(0, 2), (2, 2), (6, 2), (8, 4), (16, 4), (20, 4), (24, 4)],
    );
    records(
        &mut result[rules..contacts],
        28,
        &[(0, 2), (8, 4), (14, 2), (16, 2)],
    );
    crate::battle_callbacks::canonical_hits(id, &mut result[rules..contacts])?;
    records(
        &mut result[contacts..poses],
        32,
        &[(0, 2), (8, 4), (12, 4), (22, 2), (28, 4)],
    );
    records(&mut result[poses..commands], 12, &[(0, 2), (8, 4)]);
    let mut streams = vec![variant];
    if id == 40 && variant != 1 {
        streams.push(1);
    }
    let mut visited = std::collections::BTreeSet::new();
    for variant in streams {
        let mut cursor = commands + pe::word(bytes, 16 + variant * 28 + 24)? as usize * 2;
        while visited.insert(cursor) {
            if pe::half(bytes, cursor)? == u16::MAX {
                result[cursor..cursor + 2].reverse();
                break;
            }
            let opcode = pe::half(bytes, cursor + 2)?;
            let (width, fields): (_, &[(usize, usize)]) = match opcode {
                0 | 1 | 3 | 9 | 40 => (6, &[(0, 2), (2, 2), (4, 2)]),
                7 => (6, &[(0, 2), (2, 2)]),
                5 | 12 | 13 | 23 | 28 => (8, &[(0, 2), (2, 2), (4, 2), (6, 2)]),
                37 => (10, &[(0, 2), (2, 2), (4, 2), (6, 2), (8, 2)]),
                27 => (12, &[(0, 2), (2, 2), (4, 4), (8, 4)]),
                42 => (16, &[(0, 2), (2, 2), (4, 2), (6, 2), (8, 4), (12, 4)]),
                _ => anyhow::bail!("martial command {opcode} is not converted"),
            };
            records(
                result
                    .get_mut(cursor..cursor + width)
                    .context("truncated martial command")?,
                width,
                fields,
            );
            cursor += width;
        }
    }
    Ok(result)
}

fn hit_sound(hit: &mut ActionHit) -> Result<()> {
    hit.sound = hit.sound.map(audio_effects::id).transpose()?;
    Ok(())
}

fn track_sounds(events: &mut [(u16, Event)]) -> Result<()> {
    for (_, event) in events {
        match event {
            Event::Sound(sound) => *sound = audio_effects::id(*sound)?,
            Event::Contact(contact) => hit_sound(&mut contact.hit)?,
            Event::Throw { hit, .. } => hit_sound(hit)?,
            Event::EmitProjectile { hit, feedback, .. } => {
                hit_sound(hit)?;
                if let Some(feedback) = feedback {
                    feedback.sound = audio_effects::id(feedback.sound)?;
                }
            }
            Event::Projectile { hit, feedback, .. } => {
                if let Some(hit) = hit {
                    hit_sound(hit)?;
                }
                if let Some(feedback) = feedback {
                    feedback.sound = audio_effects::id(feedback.sound)?;
                }
            }
            _ => (),
        }
    }
    Ok(())
}

pub(crate) fn sounds(attack: &mut Attack) -> Result<()> {
    track_sounds(&mut attack.events)?;
    for hit in attack.callback_hits.values_mut() {
        hit_sound(hit)?;
    }
    if let Some(fang) = &mut attack.fierce_fang {
        for hit in &mut fang.hits {
            hit_sound(hit)?;
        }
    }
    if let Some(hit) = &mut attack.hurricane_hit {
        hit_sound(hit)?;
    }
    if let Some(hammer) = &mut attack.hammer {
        hammer.impact_sound = audio_effects::id(hammer.impact_sound)?;
        for hit in &mut hammer.hits {
            hit_sound(hit)?;
        }
        match &mut hammer.pattern {
            HammerPattern::Single { alternate_events } => track_sounds(alternate_events)?,
            HammerPattern::Ring { opening_hit, .. } => hit_sound(opening_hit)?,
            _ => (),
        }
    }
    if let Some(steal) = &mut attack.steal {
        track_sounds(&mut steal.success_events)?;
        hit_sound(&mut steal.bonus_hit)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Language;
    use std::{fs, path::PathBuf};

    fn demon_fang_startups(catalogue: &Catalogue) -> Result<()> {
        for (variant, expected) in [3, 4, 4].into_iter().enumerate() {
            ensure!(
                catalogue
                    .variant(Binding {
                        member: 1,
                        variant: variant as u8
                    })?
                    .effect
                    == Some(expected),
                "Demon Fang variant {variant} lost startup effect {expected}"
            );
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires RESONANCE_TEST_STEAM and RESONANCE_TEST_LIBRARY; verifies native Demon Fang selectors and cooks only the martial owner"]
    fn installed_demon_fang_startups_survive_normalization_and_shared_publication() -> Result<()> {
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            Language::English,
        )?;
        let library = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let usual = source.require("BTLusual.pac")?;
        let usual = Package::parse(&usual)?;
        let directory = usual.member(8)?.context("missing martial directory")?;
        let start = pe::word(directory, 8)? as usize;
        for (variant, expected) in [3, 4, 4].into_iter().enumerate() {
            ensure!(
                pe::word(directory, start + 16 + variant * 28 + 8)? == expected,
                "native Demon Fang startup selector changed"
            );
        }
        let catalogue = read(&source)?;
        demon_fang_startups(&catalogue)?;
        let document = resonance_importer::martial_attack_document(&catalogue)?;
        let document: resonance_content::battle_martial::document::Catalogue =
            serde_json::from_slice(&serde_json::to_vec(&document)?)?;
        demon_fang_startups(&document.prepare()?)?;
        // The source and shared declaration checks precede this single owner write.
        cook(&source, &library)?;
        let document: resonance_content::battle_martial::document::Catalogue =
            serde_json::from_slice(&fs::read(
                library.join(resonance_content::battle_rules::MARTIAL_ATTACKS),
            )?)?;
        demon_fang_startups(&document.prepare()?)?;
        Ok(())
    }
}
