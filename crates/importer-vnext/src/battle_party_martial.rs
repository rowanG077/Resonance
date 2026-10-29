//! Decode the shared party action directory into native attack events.
use crate::battle_party_native::{float, half, member, signed, vector, word};
use anyhow::{Context, Result, ensure};
use resonance_content::battle_action::WeaponSlot;
use resonance_content::{
    battle_martial::{
        Attack, Binding, Catalogue, Flight, Member, PoseStart, PoseStep, gameplay_tick,
    },
    battle_normal::Event,
    battle_scene::SceneAction,
};
use std::collections::BTreeMap;

pub(crate) fn read(usual: &[u8], module: &crate::rel::Rel) -> Result<Catalogue> {
    let directory = member(usual, 8)?;
    let count = word(directory, 0)? as usize;
    ensure!(
        count <= (directory.len() - 4) / 4,
        "truncated common action directory"
    );
    let offsets = (0..count)
        .map(|index| word(directory, 4 + index * 4).map(|v| v as usize))
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
            let variant_end = word(bytes, 0)? as usize;
            ensure!(
                variant_end >= 16
                    && variant_end <= bytes.len()
                    && (variant_end - 16).is_multiple_of(28),
                "invalid common action variants"
            );
            let mut variants = BTreeMap::new();
            for variant in 0..(variant_end - 16) / 28 {
                if half(bytes, 16 + variant * 28)? == 0 {
                    continue;
                }
                let prepared = prepare(id, variant, bytes, usual, module);
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
    for (catalogue, member) in (145..=151).zip(81..=87) {
        bind(catalogue, member, 6, 0);
        bind(catalogue, member, 9, 1);
    }
    for id in [36, 37, 38, 39, 41, 42, 46, 48, 50, 52] {
        bind(id, id, 2, 0);
    }
    for (catalogue, member) in (125..=136)
        .zip(62..=73)
        .chain([(140, 77), (141, 78), (143, 80)])
    {
        bind(catalogue, member, 5, 0);
    }
    for (catalogue, member) in (154..=175).zip(90..=111) {
        bind(catalogue, member, 7, 0);
    }
    for (catalogue, member) in (176..=191).zip(112..=127).chain((195..=201).zip(131..=137)) {
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
    let mut action = attack(id, variant, bytes, false)
        .with_context(|| format!("martial action {id}, variant {variant}"))?;
    let projectiles = member(usual, 7)?;
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
            .get(word(bytes, 0)? as usize..word(bytes, 4)? as usize)
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
                crate::battle_party_native::hit_rule(rules, index, kind, row[0x13])?,
            );
        }
        for &(index, recoil) in callback_recoils {
            action.callback_hits.insert(
                index,
                crate::battle_party_native::hit_rule(rules, index, DamageKind::Slash, recoil)?,
            );
        }
    }
    Ok(action)
}

fn attack(id: u16, variant: usize, bytes: &[u8], steal_success: bool) -> Result<Attack> {
    let offsets = [
        word(bytes, 0)?,
        word(bytes, 4)?,
        word(bytes, 8)?,
        word(bytes, 12)?,
    ]
    .map(|offset| offset as usize);
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
    let mut pose_sequence = matches!(id, 114..=117 | 131 | 133 | 138..=140).then(Vec::new);
    let mut holds = Vec::new();
    let mut finish_pose = None;
    let mut cursor = word(row, 20)? as usize * 12;
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
        let at = signed(poses, cursor)?;
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
                Event::AnimationRate(float(pose, 8)?),
            ));
            cursor += 12;
            continue;
        }
        ensure!(
            pose[7] == 255 && pose[6] & !0x40 == 8,
            "unsupported martial pose binding"
        );
        let blend = pose[3];
        let pose = crate::battle_party_native::action_pose(pose)?;
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
    let mut cursor = word(row, 16)? as usize * 32;
    if steal_success {
        cursor = following_stream(contacts, cursor, 32, -1)?;
    }
    loop {
        let at = signed(contacts, cursor)?;
        if at == -1 {
            break;
        }
        let at = u16::try_from(at).context("martial contact control is not prepared")?;
        let contact = contacts
            .get(cursor..cursor + 32)
            .context("truncated martial contact")?;
        if contact[2] as i8 == -2 {
            ensure!(
                contact[3] == 1 && half(contact, 22)? == 0,
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
            let contact = crate::battle_party_native::contact(rules, contact)?;
            events.push((at, Event::Contact(contact)));
        }
        cursor += 32;
    }
    let mut gravity = None;
    let mut cursor = word(row, 24)? as usize * 2;
    if !steal_success {
        let mut command_at = 0;
        loop {
            let at = signed(commands, cursor)?;
            if at == -1 {
                break;
            }
            let at = u16::try_from(at).context("martial command control is not prepared")?;
            // An overdue command still waits for earlier commands in its stream.
            command_at = command_at.max(at);
            let at = command_at;
            let operand = |offset| half(commands, cursor + offset);
            if operand(2)? == 3 {
                let value = f32::from(operand(4)? as i16) / 10.;
                gravity.get_or_insert((at, value));
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
                            slot: WeaponSlot(slot),
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
                12 => {
                    let enabled = operand(4)?;
                    let duration = operand(6)? as i16;
                    ensure!(
                        enabled <= 1 && duration >= 0,
                        "invalid martial reaction armor"
                    );
                    (
                        Event::ReactionArmor(if enabled == 0 { 0 } else { duration as u16 }),
                        8,
                    )
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
                    (
                        Event::Voice(crate::battle_party_native::sound(operand(4)?)),
                        8,
                    )
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
        half(row, 0)?
    };
    let duration = if pose_sequence.is_some() {
        duration
    } else {
        elapsed(duration, false)?
            .checked_sub(1)
            .context("empty martial action")?
    };
    let chain_at = elapsed(half(row, 6)?, false)?;
    let flight = match (gravity, finish_pose) {
        (_, None) => None,
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
        recovery: half(row, 2)?,
        effect: match half(row, 10)? {
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
                crate::battle_party_native::hit_rule(
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
            Some(crate::battle_party_native::hit_rule(
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
    let mut row: [u8; 28] = rules
        .get(offset..offset + 28)
        .context("missing martial projectile hit")?
        .try_into()?;
    const EFFECT_ON_GUARD: u16 = 0x0800;
    let flags = half(&row, 0)?;
    // Party-default and explicit techniques selectors use the same bank.
    ensure!(
        row[22] == 0 || row[24] <= 1,
        "martial projectile contact effect bank is not prepared"
    );
    let sound = half(&row, 16)?;
    let on_guard = flags & EFFECT_ON_GUARD != 0;
    ensure!(
        row[22] != 0 || sound == 0 || !on_guard,
        "martial projectile guard sound without an effect is not prepared"
    );
    let feedback =
        (row[22] != 0).then_some(resonance_content::battle_martial::ProjectileFeedback {
            effect: u16::from(row[22]),
            sound,
            on_guard,
        });
    // Feedback owns the guard flag, sound and bank selector. The strict shared
    // decoder still checks every remaining damage and reaction field.
    row[..2].copy_from_slice(&(flags & !EFFECT_ON_GUARD).to_be_bytes());
    row[16..18].fill(0);
    row[22] = 0;
    row[24] = 0;
    let mut hit = crate::battle_party_native::hit_rule(
        &row,
        0,
        crate::battle_party_native::damage_kind(contact[17])?,
        contact[20],
    )?;
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
    let start = word(bytes, 0)? as usize;
    let end = word(bytes, 4)? as usize;
    let all_rules = bytes.get(start..end).context("missing hammer hits")?;
    let rules = if id == 42 {
        all_rules.get(28..).context("missing hammer ring hits")?
    } else {
        all_rules
    };
    let projectiles = member(usual, 7)?;
    const HAMMER_PROJECTILE: usize = 7;
    const PROJECTILE_BYTES: usize = 400;
    let projectile = projectiles
        .get(HAMMER_PROJECTILE * PROJECTILE_BYTES..(HAMMER_PROJECTILE + 1) * PROJECTILE_BYTES)
        .context("missing hammer projectile contact")?;
    ensure!(rules.len() == 3 * 28, "invalid hammer hit count");
    let impact_effect = u16::from(rules[22]);
    let impact_sound = half(rules, 16)?;
    let pattern = match id {
        40 => {
            let alternate = attack(40, 1, bytes, false)?;
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
            let data = module.tail(4, 0x4018)?;
            HammerPattern::Volley {
                offset: vector(data, 0)?,
                forward: float(data, 12)?,
                jitter: float(data, 16)?,
            }
        }
        42 => {
            let data = module.tail(4, 0x31f0)?;
            HammerPattern::Ring {
                offset: vector(data, 0)?,
                radians: float(data, 12)?,
                step: float(data, 16)?,
                speed: float(data, 20)?,
                speed_jitter: float(data, 24)?,
                lift: float(data, 28)?,
                lift_jitter: float(data, 32)?,
                opening_hit: crate::battle_party_native::hit_rule(
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
            let flags = half(source, 0)?;
            ensure!(
                flags & EFFECT_ON_GUARD != 0
                    && u16::from(source[22]) == impact_effect
                    && half(source, 16)? == impact_sound
                    && source[24] == 0,
                "hammer contact feedback differs between variants"
            );
            // These fields are represented by projectile contact feedback. Everything
            // else still passes through the strict shared damage decoder.
            let mut row = source.to_vec();
            row[..2].copy_from_slice(&(flags & !EFFECT_ON_GUARD).to_be_bytes());
            row[16..18].fill(0);
            row[22] = 0;
            crate::battle_party_native::hit_rule(
                &row,
                0,
                resonance_content::battle_projectile::DamageKind::Slash,
                projectile[0x13],
            )
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
    while signed(bytes, cursor)? != sentinel {
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
    let success = attack(id, 0, bytes, true)?;
    let opening_blend = primary
        .events
        .iter()
        .find_map(|(at, event)| match event {
            Event::Pose(pose) if *at == 0 => Some(u16::from(pose.blend)),
            _ => None,
        })
        .context("missing Item Thief opening pose")?;
    let rules = bytes
        .get(word(bytes, 0)? as usize..word(bytes, 4)? as usize)
        .context("missing Item Thief hit rules")?;
    ensure!(rules.len() == 3 * 28, "invalid Item Thief hit count");
    let projectiles = member(usual, 7)?;
    const HAMMER_PROJECTILE: usize = 7;
    const PROJECTILE_BYTES: usize = 400;
    let projectile = projectiles
        .get(HAMMER_PROJECTILE * PROJECTILE_BYTES..(HAMMER_PROJECTILE + 1) * PROJECTILE_BYTES)
        .context("missing Item Thief bonus projectile")?;
    let parameters = module.tail(4, 0x3540)?;
    let [zero, height, speed, jitter] = [
        float(parameters, 0)?,
        float(parameters, 4)?,
        float(parameters, 8)?,
        float(parameters, 12)?,
    ];
    ensure!(jitter >= 0., "invalid Item Thief bonus motion");
    Ok(resonance_content::battle_martial::Steal {
        attempt_at: 60 + opening_blend,
        bonus_at: 40 + opening_blend,
        success_duration: success.duration,
        success_events: success.events,
        bonus_hit: crate::battle_party_native::hit_rule(
            rules,
            2,
            resonance_content::battle_projectile::DamageKind::Slash,
            projectile[0x13],
        )?,
        bonus_offset: [zero, zero, height],
        bonus_forward_speed: speed,
        bonus_velocity_jitter: [jitter; 3],
    })
}
