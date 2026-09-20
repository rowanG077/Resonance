//! Command boundaries follow the action interpreter; no executable bytes survive cooking.
use crate::read::{FloatOperand, Storage, u16 as half, u32 as word};
use anyhow::{Context, Result, bail, ensure};
use resonance_content::battle::action_program::*;

pub(super) const HIT_BYTES: usize = 32;
pub(super) const HIT_RULE_BYTES: usize = 28;
pub(super) const ENEMY_ACTION_BYTES: usize = 68;

/// Authored enemy selection, motion and program references. Runtime defaults and
/// admission checks belong to the consumer, not this physical row decoder.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct EnemyActionRecord {
    pub weight: i8,
    pub target_policy: u8,
    pub recovery_ticks: u8,
    pub recovery_clip: u8,
    pub recovery_rate: FloatOperand,
    pub requirements: u32,
    pub target_state: u16,
    pub duration: u16,
    pub range: [i16; 2],
    pub approach_range: i16,
    pub approach_minimum: i16,
    pub animation_index: u16,
    pub command_index: u16,
    pub hit_index: u16,
    pub combo_at: u16,
    pub followup_group: u8,
    pub effect: u8,
    pub guard_chance: u8,
    pub vulnerable: [u16; 2],
    pub movement_speed: FloatOperand,
    pub movement_rate: FloatOperand,
    pub movement_clip: u8,
    pub stagger_threshold: u8,
    pub followup_chance: u8,
    pub required_monster: u8,
    pub tp: u8,
    pub hit_recovery_clip: u8,
    pub recovery_command_index: u16,
    pub resource_decrement: u8,
    pub required_story_flag: u16,
    pub cast_voices: [u16; 2],
    pub native_technique: u16,
    pub uninterpreted_storage: [Storage; 3],
}

impl EnemyActionRecord {
    pub(super) fn table(bytes: &[u8]) -> Result<Vec<Self>> {
        ensure!(
            bytes.len().is_multiple_of(ENEMY_ACTION_BYTES),
            "misaligned enemy action rows"
        );
        bytes
            .chunks_exact(ENEMY_ACTION_BYTES)
            .map(Self::read)
            .collect()
    }

    pub(super) fn read(bytes: &[u8]) -> Result<Self> {
        let row = bytes
            .get(..ENEMY_ACTION_BYTES)
            .context("truncated enemy action row")?;
        Ok(Self {
            weight: row[0] as i8,
            target_policy: row[1],
            recovery_ticks: row[2],
            recovery_clip: row[3],
            recovery_rate: FloatOperand::read(row, 4)?,
            requirements: word(row, 8)?,
            target_state: half(row, 12)?,
            duration: half(row, 14)?,
            range: [half(row, 16)? as i16, half(row, 18)? as i16],
            approach_range: half(row, 20)? as i16,
            approach_minimum: half(row, 22)? as i16,
            animation_index: half(row, 24)?,
            command_index: half(row, 26)?,
            hit_index: half(row, 28)?,
            combo_at: half(row, 30)?,
            followup_group: row[32],
            effect: row[34],
            guard_chance: row[35],
            vulnerable: [half(row, 36)?, half(row, 38)?],
            movement_speed: FloatOperand::read(row, 40)?,
            movement_rate: FloatOperand::read(row, 44)?,
            movement_clip: row[48],
            stagger_threshold: row[49],
            followup_chance: row[50],
            required_monster: row[51],
            tp: row[52],
            hit_recovery_clip: row[53],
            recovery_command_index: half(row, 54)?,
            resource_decrement: row[56],
            required_story_flag: half(row, 58)?,
            cast_voices: [half(row, 60)?, half(row, 62)?],
            native_technique: half(row, 64)?,
            uninterpreted_storage: [33..34, 57..58, 66..68].map(|range| Storage {
                offset: range.start,
                bytes: row[range].to_vec(),
            }),
        })
    }

    #[cfg(test)]
    pub(super) fn source_bytes(&self) -> [u8; ENEMY_ACTION_BYTES] {
        let mut bytes = [0; ENEMY_ACTION_BYTES];
        for (offset, value) in [
            (0, self.weight as u8),
            (1, self.target_policy),
            (2, self.recovery_ticks),
            (3, self.recovery_clip),
            (32, self.followup_group),
            (34, self.effect),
            (35, self.guard_chance),
            (48, self.movement_clip),
            (49, self.stagger_threshold),
            (50, self.followup_chance),
            (51, self.required_monster),
            (52, self.tp),
            (53, self.hit_recovery_clip),
            (56, self.resource_decrement),
        ] {
            bytes[offset] = value;
        }
        for (offset, value) in [
            (12, self.target_state),
            (14, self.duration),
            (16, self.range[0] as u16),
            (18, self.range[1] as u16),
            (20, self.approach_range as u16),
            (22, self.approach_minimum as u16),
            (24, self.animation_index),
            (26, self.command_index),
            (28, self.hit_index),
            (30, self.combo_at),
            (36, self.vulnerable[0]),
            (38, self.vulnerable[1]),
            (54, self.recovery_command_index),
            (58, self.required_story_flag),
            (60, self.cast_voices[0]),
            (62, self.cast_voices[1]),
            (64, self.native_technique),
        ] {
            bytes[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
        }
        for (offset, value) in [
            (4, self.recovery_rate.bits()),
            (8, self.requirements),
            (40, self.movement_speed.bits()),
            (44, self.movement_rate.bits()),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        for storage in &self.uninterpreted_storage {
            bytes[storage.offset..storage.offset + storage.bytes.len()]
                .copy_from_slice(&storage.bytes);
        }
        bytes
    }
}

pub(super) const PHASE_COUNT: usize = 4;
const PHASE_BYTES: usize = 28;
const PHASE_HEADER_BYTES: usize = 16 + PHASE_COUNT * PHASE_BYTES;

#[derive(Clone, Copy)]
pub(super) enum PhaseTable {
    HitRules,
    Hits,
    Animations,
    Commands,
}

pub(super) struct PhaseRecord {
    pub duration: u16,
    pub recovery_ticks: u16,
    pub buffer_until: u16,
    pub combo_at: u16,
    startup_effect: i32,
    indices: [u32; 4],
}

impl PhaseRecord {
    pub(super) fn read(bytes: &[u8], phase: usize) -> Result<Self> {
        ensure!(phase < PHASE_COUNT, "invalid action phase {phase}");
        ensure!(
            bytes.len() >= PHASE_HEADER_BYTES,
            "truncated action phase table"
        );
        let start = 16 + phase * PHASE_BYTES;
        let row = bytes
            .get(start..start + PHASE_BYTES)
            .context("truncated action phase")?;
        Ok(Self {
            duration: half(row, 0)?,
            recovery_ticks: half(row, 2)?,
            buffer_until: half(row, 4)?,
            combo_at: half(row, 6)?,
            startup_effect: word(row, 8)? as i32,
            indices: [
                word(row, 12)?,
                word(row, 16)?,
                word(row, 20)?,
                word(row, 24)?,
            ],
        })
    }

    pub(super) fn effect(&self) -> Result<Option<u16>> {
        ensure!(
            (-1..=i32::from(u16::MAX)).contains(&self.startup_effect),
            "invalid startup effect {}",
            self.startup_effect
        );
        Ok((self.startup_effect > 0).then_some(self.startup_effect as u16))
    }

    pub(super) fn table_start(&self, bytes: &[u8], table: PhaseTable) -> Result<usize> {
        let base = word(bytes, table as usize * 4)? as usize;
        let stride = match table {
            PhaseTable::HitRules => HIT_RULE_BYTES,
            PhaseTable::Hits => HIT_BYTES,
            PhaseTable::Animations => 12,
            PhaseTable::Commands => 2,
        };
        (self.indices[table as usize] as usize)
            .checked_mul(stride)
            .and_then(|offset| base.checked_add(offset))
            .context("action table offset overflow")
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct HitRuleRecord {
    flags: u16,
    element: u8,
    hitstun: u8,
    contact_cooldown: u8,
    stun_chance: u8,
    stagger: u8,
    guard_pressure: u8,
    conditions: u32,
    condition_chance: u8,
    power_mode: u8,
    power: u16,
    sound: u16,
    stagger_resistance: u8,
    knockback_delay: u8,
    impact_effect: u8,
    condition_parameter: i8,
    impact_bank: u8,
    uninterpreted_storage: [Storage; 2],
}

impl HitRuleRecord {
    pub(super) fn read(bytes: &[u8]) -> Result<Self> {
        let row = bytes.get(..HIT_RULE_BYTES).context("truncated hit rule")?;
        Ok(Self {
            flags: half(row, 0)?,
            element: row[2],
            hitstun: row[3],
            contact_cooldown: row[4],
            stun_chance: row[5],
            stagger: row[6],
            guard_pressure: row[7],
            conditions: word(row, 8)?,
            condition_chance: row[12],
            power_mode: row[13],
            power: half(row, 14)?,
            sound: half(row, 16)?,
            stagger_resistance: row[20],
            knockback_delay: row[21],
            impact_effect: row[22],
            condition_parameter: row[23] as i8,
            impact_bank: row[24],
            uninterpreted_storage: [18..20, 25..28].map(|range| Storage {
                offset: range.start,
                bytes: row[range].to_vec(),
            }),
        })
    }

    pub(super) fn lower(&self) -> Result<resonance_content::battle::actions::HitRule> {
        use resonance_content::battle::actions::{HitElement, HitRule};
        Ok(HitRule {
            flags: self.flags,
            element: match self.element {
                0 => HitElement::Inherit,
                10 => HitElement::Neutral,
                id @ 1..=8 => HitElement::Element(
                    resonance_content::menu_data::Element::ALL[usize::from(id - 1)],
                ),
                id => bail!("unsupported hit element {id}"),
            },
            hitstun: self.hitstun,
            contact_cooldown: self.contact_cooldown,
            stun_chance: self.stun_chance,
            stagger: self.stagger,
            guard_pressure: self.guard_pressure,
            conditions: self.conditions,
            condition_chance: self.condition_chance,
            power_mode: self.power_mode,
            power: self.power,
            sound: self.sound,
            stagger_resistance: self.stagger_resistance,
            knockback_delay: self.knockback_delay,
            impact_effect: self.impact_effect,
            condition_parameter: self.condition_parameter,
            impact_bank: self.impact_bank,
        })
    }

    #[cfg(test)]
    pub(super) fn source_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![0; HIT_RULE_BYTES];
        bytes[..2].copy_from_slice(&self.flags.to_be_bytes());
        bytes[2..8].copy_from_slice(&[
            self.element,
            self.hitstun,
            self.contact_cooldown,
            self.stun_chance,
            self.stagger,
            self.guard_pressure,
        ]);
        bytes[8..12].copy_from_slice(&self.conditions.to_be_bytes());
        bytes[12] = self.condition_chance;
        bytes[13] = self.power_mode;
        bytes[14..16].copy_from_slice(&self.power.to_be_bytes());
        bytes[16..18].copy_from_slice(&self.sound.to_be_bytes());
        bytes[20..25].copy_from_slice(&[
            self.stagger_resistance,
            self.knockback_delay,
            self.impact_effect,
            self.condition_parameter as u8,
            self.impact_bank,
        ]);
        for storage in &self.uninterpreted_storage {
            bytes[storage.offset..storage.offset + storage.bytes.len()]
                .copy_from_slice(&storage.bytes);
        }
        bytes
    }
}

pub(super) fn hit_rule_pool(bytes: &[u8]) -> Result<serde_json::Value> {
    ensure!(
        bytes.len().is_multiple_of(HIT_RULE_BYTES),
        "misaligned hit rules"
    );
    let records = bytes
        .chunks_exact(HIT_RULE_BYTES)
        .map(HitRuleRecord::read)
        .collect::<Result<Vec<_>>>()?;
    Ok(serde_json::json!({"source_size":bytes.len(), "records":records}))
}

/// Authored operands remain intact even when the emission does not use them.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum HitRecord {
    End { storage: Storage },
    Window(HitOperands),
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct HitOperands {
    start: i16,
    emission: i8,
    attachment_count: i8,
    emission_operands: [u8; 4],
    radius_bits: u32,
    height_bits: u32,
    shape: u8,
    damage_kind: u8,
    rule: u8,
    hit_class: u8,
    reaction: u8,
    projectile_modifier: u16,
    inner_radius_bits: u32,
    uninterpreted_storage: [Storage; 2],
}

impl HitRecord {
    pub(super) fn read(bytes: &[u8]) -> Result<Self> {
        let start = half(bytes, 0)? as i16;
        if start == -1 {
            return Ok(Self::End {
                storage: Storage {
                    offset: 2,
                    bytes: bytes[2..bytes.len().min(HIT_BYTES)].to_vec(),
                },
            });
        }
        let row = bytes.get(..HIT_BYTES).context("truncated hit record")?;
        Ok(Self::Window(HitOperands {
            start,
            emission: row[2] as i8,
            attachment_count: row[3] as i8,
            emission_operands: row[4..8].try_into().unwrap(),
            radius_bits: word(row, 8)?,
            height_bits: word(row, 12)?,
            shape: row[16],
            damage_kind: row[17],
            rule: row[18],
            hit_class: row[19],
            reaction: row[20],
            projectile_modifier: half(row, 22)?,
            inner_radius_bits: word(row, 28)?,
            uninterpreted_storage: [21..22, 24..28].map(|range| Storage {
                offset: range.start,
                bytes: row[range].to_vec(),
            }),
        }))
    }

    pub(super) fn size(&self) -> usize {
        match self {
            Self::End { storage } => 2 + storage.bytes.len(),
            Self::Window(_) => HIT_BYTES,
        }
    }

    /// Only effect emissions read this key; preserve physical admission independently of shape.
    pub(super) fn projectile_modifier(&self) -> Option<u16> {
        match self {
            Self::Window(row) if row.emission == -2 => Some(row.projectile_modifier),
            _ => None,
        }
    }

    pub(super) fn lower(
        &self,
        rules: &[u8],
    ) -> Result<Option<resonance_content::battle::actions::HitWindow>> {
        self.lower_with(|index| {
            let start = usize::from(index) * HIT_RULE_BYTES;
            HitRuleRecord::read(
                rules
                    .get(start..start + HIT_RULE_BYTES)
                    .context("invalid hit rule index")?,
            )?
            .lower()
        })
    }

    pub(super) fn lower_with(
        &self,
        rule: impl FnOnce(u8) -> Result<resonance_content::battle::actions::HitRule>,
    ) -> Result<Option<resonance_content::battle::actions::HitWindow>> {
        use resonance_content::battle::actions::*;
        let Self::Window(row) = self else {
            return Ok(None);
        };
        ensure!(row.start >= 0, "invalid hit start time");
        let operands = row.emission_operands;
        let emission = match row.emission {
            duration if duration >= 0 => HitEmission::Contact {
                duration: duration as u8,
                attachment: match operands[0] as i8 {
                    -1 => HitAttachment::Center,
                    -2 => HitAttachment::BodyBone(operands[1]),
                    _ => {
                        ensure!(
                            (0..=4).contains(&row.attachment_count),
                            "invalid hit attachment count"
                        );
                        HitAttachment::Groups(operands[..row.attachment_count as usize].to_vec())
                    }
                },
            },
            -2 => HitEmission::Effect {
                effect: operands[0],
                bone: (operands[1] != 0).then_some(operands[1]),
            },
            kind if kind <= -3 => HitEmission::Projectile {
                motion: (-3i16 - i16::from(kind)) as u8,
                slot: operands[0],
            },
            _ => bail!("unsupported hit emission"),
        };
        let scalar = |bits, offset| -> Result<f32> {
            let value = f32::from_bits(bits);
            ensure!(value.is_finite(), "non-finite float at {offset:#x}");
            Ok(value)
        };
        Ok(Some(HitWindow {
            start: row.start as u16,
            emission,
            shape: HitShape {
                radius: scalar(row.radius_bits, 8)?,
                height: scalar(row.height_bits, 12)?,
                kind: match row.shape {
                    0 => HitShapeKind::Box,
                    1 => HitShapeKind::Cylinder,
                    2 => HitShapeKind::GroundCircle,
                    3 => HitShapeKind::Ring,
                    4 => HitShapeKind::Sphere,
                    other => bail!("unknown hit shape {other}"),
                },
                inner_radius: scalar(row.inner_radius_bits, 28)?,
                damage_kind: row.damage_kind,
                hit_class: row.hit_class,
                reaction: row.reaction,
            },
            rule: rule(row.rule)?,
            projectile_modifier: (row.projectile_modifier != 0).then_some(row.projectile_modifier),
        }))
    }

    #[cfg(test)]
    pub(super) fn source_bytes(&self) -> Vec<u8> {
        let row = match self {
            Self::Window(row) => row,
            Self::End { storage } => {
                return [(-1i16).to_be_bytes().as_slice(), &storage.bytes].concat();
            }
        };
        let mut bytes = vec![0; HIT_BYTES];
        bytes[..2].copy_from_slice(&row.start.to_be_bytes());
        bytes[2] = row.emission as u8;
        bytes[3] = row.attachment_count as u8;
        bytes[4..8].copy_from_slice(&row.emission_operands);
        for (at, bits) in [
            (8, row.radius_bits),
            (12, row.height_bits),
            (28, row.inner_radius_bits),
        ] {
            bytes[at..at + 4].copy_from_slice(&bits.to_be_bytes());
        }
        bytes[16..21].copy_from_slice(&[
            row.shape,
            row.damage_kind,
            row.rule,
            row.hit_class,
            row.reaction,
        ]);
        bytes[22..24].copy_from_slice(&row.projectile_modifier.to_be_bytes());
        for storage in &row.uninterpreted_storage {
            bytes[storage.offset..storage.offset + storage.bytes.len()]
                .copy_from_slice(&storage.bytes);
        }
        bytes
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct Command {
    pub tick: u16,
    pub kind: CommandKind,
    pub operands: Vec<u16>,
}

impl Command {
    pub(super) fn validate(&self) -> Result<()> {
        let (size, _) = command_layout(self.kind as i16)?;
        ensure!(
            self.tick <= i16::MAX as u16 && self.operands.len() == (size - 4) / 2,
            "invalid cooked action command {:?}",
            self.kind
        );
        Ok(())
    }
}

pub(super) enum Record {
    Command(Command),
    End { loops: bool },
}

/// Decode a complete native record once; projections retain their own admission rules.
pub(super) fn record(bytes: &[u8], cursor: &mut usize) -> Result<Record> {
    let tick = half(bytes, *cursor)? as i16;
    if matches!(tick, -1 | -2) {
        *cursor += 2;
        return Ok(Record::End { loops: tick == -2 });
    }
    ensure!(tick >= 0, "unsupported action time {tick} at {cursor:#x}");
    let (row, kind) = command(bytes, *cursor)?;
    let operands = row[4..]
        .chunks_exact(2)
        .map(|pair| u16::from_be_bytes(pair.try_into().unwrap()))
        .collect();
    *cursor += row.len();
    Ok(Record::Command(Command {
        tick: tick as u16,
        kind,
        operands,
    }))
}

pub(super) fn decode_commands(bytes: &[u8]) -> Result<CommandProgram> {
    use CommandDependency as D;
    use CommandKind as K;
    let mut cursor = 0;
    let mut commands = Vec::new();
    loop {
        let command = match record(bytes, &mut cursor)? {
            Record::Command(command) => command,
            Record::End { loops } => return Ok(CommandProgram { commands, loops }),
        };
        let kind = command.kind;
        let arg = |index: usize| command.operands[index];
        let dependencies = match kind {
            K::Sound => vec![D::Sound { id: arg(0) }],
            K::Voice => vec![D::Voice {
                id: arg(0),
                priority: arg(1) as u8,
            }],
            K::RandomVoice => vec![
                D::Voice {
                    id: arg(0),
                    priority: arg(2) as u8,
                },
                D::Voice {
                    id: arg(1),
                    priority: arg(2) as u8,
                },
            ],
            K::CommonEffect => vec![D::CommonEffect { id: arg(0) as u8 }],
            K::CommonImpactFlash => vec![D::CommonEffect { id: 13 }],
            K::AttachmentAnimation => vec![D::AttachmentAnimation {
                attachment: arg(0) as i16,
                animation: arg(1) as i16,
            }],
            K::CopyAttachmentAnimation => vec![D::CopyAttachmentAnimation {
                destination: arg(0) as i16,
                source: arg(1) as i16,
            }],
            K::CastPrimaryTechnique | K::CastSecondaryTechnique => vec![D::NativeTechnique {
                id: arg(0),
                slot: if kind == K::CastPrimaryTechnique {
                    CastSlot::Primary
                } else {
                    CastSlot::Secondary
                },
            }],
            K::ApplyPreviousTargetEvent | K::ApplyTargetEvent => vec![D::TargetEvent {
                id: arg(0) as i16,
                target: if kind == K::ApplyPreviousTargetEvent {
                    EventTarget::Previous
                } else {
                    EventTarget::Selected
                },
            }],
            _ => Vec::new(),
        };
        commands.push(CommandUse {
            tick: command.tick,
            kind,
            dependencies,
        });
    }
}

fn command(bytes: &[u8], cursor: usize) -> Result<(&[u8], CommandKind)> {
    let opcode = half(bytes, cursor + 2)? as i16;
    let (size, kind) =
        command_layout(opcode).map_err(|error| anyhow::anyhow!("{error} at {cursor:#x}"))?;
    Ok((
        bytes
            .get(cursor..cursor + size)
            .context("truncated action command")?,
        kind,
    ))
}

fn command_layout(opcode: i16) -> Result<(usize, CommandKind)> {
    use CommandKind as K;
    let (size, kind) = match opcode {
        -5 => (4, K::WaitActionResult),
        -3 => (4, K::WaitHit),
        0 => (6, K::ForwardSpeed),
        1 => (6, K::VerticalSpeed),
        2 => (6, K::ForwardAcceleration),
        3 => (6, K::Gravity),
        4 => (4, K::Reverse),
        5 => (8, K::SetActorCollisionMode),
        6 => (8, K::TextureLayers),
        7 => (6, K::AttachmentVisibility),
        8 => (6, K::AdvancePosition),
        9 => (6, K::ActorMotionScale),
        10 => (10, K::CameraMotion),
        12 => (8, K::SetActorProtection),
        13 => (8, K::AttachmentTrail),
        14 => (16, K::ApplyConditionAndTransition),
        15 => (8, K::RecoverHp),
        16 => (8, K::RecoverTp),
        17 => (8, K::ExtendAction),
        18 => (8, K::ReleaseCapturedTarget),
        19 => (8, K::CapturedTargetVisibility),
        20 => (6, K::CapturedTargetForwardSpeed),
        21 => (6, K::CapturedTargetVerticalSpeed),
        22 => (4, K::CommonImpactFlash),
        23 => (8, K::SetActorAttackMode),
        24 => (8, K::DamagePower),
        25 => (16, K::AttachmentAnimation),
        26 => (8, K::CommonEffect),
        27 => (8, K::Voice),
        28 => (8, K::Sound),
        29 => (12, K::SetActorAmbientColor),
        30 => (4, K::Withdraw),
        31 => (4, K::WithdrawAndRemove),
        32 => (12, K::ModelTransform),
        33 => (20, K::Reserved),
        34 => (8, K::SetActorStateTimer),
        35 => (8, K::TextureVariant),
        36 => (8, K::CopyAttachmentAnimation),
        37 => (10, K::PlayerCameraMotion),
        38 => (8, K::CastPrimaryTechnique),
        39 => (8, K::CastSecondaryTechnique),
        40 => (6, K::TurnHeading),
        41 => (6, K::TurnMotion),
        42 => (12, K::RandomVoice),
        43 => (8, K::ApplyPreviousTargetEvent),
        44 => (8, K::ApplyTargetEvent),
        45 => (12, K::SetPosition),
        46 => (12, K::PositionFromTarget),
        47 => (12, K::OffsetPosition),
        48 => (6, K::FaceTargetDirection),
        _ => bail!("unknown action opcode {opcode}"),
    };
    Ok((size, kind))
}

/// Convert retained action records without selecting an actor or native controller.
pub(crate) fn physical_bundle(bytes: &[u8]) -> Result<serde_json::Value> {
    if word(bytes, 12)? == 28 {
        return compact_bundle(bytes);
    }
    use serde_json::json;
    let bases = (0..4)
        .map(|index| word(bytes, index * 4).map(|base| base as usize))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        bytes.len() >= PHASE_HEADER_BYTES
            && bases[0] == PHASE_HEADER_BYTES
            && bases.windows(2).all(|pair| pair[0] <= pair[1])
            && bases[3] <= bytes.len(),
        "invalid four-phase action bundle"
    );
    let mut animation_roots = Vec::new();
    let hit_rules = hit_rule_pool(&bytes[bases[0]..bases[1]])?;
    let hit_pool = &bytes[bases[1]..bases[2]];
    let mut hit_records = std::collections::BTreeMap::new();
    let phases = (0..PHASE_COUNT)
        .map(|phase| -> Result<_> {
            let descriptor = PhaseRecord::read(bytes, phase)?;
            let table = |kind: PhaseTable| -> Result<(usize, &[u8])> {
                let start = descriptor.table_start(bytes, kind)?;
                let end = bases.get(kind as usize + 1).copied().unwrap_or(bytes.len());
                Ok((
                    start,
                    bytes
                        .get(start..end)
                        .context("action table index outside bundle")?,
                ))
            };
            let (rule_start, _) = table(PhaseTable::HitRules)?;
            let (hit_start, hit_bytes) = table(PhaseTable::Hits)?;
            let hit_root = (!hit_bytes.is_empty()).then_some(hit_start - bases[1]);
            if let Some(mut offset) = hit_root {
                loop {
                    let record = HitRecord::read(
                        hit_pool.get(offset..).context("unterminated hit program")?,
                    )?;
                    let terminated = matches!(&record, HitRecord::End { .. });
                    hit_records.insert(offset, record);
                    if terminated {
                        break;
                    }
                    offset += HIT_BYTES;
                }
            }
            let (start, animations) = table(PhaseTable::Animations)?;
            let animation_root = (!animations.is_empty()).then_some(start - bases[2]);
            animation_roots.extend(animation_root);
            let (_, command_storage) = table(PhaseTable::Commands)?;
            // Physical records can reserve command storage without initializing
            // a program. BTLskit's fixed records do this; its original entry and
            // update callbacks (80A78/80A7C) return without consuming the storage.
            // Preserve that distinction rather than inventing timed zero-speed
            // commands or accepting an unterminated live command stream.
            let commands = if command_storage.iter().all(|&byte| byte == 0) {
                json!({"commands": [], "loops": false, "initialized": false})
            // Rule-only bundles end all three program tables together. Remaining
            // file-alignment bytes are not initialized action commands.
            } else if bases[1] == bases[3] && descriptor.indices[PhaseTable::Commands as usize] == 0
            {
                json!({"commands": [], "loops": false})
            } else {
                physical_commands(command_storage)?
            };
            Ok(json!({
                "duration": descriptor.duration,
                "recovery_ticks": descriptor.recovery_ticks,
                "buffer_until": descriptor.buffer_until,
                "combo_at": descriptor.combo_at,
                "startup_effect": descriptor.effect()?,
                "hit_rule_root": rule_start - bases[0],
                "hit_root": hit_root,
                "animation_root": animation_root,
                "commands": commands,
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    // Callbacks can resume after a terminator. Publish the complete fixed-stride
    // pools so those continuations bind records instead of re-reading raw storage.
    for (index, row) in hit_pool.chunks_exact(HIT_BYTES).enumerate() {
        hit_records
            .entry(index * HIT_BYTES)
            .or_insert(HitRecord::read(row)?);
    }
    let tail = hit_pool.len() / HIT_BYTES * HIT_BYTES;
    if hit_pool.len() - tail >= 2 && half(hit_pool, tail)? == u16::MAX {
        hit_records
            .entry(tail)
            .or_insert(HitRecord::read(&hit_pool[tail..])?);
    }
    let animation_pool = &bytes[bases[2]..bases[3]];
    let animations = super::animation_table::decode_pool(animation_pool, animation_roots)?;
    let covered = hit_records
        .iter()
        .map(|(&offset, record)| offset..offset + record.size())
        .collect();
    let hit_records: Vec<_> = hit_records
        .into_iter()
        .map(|(offset, record)| json!({"offset":offset,"record":record}))
        .collect();
    Ok(
        json!({"phases": phases, "animations": animations, "hit_rules":hit_rules, "hit_records": {
            "source_size":hit_pool.len(), "records":hit_records,
            "unreferenced_storage":crate::read::unreferenced_storage(hit_pool, covered),
        }}),
    )
}

/// Retained single-phase records store direct table offsets after their scalars.
/// The shipped compact records contain only hit rules, not live program tables.
pub(super) fn compact_bundle(bytes: &[u8]) -> Result<serde_json::Value> {
    use serde_json::json;
    let bases = (12..28)
        .step_by(4)
        .map(|at| word(bytes, at).map(|v| v as usize))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        bases[0] == 28 && bases[1] == bases[2] && bases[2] == bases[3],
        "unsupported compact action program tables"
    );
    let rules = bytes
        .get(28..bases[1])
        .context("compact rules exceed record")?;
    let rules = hit_rule_pool(rules)?;
    let effect = word(bytes, 8)? as i32;
    ensure!(
        (-1..=i32::from(u16::MAX)).contains(&effect),
        "invalid compact startup effect"
    );
    Ok(json!({"phases": [{
        "duration": half(bytes, 0)?, "recovery_ticks": half(bytes, 2)?,
        "buffer_until": half(bytes, 4)?, "combo_at": half(bytes, 6)?,
        "startup_effect": (effect > 0).then_some(effect as u16),
        "hit_rule_root":0, "hit_root":null, "animation_root": null,
        "commands": {"commands": [], "loops": false}
    }], "animations": super::animation_table::decode(&[], [])?, "hit_rules":rules,
        "hit_records":{"source_size":0,"records":[],"unreferenced_storage":[]},
        "unused_storage": {"offset":bases[3], "bytes": &bytes[bases[3]..]}}))
}

fn physical_commands(bytes: &[u8]) -> Result<serde_json::Value> {
    let mut cursor = 0;
    let mut commands = Vec::new();
    loop {
        match record(bytes, &mut cursor)? {
            Record::Command(command) => commands.push(command),
            Record::End { loops } => {
                return Ok(serde_json::json!({"commands": commands, "loops": loops}));
            }
        }
    }
}

pub(super) fn physical_command_table(bytes: &[u8]) -> Result<serde_json::Value> {
    let (mut cursor, mut entries, mut terminated) = (0, Vec::new(), false);
    while cursor < bytes.len() {
        if terminated && bytes.len() - cursor < 32 && bytes[cursor..].iter().all(|&v| v == 0) {
            break;
        }
        let offset = cursor;
        let entry = record(bytes, &mut cursor)?;
        terminated = matches!(entry, Record::End { .. });
        entries.push(match entry {
            Record::End { loops } => serde_json::json!({"offset":offset,"end":true,"loops":loops}),
            Record::Command(command) => {
                let mut value = serde_json::to_value(command)?;
                value["offset"] = offset.into();
                value
            }
        });
    }
    ensure!(
        terminated || entries.is_empty(),
        "unterminated action command table"
    );
    Ok(serde_json::json!({"entries":entries}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_hit_storage(bundle: &serde_json::Value, source: &[u8]) -> Result<()> {
        let table = &bundle["hit_records"];
        let mut recovered = vec![0; table["source_size"].as_u64().unwrap() as usize];
        for entry in table["records"].as_array().unwrap() {
            let record: HitRecord = serde_json::from_value(entry["record"].clone())?;
            let bytes = record.source_bytes();
            let offset = entry["offset"].as_u64().unwrap() as usize;
            assert_eq!(bytes, &source[offset..offset + bytes.len()]);
            recovered[offset..offset + bytes.len()].copy_from_slice(&bytes);
        }
        for storage in
            serde_json::from_value::<Vec<Storage>>(table["unreferenced_storage"].clone())?
        {
            recovered[storage.offset..storage.offset + storage.bytes.len()]
                .copy_from_slice(&storage.bytes);
        }
        assert_eq!(recovered, source);
        Ok(())
    }

    fn check_rule_storage(table: &serde_json::Value, source: &[u8]) -> Result<()> {
        let rules: Vec<HitRuleRecord> = serde_json::from_value(table["records"].clone())?;
        assert_eq!(table["source_size"], source.len());
        assert_eq!(
            rules
                .iter()
                .flat_map(HitRuleRecord::source_bytes)
                .collect::<Vec<_>>(),
            source
        );
        Ok(())
    }

    #[test]
    fn hit_records_retain_inactive_operands_and_runtime_admission() -> Result<()> {
        let rules = [0; 28];
        let mut bytes = [0; HIT_BYTES];
        bytes[3] = 255; // Count is inactive for center, bone, effect and projectile emissions.
        bytes[4..8].copy_from_slice(&[255, 7, 128, 253]);
        bytes[8..12].copy_from_slice(&(-0.0f32).to_be_bytes());
        bytes[21..28].copy_from_slice(&[128, 0, 3, 1, 2, 3, 255]);
        for emission in [0, 127, 254, 253, 128] {
            bytes[2] = emission;
            let decoded = HitRecord::read(&bytes)?;
            let published: HitRecord = serde_json::from_value(serde_json::to_value(&decoded)?)?;
            assert_eq!(published.source_bytes(), bytes);
            assert!(published.lower(&rules)?.is_some());
        }
        bytes[2] = 255;
        assert!(HitRecord::read(&bytes)?.lower(&rules).is_err());
        bytes[2] = 0;
        bytes[4] = 0;
        assert!(HitRecord::read(&bytes)?.lower(&rules).is_err());
        bytes[3] = 4;
        assert!(HitRecord::read(&bytes)?.lower(&rules)?.is_some());
        bytes[8..12].copy_from_slice(&0x7fc01234u32.to_be_bytes());
        let decoded = HitRecord::read(&bytes)?;
        assert_eq!(decoded.source_bytes(), bytes);
        assert!(decoded.lower(&rules).is_err());
        bytes[..2].copy_from_slice(&(-2i16).to_be_bytes());
        assert!(HitRecord::read(&bytes)?.lower(&rules).is_err());
        bytes[..2].copy_from_slice(&(-1i16).to_be_bytes());
        for size in [2, 3, HIT_BYTES] {
            let decoded = HitRecord::read(&bytes[..size])?;
            assert_eq!(decoded.source_bytes(), &bytes[..size]);
            assert!(decoded.lower(&[])?.is_none());
        }
        // Runtime programs still require a full terminal row and a bounded length.
        assert!(super::super::actions::hits(&bytes[..2], &rules).is_err());
        assert!(super::super::actions::hits(&bytes, &rules)?.is_empty());

        let mut rule = std::array::from_fn::<_, HIT_RULE_BYTES, _>(|index| index as u8);
        rule[23] = 255;
        for element in [0, 1, 8, 10, 9, 255] {
            rule[2] = element;
            let physical = hit_rule_pool(&rule)?;
            check_rule_storage(&physical, &rule)?;
            let decoded = HitRuleRecord::read(&rule)?;
            match element {
                9 | 255 => assert!(decoded.lower().is_err()),
                _ => assert_eq!(decoded.lower()?.condition_parameter, -1),
            }
        }
        // Physical roots share typed pools even when their runtime projection is unsupported.
        let (rule_base, hit_base) = (128, 128 + HIT_RULE_BYTES);
        let end = hit_base + HIT_BYTES + 2;
        let mut bundle = vec![0; end];
        for (at, base) in [(0, rule_base), (4, hit_base), (8, end), (12, end)] {
            bundle[at..at + 4].copy_from_slice(&(base as u32).to_be_bytes());
        }
        bundle[rule_base..hit_base].copy_from_slice(&rule);
        bytes[..2].copy_from_slice(&(-2i16).to_be_bytes());
        bundle[hit_base..hit_base + HIT_BYTES].copy_from_slice(&bytes);
        bundle[hit_base + HIT_BYTES..end].copy_from_slice(&(-1i16).to_be_bytes());
        for (phase, effect) in [-1i32, 0, 1, 65535].into_iter().enumerate() {
            let at = 16 + phase * 28;
            for (field, value) in [phase as u16 + 1, 0x7fff, 0x8000, 0xffff]
                .into_iter()
                .enumerate()
            {
                bundle[at + field * 2..at + field * 2 + 2].copy_from_slice(&value.to_be_bytes());
            }
            bundle[at + 8..at + 12].copy_from_slice(&effect.to_be_bytes());
        }
        let decoded = physical_bundle(&bundle)?;
        check_rule_storage(&decoded["hit_rules"], &rule)?;
        check_hit_storage(&decoded, &bundle[hit_base..end])?;
        assert_eq!(
            decoded["hit_records"]["records"].as_array().unwrap().len(),
            2
        );
        for (index, phase) in decoded["phases"].as_array().unwrap().iter().enumerate() {
            assert_eq!(phase["duration"], index + 1);
            assert_eq!(phase["recovery_ticks"], 0x7fff);
            assert_eq!(phase["buffer_until"], 0x8000);
            assert_eq!(phase["combo_at"], 0xffff);
            assert_eq!(
                phase["startup_effect"],
                serde_json::json!([None, None, Some(1), Some(65535)][index])
            );
            assert_eq!(phase["hit_rule_root"], 0);
            assert_eq!(phase["hit_root"], 0);
            assert!(phase.get("hits").is_none() && phase.get("hit_rules").is_none());
        }
        assert!(PhaseRecord::read(&bundle[..127], 0).is_err());
        assert!(PhaseRecord::read(&bundle, 4).is_err());
        let mut continuation = bundle[..hit_base].to_vec();
        let mut terminal = [0; HIT_BYTES];
        terminal[..2].copy_from_slice(&u16::MAX.to_be_bytes());
        continuation.extend(terminal);
        continuation.extend(bytes);
        continuation.extend(u16::MAX.to_be_bytes());
        let end = continuation.len() as u32;
        continuation[8..16].copy_from_slice(&[end, end].map(u32::to_be_bytes).concat());
        let decoded = physical_bundle(&continuation)?;
        check_hit_storage(&decoded, &continuation[hit_base..])?;
        assert_eq!(
            decoded["hit_records"]["records"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| entry["offset"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            [0, 32, 64]
        );
        for effect in [-2i32, 65536] {
            bundle[24..28].copy_from_slice(&effect.to_be_bytes());
            assert!(PhaseRecord::read(&bundle, 0)?.effect().is_err());
            assert!(physical_bundle(&bundle).is_err());
        }
        Ok(())
    }

    #[test]
    fn compact_records_preserve_rules_and_storage_without_inventing_programs() -> Result<()> {
        let mut bytes = vec![0; 64];
        bytes[..2].copy_from_slice(&180u16.to_be_bytes());
        bytes[8..12].copy_from_slice(&(-1i32).to_be_bytes());
        for (at, offset) in [(12, 28u32), (16, 56), (20, 56), (24, 56)] {
            bytes[at..at + 4].copy_from_slice(&offset.to_be_bytes());
        }
        bytes[42..44].copy_from_slice(&140u16.to_be_bytes());
        bytes[56..].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let bundle = physical_bundle(&bytes)?;
        assert_eq!(bundle["phases"][0]["duration"], 180);
        assert_eq!(bundle["hit_rules"]["records"][0]["power"], 140);
        check_rule_storage(&bundle["hit_rules"], &bytes[28..56])?;
        assert_eq!(
            bundle["unused_storage"],
            serde_json::json!({"offset":56,"bytes":[1,2,3,4,5,6,7,8]})
        );
        assert!(physical_bundle(&bytes[..55]).is_err());
        bytes[20..24].copy_from_slice(&60u32.to_be_bytes());
        assert!(physical_bundle(&bytes).is_err());
        bytes[16..28].copy_from_slice(&[55u32; 3].map(u32::to_be_bytes).concat());
        assert!(physical_bundle(&bytes).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires both original extracted battle archives; no asset conversion"]
    fn original_compact_records_are_rule_only_and_retain_their_complete_extent() -> Result<()> {
        use std::{fs, path::Path};
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        for disc in ["disc1", "disc2"] {
            let usual = fs::read(extracted.join(disc).join("files/BTL/BTLusual.dat"))?;
            let bank = super::super::actions::member(&usual, 9)?;
            let mut compact = 0;
            let mut physical = 0;
            for range in crate::field::sections(bank)?.into_iter().flatten() {
                let record = &bank[range];
                let bundle = physical_bundle(record)?;
                if word(record, 12)? != 28 {
                    check_rule_storage(
                        &bundle["hit_rules"],
                        &record[word(record, 0)? as usize..word(record, 4)? as usize],
                    )?;
                    let hits = &record[word(record, 4)? as usize..word(record, 8)? as usize];
                    check_hit_storage(&bundle, hits)?;
                    for (index, phase) in bundle["phases"].as_array().unwrap().iter().enumerate() {
                        for (field, at) in [
                            ("duration", 0),
                            ("recovery_ticks", 2),
                            ("buffer_until", 4),
                            ("combo_at", 6),
                        ] {
                            assert_eq!(phase[field], half(record, 16 + index * 28 + at)?);
                        }
                        let startup = word(record, 24 + index * 28)? as i32;
                        assert_eq!(
                            phase["startup_effect"],
                            serde_json::json!((startup > 0).then_some(startup as u16))
                        );
                        assert_eq!(
                            phase["hit_rule_root"],
                            word(record, 28 + index * 28)? as usize * HIT_RULE_BYTES
                        );
                        let offset = word(record, 32 + index * 28)? as usize * HIT_BYTES;
                        assert_eq!(
                            phase["hit_root"],
                            serde_json::json!((offset < hits.len()).then_some(offset))
                        );
                    }
                    physical += 1;
                    continue;
                }
                compact += 1;
                let end = word(record, 16)? as usize;
                assert_eq!(word(record, 20)? as usize, end);
                assert_eq!(word(record, 24)? as usize, end);
                let phases = bundle["phases"].as_array().unwrap();
                assert_eq!(phases.len(), 1);
                for (field, at) in [
                    ("duration", 0),
                    ("recovery_ticks", 2),
                    ("buffer_until", 4),
                    ("combo_at", 6),
                ] {
                    assert_eq!(phases[0][field], half(record, at)?);
                }
                check_rule_storage(&bundle["hit_rules"], &record[28..end])?;
                let rules = bundle["hit_rules"]["records"].as_array().unwrap();
                assert_eq!(rules.len() * 28, end - 28);
                for (rule, source) in rules.iter().zip(record[28..end].chunks_exact(28)) {
                    assert_eq!(rule["power"], half(source, 14)?);
                    assert_eq!(rule["sound"], half(source, 16)?);
                    assert_eq!(rule["conditions"], word(source, 8)?);
                }
                assert_eq!(
                    bundle["unused_storage"],
                    serde_json::json!({"offset":end,"bytes":&record[end..]})
                );
            }
            assert_eq!(compact, 24);
            assert!(physical > 0);
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires the locally extracted original battle assets"]
    fn original_battle_skit_records_preserve_animation_and_uninitialized_commands() {
        use std::{fs, path::Path};
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        for disc in ["disc1", "disc2"] {
            let extracted = extracted.join(disc);
            let rel = super::super::actions::Rel::read(&extracted.join("files/US_r_Top2Btl.rel"))
                .unwrap();
            assert!(
                rel.at((4, 0x1d0))
                    .unwrap()
                    .starts_with(b"./Btl/BTLskit.dat\0")
            );
            for callback in [0x80a78, 0x80a7c] {
                assert_eq!(word(rel.at((1, callback)).unwrap(), 0).unwrap(), 0x4e800020);
            }
            let bytes = fs::read(extracted.join("files/BTL/BTLskit.dat")).unwrap();
            assert_eq!(bytes.len(), 90 * 2048);
            for record in bytes.chunks_exact(2048) {
                let bundle = physical_bundle(record).unwrap();
                check_rule_storage(
                    &bundle["hit_rules"],
                    &record[word(record, 0).unwrap() as usize..word(record, 4).unwrap() as usize],
                )
                .unwrap();
                check_hit_storage(
                    &bundle,
                    &record[word(record, 4).unwrap() as usize..word(record, 8).unwrap() as usize],
                )
                .unwrap();
                let phases = bundle["phases"].as_array().unwrap();
                assert_eq!(phases.len(), 4);
                for (index, phase) in phases.iter().enumerate() {
                    assert_eq!(phase["duration"], 0);
                    let root = phase["animation_root"].as_u64().unwrap();
                    let records = bundle["animations"]["records"].as_array().unwrap();
                    let animation =
                        &records.iter().find(|r| r["offset"] == root).unwrap()["forced_bind"];
                    assert_eq!(animation["kind"], "play");
                    assert_eq!(animation["clip"], index + 12);
                    assert_eq!(animation["resource"], -1);
                    assert_eq!(animation["rate"], 0.5);
                    assert_eq!(phase["commands"]["initialized"], false);
                    assert!(phase["commands"]["commands"].as_array().unwrap().is_empty());
                }
                let animation_base = word(record, 8).unwrap() as usize;
                let command_base = word(record, 12).unwrap() as usize;
                assert_eq!(
                    bundle["animations"]["records"].as_array().unwrap().len(),
                    (command_base - animation_base) / crate::battle::animation_table::ROW_BYTES
                );
                for storage in serde_json::from_value::<Vec<Storage>>(
                    bundle["animations"]["unreferenced_storage"].clone(),
                )
                .unwrap()
                {
                    let start = animation_base + storage.offset;
                    assert_eq!(storage.bytes, record[start..start + storage.bytes.len()]);
                }
            }
            // This is physical uninitialized storage, never a valid live program.
            assert!(decode_commands(&bytes[0xe0..2048]).is_err());
            let mut record = bytes[..2048].to_vec();
            record[0xe2..0xe4].copy_from_slice(&128u16.to_be_bytes());
            assert!(
                physical_bundle(&record)
                    .unwrap_err()
                    .to_string()
                    .contains("unknown action opcode 128 at 0x0")
            );
        }
    }

    #[test]
    fn animation_storage_unions_shared_phase_programs() -> Result<()> {
        let mut bytes = vec![0; 256];
        for (at, value) in [(0, 128u32), (4, 128), (8, 128), (12, 224)] {
            bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
        for phase in 0..4 {
            let start = 128 + phase * 24;
            bytes[36 + phase * 28..40 + phase * 28]
                .copy_from_slice(&(phase as u32 * 2).to_be_bytes());
            bytes[start + 2] = phase as u8 + 12;
            bytes[start + 8..start + 12].copy_from_slice(&0.5f32.to_be_bytes());
            bytes[start + 12..start + 14].copy_from_slice(&(-2i16).to_be_bytes());
            bytes[start + 14..start + 24].fill(0xaa);
        }
        let bundle = physical_bundle(&bytes)?;
        let storage = bundle["animations"]["unreferenced_storage"]
            .as_array()
            .unwrap();
        assert!(storage.is_empty());
        // Aliased phase roots still retain every complete physical row for callbacks.
        bytes[64..68].fill(0);
        let bundle = physical_bundle(&bytes)?;
        assert_eq!(
            bundle["phases"][0]["animation_root"],
            bundle["phases"][1]["animation_root"]
        );
        assert_eq!(
            bundle["animations"]["unreferenced_storage"],
            serde_json::json!([])
        );
        assert_eq!(bundle["animations"]["records"].as_array().unwrap().len(), 8);
        Ok(())
    }

    #[test]
    fn variable_width_commands_preserve_native_and_audio_dependencies() {
        let words: [i16; 19] = [
            0, 38, 204, 0, 3, 43, 11, 0, 7, 42, 100, 101, 2, 50, 9, -3, -2, 0, 0,
        ];
        let bytes: Vec<_> = words.into_iter().flat_map(i16::to_be_bytes).collect();
        let program = decode_commands(&bytes).unwrap();
        assert!(program.loops);
        assert_eq!(
            program.commands.iter().map(|c| c.kind).collect::<Vec<_>>(),
            [
                CommandKind::CastPrimaryTechnique,
                CommandKind::ApplyPreviousTargetEvent,
                CommandKind::RandomVoice,
                CommandKind::WaitHit
            ]
        );
        assert_eq!(
            program.commands[0].dependencies,
            [CommandDependency::NativeTechnique {
                id: 204,
                slot: CastSlot::Primary
            }]
        );
        assert_eq!(
            program.commands[2].dependencies,
            [
                CommandDependency::Voice {
                    id: 100,
                    priority: 2
                },
                CommandDependency::Voice {
                    id: 101,
                    priority: 2
                }
            ]
        );
        let physical = physical_commands(&bytes).unwrap();
        assert_eq!(
            physical["commands"][2]["operands"],
            serde_json::json!([100, 101, 2, 50])
        );
        let table = physical_command_table(&bytes).unwrap();
        assert_eq!(
            table["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["offset"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            [0, 8, 16, 28, 32]
        );
        assert!(table["entries"][4]["loops"].as_bool().unwrap());
        assert!(
            super::super::actions::commands(&bytes)
                .unwrap_err()
                .to_string()
                .contains("unsupported battle action command 43 at 0x8")
        );
        // A runtime projection retains its stricter priority guard; physical
        // recovery preserves the full operand and native dependency truncation.
        let mut voice = bytes[16..28].to_vec();
        voice[8..10].copy_from_slice(&0x102u16.to_be_bytes());
        voice.extend((-2i16).to_be_bytes());
        assert_eq!(
            physical_commands(&voice).unwrap()["commands"][0]["operands"][2],
            258
        );
        assert_eq!(
            decode_commands(&voice).unwrap().commands[0].dependencies,
            program.commands[2].dependencies
        );
        assert!(super::super::actions::commands(&voice).is_err());
        voice[8..10].copy_from_slice(&2u16.to_be_bytes());
        voice.extend([0xab, 0xcd]);
        let (runtime, loops, tail) = super::super::actions::commands_with_tail(&voice).unwrap();
        assert_eq!((runtime.len(), loops, tail), (1, true, &[0xab, 0xcd][..]));
        assert!(decode_commands(&bytes[..27]).is_err());
        assert!(decode_commands(&[0, 0, 0, 11]).is_err());
    }
}
