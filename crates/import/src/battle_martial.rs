//! Decode the shared party action directory into native attack events.
use crate::read::Field;
use anyhow::{Context, Result, ensure};
use resonance_content::{
    battle_martial::{
        Attack, Binding, Catalogue, Flight, Member, PoseStart, PoseStep, gameplay_tick,
    },
    battle_normal::Event,
    battle_scene::SceneAction,
};
use std::collections::BTreeMap;

pub(crate) fn read(usual: &[u8], module: &crate::rel::Rel) -> Result<Catalogue> {
    let directory = crate::source_assets::section(usual, 8)?;
    let count = u32::read(directory, 0)? as usize;
    ensure!(
        count <= (directory.len() - 4) / 4,
        "truncated common action directory"
    );
    let offsets = (0..count)
        .map(|index| u32::read(directory, 4 + index * 4).map(|v| v as usize))
        .collect::<Result<Vec<_>>>()?;
    let mut members = BTreeMap::new();
    for (id, &start) in offsets.iter().enumerate() {
        if start == 0 || start == directory.len() {
            continue;
        }
        let id = id.try_into().context("common action ID exceeds limit")?;
        let result = (|| {
            ensure!(start >= 4 + count * 4, "invalid common action offset");
            let end = offsets
                .iter()
                .copied()
                .filter(|&value| value > start)
                .min()
                .unwrap_or(directory.len());
            let bytes = directory
                .get(start..end)
                .context("common action outside directory")?;
            let variant_end = u32::read(bytes, 0)? as usize;
            ensure!(
                variant_end >= 16
                    && variant_end <= bytes.len()
                    && (variant_end - 16).is_multiple_of(28),
                "invalid common action variants"
            );
            let mut variants = BTreeMap::new();
            for variant in 0..(variant_end - 16) / 28 {
                let prepared = if u16::read(bytes, 16 + variant * 28)? == 0 {
                    Err(anyhow::anyhow!("empty authored action variant"))
                } else {
                    prepare(id, variant, bytes, usual, module)
                };
                variants.insert(
                    variant
                        .try_into()
                        .context("common action variant exceeds limit")?,
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
    let mut bindings: BTreeMap<u16, BTreeMap<u8, Binding>> = [
        1, 2, 4, 6, 10, 11, 12, 13, 17, 20, 35, 40, 45, 207, 208, 209, 210, 223,
    ]
    .into_iter()
    .map(|catalogue| {
        let (member, characters): (_, &[(u8, u8)]) = match catalogue {
            207 => (1, &[(6, 1), (9, 2)]),
            208 => (2, &[(6, 1), (9, 2)]),
            209 => (4, &[(6, 1), (9, 2)]),
            210 => (10, &[(9, 2)]),
            223 => (43, &[(2, 0)]),
            id @ (35 | 40 | 45) => (id, &[(2, 0)]),
            id => (id, &[(1, 0)]),
        };
        let characters = characters
            .iter()
            .map(|&(character, variant)| (character, Binding { member, variant }))
            .collect();
        (catalogue, characters)
    })
    .collect();
    // Catalogue identities and track ownership are separate from physical member IDs.
    let mut bind = |catalogue, member, character, variant| {
        bindings
            .entry(catalogue)
            .or_default()
            .insert(character, Binding { member, variant });
    };
    for id in [
        3, 5, 7, 8, 9, 14, 15, 16, 18, 19, 21, 22, 23, 24, 25, 26, 27, 28, 29,
    ] {
        bind(id, id, 1, 0);
    }
    bind(228, 140, 1, 0);
    bind(224, 44, 2, 0);
    bind(43, 138, 2, 0);
    bind(44, 139, 2, 0);
    for (catalogue, member) in [(210, 10), (211, 11), (212, 12)] {
        bind(catalogue, member, 6, 1);
        bind(catalogue, member, 9, 2);
    }
    for (catalogue, member) in [
        (145, 81),
        (146, 82),
        (147, 83),
        (148, 84),
        (149, 85),
        (150, 86),
        (151, 87),
    ] {
        bind(catalogue, member, 6, 0);
        bind(catalogue, member, 9, 1);
    }
    for id in [36, 37, 38, 39, 41, 42, 46, 48, 50, 52] {
        bind(id, id, 2, 0);
    }
    for (catalogue, member) in [
        (125, 62),
        (126, 63),
        (127, 64),
        (128, 65),
        (129, 66),
        (130, 67),
        (131, 68),
        (132, 69),
        (133, 70),
        (134, 71),
        (135, 72),
        (136, 73),
        (140, 77),
        (141, 78),
        (143, 80),
    ] {
        bind(catalogue, member, 5, 0);
    }
    for (catalogue, member) in [
        (154, 90),
        (155, 91),
        (156, 92),
        (157, 93),
        (158, 94),
        (159, 95),
        (160, 96),
        (161, 97),
        (162, 98),
        (163, 99),
        (164, 100),
        (165, 101),
        (166, 102),
        (167, 103),
        (168, 104),
        (169, 105),
        (170, 106),
        (171, 107),
        (172, 108),
        (173, 109),
        (174, 110),
        (175, 111),
    ] {
        bind(catalogue, member, 7, 0);
    }
    for (catalogue, member) in [
        (176, 112),
        (177, 113),
        (178, 114),
        (179, 115),
        (180, 116),
        (181, 117),
        (182, 118),
        (183, 119),
        (184, 120),
        (185, 121),
        (186, 122),
        (187, 123),
        (188, 124),
        (189, 125),
        (190, 126),
        (191, 127),
        (195, 131),
        (196, 132),
        (197, 133),
        (198, 134),
        (199, 135),
        (200, 136),
        (201, 137),
    ] {
        bind(catalogue, member, 8, 0);
    }
    Ok(Catalogue { members, bindings })
}

fn prepare(
    id: u16,
    variant: usize,
    bytes: &[u8],
    usual: &[u8],
    module: &crate::rel::Rel,
) -> Result<Attack> {
    let mut action = attack(id, variant, bytes, false, false)
        .with_context(|| format!("martial action {id}, variant {variant}"))?;
    let projectiles = crate::source_assets::section(usual, 7)?;
    const PROJECTILE_BYTES: usize = 400;
    for (_, event) in &mut action.events {
        if let Event::Projectile {
            member,
            hit: Some(hit),
            ..
        } = event
        {
            hit.recoil = *projectiles
                .get(usize::from(*member) * PROJECTILE_BYTES + 0x13)
                .context("missing martial projectile recoil")?;
        }
    }
    if let Some(fierce) = &mut action.fierce_fang {
        const GRAVE_BLADE_PROJECTILE: usize = 9;
        fierce.hits[1].recoil = *projectiles
            .get(GRAVE_BLADE_PROJECTILE * PROJECTILE_BYTES + 0x13)
            .context("missing Grave Blade recoil")?;
    }
    if matches!(id, 40..=42) {
        action.hammer = Some(hammer(id, usual, module, bytes, &action)?);
    }
    if matches!(id, 43 | 44) {
        action.steal = Some(steal(id, usual, module, bytes, &action)?);
    }
    use resonance_content::battle_projectile::DamageKind;
    let template_hits: &[(u8, usize)] = match id {
        37 => &[(0, 6)],
        39 => &[(0, 11)],
        50 => &[(0, 25), (1, 4)],
        52 => &[(0, 13), (1, 24)],
        64 | 67 | 70 => &[(1, 12)],
        90..=93 | 106 | 107 => &[(0, 9)],
        123 => &[(1, 16)],
        138 | 139 => &[(0, 10), (1, 4)],
        _ => &[],
    };
    let callback_recoils: &[(u8, u8)] = match id {
        80 => &[(0, 13), (1, 7)],
        85 => &[(1, 1)],
        87 => &[(1, 8)],
        93 => &[(0, 10)],
        140 => &[(0, 10), (1, 12), (2, 7), (3, 17)],
        _ => &[],
    };
    if !template_hits.is_empty() || !callback_recoils.is_empty() {
        let rules = bytes
            .get(u32::read(bytes, 0)? as usize..u32::read(bytes, 4)? as usize)
            .context("missing callback hit rules")?;
        for &(index, member) in template_hits {
            let row = projectiles
                .get(member * PROJECTILE_BYTES..(member + 1) * PROJECTILE_BYTES)
                .context("missing callback projectile")?;
            let kind = match row[0x11] {
                0 => DamageKind::Slash,
                1 => DamageKind::Thrust,
                2 => DamageKind::Magic,
                kind => anyhow::bail!("unknown callback projectile damage kind {kind}"),
            };
            action.callback_hits.insert(
                index,
                crate::battle_action::hit_rule(rules, index, kind, row[0x13])?,
            );
        }
        for &(index, recoil) in callback_recoils {
            action.callback_hits.insert(
                index,
                crate::battle_action::hit_rule(rules, index, DamageKind::Slash, recoil)?,
            );
        }
    }
    Ok(action)
}

/// Stored combinations use the same participant-track format as ordinary artes.
pub(crate) fn combined(
    bytes: &[u8],
) -> Result<BTreeMap<u8, resonance_content::battle_scene::SceneAction>> {
    let end = u32::read(bytes, 0)? as usize;
    ensure!(
        end >= 16 && end <= bytes.len() && (end - 16).is_multiple_of(28),
        "invalid combination variants"
    );
    let mut actions = BTreeMap::new();
    for variant in 0..(end - 16) / 28 {
        if u16::read(bytes, 16 + variant * 28)? == 0 {
            continue;
        }
        let action = match attack(0, variant, bytes, false, true) {
            Ok(attack) => resonance_content::battle_scene::SceneAction::Prepared(Box::new(attack)),
            Err(error) => resonance_content::battle_scene::SceneAction::Unsupported {
                reason: format!("{error:#}"),
            },
        };
        actions.insert(
            variant
                .try_into()
                .context("combination variant exceeds limit")?,
            action,
        );
    }
    Ok(actions)
}

fn attack(
    id: u16,
    variant: usize,
    bytes: &[u8],
    steal_success: bool,
    combined: bool,
) -> Result<Attack> {
    let offsets = (0..4)
        .map(|index| u32::read(bytes, index * 4).map(|v| v as usize))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        offsets[0] >= 44 && offsets.windows(2).all(|p| p[0] <= p[1]),
        "invalid martial action sections"
    );
    let rules = bytes
        .get(offsets[0]..offsets[1])
        .context("missing martial hit rules")?;
    let contacts = bytes
        .get(offsets[1]..offsets[2])
        .context("missing martial contacts")?;
    let poses = bytes
        .get(offsets[2]..offsets[3])
        .context("missing martial poses")?;
    let commands = bytes
        .get(offsets[3]..)
        .context("missing martial commands")?;
    const VARIANT_BYTES: usize = 28;
    let row = bytes
        .get(16..offsets[0])
        .and_then(|rows| rows.get(variant * VARIANT_BYTES..(variant + 1) * VARIANT_BYTES))
        .context("missing martial variant")?;
    let mut events = Vec::new();
    let mut pose_sequence =
        (!combined && matches!(id, 114..=117 | 131 | 133 | 138..=140)).then(Vec::new);
    let mut holds = Vec::new();
    let mut finish_pose = None;
    let mut cursor = u32::read(row, 20)? as usize * 12;
    if steal_success {
        ensure!(matches!(id, 43 | 44), "unexpected alternate steal track");
        // The opening blend already elapsed before this branch is selected.
        holds.push((
            0,
            *poses
                .get(cursor + 3)
                .context("missing steal opening blend")?,
        ));
        cursor = following_stream(poses, cursor, 12, -2)?;
    }
    loop {
        let at = i16::read(poses, cursor)?;
        if at == -2 {
            break;
        }
        let pose = poses
            .get(cursor..cursor + 12)
            .context("truncated martial pose")?;
        if pose[2] == 254 {
            ensure!(
                pose[3..8].iter().all(|&value| value == 0),
                "secondary martial animation rate is not prepared"
            );
            events.push((
                at.try_into()
                    .context("martial animation rate control is not prepared")?,
                Event::AnimationRate(f32::read(pose, 8)?),
            ));
            cursor += 12;
            continue;
        }
        ensure!(
            pose[7] == 255 && pose[6] & !0x40 == 8,
            "unsupported martial pose binding"
        );
        let blend = pose[3];
        let pose = crate::battle_action::action_pose(pose)?;
        if let Some(sequence) = &mut pose_sequence {
            sequence.push(PoseStep {
                start: if at == -3 {
                    PoseStart::AfterPrevious
                } else {
                    PoseStart::At(
                        at.try_into()
                            .context("martial pose control is not prepared")?,
                    )
                },
                pose,
            });
            cursor += 12;
            continue;
        }
        if at == -3 {
            ensure!(finish_pose.is_none(), "duplicate martial finish pose");
            finish_pose = Some(pose);
            cursor += 12;
            continue;
        }
        let at = u16::try_from(at).context("martial pose control is not prepared")?;
        holds.push((at, blend));
        events.push((at, Event::Pose(pose)));
        cursor += 12;
    }
    let mut cursor = u32::read(row, 16)? as usize * 32;
    if steal_success {
        cursor = following_stream(contacts, cursor, 32, -1)?;
    }
    loop {
        let at = i16::read(contacts, cursor)?;
        if at == -1 {
            break;
        }
        let at = u16::try_from(at).context("martial contact control is not prepared")?;
        let contact = contacts
            .get(cursor..cursor + 32)
            .context("truncated martial contact")?;
        if contact[2] as i8 == -2 {
            ensure!(
                contact[3] == 1 && u16::read(contact, 22)? == 0,
                "martial projectile count or modifier is not prepared"
            );
            let (hit, feedback) = projectile_hit(rules, contact)?;
            events.push((
                at,
                Event::Projectile {
                    member: contact[4],
                    hit: Some(hit),
                    feedback,
                },
            ));
        } else {
            let contact = crate::battle_action::contact(rules, contact)?;
            events.push((at, Event::Contact(contact)));
        }
        cursor += 32;
    }
    let mut gravity = None;
    let mut cursor = u32::read(row, 24)? as usize * 2;
    if !steal_success {
        let mut command_at = 0;
        loop {
            let at = i16::read(commands, cursor)?;
            if at == -1 {
                break;
            }
            let at = u16::try_from(at).context("martial command control is not prepared")?;
            // An overdue command still waits for earlier commands in its stream.
            command_at = command_at.max(at);
            let at = command_at;
            let operand = |offset| u16::read(commands, cursor + offset);
            if operand(2)? == 3 {
                let value = f32::from(operand(4)? as i16) / 10.;
                if !combined {
                    gravity.get_or_insert((at, value));
                }
                events.push((at, Event::Gravity(value)));
                cursor += 6;
                continue;
            }
            let (event, width) = match operand(2)? {
                0 => (Event::Forward(f32::from(operand(4)? as i16) / 10.), 6),
                1 => (Event::Vertical(f32::from(operand(4)? as i16) / 10.), 6),
                5 => {
                    ensure!(
                        operand(4)? <= 1 && operand(6)? == 0,
                        "unsupported martial collision mode"
                    );
                    (Event::PassThrough(operand(4)? != 0), 8)
                }
                7 => {
                    let [slot, visible] = operand(4)?.to_be_bytes();
                    ensure!(visible <= 1, "invalid martial weapon visibility");
                    (
                        Event::WeaponVisibility {
                            slot,
                            visible: visible != 0,
                        },
                        6,
                    )
                }
                9 => {
                    let braking = f32::from(operand(4)? as i16) / 10.;
                    ensure!(braking >= 0., "negative martial braking");
                    (Event::Braking(braking), 6)
                }
                13 => (
                    Event::Trail {
                        slot: operand(4)?
                            .try_into()
                            .context("martial trail slot exceeds limit")?,
                        duration: operand(6)?,
                    },
                    8,
                ),
                23 => {
                    let enabled = operand(4)?;
                    ensure!(enabled <= 1, "invalid martial secondary wind mode");
                    (
                        Event::SecondaryWind {
                            enabled: enabled != 0,
                            duration: operand(6)?,
                        },
                        8,
                    )
                }
                27 => {
                    ensure!(operand(6)? == 2, "unsupported martial voice priority");
                    (Event::Voice(crate::battle_voice::sound(operand(4)?)), 8)
                }
                28 => {
                    ensure!(operand(6)? == 0, "unsupported martial sound mode");
                    (Event::Sound(operand(4)?), 8)
                }
                37 => {
                    let duration = operand(4)?;
                    let distance = operand(6)?;
                    ensure!(
                        distance == operand(8)?,
                        "martial camera distance range is not prepared"
                    );
                    ensure!(
                        duration != 0 && distance != 0,
                        "martial camera distance and duration must be positive"
                    );
                    (
                        Event::CameraDistance {
                            distance: f32::from(distance),
                            duration,
                        },
                        10,
                    )
                }
                40 if id == 137 => (Event::Turn(f32::from(operand(4)? as i16) / 10.), 6),
                opcode => anyhow::bail!("martial command {opcode} is not prepared"),
            };
            events.push((at, event));
            cursor += width;
        }
    }
    // Pose clocks consume full blends. Gameplay runs before a new pose is selected.
    let model_timing = pose_sequence.is_some();
    let elapsed = |at: u16, pose_clock: bool| -> Result<u16> {
        if model_timing {
            return Ok(at);
        }
        if !pose_clock && at != 0 {
            return gameplay_tick(at, 0, &holds);
        }
        let hold: u32 = holds
            .iter()
            .filter(|(start, _)| *start < at)
            .map(|(_, blend)| u32::from(*blend))
            .sum();
        (u32::from(at) + hold)
            .try_into()
            .context("martial timeline exceeds limit")
    };
    let mut preceding_contacts = 0;
    for (at, event) in &mut events {
        if !model_timing && matches!(event, Event::Contact(_) | Event::Projectile { .. }) {
            *at = gameplay_tick(*at, preceding_contacts, &holds)?;
            if matches!(event, Event::Contact(_)) {
                preceding_contacts = preceding_contacts
                    .checked_add(1)
                    .context("too many martial contacts")?;
            }
            continue;
        }
        *at = if *at == 0
            && matches!(
                event,
                Event::Forward(_) | Event::Vertical(_) | Event::Gravity(_)
            ) {
            holds.first().map_or(0, |(_, ticks)| u16::from(*ticks))
        } else {
            elapsed(
                *at,
                matches!(event, Event::Pose(_) | Event::AnimationRate(_)),
            )?
        };
        if pose_sequence.is_none() && matches!(event, Event::AnimationRate(_)) {
            *at = at
                .checked_add(1)
                .context("martial animation rate exceeds timeline limit")?;
        }
    }
    events.sort_by_key(|(at, _)| *at);
    // The success tail ends before the unused idle pose at the end of its stream.
    const STEAL_SUCCESS_END: u16 = 130;
    let duration = if steal_success {
        STEAL_SUCCESS_END
    } else {
        u16::read(row, 0)?
    };
    let duration = if pose_sequence.is_some() {
        duration
    } else {
        elapsed(duration, false)?
            .checked_sub(1)
            .context("empty martial action")?
    };
    let chain_at = elapsed(u16::read(row, 6)?, false)?;
    let flight = match (gravity, finish_pose) {
        (_, None) => None,
        (None, Some(finish_pose)) if combined => Some(Flight {
            gravity_at: 0,
            gravity: -1.,
            finish_pose,
        }),
        (Some((at, gravity)), Some(finish_pose)) => Some(Flight {
            gravity_at: elapsed(at, false)?,
            gravity,
            finish_pose,
        }),
        _ => anyhow::bail!("incomplete martial flight"),
    };
    if steal_success {
        events.retain(|(at, _)| *at <= duration);
    }
    Ok(Attack {
        duration,
        chain_at: (!steal_success && chain_at <= duration).then_some(chain_at),
        recovery: u16::read(row, 2)?,
        effect: match u16::read(row, 10)? {
            u16::MAX => None,
            effect => Some(effect),
        },
        events,
        callback_hits: BTreeMap::new(),
        pose_sequence,
        flight,
        hammer: None,
        steal: None,
        fierce_fang: if id == 4 {
            let hit = |index, recoil| {
                crate::battle_action::hit_rule(
                    rules,
                    index,
                    resonance_content::battle_projectile::DamageKind::Slash,
                    recoil,
                )
            };
            Some(resonance_content::battle_martial::FierceFang {
                hits: [hit(1, 1)?, hit(2, 0)?],
            })
        } else {
            None
        },
        hurricane_hit: if id == 11 {
            Some(crate::battle_action::hit_rule(
                rules,
                1,
                resonance_content::battle_projectile::DamageKind::Slash,
                11,
            )?)
        } else {
            None
        },
    })
}

pub(crate) fn projectile_hit(
    rules: &[u8],
    contact: &[u8],
) -> Result<(
    resonance_content::battle_action::ActionHit,
    Option<resonance_content::battle_martial::ProjectileFeedback>,
)> {
    let offset = usize::from(contact[18]) * 28;
    let source = rules
        .get(offset..offset + 28)
        .context("missing martial projectile hit")?;
    const EFFECT_ON_GUARD: u16 = 0x0800;
    let flags = u16::read(source, 0)?;
    // Party-default and explicit techniques selectors use the same bank.
    ensure!(
        source[22] == 0 || source[24] <= 1,
        "martial projectile contact effect bank is not prepared"
    );
    let sound = u16::read(source, 16)?;
    let on_guard = flags & EFFECT_ON_GUARD != 0;
    ensure!(
        source[22] != 0 || sound == 0 || !on_guard,
        "martial projectile guard sound without an effect is not prepared"
    );
    let feedback =
        (source[22] != 0).then_some(resonance_content::battle_martial::ProjectileFeedback {
            effect: u16::from(source[22]),
            sound,
            on_guard,
        });
    // Feedback owns the guard flag, sound and bank selector. The strict shared
    // decoder still checks every remaining damage and reaction field.
    let mut row = source.to_vec();
    row[..2].copy_from_slice(&(flags & !EFFECT_ON_GUARD).to_be_bytes());
    row[16..18].fill(0);
    row[22] = 0;
    row[24] = 0;
    let mut contact = contact.to_vec();
    contact[18] = 0;
    let mut hit = crate::battle_action::hit(&row, &contact)?;
    if feedback.is_none() && sound != 0 {
        hit.sound = Some(sound);
    }
    Ok((hit, feedback))
}

fn hammer(
    id: u16,
    usual: &[u8],
    module: &crate::rel::Rel,
    bytes: &[u8],
    primary: &Attack,
) -> Result<resonance_content::battle_martial::Hammer> {
    use resonance_content::battle_martial::HammerPattern;
    let start = u32::read(bytes, 0)? as usize;
    let end = u32::read(bytes, 4)? as usize;
    let all_rules = bytes.get(start..end).context("missing hammer hits")?;
    let rules = if id == 42 {
        all_rules.get(28..).context("missing hammer ring hits")?
    } else {
        all_rules
    };
    let projectiles = crate::source_assets::section(usual, 7)?;
    const HAMMER_PROJECTILE: usize = 7;
    const PROJECTILE_BYTES: usize = 400;
    let projectile = projectiles
        .get(HAMMER_PROJECTILE * PROJECTILE_BYTES..(HAMMER_PROJECTILE + 1) * PROJECTILE_BYTES)
        .context("missing hammer projectile contact")?;
    ensure!(rules.len() == 3 * 28, "invalid hammer hit count");
    let impact_effect = u16::from(rules[22]);
    let impact_sound = u16::read(rules, 16)?;
    let pattern = match id {
        40 => {
            let alternate = attack(40, 1, bytes, false, false)?;
            ensure!(
                (
                    primary.duration,
                    primary.chain_at,
                    primary.recovery,
                    primary.effect
                ) == (
                    alternate.duration,
                    alternate.chain_at,
                    alternate.recovery,
                    alternate.effect
                ) && primary.flight.is_none()
                    && alternate.flight.is_none(),
                "hammer variants have different timing"
            );
            HammerPattern::Single {
                alternate_events: alternate.events,
            }
        }
        41 => {
            let data = module.at((4, 0x4018))?;
            HammerPattern::Volley {
                offset: [
                    f32::read(data, 0)?,
                    f32::read(data, 4)?,
                    f32::read(data, 8)?,
                ],
                forward: f32::read(data, 12)?,
                jitter: f32::read(data, 16)?,
            }
        }
        42 => {
            let data = module.at((4, 0x31f0))?;
            HammerPattern::Ring {
                offset: [
                    f32::read(data, 0)?,
                    f32::read(data, 4)?,
                    f32::read(data, 8)?,
                ],
                radians: f32::read(data, 12)?,
                step: f32::read(data, 16)?,
                speed: f32::read(data, 20)?,
                speed_jitter: f32::read(data, 24)?,
                lift: f32::read(data, 28)?,
                lift_jitter: f32::read(data, 32)?,
                opening_hit: crate::battle_action::hit_rule(
                    all_rules,
                    0,
                    resonance_content::battle_projectile::DamageKind::Slash,
                    *projectiles
                        .get(13 * PROJECTILE_BYTES + 0x13)
                        .context("missing hammer ring opening recoil")?,
                )?,
            }
        }
        _ => unreachable!("hammer action ID"),
    };
    let hits = rules
        .chunks_exact(28)
        .map(|source| {
            const EFFECT_ON_GUARD: u16 = 0x0800;
            let flags = u16::read(source, 0)?;
            ensure!(
                flags & EFFECT_ON_GUARD != 0
                    && u16::from(source[22]) == impact_effect
                    && u16::read(source, 16)? == impact_sound
                    && source[24] == 0,
                "hammer contact feedback differs between variants"
            );
            // These fields are represented by projectile contact feedback. Everything
            // else still passes through the strict shared damage decoder.
            let mut row = source.to_vec();
            row[..2].copy_from_slice(&(flags & !EFFECT_ON_GUARD).to_be_bytes());
            row[16..18].fill(0);
            row[22] = 0;
            let mut contact = [0; 32];
            contact[20] = projectile[0x13];
            crate::battle_action::hit(&row, &contact)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(resonance_content::battle_martial::Hammer {
        pattern,
        hits: hits
            .try_into()
            .map_err(|_| anyhow::anyhow!("missing hammer hit"))?,
        impact_effect,
        impact_sound,
    })
}

fn following_stream(
    bytes: &[u8],
    mut cursor: usize,
    stride: usize,
    sentinel: i16,
) -> Result<usize> {
    while i16::read(bytes, cursor)? != sentinel {
        cursor += stride;
    }
    Ok(cursor + stride)
}

fn steal(
    id: u16,
    usual: &[u8],
    module: &crate::rel::Rel,
    bytes: &[u8],
    primary: &Attack,
) -> Result<resonance_content::battle_martial::Steal> {
    let success = attack(id, 0, bytes, true, false)?;
    let opening_blend = primary
        .events
        .iter()
        .find_map(|(at, event)| match event {
            Event::Pose(pose) if *at == 0 => Some(u16::from(pose.blend)),
            _ => None,
        })
        .context("missing Item Thief opening pose")?;
    let rules = bytes
        .get(u32::read(bytes, 0)? as usize..u32::read(bytes, 4)? as usize)
        .context("missing Item Thief hit rules")?;
    ensure!(rules.len() == 3 * 28, "invalid Item Thief hit count");
    let projectiles = crate::source_assets::section(usual, 7)?;
    const HAMMER_PROJECTILE: usize = 7;
    const PROJECTILE_BYTES: usize = 400;
    let projectile = projectiles
        .get(HAMMER_PROJECTILE * PROJECTILE_BYTES..(HAMMER_PROJECTILE + 1) * PROJECTILE_BYTES)
        .context("missing Item Thief bonus projectile")?;
    let mut contact = [0; 32];
    contact[18] = 2;
    contact[20] = projectile[0x13];
    let parameters = module.at((4, 0x3540))?;
    let [zero, height, speed, jitter]: [f32; 4] = [0, 4, 8, 12]
        .map(|offset| f32::read(parameters, offset))
        .into_iter()
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .unwrap();
    ensure!(
        [zero, height, speed, jitter].iter().all(|v| v.is_finite()) && jitter >= 0.,
        "invalid Item Thief bonus motion"
    );
    Ok(resonance_content::battle_martial::Steal {
        attempt_at: 60 + opening_blend,
        bonus_at: 40 + opening_blend,
        success_duration: success.duration,
        success_events: success.events,
        bonus_hit: crate::battle_action::hit(rules, &contact)?,
        bonus_offset: [zero, zero, height],
        bonus_forward_speed: speed,
        bonus_velocity_jitter: [jitter; 3],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires original BTLusual data; CPU only"]
    fn early_melee_records_match_observed_contacts_and_timing() -> Result<()> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/extracted/disc1/files/BTL/BTLusual.dat");
        let usual = std::fs::read(path)?;
        let module = crate::rel::Rel::read(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../local/extracted/disc1/files/US_r_Top2Btl.rel"),
        )?;
        let actions = read(&usual, &module)?;
        let thief = actions.attack(223, 2)?;
        let steal = thief.steal.as_ref().unwrap();
        assert_eq!(
            (
                thief.duration,
                steal.attempt_at,
                steal.bonus_at,
                steal.success_duration
            ),
            (176, 64, 44, 136)
        );
        assert!(
            steal
                .success_events
                .iter()
                .any(|(at, event)| *at == 69
                    && matches!(event, Event::Pose(pose) if pose.clip == 55))
        );
        // The contact at 95 follows the opening blend (4) and crossed blend (3).
        assert!(steal.success_events.iter().any(|(at, event)| *at == 102 && matches!(event, Event::Contact(contact) if contact.hit.bypass_guard && contact.hit.recoil == 7 && contact.duration == 16)));
        let fang = actions.attack(1, 1)?;
        for (catalogue, character, duration, chain, recovery) in [
            (4, 1, 68, 48, 10),
            (209, 6, 63, 49, 15),
            (209, 9, 63, 49, 15),
        ] {
            let action = actions.attack(catalogue, character)?;
            assert_eq!(
                (action.duration, action.chain_at, action.recovery),
                (duration, Some(chain), recovery)
            );
            let fierce = action.fierce_fang.as_ref().unwrap();
            assert!(action.events.iter().any(|(at, event)| *at == 24
                && matches!(
                    event,
                    Event::SecondaryWind {
                        enabled: true,
                        duration: 40
                    }
                )));
            assert_eq!(
                fierce.hits[0].power,
                resonance_content::battle_action::Power::Percent(60)
            );
            assert_eq!(
                fierce.hits[1].power,
                resonance_content::battle_action::Power::Percent(70)
            );
            assert_eq!(fierce.hits[1].recoil, 6);
        }
        assert_eq!(
            (fang.duration, fang.chain_at, fang.recovery, fang.effect),
            (47, Some(29), 10, Some(3))
        );
        assert!(fang.events.iter().any(|(at, event)| *at == 0
            && matches!(event, Event::Pose(pose) if pose.clip == 43 && pose.blend == 4)));
        assert!(fang.events.iter().any(|(at, event)| *at == 0
            && matches!(
                event,
                Event::Trail {
                    slot: 1,
                    duration: 90
                }
            )));
        assert!(
            fang.events
                .iter()
                .any(|(at, event)| *at == 8 && matches!(event, Event::Sound(60)))
        );
        assert!(fang.events.iter().any(|(at, event)| *at == 16
            && matches!(
                event,
                Event::Projectile {
                    member: 2,
                    hit: Some(hit),
                    ..
                } if hit.power == resonance_content::battle_action::Power::Percent(120)
            )));
        for (catalogue, character, duration, chain_at, effect) in
            [(2, 1, 68, 49, 5), (208, 6, 62, 59, 6), (208, 9, 62, 59, 6)]
        {
            let double = actions.attack(catalogue, character)?;
            assert_eq!(
                (double.duration, double.chain_at, double.effect),
                (duration, Some(chain_at), Some(effect))
            );
            assert_eq!(
                double
                    .events
                    .iter()
                    .filter_map(|(at, event)| match event {
                        Event::Projectile {
                            member: 2,
                            hit: Some(hit),
                            feedback: Some(feedback),
                        } => {
                            assert_eq!(
                                (feedback.effect, feedback.sound, feedback.on_guard),
                                (8, 0, true)
                            );
                            assert_eq!(
                                hit.power,
                                resonance_content::battle_action::Power::Percent(105)
                            );
                            assert_eq!((hit.hitstun, hit.cooldown), (30, 30));
                            Some(*at)
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
                // Projectile emissions do not advance the melee contact clock.
                [14, 37]
            );
        }
        // Shared catalogue IDs must retain each knight's own voice and movement.
        for (character, voice, forward_at) in [(6, 650, 24), (9, 969, 35)] {
            let double = actions.attack(208, character)?;
            assert!(double.events.iter().any(|(_, event)| matches!(event,
                Event::Voice(Some(resonance_content::battle_voice::Sound::Stream(id))) if *id == voice)));
            assert!(
                double
                    .events
                    .iter()
                    .any(|(at, event)| *at == forward_at && matches!(event, Event::Forward(6.)))
            );
        }
        let kratos = actions.attack(207, 9)?;
        assert_eq!(
            (
                kratos.duration,
                kratos.chain_at,
                kratos.recovery,
                kratos.effect
            ),
            (39, Some(32), 8, Some(4))
        );
        assert!(kratos.events.iter().any(|(at, event)| *at == 0
            && matches!(
                event,
                Event::Voice(Some(resonance_content::battle_voice::Sound::Stream(968)))
            )));
        assert!(
            kratos
                .events
                .iter()
                .any(|(at, event)| *at == 4 && matches!(event, Event::Forward(4.)))
        );
        // Unlike Demon Fang, Sonic Thrust has unequal contact/pose indices.
        let sonic_knight = actions.attack(210, 9)?;
        assert_eq!((sonic_knight.duration, sonic_knight.recovery), (71, 5));
        assert!(sonic_knight.events.iter().any(|(_, event)| matches!(event,
            Event::Pose(pose) if pose.clip == 50 && pose.rate == 1.)));
        assert!(sonic_knight.events.iter().any(|(at, event)| *at == 18
            && matches!(event, Event::Contact(contact) if contact.duration == 5)));
        assert!(
            actions
                .attack(12, 1)?
                .events
                .iter()
                .any(|(_, event)| matches!(event,
            Event::Pose(pose) if pose.clip == 50 && pose.rate == 1. && pose.start == 8.))
        );
        let beast = actions.attack(20, 1)?;
        assert_eq!((beast.duration, beast.recovery), (81, 10));
        assert_eq!(
            beast
                .events
                .iter()
                .filter_map(|(at, event)| match event {
                    Event::Contact(contact) =>
                        Some((*at, contact.duration, contact.hit.knock_down)),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            [(16, 9, false), (38, 11, true)]
        );
        let ray = actions.attack(35, 2)?;
        assert_eq!(
            (ray.duration, ray.chain_at, ray.recovery, ray.effect),
            (98, Some(50), 10, None)
        );
        assert!(ray.events.iter().any(|(at, event)| *at == 28
            && matches!(event, Event::Projectile { member: 6, hit: Some(hit), .. }
                if hit.power == resonance_content::battle_action::Power::Percent(130)
                && hit.kind == resonance_content::battle_projectile::DamageKind::Slash
                && hit.hitstun == 45 && hit.cooldown == 30 && hit.stagger == 5
                && hit.recoil == 1)));
        let tempest = actions.attack(17, 1)?;
        assert_eq!((tempest.recovery, tempest.effect), (15, Some(45)));
        assert_eq!(
            tempest
                .events
                .iter()
                .filter_map(|(at, event)| match event {
                    Event::Contact(contact) => Some((*at, contact.duration)),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            [(6, 9), (16, 9), (26, 9), (36, 9)]
        );
        assert!(tempest.events.iter().any(|(at, event)| *at == 0
            && matches!(
                event,
                Event::Trail {
                    slot: 1,
                    duration: 90
                }
            )));
        let omega_tempest = actions.attack(19, 1)?;
        assert!(
            omega_tempest
                .events
                .iter()
                .any(|(at, event)| *at == 4
                    && matches!(event, Event::Gravity(value) if *value == 0.1))
        );
        assert!(
            !omega_tempest
                .events
                .iter()
                .any(|(at, event)| *at == 0 && matches!(event, Event::Gravity(_)))
        );
        let tiger = actions.attack(6, 1)?;
        assert_eq!(
            (tiger.duration, tiger.recovery, tiger.effect),
            (47, 12, Some(48))
        );
        let flight = tiger.flight.as_ref().unwrap();
        assert_eq!((flight.gravity_at, flight.gravity), (6, -1.2));
        assert_eq!((flight.finish_pose.clip, flight.finish_pose.blend), (0, 8));
        assert_eq!(
            tiger
                .events
                .iter()
                .filter_map(|(at, event)| match event {
                    Event::Pose(pose) => Some((*at, pose.clip)),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            [(0, 46), (18, 47), (38, 42)]
        );
        assert_eq!(
            tiger
                .events
                .iter()
                .filter_map(|(at, event)| match event {
                    Event::Contact(contact) => Some((*at, contact.duration, contact.hit.recoil)),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            [(6, 7, 7), (26, 11, 1)]
        );
        let tiger_rage = actions.attack(7, 1)?;
        assert!(tiger_rage.events.iter().any(
            |(at, event)| *at == 33 && matches!(event, Event::Gravity(value) if *value == -1.5)
        ));
        let sonic = actions.attack(10, 1)?;
        assert_eq!(
            (sonic.duration, sonic.recovery, sonic.effect),
            (53, 10, Some(34))
        );
        let contacts: Vec<_> = sonic
            .events
            .iter()
            .filter_map(|(at, event)| {
                if let Event::Contact(contact) = event {
                    Some((*at, contact))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(contacts.len(), 1);
        assert_eq!((contacts[0].0, contacts[0].1.duration), (16, 7));
        assert_eq!(
            contacts[0].1.hit.power,
            resonance_content::battle_action::Power::Percent(140)
        );
        let sword = actions.attack(13, 1)?;
        assert_eq!((sword.duration, sword.recovery), (79, 10));
        assert_eq!(
            sword
                .events
                .iter()
                .filter_map(|(at, event)| match event {
                    Event::Pose(pose) => Some((*at, pose.clip)),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            [(0, 52), (9, 51), (16, 52), (23, 53), (30, 50)]
        );
        assert_eq!(
            sword
                .events
                .iter()
                .filter_map(|(at, event)| { matches!(event, Event::Contact(_)).then_some(*at) })
                .collect::<Vec<_>>(),
            [6, 13, 20, 27, 47]
        );
        assert_eq!(
            sword
                .events
                .iter()
                .filter_map(|(at, event)| match event {
                    Event::Forward(speed) => Some((*at, *speed)),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            [(4, 4.), (9, 4.), (15, 4.), (48, 6.)]
        );
        let attack = actions.attack(161, 7)?;
        assert_eq!(
            attack
                .events
                .iter()
                .filter_map(|(at, event)| match event {
                    Event::Forward(speed) => Some((*at, *speed)),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            [(30, 7.), (44, 7.), (58, 7.), (66, 7.)]
        );
        Ok(())
    }
}
