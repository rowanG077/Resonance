//! Recover complete Unison recipes and actor-specific timing as semantic data.
mod binding;
mod combined_pair;
mod parameters;
mod pow_blade;
mod strike;
mod thrust;
use super::actions::{Bundle, Rel};
#[cfg(test)]
use crate::dol;
#[cfg(test)]
use crate::read::u16 as half;
use crate::read::{f32 as float, u32 as word};
use anyhow::{Context, Result, ensure};
pub(super) use binding::Inputs;
pub(crate) use parameters::cook as cook_parameters;
use resonance_content::{
    battle::{
        actions::{
            Action, ActionCommand, AnimationCommand, HitAttachment, HitEmission, TechniquePhase,
        },
        unison::*,
    },
    menu_data::TECHNIQUE_COUNT,
};
#[cfg(test)]
use std::fs;
use std::path::Path;

pub(super) fn cook(
    inputs: &Inputs,
    tables: &super::unison_tables::UnisonTables,
    recipe: &super::unison_opener::OpenerRecipe,
    catalogue: &crate::arte::Catalogue,
) -> Result<UnisonData> {
    ensure!(
        catalogue.definitions.len() == TECHNIQUE_COUNT
            && catalogue.combinations.len() == COMBINATION_COUNT + 1,
        "incomplete Unison arte catalogue"
    );
    let data = UnisonData {
        pow: PowWeapon::ALL
            .into_iter()
            .map(|kind| pow_blade::cook(inputs, kind).map(|program| (kind, program)))
            .collect::<Result<_>>()?,
        thrusts: thrust::cook(inputs)?,
        strikes: strike::cook(inputs)?,
        plasma_blade: None,
        pairs: combined_pair::cook(inputs)?,
        artes: catalogue
            .definitions
            .iter()
            .enumerate()
            .map(|(id, definition)| {
                Ok((
                    u16::try_from(id)?,
                    Arte {
                        native_id: definition.native_id.try_into()?,
                        usable: definition.flags & 0x100 != 0
                            && definition.flags & 0x2000_0000 == 0,
                        distance: definition.unison_distance.try_into()?,
                        duration: definition.unison_duration.try_into()?,
                        altitude: definition.unison_altitude,
                    },
                ))
            })
            .collect::<Result<_>>()?,
        combinations: catalogue
            .combinations
            .iter()
            .enumerate()
            .skip(1)
            .map(|(id, combination)| {
                let participants = combination.participant_count;
                ensure!(
                    (2..=4).contains(&participants),
                    "invalid Unison participant count"
                );
                let mut recipes = Vec::new();
                for values in &combination.recipe_slots {
                    if values[0] == 0 {
                        ensure!(
                            values.iter().all(|&id| id == 0),
                            "partially empty Unison recipe"
                        );
                    } else {
                        ensure!(
                            values[usize::from(participants)..]
                                .iter()
                                .all(|&id| id == 0),
                            "extra Unison ingredients"
                        );
                        recipes.push(
                            values[..usize::from(participants)]
                                .iter()
                                .copied()
                                .map(u16::try_from)
                                .collect::<std::result::Result<_, _>>()?,
                        );
                    }
                }
                ensure!(
                    combination.storage == [0; 2],
                    "unknown Unison recipe metadata"
                );
                Ok(Combination {
                    id: id.try_into()?,
                    name: combination.name.clone().context("missing Unison name")?,
                    native_id: combination.native_id.try_into()?,
                    participants: participants.try_into()?,
                    recipes,
                    duration: combination.duration_ticks.try_into()?,
                    camera_pitch: combination.camera_pitch_offset_degrees,
                })
            })
            .collect::<Result<_>>()?,
        party: (0..9)
            .map(|id| {
                Ok(PartyTraits {
                    opener_contact_ticks: u16::from(tables.opener_contact_delays[id]) + 30,
                    overlimit_rate: inputs.actor(id as u8 + 1)?.effects.overlimit_rate,
                    overlimit_voice: tables.overlimit_voices[id],
                })
            })
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
        opener: (1..=9)
            .map(|character| {
                let metadata = inputs.actor(character)?;
                ensure!(
                    metadata.appearance.channel_count <= 4,
                    "too many Unison opener texture layers"
                );
                let mut opener = opener(recipe, character, metadata.appearance.attack_face)?;
                let carried = match (character, metadata.model.attachment_count) {
                    // Regal obtains his count from the equipped package at battle entry.
                    (8, 0) => None,
                    (8, _) => anyhow::bail!("Regal carried count must be equipment-bound"),
                    (_, count @ 1..=8) => Some(u16::from(count)),
                    _ => anyhow::bail!("invalid Unison carried instance count"),
                };
                opener.inactive_trail_slots = opener
                    .action
                    .commands
                    .iter()
                    .filter_map(|step| match step.command {
                        ActionCommand::AttachmentTrail { slot, .. }
                            if carried.is_some_and(|count| slot >= count) =>
                        {
                            Some(slot)
                        }
                        _ => None,
                    })
                    .collect();
                opener.optional_hit_groups = opener
                    .action
                    .hits
                    .iter()
                    .filter_map(|hit| match &hit.emission {
                        HitEmission::Contact {
                            attachment: HitAttachment::Groups(groups),
                            ..
                        } => Some(groups),
                        _ => None,
                    })
                    .flatten()
                    .copied()
                    .filter(|&group| carried.is_some_and(|count| u16::from(group) >= count))
                    .collect();
                Ok(opener)
            })
            .collect::<Result<Vec<_>>>()?
            .try_into()
            .unwrap(),
        windup: AnimationCommand::Play {
            clip: 29,
            blend: 4,
            start: 0,
            end: None,
            layer: 8,
            looping: false,
            mirror: false,
            resource: -1,
            rate: tables.windup_rate,
        },
        windup_color: tables.windup_color,
        combined_prelude: tables.combined_prelude.clone(),
        placement: tables.placement,
        restore_ground_height: tables.combined_prelude.hidden_position[1],
        short_weapon_penalty: tables.short_weapon_penalty,
        minimum_distance: tables.minimum_distance,
    };
    data.validate()?;
    Ok(data)
}

fn opener(
    recipe: &super::unison_opener::OpenerRecipe,
    character: u8,
    texture_layers: [u8; 4],
) -> Result<Opener> {
    ensure!(
        (1..=9).contains(&character),
        "invalid Unison opener character"
    );
    // The opener bypasses normal selection/combo handling.
    ensure!(
        recipe.combo_first == 0
            && recipe.combo_second.is_none()
            && recipe.buffer_until == 0
            && recipe.recovery.animation.is_none()
            && recipe.effect.is_none()
            && recipe.airborne_reach == 0,
        "unsupported Unison opener descriptor"
    );
    Ok(Opener {
        inactive_trail_slots: Default::default(),
        optional_hit_groups: Default::default(),
        minimum_active_ticks: 45,
        action: Action {
            duration: recipe.duration,
            tp: 0,
            animations: recipe.animations.clone(),
            commands: recipe.commands.clone(),
            loop_commands: recipe.loop_commands,
            // Genis's carried weapon contacts later and uses one attack group.
            hits: recipe.hits[usize::from(character == 3)].clone(),
        },
        recovery: recipe.recovery,
        reach: recipe.reach,
        texture_layers,
    })
}

fn phase(source: &Bundle, index: usize) -> Result<TechniquePhase> {
    let row = source.phase(index)?;
    ensure!(
        row.startup_effect.is_none_or(|effect| effect <= 255),
        "invalid combined startup effect"
    );
    Ok(TechniquePhase {
        alternate: None,
        caption: None,
        callback: None,
        variant: index.try_into()?,
        recovery_ticks: row.recovery_ticks,
        buffer_until: row.buffer_until,
        combo_at: row.combo_at,
        effect: row.startup_effect,
        action: source.action(index, 0)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::unison_opener::decode;
    use resonance_content::battle::actions::{ActionCommand, HitAttachment, HitEmission};

    fn original_opener() -> Vec<u8> {
        // Descriptor, shared rule, animation, two contact streams and commands.
        "00 1e 00 0a 00 00 00 00 00 00 00 00 3f 00 00 00
         00 00 00 00 00 c8 00 00 00 03 00 1e 0a 00 00 00
         00 00 00 00 00 01 00 00 00 00 00 00 00 00 00 00
         00 00 00 00 00 00 19 04 00 00 08 ff 3f 00 00 00
         ff fe 00 00 00 00 00 00 00 00 00 00 00 0a 08 02
         00 01 00 00 41 f0 00 00 41 f0 00 00 00 00 00 00
         02 00 00 00 00 00 00 00 00 00 00 00 ff ff 00 00
         00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
         00 00 00 00 00 00 00 00 00 00 00 00 00 14 08 01
         00 00 00 00 41 f0 00 00 41 f0 00 00 00 00 00 00
         02 00 00 00 00 00 00 00 00 00 00 00 ff ff 00 00
         00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
         00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 0d
         00 00 00 5a 00 00 00 0d 00 01 00 5a 00 08 00 00
         00 28 ff ff"
            .split_whitespace()
            .map(|byte| u8::from_str_radix(byte, 16).unwrap())
            .collect()
    }

    #[test]
    fn opener_preserves_genis_contact_and_shared_motion_commands() {
        let bytes = original_opener();
        let recipe = decode(&bytes).unwrap();
        for character in 1..=9 {
            let opener = opener(&recipe, character, [2, 1, 0, 0]).unwrap();
            opener.action.validate(0).unwrap();
            assert_eq!(opener.action.audio_ids(), Default::default());
            assert!(
                opener
                    .action
                    .impact_programs(resonance_content::battle::effects::EffectBank::Techniques)
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(
                (
                    opener.action.duration,
                    opener.recovery.duration,
                    opener.reach
                ),
                (30, 10, 200)
            );
            assert_eq!(opener.recovery.rate, 0.5);
            assert_eq!(opener.minimum_active_ticks, 45);
            assert_eq!(opener.texture_layers, [2, 1, 0, 0]);
            assert_eq!(opener.action.animations.commands().count(), 1);
            assert!(matches!(
                opener.action.animations.initial.unwrap(),
                AnimationCommand::Play {
                    clip: 25,
                    blend: 4,
                    resource: -1,
                    rate: 0.5,
                    ..
                }
            ));
            let [hit] = &opener.action.hits[..] else {
                panic!("one opener contact")
            };
            assert_eq!(hit.start, if character == 3 { 20 } else { 10 });
            assert!(
                matches!(&hit.emission, HitEmission::Contact {duration:8, attachment:HitAttachment::Groups(groups)}
                if groups.as_slice() == if character == 3 {&[0][..]} else {&[0,1][..]})
            );
            assert_eq!(
                (hit.shape.radius, hit.shape.height, hit.shape.reaction),
                (30., 30., 2)
            );
            assert_eq!(
                (
                    hit.rule.flags,
                    hit.rule.hitstun,
                    hit.rule.contact_cooldown,
                    hit.rule.power_mode,
                    hit.rule.power
                ),
                (3, 30, 10, 1, 0)
            );
            assert!(!opener.action.loop_commands);
            let [first, second, motion] = &opener.action.commands[..] else {
                panic!("three opener commands")
            };
            assert_eq!((first.tick, second.tick, motion.tick), (0, 0, 8));
            assert!(matches!(
                first.command,
                ActionCommand::AttachmentTrail { slot: 0, ticks: 90 }
            ));
            assert!(matches!(
                second.command,
                ActionCommand::AttachmentTrail { slot: 1, ticks: 90 }
            ));
            assert!(matches!(motion.command, ActionCommand::ForwardSpeed(4.)));
        }
    }

    #[test]
    fn opener_rejects_truncation_unbound_rules_and_unsupported_descriptor_fields() {
        let bytes = original_opener();
        assert_eq!(bytes.len(), 0xe4);
        assert!(decode(&bytes[..0xe2]).is_err());
        assert!(opener(&decode(&bytes).unwrap(), 0, [0; 4]).is_err());
        for offset in [4, 9, 16, 22, 0x4c + 0x12] {
            let mut invalid = bytes.clone();
            invalid[offset] = 1;
            assert!(
                decode(&invalid)
                    .and_then(|recipe| opener(&recipe, 1, [0; 4]))
                    .is_err(),
                "offset {offset:x}"
            );
        }
    }
}
