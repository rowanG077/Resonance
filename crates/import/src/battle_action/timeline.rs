use super::*;
use crate::battle_projectile::EnemyProjectile;
use resonance_content::battle_projectile::{DamageKind, HitShape, ProjectileOrigin};
use std::borrow::Cow;

pub(super) struct ActionPackage<'a> {
    bytes: &'a [u8],
    rules: Cow<'a, [u8]>,
    common_actions: &'a [u8],
    animations: &'a [u8],
    contacts: &'a [u8],
    commands: &'a [u8],
    projectiles: &'a [EnemyProjectile],
    projectile_modifiers: &'a [u8],
    members: crate::model_preview::PointerMembers<'a>,
    expressions: [u8; 4],
    body_motion_disabled: bool,
}

impl<'a> ActionPackage<'a> {
    pub fn new(
        bytes: &'a [u8],
        projectiles: &'a [EnemyProjectile],
        common_actions: &'a [u8],
    ) -> Result<Self> {
        let members = crate::model_preview::PointerMembers::new(bytes, 24..488)?;
        let projectile_modifiers = members.model(0x1dc)?.unwrap_or_default();
        let profile = usize::from(u16::read(bytes, 4)?);
        let mut rules = Cow::Borrowed(section(bytes, 8)?.1);
        // Bank two explicitly selects this enemy's bank. Resolve that ownership
        // once so contacts share the ordinary owner-relative hit representation.
        if rules
            .chunks_exact(28)
            .any(|rule| rule[22] != 0 && rule[24] == 2)
        {
            for rule in rules.to_mut().chunks_exact_mut(28) {
                if rule[24] == 2 {
                    rule[24] = 0;
                }
            }
        }
        Ok(Self {
            bytes,
            rules,
            common_actions,
            animations: section(bytes, 18)?.1,
            contacts: section(bytes, 16)?.1,
            commands: section(bytes, 14)?.1,
            projectiles,
            projectile_modifiers,
            members,
            expressions: <[u8; 4]>::read(bytes, profile + 0xd7)?,
            body_motion_disabled: u32::read(bytes, profile + 0x5c)? & 0x0800_0000 != 0,
        })
    }

    fn projectile_row(&self, index: u8) -> Result<&[u8]> {
        let start = usize::from(index) * 400;
        self.members
            .model(0x1c8)?
            .and_then(|table| table.get(start..start + 400))
            .context("missing enemy projectile template")
    }

    pub fn ground_projectiles(
        &self,
        index: u8,
        effects: [u8; 2],
        retreat: f32,
    ) -> Result<EnemyEvent> {
        let projectile = self
            .projectiles
            .get(usize::from(index))
            .context("missing ground projectile")?;
        let row = self.projectile_row(index)?;
        let kind = damage_kind(row[0x11])?;
        let prepare = |member| {
            crate::battle_projectile::with_model_effect(
                row,
                &projectile.definition,
                &self.members,
                member,
            )
        };
        Ok(EnemyEvent::GroundProjectiles {
            definitions: Box::new([prepare(effects[0])?, prepare(effects[1])?]),
            // These policy contacts use the base attack rule.
            hit: hit_rule(&self.rules, 0, kind, projectile.recoil)?,
            retreat,
        })
    }

    pub fn execution(&self, row: &[u8], policy_ticks: &mut [u16]) -> Result<EnemyExecution> {
        let technique = u16::read(row, 64)?;
        const FIRST_SPELL: u16 = 200;
        if technique >= FIRST_SPELL {
            ensure!(
                policy_ticks.is_empty(),
                "enemy policy requires an attack clock"
            );
            return Ok(EnemyExecution::Spell(technique));
        }
        let common = (technique != 0)
            .then(|| crate::source_assets::section(self.common_actions, usize::from(technique)))
            .transpose()?;
        let rules = if let Some(common) = common {
            common
                .get(u32::read(common, 0)? as usize..u32::read(common, 4)? as usize)
                .context("missing physical arte hit rules")?
        } else {
            &self.rules
        };
        let contact_recovery = self.contact_recovery(row)?;
        let mut events = Vec::new();
        if row[34] != 0 {
            events.push((0, EnemyEvent::Effect(u16::from(row[34]))));
        }
        let animations = self.pose_rows(u16::read(row, 24)?)?;
        let mut partial_events = Vec::new();
        let mut partial_holds = None;
        if animations.iter().any(|(_, row)| is_partial_pose(row)) {
            let prepared = partial_animations(&animations)?;
            partial_events = prepared.events;
            partial_holds = Some(prepared.holds);
        } else {
            for (at, row) in &animations {
                if row[2] == 254 {
                    ensure!(
                        row[3..8].iter().all(|&value| value == 0),
                        "secondary animation rate change is not prepared"
                    );
                    events.push((*at, EnemyEvent::AnimationRate(f32::read(row, 8)?)));
                    continue;
                }
                ensure!(
                    matches!(row[7], 0 | 255),
                    "secondary enemy action animation is not prepared"
                );
                const LOOP: u8 = 0x40;
                ensure!(
                    row[6] & !LOOP == 8,
                    "enemy animation flags {:#x} are not prepared",
                    row[6]
                );
                let member = 0x20 + usize::from(row[2]) * 4;
                ensure!(member < 0x1dc, "enemy pose exceeds animation table");
                let mut pose = action_pose(row)?;
                if self.members.model(member)?.is_none() {
                    // Empty visual slots retain the current pose without a blend hold.
                    if technique != 0 {
                        continue;
                    }
                    pose.blend = 0;
                }
                events.push((*at, EnemyEvent::Pose(pose)));
            }
        }
        let mut next_contact_at = 0_u16;
        let mut contact_delay = 0_u16;
        stream(
            self.contacts,
            u16::read(row, 28)?,
            32,
            -1,
            |authored_at, row| {
                let contact_at = authored_at.max(next_contact_at);
                let at = contact_at
                    .checked_add(contact_delay)
                    .context("enemy contact timeline exceeds tick range")?;
                let count =
                    usize::try_from(row[3] as i8).context("negative enemy contact count")?;
                ensure!(
                    (1..=4).contains(&count),
                    "invalid enemy contact count {count}"
                );
                match row[2] as i8 {
                    -2 => {
                        let rule = rules
                            .get(usize::from(row[18]) * 28..)
                            .context("missing projectile hit rule")?;
                        let (hit, feedback) = if common.is_some() && u8::read(rule, 24)? == 1 {
                            crate::battle_martial::projectile_hit(rules, row)?
                        } else {
                            (hit(rules, row)?, None)
                        };
                        ensure!(!hit.capture, "capture requires a melee body contact");
                        // A projectile row consumes an update even when the next
                        // row has the same authored time.
                        next_contact_at = contact_at
                            .checked_add(1)
                            .context("projectile contact timeline exceeds tick range")?;
                        ensure!(count == 1, "invalid projectile emission count {count}");
                        {
                            let projectile = row[4];
                            let definition = self
                                .projectiles
                                .get(usize::from(projectile))
                                .context("missing enemy projectile")?;
                            let mut shot = crate::battle_projectile::with_modifier(
                                definition,
                                self.projectile_modifiers,
                                u16::read(row, 22)?,
                            )?;
                            if shot.definition.motion.origin != ProjectileOrigin::TargetFloor {
                                shot.definition.motion.origin = if row[5] == 0 {
                                    ProjectileOrigin::Actor
                                } else {
                                    ProjectileOrigin::Bone(row[5])
                                };
                            }
                            let hit = ActionHit {
                                kind: damage_kind(self.projectile_row(projectile)?[0x11])?,
                                recoil: shot.recoil,
                                ..hit
                            };
                            events.push((
                                at,
                                EnemyEvent::Projectile {
                                    definition: Box::new(shot.definition),
                                    hit,
                                    feedback,
                                },
                            ));
                        }
                    }
                    duration if duration > 0 => {
                        let mut contact = contact(rules, row)?;
                        // Contact rows share a clock that pauses for one update
                        // at each inclusive endpoint. Keep authored expiry in
                        // that clock, then translate to elapsed action time.
                        let end = authored_at
                            .checked_add(contact.duration)
                            .context("melee contact timeline exceeds tick range")?;
                        contact.duration = end
                            .checked_sub(contact_at)
                            .filter(|duration| *duration > 0)
                            .context("fully elapsed enemy contact row is not prepared")?;
                        next_contact_at = end - 1;
                        contact_delay = contact_delay
                            .checked_add(1)
                            .context("enemy contact retirement delay exceeds tick range")?;
                        let event = if contact_at > authored_at {
                            EnemyEvent::ContinueContact(contact)
                        } else {
                            EnemyEvent::Contact(contact)
                        };
                        events.push((at, event));
                    }
                    emission => anyhow::bail!("enemy contact emission {emission} is not prepared"),
                }
                Ok(())
            },
        )?;
        let mut commands = Vec::new();
        let duration = self.commands(u16::read(row, 26)?, u16::read(row, 14)?, &mut commands)?;
        // Commands run before contact and animation rows at the same authored age.
        commands.extend(events);
        let mut events = commands;
        if common.is_some() {
            for (_, event) in &mut events {
                let hit = match event {
                    EnemyEvent::Contact(contact) | EnemyEvent::ContinueContact(contact) => {
                        &mut contact.hit
                    }
                    EnemyEvent::Projectile { hit, .. } => hit,
                    _ => continue,
                };
                self.physical_feedback(hit)?;
            }
        }
        events.sort_by_key(|(at, _)| *at);
        // Resolve blend holds here so native attacks need only one elapsed clock.
        // The opening blend holds the initial action tick too; later blends
        // start after their pose event's tick has already advanced.
        let holds: Vec<_> = partial_holds.unwrap_or_else(|| {
            events
                .iter()
                .filter_map(|(at, event)| match event {
                    EnemyEvent::Pose(pose) => Some((
                        *at,
                        if *at == 0 {
                            pose.blend
                        } else {
                            pose.blend.saturating_sub(1)
                        },
                    )),
                    _ => None,
                })
                .collect()
        });
        ensure!(
            holds.windows(2).all(|pair| pair[0].0 != pair[1].0),
            "simultaneous enemy pose changes are not prepared"
        );
        // Later commands wait for admission unless an opening blend already holds them.
        // The opening pose and unblended time-zero movement remain immediate.
        let opening_pose = holds.first().is_some_and(|(at, _)| *at == 0);
        let admission_delay = u16::from(
            holds
                .first()
                .is_none_or(|(at, blend)| *at != 0 || *blend == 0),
        );
        let elapsed = |at: u16| -> Result<u16> {
            let delay: u32 = holds
                .iter()
                .filter(|(start, _)| *start < at)
                .map(|(_, blend)| u32::from(*blend))
                .sum();
            let admission = admission_delay * u16::from(at > 0 || !opening_pose);
            (u32::from(at) + delay + u32::from(admission))
                .try_into()
                .context("enemy action with blends exceeds timeline limit")
        };
        let opening_hold = holds.first().filter(|(at, _)| *at == 0).map_or(
            admission_delay
                * u16::from(
                    technique != 0 || animations.first().is_some_and(|(_, pose)| pose[3] != 0),
                ),
            |(_, blend)| u16::from(*blend),
        );
        let action_end = elapsed(duration)?
            .checked_sub(u16::from(technique != 0))
            .context("empty physical arte duration")?;
        for (at, event) in &mut events {
            if let EnemyEvent::Contact(contact) | EnemyEvent::ContinueContact(contact) = event {
                // Serialization already retained the row's authored expiry.
                // Translate that endpoint too, including blends crossed by the window.
                let end = at
                    .checked_add(contact.duration)
                    .context("enemy contact expiry exceeds tick range")?;
                // The deadline update can contact, but recovery must not wait
                // for a contact stream that outlasts its action.
                contact.duration = elapsed(end)?
                    .min(action_end.saturating_add(1))
                    .checked_sub(if *at == 0 {
                        opening_hold
                    } else {
                        elapsed(*at)?
                    })
                    .filter(|duration| *duration > 0)
                    .context("empty translated enemy contact window")?;
            }
            if let EnemyEvent::WaitForHit { deadline }
            | EnemyEvent::WaitForEffectiveHit { deadline } = event
            {
                // An unmet wait never starts later poses or their blend holds.
                *deadline = deadline
                    .checked_add(elapsed(*at)? - *at)
                    .context("enemy hit wait deadline exceeds tick range")?;
            }
            *at = if *at == 0
                && matches!(
                    event,
                    EnemyEvent::Forward(_)
                        | EnemyEvent::Vertical(_)
                        | EnemyEvent::Acceleration(_)
                        | EnemyEvent::Gravity(_)
                        | EnemyEvent::CommonEffect(_)
                        | EnemyEvent::Tint(_)
                        | EnemyEvent::PassThrough { .. }
                        | EnemyEvent::ReactionArmor(_)
                        | EnemyEvent::CarriedPose { .. }
                        | EnemyEvent::Contact(_)
                        | EnemyEvent::ContinueContact(_)
                        | EnemyEvent::Projectile { .. }
                ) {
                opening_hold
            } else {
                elapsed(*at)?
            };
            if matches!(event, EnemyEvent::AnimationRate(_)) {
                // The new rate affects the next motion sample, keeping this tick's pose.
                *at = at
                    .checked_add(1)
                    .context("enemy animation rate exceeds timeline limit")?;
            }
        }
        events.extend(partial_events);
        events.sort_by_key(|(at, _)| *at);
        let chain_at = u16::read(row, 30)?;
        let recovery_ticks = if let Some(common) = common {
            let binding = resonance_content::battle_martial::Binding::enemy(technique);
            u16::read(common, 18 + usize::from(binding.variant) * 28)?
                .try_into()
                .context("physical arte recovery exceeds limit")?
        } else {
            row[2]
        };
        let recovery_pose = if common.is_some() {
            ActionPose {
                clip: 0,
                start: 0.,
                end: None,
                rate: 0.5,
                blend: recovery_ticks,
                repeat: true,
            }
        } else {
            recovery_pose(row)?
        };
        let attack = EnemyAttack {
            duration: action_end,
            chain_at: (row[50] as i8 > 0 && chain_at <= duration)
                .then(|| elapsed(chain_at))
                .transpose()?,
            recovery_ticks,
            recovery_pose,
            armor: row[49],
            vulnerable: [elapsed(u16::read(row, 36)?)?, elapsed(u16::read(row, 38)?)?],
            contact_recovery,
            events,
        };
        for at in policy_ticks {
            ensure!(*at < duration, "enemy policy time exceeds action duration");
            *at = elapsed(*at)?;
        }
        Ok(if technique == 0 {
            EnemyExecution::Attack(attack)
        } else {
            EnemyExecution::Physical { technique, attack }
        })
    }

    fn physical_feedback(&self, hit: &mut ActionHit) -> Result<()> {
        let Some(effect) = hit.effect else {
            return Ok(());
        };
        let start = u32::read(self.bytes, 0x1cc)? as usize;
        ensure!(start != 0, "physical contact has no enemy effect bank");
        let bank = self
            .bytes
            .get(start..)
            .context("physical contact effect bank exceeds package")?;
        if effect < u16::from(u8::read(bank, 4)?) {
            return Ok(());
        }
        // Shared contact rules can address an empty alias beyond this enemy's
        // declared catalogue. Keep its observable program without inventing art.
        let table = usize::from(u16::read(bank, 16)?);
        let offset = i16::read(bank, table + usize::from(effect) * 2)?;
        ensure!(
            offset == i16::read(bank, table)?,
            "physical contact effect {effect} is not an empty alias"
        );
        let program = usize::try_from(i32::from(u16::read(bank, 10)?) + i32::from(offset))?;
        ensure!(
            <[u8; 6]>::read(bank, program)? == [0, 0, 254, 0, 0, 0],
            "physical contact effect {effect} aliases a nonempty program"
        );
        hit.effect = Some(0);
        Ok(())
    }

    fn contact_recovery(&self, row: &[u8]) -> Result<Option<EnemyContactRecovery>> {
        if u32::read(row, 8)? & CONTACT_RECOVERY == 0 {
            return Ok(None);
        }
        let mut recovery = EnemyContactRecovery {
            pose: (row[53] != 0).then_some(ActionPose {
                clip: u16::from(row[53]),
                start: 0.,
                end: None,
                rate: 0.5,
                blend: 4,
                repeat: false,
            }),
            movement: [None; 2],
            braking: None,
            pass_through_ticks: 0,
            voice: None,
            expressions: self.expressions,
        };
        let start = u16::read(row, 54)?;
        if start == 0 {
            return Ok(Some(recovery));
        }
        let mut cursor = usize::from(start) * 2;
        loop {
            let at = i16::read(self.commands, cursor)?;
            if at == -1 {
                return Ok(Some(recovery));
            }
            ensure!(at == 0, "enemy contact recovery must be immediate");
            let operand = |offset| u16::read(self.commands, cursor + offset);
            let opcode = operand(2)?;
            cursor += match opcode {
                0 | 1 => {
                    recovery.movement[usize::from(opcode)] =
                        Some(f32::from(operand(4)? as i16) / 10.);
                    6
                }
                9 => {
                    let braking = f32::from(operand(4)? as i16) / 10.;
                    ensure!(braking >= 0., "negative enemy recovery braking");
                    recovery.braking = Some(braking);
                    6
                }
                5 => {
                    ensure!(
                        operand(4)? == 1,
                        "unsupported enemy recovery collision mode"
                    );
                    recovery.pass_through_ticks = operand(6)?
                        .try_into()
                        .context("enemy recovery collision timer exceeds 255")?;
                    ensure!(
                        recovery.pass_through_ticks > 0,
                        "untimed enemy recovery collision bypass is not prepared"
                    );
                    8
                }
                27 => {
                    ensure!(
                        operand(6)? == 2,
                        "unsupported enemy recovery voice priority"
                    );
                    recovery.voice = crate::battle_voice::sound(operand(4)?);
                    8
                }
                opcode => anyhow::bail!("enemy recovery command {opcode} is not prepared"),
            };
        }
    }

    fn pose_rows(&self, start: u16) -> Result<Vec<(u16, [u8; 12])>> {
        if start == u16::MAX {
            return Ok(Vec::new());
        }
        let mut authored = Vec::<[u8; 12]>::new();
        let mut cursor = usize::from(start) * 12;
        loop {
            let terminal = i16::read(self.animations, cursor)? == -2;
            if terminal && !authored.is_empty() {
                break;
            }
            // The initial record supplies a pose even when it ends the sequence.
            authored.push(
                self.animations
                    .get(cursor..cursor + 12)
                    .context("truncated enemy pose")?
                    .try_into()?,
            );
            if terminal {
                break;
            }
            cursor += 12;
        }
        let partial = authored.iter().any(is_partial_pose);
        let mut rows: Vec<(u16, [u8; 12])> = Vec::new();
        let mut animation_delay = 0_u16;
        for mut row in authored {
            const FIRST_ACTION_MOTION: u8 = 30;
            if self.body_motion_disabled && row[2] < FIRST_ACTION_MOTION {
                // Suppressed body poses retain the live controller without a blend hold.
                row[3] = 0;
            }
            let trigger = i16::read(&row, 0)?;
            // The first record establishes the action's pose immediately;
            // only subsequent records wait for their animation-clock trigger.
            let at = if rows.is_empty() {
                0
            } else if trigger == -3 {
                let &(previous_at, previous) = rows
                    .last()
                    .context("enemy pose continuation has no preceding pose")?;
                ensure!(
                    previous[2] < 254 && matches!(previous[7], 0 | 255),
                    "enemy pose continuation requires a body motion"
                );
                let pose = action_pose(&previous)?;
                ensure!(
                    !pose.repeat && pose.rate > 0.,
                    "enemy pose continuation requires a finite forward motion"
                );
                let end = if let Some(end) = pose.end {
                    end
                } else {
                    let bytes = self
                        .members
                        .model(0x20 + usize::from(pose.clip) * 4)?
                        .context("missing continued enemy motion")?;
                    crate::animation::read_member(bytes)?.duration_frames()
                };
                let duration = (end - pose.start) / pose.rate;
                ensure!(
                    duration.is_finite() && duration >= 0. && duration < f32::from(u16::MAX),
                    "invalid continued enemy motion length"
                );
                // Show the preceding clip's final sample before the successor.
                // A later blended pose consumes its selection update too.
                let selection_hold = u16::from(!partial && rows.len() > 1 && pose.blend > 0);
                previous_at
                    .checked_add(duration.floor() as u16)
                    .and_then(|at| at.checked_add(1 + selection_hold))
                    .context("enemy pose continuation exceeds timeline limit")?
            } else {
                u16::try_from(trigger)
                    .context("enemy pose control is not prepared")?
                    .checked_add(animation_delay)
                    .context("enemy pose timeline exceeds tick range")?
            };
            ensure!(
                rows.last()
                    .is_none_or(|(previous_at, _)| at >= *previous_at),
                "enemy pose sequence moves backwards"
            );
            rows.push((at, row));
            // Later blended selections pause the animation clock on the selection
            // update; commands and contacts still advance. Partial poses account
            // for their independent clocks when their groups are prepared.
            if !partial && rows.len() > 1 && row[2] < 254 && row[3] > 0 {
                animation_delay = animation_delay
                    .checked_add(1)
                    .context("enemy pose timeline exceeds tick range")?;
            }
        }
        Ok(rows)
    }

    fn commands(
        &self,
        start: u16,
        mut duration: u16,
        events: &mut Vec<(u16, EnemyEvent)>,
    ) -> Result<u16> {
        if start == u16::MAX {
            return Ok(duration);
        }
        let mut cursor = usize::from(start) * 2;
        let mut reached = 0;
        loop {
            let time = i16::read(self.commands, cursor)?;
            if time == -1 {
                return Ok(duration);
            }
            // Records execute in stream order, even when a later timestamp is earlier.
            let time = u16::try_from(time)
                .context("enemy command loop is not prepared")?
                .max(reached);
            reached = time;
            let opcode = i16::read(self.commands, cursor + 2)?;
            let operand = |offset| u16::read(self.commands, cursor + offset);
            if opcode == 17 {
                let extension = operand(4)?;
                ensure!(
                    extension <= i16::MAX as u16 && extension == operand(6)?,
                    "enemy deadline extension pair is not prepared"
                );
                ensure!(
                    time <= duration,
                    "enemy deadline extension starts after its deadline"
                );
                // Event ages stay unchanged. Earlier hit waits retain their shorter
                // deadline, so a missed action never receives a later extension.
                duration = duration
                    .checked_add(extension)
                    .context("enemy deadline extension exceeds tick range")?;
                cursor += 8;
                continue;
            }
            let speed = || Ok::<_, anyhow::Error>(f32::from(operand(4)? as i16) / 10.);
            let (event, width) = match opcode {
                -5 | -3 => {
                    ensure!(time <= duration, "enemy hit wait starts after its deadline");
                    (
                        if opcode == -5 {
                            EnemyEvent::WaitForEffectiveHit { deadline: duration }
                        } else {
                            EnemyEvent::WaitForHit { deadline: duration }
                        },
                        4,
                    )
                }
                0 => (EnemyEvent::Forward(speed()?), 6),
                1 => (EnemyEvent::Vertical(speed()?), 6),
                2 => (EnemyEvent::Acceleration(speed()?), 6),
                3 => (EnemyEvent::Gravity(speed()?), 6),
                5 => {
                    let enabled = operand(4)?;
                    ensure!(enabled <= 1, "invalid enemy body collision mode");
                    (
                        EnemyEvent::PassThrough {
                            enabled: enabled != 0,
                            duration: operand(6)? as u8,
                        },
                        8,
                    )
                }
                7 => {
                    let [slot, visible] = operand(4)?.to_be_bytes();
                    ensure!(visible <= 1, "invalid enemy weapon visibility");
                    ensure!(slot < 8, "enemy weapon visibility exceeds attachment slots");
                    let profile = usize::from(u16::read(self.bytes, 4)?);
                    if slot >= u8::read(self.bytes, profile + 0x1e4)?
                        && self.members.model(0x160 + usize::from(slot) * 4)?.is_none()
                    {
                        // Shared action streams can toggle an intentionally empty slot.
                        cursor += 6;
                        continue;
                    }
                    (
                        EnemyEvent::WeaponVisibility {
                            slot: WeaponSlot(slot),
                            visible: visible != 0,
                        },
                        6,
                    )
                }
                9 => {
                    let braking = speed()?;
                    ensure!(braking >= 0., "negative enemy braking");
                    (EnemyEvent::Braking(braking), 6)
                }
                12 => {
                    let enabled = operand(4)?;
                    let duration = operand(6)?;
                    ensure!(
                        matches!((enabled, duration), (0, 0))
                            || (enabled == 1 && (1..=i16::MAX as u16).contains(&duration)),
                        "enemy reaction armor mode is not prepared"
                    );
                    (EnemyEvent::ReactionArmor(duration), 8)
                }
                13 => (
                    EnemyEvent::Trail {
                        slot: operand(4)?.try_into().context("invalid enemy trail slot")?,
                        duration: operand(6)?,
                    },
                    8,
                ),
                14 => {
                    // The first operand is reserved; the condition always affects its owner.
                    let amount = operand(6)? as i16;
                    let mask = u64::from_be_bytes(<[u8; 8]>::read(self.commands, cursor + 8)?);
                    let event = match mask {
                        4 => EnemyEvent::Stun,
                        mask @ (0x1000 | 0x2000) if amount >= 0 => EnemyEvent::StatUp {
                            condition: if mask == 0x1000 {
                                Condition::AttackUp
                            } else {
                                Condition::DefenseUp
                            },
                            amount,
                        },
                        _ => anyhow::bail!("enemy condition command is not prepared"),
                    };
                    (event, 16)
                }
                18 => {
                    let hitstun = operand(6)?;
                    ensure!(
                        operand(4)? == 0 && hitstun <= i16::MAX as u16,
                        "enemy captured release mode is not prepared"
                    );
                    (EnemyEvent::ReleaseCaptured { hitstun }, 8)
                }
                19 => {
                    let visible = operand(4)?;
                    ensure!(
                        visible <= 1 && operand(6)? == 0,
                        "enemy captured visibility mode is not prepared"
                    );
                    (EnemyEvent::CapturedVisibility(visible != 0), 8)
                }
                20 => (EnemyEvent::CapturedForward(speed()?), 6),
                21 => (EnemyEvent::CapturedVertical(speed()?), 6),
                23 => {
                    let enabled = operand(4)?;
                    let duration = operand(6)?;
                    ensure!(
                        enabled <= 1 && duration <= i16::MAX as u16,
                        "invalid enemy secondary wind"
                    );
                    (
                        EnemyEvent::SecondaryWind {
                            enabled: enabled != 0,
                            duration,
                        },
                        8,
                    )
                }
                25 => {
                    let rate = f32::read(self.commands, cursor + 8)?;
                    let repeat = u32::read(self.commands, cursor + 12)?;
                    ensure!(rate > 0. && repeat <= 1, "invalid enemy carried animation");
                    (
                        EnemyEvent::CarriedPose {
                            slot: operand(4)?
                                .try_into()
                                .context("invalid enemy carried animation slot")?,
                            clip: operand(6)?,
                            rate,
                            repeat: repeat != 0,
                        },
                        16,
                    )
                }
                26 => {
                    ensure!(
                        operand(6)? == 1,
                        "enemy common effect attachment is not prepared"
                    );
                    (EnemyEvent::CommonEffect(operand(4)?), 8)
                }
                27 => {
                    let priority = operand(6)?;
                    ensure!(
                        matches!(priority, 2 | 3),
                        "enemy voice priority is not prepared"
                    );
                    (
                        EnemyEvent::Voice {
                            sound: crate::battle_voice::sound(operand(4)?),
                            announcement: priority == 3,
                        },
                        8,
                    )
                }
                28 => {
                    ensure!(
                        operand(6)? == 0,
                        "enemy positional sound mode is not prepared"
                    );
                    (EnemyEvent::Sound(operand(4)?), 8)
                }
                29 => {
                    ensure!(
                        matches!(operand(10)?, 0 | 64 | 255),
                        "enemy tint mode is not prepared"
                    );
                    let channel = |offset| {
                        u8::try_from(operand(offset)?).context("enemy tint channel exceeds 255")
                    };
                    let color = [channel(4)?, channel(6)?, channel(8)?];
                    (EnemyEvent::Tint(color), 12)
                }
                31 => (EnemyEvent::Defeat { rewards: true }, 4),
                38 | 39 => {
                    ensure!(
                        operand(6)? == 0,
                        "enemy spell release modifier is not prepared"
                    );
                    (
                        EnemyEvent::ReleaseSpell {
                            technique: operand(4)?,
                            slot: if opcode == 38 {
                                SpellSlot::Primary
                            } else {
                                SpellSlot::Secondary
                            },
                        },
                        8,
                    )
                }
                41 => (EnemyEvent::TurnMotion(speed()?), 6),
                43 | 44 => {
                    ensure!(operand(6)? == 0, "enemy item modifier is not prepared");
                    (
                        EnemyEvent::UseItem {
                            item: operand(4)?,
                            party: opcode == 44,
                        },
                        8,
                    )
                }
                45 => {
                    ensure!(operand(10)? == 0, "enemy position mode is not prepared");
                    (
                        EnemyEvent::SetPosition([
                            operand(4)? as i16,
                            operand(6)? as i16,
                            operand(8)? as i16,
                        ]),
                        12,
                    )
                }
                46 | 47 => {
                    ensure!(
                        opcode == 47 || (operand(4)? == 0 && operand(10)? == 0),
                        "enemy relative position mode is not prepared"
                    );
                    let height = operand(6)? as i16;
                    let retreat = operand(8)? as i16;
                    (
                        if opcode == 46 {
                            EnemyEvent::PositionFromTarget { height, retreat }
                        } else {
                            EnemyEvent::OffsetPosition { height, retreat }
                        },
                        12,
                    )
                }
                48 => {
                    ensure!(operand(4)? == 0, "enemy facing mode is not prepared");
                    (EnemyEvent::FaceTarget, 6)
                }
                other => anyhow::bail!("enemy action command {other} is not prepared"),
            };
            events.push((time, event));
            cursor += width;
        }
    }
}

fn recovery_pose(row: &[u8]) -> Result<ActionPose> {
    let idle = row[3] == 0;
    Ok(ActionPose {
        clip: u16::from(row[3]),
        start: 0.,
        end: None,
        rate: if idle { 0.5 } else { f32::read(row, 4)? },
        blend: if idle { row[2] } else { 0 },
        repeat: idle,
    })
}

fn is_partial_pose(row: &[u8; 12]) -> bool {
    row[2] < 254 && row[7] != u8::MAX && row[6] & !0x40 != 8
}

struct PartialAnimations {
    events: Vec<(u16, EnemyEvent)>,
    holds: Vec<(u16, u8)>,
}

/// Partial blends run independently; only body blends hold the gameplay clock.
fn partial_animations(rows: &[(u16, [u8; 12])]) -> Result<PartialAnimations> {
    if let [(at, row)] = rows
        && row[2] < 254
        && matches!(row[6] & !0x40, 0 | 2)
        && row[7] != u8::MAX
    {
        return Ok(PartialAnimations {
            events: vec![(
                *at,
                EnemyEvent::PartialPose {
                    first_bone: u16::from(row[7]),
                    pose: action_pose(row)?,
                },
            )],
            holds: Vec::new(),
        });
    }
    ensure!(
        rows.len().is_multiple_of(3),
        "incomplete partial-pose group"
    );
    let mut events = Vec::new();
    let mut holds = Vec::new();
    let mut animation_delay = 0_u16;
    let mut gameplay_delay = 0_u16;
    for group in rows.chunks_exact(3) {
        let [(time, first), (second_time, covered), (body_time, body)] = group else {
            unreachable!()
        };
        ensure!(
            time == second_time
                && time == body_time
                && first[6..8] == [0, 5]
                && covered[6..8] == [1, 7]
                && body[6..8] == [6, 255]
                && first[2] == covered[2]
                && first[2] < 254
                && body[2] < 254
                && first[3] == 1
                && covered[3] == 1
                && body[3] > 0,
            "secondary enemy action animation layout is not prepared"
        );
        let start = time
            .checked_add(animation_delay)
            .context("partial-pose timeline exceeds tick range")?;
        let body_at = start
            .checked_add(2)
            .context("partial-pose timeline exceeds tick range")?;
        let mut part = action_pose(first)?;
        // A one-tick selection presents the start pose immediately.
        part.blend = 0;
        events.push((
            start,
            EnemyEvent::PartialPose {
                first_bone: 5,
                pose: part,
            },
        ));
        // The second partial row is visually covered by the first. Consume its
        // timing without inventing a bone mask for an unobservable layer.
        let mut body_pose = action_pose(body)?;
        if body_pose.blend == 1 {
            body_pose.blend = 0;
        }
        events.push((body_at, EnemyEvent::PoseKeepingParts(body_pose)));
        let hold = body[3] - 1;
        holds.push((body_at - gameplay_delay, hold));
        gameplay_delay = gameplay_delay
            .checked_add(u16::from(hold))
            .context("partial-pose timeline exceeds tick range")?;
        animation_delay = animation_delay
            .checked_add(3 + u16::from(hold))
            .context("partial-pose timeline exceeds tick range")?;
    }
    Ok(PartialAnimations { events, holds })
}

/// Positive contact windows include their authored final tick.
pub(crate) fn contact(rules: &[u8], row: &[u8]) -> Result<MeleeContact> {
    let duration = u16::try_from(row[2] as i8).context("negative melee contact duration")?;
    ensure!(duration > 0, "empty melee contact duration");
    let count = usize::from(row[3]);
    ensure!(
        (1..=4).contains(&count),
        "invalid melee contact count {count}"
    );
    ensure!(
        u16::read(row, 22)? == 0,
        "melee contact modifier is not prepared"
    );
    let attachments = if row[4] == u8::MAX {
        ensure!(count == 1, "invalid body center contact count {count}");
        ContactAttachments::BodyCenter {}
    } else if row[4] == 254 {
        ensure!(count == 1, "invalid body bone contact count {count}");
        ContactAttachments::BodyBone(u16::from(row[5]))
    } else {
        ContactAttachments::Groups(row[4..4 + count].to_vec())
    };
    ensure!(row[19] <= 1, "contact class {} is not prepared", row[19]);
    let shape = match row[16] {
        0 => HitShape::Box,
        1 => HitShape::Cylinder,
        2 => HitShape::GroundCircle,
        3 => HitShape::Ring {
            width: f32::read(row, 28)?,
        },
        4 => HitShape::Sphere,
        other => anyhow::bail!("unknown contact shape {other}"),
    };
    Ok(MeleeContact {
        duration: duration + 1,
        attachments,
        shape,
        radius: f32::read(row, 8)?,
        height: f32::read(row, 12)?,
        hit: ActionHit {
            bypass_guard: row[19] == 0,
            ..hit(rules, row)?
        },
    })
}

pub(crate) fn damage_kind(value: u8) -> Result<DamageKind> {
    Ok(match value {
        0 => DamageKind::Slash,
        1 => DamageKind::Thrust,
        2 => DamageKind::Magic,
        kind => anyhow::bail!("unknown damage kind {kind}"),
    })
}

pub(crate) fn hit(rules: &[u8], contact: &[u8]) -> Result<ActionHit> {
    hit_rule(rules, contact[18], damage_kind(contact[17])?, contact[20])
}

pub(crate) fn hit_rule(rules: &[u8], index: u8, kind: DamageKind, recoil: u8) -> Result<ActionHit> {
    const PREVENT_DEFEAT: u16 = 1;
    const ALTERNATE_MOTION: u16 = 2;
    const KNOCK_DOWN: u16 = 4;
    const LAUNCH: u16 = 8;
    const CAPTURE: u16 = 0x10;
    const ARTE: u16 = 0x20;
    const HITS_DOWN: u16 = 0x40;
    const BREAK_GUARD: u16 = 0x80;
    const LIFT_GUARD: u16 = 0x100;
    const FINISHER_GRADE: u16 = 0x200;
    const HP_DRAIN: u16 = 0x400;
    const EFFECT_ON_GUARD: u16 = 0x800;
    const UNBREAKABLE_GUARD: u16 = 0x1000;
    const FORCE_REACTION: u16 = 0x2000;
    const SUPPRESS_REACTION: u16 = 0x4000;
    const SUPPRESS_GUARD_SOUND: u16 = 0x8000;
    let offset = usize::from(index) * 28;
    let row = rules
        .get(offset..offset + 28)
        .context("missing enemy hit rule")?;
    let flags = u16::read(row, 0)?;
    let conditions = crate::battle_profile::conditions([0, u32::read(row, 8)?], false)?;
    let mut conditions = conditions.iter();
    let condition = conditions.next().map(|condition| HitCondition {
        condition,
        chance: row[12],
        value: row[23] as i8,
    });
    ensure!(
        conditions.next().is_none(),
        "compound enemy contact condition is not prepared"
    );
    ensure!(
        condition.is_none_or(|hit| matches!(
            hit.condition,
            Condition::PoisonMild
                | Condition::PoisonSevere
                | Condition::Paralysis
                | Condition::Weak
                | Condition::Curse
                | Condition::Heavy
                | Condition::DefenseDown
                | Condition::AccuracyDown
                | Condition::Petrified
                | Condition::ReduceItemEffect
        )),
        "enemy contact condition is not prepared"
    );
    ensure!(
        row[22] == 0 || row[24] == 0,
        "enemy contact effect mode is not prepared"
    );
    let element = match row[2] {
        0 => HitElement::Inherited,
        10 => HitElement::Neutral,
        element @ 1..=8 => HitElement::Element(
            resonance_content::menu_data::Element::ALL[usize::from(element - 1)],
        ),
        element => anyhow::bail!("unknown enemy hit element {element}"),
    };
    let power = match row[13] {
        0 | 2 => Power::Normal,
        1 => Power::Percent(u16::read(row, 14)?),
        3 => Power::Fixed(u16::read(row, 14)?),
        4 => Power::AdjustAfterContact(i16::read(row, 14)?),
        mode => anyhow::bail!("unknown enemy power mode {mode}"),
    };
    Ok(ActionHit {
        sound: Some(u16::read(row, 16)?).filter(|sound| *sound != 0),
        kind,
        effect: (row[22] != 0).then_some(u16::from(row[22])),
        effect_on_guard: flags & EFFECT_ON_GUARD != 0,
        suppress_guard_sound: flags & SUPPRESS_GUARD_SOUND != 0,
        capture: flags & CAPTURE != 0,
        power,
        element,
        arte: flags & ARTE != 0,
        prevents_defeat: flags & PREVENT_DEFEAT != 0,
        finisher_grade: flags & FINISHER_GRADE != 0,
        hp_drain: flags & HP_DRAIN != 0,
        suppress_reaction: flags & SUPPRESS_REACTION != 0,
        force_reaction: flags & FORCE_REACTION != 0,
        alternate_motion: flags & ALTERNATE_MOTION != 0,
        knock_down: flags & KNOCK_DOWN != 0,
        launch: flags & LAUNCH != 0,
        hits_down: flags & HITS_DOWN != 0,
        breaks_guard: flags & BREAK_GUARD != 0,
        unbreakable_guard: flags & UNBREAKABLE_GUARD != 0,
        lift_guard: flags & LIFT_GUARD != 0,
        guard_pressure: row[7],
        bypass_guard: false,
        hitstun: row[3],
        cooldown: row[4],
        stun_chance: row[5],
        stagger: row[6],
        armor_damage: row[20],
        recoil,
        recoil_delay: row[21],
        condition,
    })
}

/// A terminator needs only its time field; its padding is never interpreted.
fn stream(
    bytes: &[u8],
    start: u16,
    stride: usize,
    end: i16,
    mut decode: impl FnMut(u16, &[u8]) -> Result<()>,
) -> Result<()> {
    if start == u16::MAX {
        return Ok(());
    }
    let mut cursor = usize::from(start) * stride;
    loop {
        let time = i16::read(bytes, cursor)?;
        if time == end {
            return Ok(());
        }
        let time = u16::try_from(time).context("enemy action stream control is not prepared")?;
        let row = bytes
            .get(cursor..cursor + stride)
            .context("truncated enemy action stream")?;
        decode(time, row)?;
        cursor += stride;
    }
}
