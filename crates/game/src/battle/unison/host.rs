//! Assemble the team-attack owner from the encounter's prepared party resources.
use super::*;
use resonance_battle::{
    ActionKey, CameraPose, Pose, UnisonArte, UnisonArteId, UnisonDefinition, UnisonMember,
    UnisonPrelude, UnisonRecipe, UnisonRecipeId,
};
use resonance_content::{battle_profile::Profile, battle_unison};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct HostMember<'a> {
    pub actor: ActorId,
    pub character: Character,
    pub model: &'a ModelDefinition,
    pub null_motions: &'a BTreeSet<u16>,
    pub profile: &'a Profile,
    pub techniques: &'a BTreeMap<u16, ActionKey>,
    pub temporary_weapons: &'a BTreeMap<u16, Vec<Arc<WeaponDefinition>>>,
    pub short_weapon: bool,
}

pub(crate) fn prepare_host<S: FnMut(Sound) -> Result<Option<Sound>>>(
    catalogue: &battle_unison::Catalogue,
    party: &[HostMember<'_>],
    immune_targets: BTreeSet<ActorId>,
    retain_gauge: bool,
    damage_bonus: bool,
    resources: &mut Resources<'_, S>,
    actions: &mut ActionDefinitions,
) -> Result<UnisonDefinition> {
    let host = &catalogue.host;
    const WINDUP_CLIP: u16 = 29;
    const WINDUP_EFFECT: u16 = 13;
    let common = resources.common.clone();
    let effect = resources.prepared.effect_member(&common, WINDUP_EFFECT)?;
    let sound = (resources.sound)(Sound::Cue(77))?.context("missing Unison start sound")?;
    let mut members = Vec::with_capacity(party.len());
    let mut prelude_voices = Vec::new();
    for actor in party {
        let index = actor.character.index();
        let opener = catalogue
            .openers
            .get(index)
            .context("missing Unison opener")?;
        let mut track = resonance_content::battle_martial::Attack {
            duration: opener.duration,
            chain_at: None,
            recovery: opener.recovery,
            effect: None,
            events: opener.events.clone(),
            flight: None,
            pose_sequence: None,
            callback_hits: Default::default(),
            hammer: None,
            steal: None,
            fierce_fang: None,
            hurricane_hit: None,
        };
        action::remove_empty_opening_blend(&mut track, actor.null_motions)?;
        // The shared opener addresses both weapon groups. A single-weapon
        // character strikes with the markers present on their equipped model.
        for (_, event) in &mut track.events {
            if let resonance_content::battle_normal::Event::Contact(contact) = event
                && let resonance_content::battle_action::ContactAttachments::Groups(groups) =
                    &mut contact.attachments
            {
                groups.retain(|group| !actor.model.contact_group_anchors(*group).is_empty());
                anyhow::ensure!(
                    !groups.is_empty(),
                    "Unison opener for character {:?} has no contact anchors",
                    actor.character
                );
            }
        }
        let participant = Participant {
            actor: actor.actor,
            character: actor.character,
            model: actor.model,
            null_motions: actor.null_motions,
            temporary_weapons: Vec::new(),
        };
        let opener_attack = action::prepare(&track, &participant, &common, resources)
            .with_context(|| {
                format!("prepare Unison opener for character {:?}", actor.character)
            })?;
        let mut artes = Vec::new();
        for (&catalogue_id, &action) in actor.techniques {
            let rule = resources.catalogue.definition(usize::from(catalogue_id))?;
            if rule.capabilities.target != resonance_content::arte::TechniqueTarget::Enemy {
                continue;
            }
            let elemental = resonance_content::arte::elemental::find(catalogue_id);
            let placement = catalogue
                .placements
                .get(usize::from(
                    elemental.map_or(catalogue_id, |variant| variant.base),
                ))
                .context("missing Unison arte placement")?;
            let duration = if elemental.is_some()
                && let resonance_battle::ActionExecution::Attack(attack) =
                    &actions[action].execution
            {
                placement.duration.max(attack.end_at)
            } else {
                placement.duration
            };
            artes.push(UnisonArte {
                action,
                native_id: UnisonArteId(placement.native_id),
                duration,
                distance: f32::from(placement.distance),
                altitude: placement.altitude as f32,
            });
        }
        let windup_voice = actor
            .profile
            .voices
            .as_ref()
            .and_then(|voices| voices.unison_start)
            .map(&mut resources.sound)
            .transpose()?
            .flatten();
        if let Some(voice) = host.prelude.voices.get(index).copied().flatten()
            && let Some(sound) = (resources.sound)(voice)?
        {
            prelude_voices.push((actor.actor, sound));
        }
        members.push(UnisonMember {
            actor: actor.actor,
            opener: opener_attack,
            opener_contact_ticks: opener.contact_ticks,
            windup_motion: crate::battle::model::motion(Some(actor.model), WINDUP_CLIP)
                .context("missing Unison windup motion")?,
            windup_pose: Pose {
                rate: host.windup_rate,
                ..Default::default()
            },
            windup_voice,
            artes,
            short_weapon: actor.short_weapon,
        });
    }
    let available_artes: BTreeSet<_> = members
        .iter()
        .flat_map(|member| member.artes.iter().map(|arte| arte.native_id))
        .collect();
    let mut recipes = Vec::with_capacity(catalogue.combinations.len());
    for combination in &catalogue.combinations {
        let participants: Vec<_> = party
            .iter()
            .map(|actor| Participant {
                actor: actor.actor,
                character: actor.character,
                model: actor.model,
                null_motions: actor.null_motions,
                temporary_weapons: actor
                    .temporary_weapons
                    .get(&combination.native_id)
                    .cloned()
                    .unwrap_or_default(),
            })
            .collect();
        let eligible = combination.recipes.iter().any(|recipe| {
            recipe
                .iter()
                .all(|arte| available_artes.contains(&UnisonArteId(*arte)))
        });
        let execution = if eligible && family::has_pair(combination.native_id, &participants) {
            let scene: Scene = resources
                .files
                .json(&resonance_content::battle_scene::path(
                    combination.native_id,
                ))?;
            let bank = resources
                .spell_scenes
                .get(&combination.native_id)
                .cloned()
                .context("missing Unison scene effect bank")?;
            super::prepare(
                combination,
                &scene,
                &bank,
                &participants,
                resources,
                actions,
            )
            .with_context(|| format!("prepare Unison combination {}", combination.native_id))?
        } else {
            None
        };
        recipes.push(UnisonRecipe {
            id: UnisonRecipeId(combination.id),
            native_id: UnisonArteId(combination.native_id),
            duration: combination.duration,
            camera_pitch: combination.camera_pitch,
            ingredients: combination
                .recipes
                .iter()
                .map(|row| row.iter().copied().map(UnisonArteId).collect())
                .collect(),
            execution,
        });
    }
    let offset: [f32; 3] = std::array::from_fn(|axis| {
        host.prelude.camera_eye[axis] - host.prelude.camera_target[axis]
    });
    let horizontal = offset[0].hypot(offset[2]);
    Ok(UnisonDefinition {
        members,
        placement: host.placement,
        minimum_distance: host.minimum_distance,
        short_weapon_penalty: host.short_weapon_penalty,
        restore_height: host.restore_height,
        immune_targets,
        retain_gauge,
        windup_effect: effect,
        windup_sound: sound,
        windup_color: Some(host.windup_color),
        recipes: Some(recipes),
        combination_prelude: Some(UnisonPrelude {
            first_x: host.prelude.first_x,
            spacing: host.prelude.spacing,
            camera: CameraPose {
                eye: host.prelude.camera_eye,
                focus: host.prelude.camera_target,
                pitch: offset[1].atan2(horizontal).to_degrees(),
                yaw: offset[2].atan2(offset[0]).to_degrees(),
                radius: horizontal.hypot(offset[1]),
            },
            color: Some(host.prelude.color),
            effect,
            sound,
            voices: prelude_voices,
        }),
        damage_bonus,
    })
}
