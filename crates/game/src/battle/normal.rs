//! Native normal-attack volumes, timing, and feedback.
use super::party::Character;
use super::{ActionDefinition, voice::Sound};
use anyhow::Result;
use resonance_battle::{AttackEvent, AttackPose, NormalAttack as Attack};
use std::sync::Arc;

/// Native cadence, shared across directions. Heavy weapons trade speed for reach.
struct Timing {
    windup: u16,
    active: u16,
    recovery: u16,
}

fn timing(character: Character, attack: Attack) -> Timing {
    let windup = match character {
        Character::Presea => 18,
        Character::Genis | Character::Raine => 12,
        _ => 8,
    };
    Timing {
        windup: windup + if attack == Attack::Finisher { 4 } else { 0 },
        active: 8,
        recovery: if character == Character::Presea {
            16
        } else {
            10
        },
    }
}

/// Resolved artwork; gameplay timing is independent of clip playback.
struct Artwork {
    opening: Option<AttackPose>,
    follow_through: Option<AttackPose>,
    voice: Sound,
}

fn artwork(
    character: Character,
    attack: Attack,
    model: Option<&resonance_battle::ModelDefinition>,
) -> Artwork {
    use Attack::*;
    use Character::*;
    let (opening, voice) = match (character, attack) {
        (Lloyd, Neutral) => (30, Sound::Cue(502)),
        (Lloyd, Rising) => (38, Sound::Cue(502)),
        (Lloyd, Thrust) => (32, Sound::Cue(502)),
        (Lloyd, Low) => (33, Sound::Cue(503)),
        (Lloyd, Finisher) => (31, Sound::Cue(504)),
        (Lloyd, AerialSlash) => (41, Sound::Cue(503)),
        (Lloyd, AerialThrust) => (40, Sound::Cue(503)),
        (Colette, Neutral) => (31, Sound::Cue(622)),
        (Colette, Rising) => (38, Sound::Cue(624)),
        (Colette, Thrust | Low) => (if attack == Thrust { 30 } else { 33 }, Sound::Cue(623)),
        (Colette, Finisher) => (32, Sound::Cue(622)),
        (Colette, AerialSlash) => (41, Sound::Cue(622)),
        (Colette, AerialThrust) => (40, Sound::Cue(623)),
        (Genis, Neutral | Finisher) => (43, Sound::Cue(744)),
        (Genis, Rising) => (46, Sound::Cue(743)),
        (Genis, Thrust) => (44, Sound::Cue(743)),
        (Genis, Low) => (45, Sound::Cue(744)),
        (Genis, AerialSlash) => (46, Sound::Cue(744)),
        (Genis, AerialThrust) => (47, Sound::Cue(744)),
        (Raine, Neutral) => (30, Sound::Cue(864)),
        (Raine, Rising | Low) => (33, Sound::Cue(865)),
        (Raine, Thrust) => (32, Sound::Cue(863)),
        (Raine, Finisher) => (31, Sound::Cue(864)),
        (Raine, AerialSlash | AerialThrust) => (41, Sound::Cue(864)),
        (Sheena, Neutral) => (30, Sound::Cue(970)),
        (Sheena, Rising) => (38, Sound::Cue(972)),
        (Sheena, Thrust) => (33, Sound::Cue(970)),
        (Sheena, Low) => (32, Sound::Cue(971)),
        (Sheena, Finisher) => (31, Sound::Cue(970)),
        (Sheena, AerialSlash | AerialThrust) => (
            if attack == AerialSlash { 41 } else { 40 },
            Sound::Cue(if attack == AerialSlash { 970 } else { 971 }),
        ),
        (Zelos, Neutral) => (30, Sound::Cue(1089)),
        (Zelos, Rising) => (38, Sound::Cue(1090)),
        (Zelos, Thrust) => (32, Sound::Cue(1088)),
        (Zelos, Low) => (33, Sound::Cue(1089)),
        (Zelos, Finisher) => (31, Sound::Cue(1089)),
        (Zelos, AerialSlash) => (41, Sound::Cue(1088)),
        (Zelos, AerialThrust) => (40, Sound::Cue(1088)),
        (Presea, Neutral) => (30, Sound::Cue(1200)),
        (Presea, Rising) => (34, Sound::Cue(1202)),
        (Presea, Thrust) => (31, Sound::Cue(1200)),
        (Presea, Low) => (32, Sound::Cue(1201)),
        (Presea, Finisher) => (33, Sound::Cue(1200)),
        (Presea, AerialSlash) => (35, Sound::Cue(1202)),
        (Presea, AerialThrust) => (36, Sound::Cue(1202)),
        (Regal, Neutral) => (30, Sound::Cue(1308)),
        (Regal, Rising) => (38, Sound::Cue(1309)),
        (Regal, Thrust) => (32, Sound::Cue(1310)),
        (Regal, Low) => (33, Sound::Cue(1310)),
        (Regal, Finisher) => (31, Sound::Cue(1308)),
        (Regal, AerialSlash) => (41, Sound::Cue(1310)),
        (Regal, AerialThrust) => (40, Sound::Cue(1310)),
        (Kratos, Neutral) => (30, Sound::Cue(1410)),
        (Kratos, Rising) => (38, Sound::Cue(1412)),
        (Kratos, Thrust) => (32, Sound::Cue(1411)),
        (Kratos, Low) => (33, Sound::Cue(1410)),
        (Kratos, Finisher) => (31, Sound::Cue(1410)),
        (Kratos, AerialSlash) => (41, Sound::Cue(1411)),
        (Kratos, AerialThrust) => (40, Sound::Cue(1411)),
    };
    let follow_through = match (character, attack) {
        (Lloyd, Rising) => Some(16),
        (Colette, Rising) => Some(39),
        (Colette, Neutral) => Some(34),
        (Colette, Finisher) => Some(35),
        (Colette, Thrust | Low) => Some(91),
        _ => None,
    };
    let pose = |clip| {
        super::model::motion(model, clip).map(|motion| AttackPose {
            motion,
            blend: 4,
            start: 0.,
            rate: 0.5,
        })
    };
    Artwork {
        opening: pose(opening),
        follow_through: follow_through.and_then(pose),
        voice,
    }
}

/// Native reach and height; visual weapon length does not change a hit volume.
fn volume(character: Character, attack: Attack) -> Option<resonance_battle::MeleeVolume> {
    use Attack::*;
    use Character::*;
    if character == Colette && matches!(attack, Thrust | Low | AerialThrust) {
        return None;
    }
    let mut volume = match character {
        Genis | Sheena => resonance_battle::MeleeVolume {
            offset: [0., 55., 60.],
            radius: 45.,
            half_height: 50.,
        },
        Presea => resonance_battle::MeleeVolume {
            offset: [0., 70., 90.],
            radius: 80.,
            half_height: 75.,
        },
        Regal => resonance_battle::MeleeVolume {
            offset: [0., 70., 60.],
            radius: 50.,
            half_height: 60.,
        },
        _ => resonance_battle::MeleeVolume {
            offset: [0., 75., 80.],
            radius: 60.,
            half_height: 60.,
        },
    };
    match attack {
        Low => {
            volume.offset[1] = 30.;
            volume.half_height = 30.;
        }
        Rising => {
            volume.offset[1] = 90.;
            volume.half_height = 85.;
        }
        AerialSlash | AerialThrust => {
            volume.offset[1] = 35.;
        }
        Thrust => {
            volume.offset[2] += 20.;
        }
        _ => {}
    }
    Some(volume)
}

/// Normal attacks make one contact. Finishers hit harder and knock down;
/// rising attacks launch. These rules share the locally authored timing and volume.
fn hit(attack: Attack, thrown: bool) -> resonance_battle::HitRule {
    let mut hit = super::hit::physical(if attack == Attack::Finisher { 125 } else { 100 });
    if matches!(attack, Attack::Thrust | Attack::AerialThrust) {
        hit.kind = resonance_battle::DamageKind::Thrust;
    }
    hit.overlimit_pause = !thrown;
    hit.reaction.recoil.launch = attack == Attack::Rising;
    hit.reaction.recoil.knock_down = attack == Attack::Finisher;
    if attack == Attack::Rising {
        hit.reaction.recoil.impulse[1] = 8.;
    }
    hit
}

/// A single weapon flies outward and returns more quickly to its owner.
fn flight(attack: Attack) -> resonance_battle::WeaponFlightDefinition {
    resonance_battle::WeaponFlightDefinition {
        slot: 0,
        origin: [0., 70., 25.],
        outbound_ticks: 12,
        speed: 24.,
        return_speed: 32.,
        direction_y: match attack {
            Attack::Low => 0.35,
            Attack::AerialThrust => -0.8,
            _ => 0.,
        },
        hit: hit(attack, true),
        radius: 45.,
    }
}

/// Input routing and approach distances for the native seven-attack set.
pub(super) fn controls(
    character: u8,
    actions: [resonance_battle::ActionKey; 7],
) -> Result<[resonance_battle::NormalControl; 7]> {
    let character = Character::try_from(character)?;
    Ok(std::array::from_fn(|index| {
        let attack = Attack::ALL[index];
        let volume = volume(character, attack);
        resonance_battle::NormalControl {
            action: actions[index],
            // Thrown weapons prefer some distance from their target.
            reach: volume.map_or(220., |volume| volume.offset[2]),
            minimum_reach: if volume.is_some() { 0. } else { 60. },
        }
    }))
}

/// Prepared actor inputs shared by encounter and focused action preparation.
pub struct Resources<'a> {
    pub character: u8,
    pub model: Option<&'a resonance_battle::ModelDefinition>,
    pub common: Option<&'a super::EffectResource>,
    pub tints: Option<&'a resonance_content::battle_effect::Tints>,
}

/// Voice starts at admission, movement halfway through windup, and sound at contact.
/// The final contact window ends the attack; recovery also waits for landing.
pub fn prepare_resources(
    resolver: &super::voice::Resolver<'_>,
    actor: &Resources<'_>,
    resources: &mut super::ActionResources,
    sound: &mut impl FnMut(Sound) -> Result<Option<resonance_battle::Sound>>,
) -> Result<Vec<ActionDefinition>> {
    use Attack::*;
    use Character::*;
    let character = Character::try_from(actor.character)?;
    let swing = sound(Sound::Cue(match character {
        Lloyd | Colette | Zelos | Kratos => 60,
        Genis => 115,
        Raine => 62,
        Sheena => 114,
        Presea => 63,
        Regal => 64,
    }))?;
    let mut bindings = Vec::with_capacity(Attack::ALL.len());
    for attack in Attack::ALL {
        let art = artwork(character, attack, actor.model);
        let t = timing(character, attack);
        let mut events = Vec::new();
        let volume = volume(character, attack);
        let attack_sound = if volume.is_none() {
            sound(Sound::Cue(61))?
        } else {
            swing
        };
        let contact = if let Some(volume) = volume {
            let trail =
                u8::from(character == Sheena && matches!(attack, Rising | Low | AerialSlash));
            AttackEvent::Contact {
                definition: Arc::new(resonance_battle::MeleeDefinition {
                    hit: hit(attack, false),
                    volume,
                    trail: Some(trail),
                }),
                duration: t.active,
            }
        } else {
            AttackEvent::Throw(Arc::new(flight(attack)))
        };
        events.push((t.windup, contact));
        events.push((t.windup, AttackEvent::Sound(attack_sound)));
        let contact_end = t.windup + t.active;
        let voice = resolver.absolute(
            super::model::ModelSource::Party(actor.character),
            Some(art.voice),
            &mut *sound,
        )?;
        if let Some(line) = voice {
            events.push((0, AttackEvent::Voice(line)));
        }
        if !matches!(attack, AerialSlash | AerialThrust) {
            events.push((
                t.windup / 2,
                AttackEvent::Move {
                    forward: Some(5.),
                    vertical: (attack == Rising).then_some(20.),
                },
            ));
        }
        if let Some(pose) = art.follow_through {
            events.push((contact_end, AttackEvent::Pose(pose)));
        }
        if character == Sheena {
            let right = matches!(attack, Neutral | Thrust | Finisher | AerialThrust);
            events.push((
                0,
                AttackEvent::WeaponVisibility(vec![
                    (0, right),
                    (1, !right),
                    (2, right),
                    (3, !right),
                ]),
            ));
            let member = match attack {
                Neutral => Some(23),
                Thrust => Some(24),
                Low => Some(25),
                AerialSlash => Some(27),
                AerialThrust => Some(28),
                _ => None,
            };
            if let Some(member) = member
                && let Some(common) = actor.common
            {
                let mut effect = common.clone();
                effect.members = vec![member];
                super::require_effect(&mut resources.effects, effect)?;
                events.push((
                    0,
                    AttackEvent::Effect {
                        centered: false,
                        appearance: resonance_battle::EffectAppearance {
                            resource: common.resource,
                            member,
                        },
                        tints: std::array::from_fn(|element| {
                            actor
                                .tints
                                .and_then(|tints| tints.effect(element))
                                .unwrap_or_default()
                        }),
                    },
                ));
            }
        }
        events.sort_by_key(|(age, _)| *age);
        bindings.push(ActionDefinition {
            normal: Some(attack),
            execution: super::ActionExecution::Attack(resonance_battle::PreparedAttack {
                chain_at: Some(contact_end),
                end_at: contact_end,
                opening: art.opening,
                events,
                recovery: t.recovery,
            }),
            tp_cost: 0,
        });
    }
    Ok(bindings)
}
