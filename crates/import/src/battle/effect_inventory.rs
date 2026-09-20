//! Enumerate authored effect programs without preparing textures, models or audio.
use super::effect_program::{ACTOR_BYTES, actor, events, modifiers};
use crate::{
    compression, digest,
    field::{MapArchive, sections},
    read::{u16 as half, u32 as word},
};
use anyhow::{Context, Result, ensure};
use resonance_content::{battle::effect_inventory::*, menu_data::TECHNIQUE_COUNT};
use std::{collections::BTreeSet, fs, path::Path};

const RECORD_LIMIT: usize = 4096;

pub(crate) fn recover(extracted: &Path) -> Result<EffectInventory> {
    let mut inventory = EffectInventory {
        sources: Vec::new(),
        banks: Vec::new(),
        unresolved: Vec::new(),
    };
    let executable = fs::read(extracted.join("sys/main.dol"))?;
    add_source(
        &mut inventory,
        "sys/main.dol",
        &executable,
        EffectArchiveKind::Catalog,
        TECHNIQUE_COUNT,
    )?;
    let mut loaded_magic = BTreeSet::new();
    for row in crate::arte::read(&executable)?.definitions {
        let native = row.native_id;
        if native >= 200 && row.flags & 0x10000001 != 0 {
            loaded_magic.insert((native - 200) as usize);
        }
    }
    let usual = fs::read(extracted.join("files/BTL/BTLusual.dat"))?;
    let members = sections(&usual)?;
    let source = add_source(
        &mut inventory,
        "BTL/BTLusual.dat",
        &usual,
        EffectArchiveKind::Usual,
        members.len(),
    )?;
    for (index, range) in members.iter().enumerate() {
        let Some(range) = range else { continue };
        let bytes = &usual[range.clone()];
        if !bytes.starts_with(b"ef1\0") && !matches!(index, 2 | 3) {
            continue;
        }
        let (id, binding) = match index {
            2 => (
                EffectSourceBank::Common,
                EffectBankBinding::Fixed { slot: 0 },
            ),
            3 => (
                EffectSourceBank::Techniques,
                EffectBankBinding::Fixed { slot: 1 },
            ),
            _ => (
                EffectSourceBank::Unclassified {
                    source,
                    member: index as u16,
                },
                EffectBankBinding::Unresolved,
            ),
        };
        add_bank(&mut inventory, source, id, binding, range.start, bytes);
    }

    let rel = fs::read(extracted.join("files/US_r_Top2Btl.rel"))?;
    add_source(
        &mut inventory,
        "files/US_r_Top2Btl.rel",
        &rel,
        EffectArchiveKind::Catalog,
        0,
    )?;
    let table = word(&rel, 16)? as usize + 5 * 8;
    let base = (word(&rel, table)? & !3) as usize;
    let size = word(&rel, table + 4)? as usize;
    let data = rel
        .get(base..base + size)
        .context("missing battle resource catalogs")?;
    for (file, kind, offsets, count) in [
        ("BTL/BTLmagic.dat", EffectArchiveKind::Magic, 0xfe0, 121),
        (
            "BTL/BTLskill.dat",
            EffectArchiveKind::Skill,
            0x11c4,
            super::all::SKILL_ARCHIVE_ROWS,
        ),
        ("BTL/BTLbg.dat", EffectArchiveKind::Arena, 0x3b90, 98),
    ] {
        let archive = fs::read(extracted.join("files").join(file))?;
        let offsets = (0..count)
            .map(|i| word(data, offsets + i * 4).map(|v| v as usize))
            .collect::<Result<Vec<_>>>()?;
        let end = offsets
            .iter()
            .position(|&v| v == archive.len())
            .context("missing battle archive EOF entry")?;
        ensure!(
            offsets[end + 1..].iter().all(|&v| v == 0),
            "nonempty catalog after EOF"
        );
        let source = add_source(&mut inventory, file, &archive, kind, end)?;
        for index in 0..end {
            // Magic/skill entry 1 is their first package; entry 0 is unused by the action table.
            let initial = if kind == EffectArchiveKind::Arena {
                0
            } else {
                1
            };
            if index < initial
                || (offsets[index] == 0
                    && index != initial
                    && !(kind == EffectArchiveKind::Magic && loaded_magic.contains(&index)))
            {
                continue;
            }
            let id = match kind {
                EffectArchiveKind::Magic => EffectSourceBank::Magic {
                    package: index as u16,
                },
                EffectArchiveKind::Skill => EffectSourceBank::Skill {
                    package: index as u16,
                },
                _ => EffectSourceBank::Arena {
                    arena: index as u16,
                },
            };
            let result = (|| -> Result<()> {
                let end = offsets[index + 1..]
                    .iter()
                    .copied()
                    .find(|&v| v != 0)
                    .context("unterminated resource range")?;
                let package = archive
                    .get(offsets[index]..end)
                    .context("invalid resource package range")?;
                if kind == EffectArchiveKind::Arena {
                    let package = MapArchive::decode(package)?;
                    if let Some(range) = package.sections.get(9).and_then(Option::as_ref) {
                        add_bank(
                            &mut inventory,
                            source,
                            id,
                            EffectBankBinding::Fixed { slot: 9 },
                            range.start,
                            &package.bytes[range.clone()],
                        );
                    }
                } else {
                    let start = word(package, 4)? as usize;
                    if start != 0 {
                        let end = word(package, 8)? as usize;
                        add_bank(
                            &mut inventory,
                            source,
                            id,
                            EffectBankBinding::DynamicSlot { base: 6 },
                            start,
                            package
                                .get(start..end)
                                .context("invalid dynamic effect bank range")?,
                        );
                    }
                }
                Ok(())
            })();
            if let Err(error) = result {
                issue(
                    &mut inventory,
                    source,
                    Some(id),
                    offsets[index],
                    EffectInventoryIssueKind::Package,
                    error.to_string(),
                );
            }
            if kind == EffectArchiveKind::Magic
                && offsets[index] == 0
                && index != initial
                && let Some(bank) = inventory.banks.iter_mut().find(|b| b.id == id)
            {
                bank.alias_of = Some(EffectSourceBank::Magic {
                    package: initial as u16,
                });
            }
        }
    }

    let archive = fs::read(extracted.join("files/BTL/BTLenemy.dat"))?;
    let table = &usual[members
        .get(10)
        .and_then(Option::as_ref)
        .context("missing enemy directory")?
        .clone()];
    let offsets = table
        .chunks_exact(4)
        .map(|v| word(v, 0).map(|v| v as usize))
        .collect::<Result<Vec<_>>>()?;
    let end = offsets
        .iter()
        .position(|&v| v == archive.len())
        .context("missing enemy archive EOF entry")?;
    ensure!(
        offsets[end + 1..].iter().all(|&v| v == 0),
        "nonempty enemy catalog after EOF"
    );
    let source = add_source(
        &mut inventory,
        "BTL/BTLenemy.dat",
        &archive,
        EffectArchiveKind::Enemy,
        end,
    )?;
    for index in 0..end {
        let id = EffectSourceBank::Enemy {
            monster: index as u16,
        };
        let result = (|| -> Result<()> {
            let bytes = compression::decode(
                archive
                    .get(offsets[index]..offsets[index + 1])
                    .context("invalid enemy package range")?,
            )?;
            ensure!(bytes.starts_with(b"em8\0"), "invalid enemy package");
            let start = word(&bytes, 0x1cc)? as usize;
            if start != 0 {
                let end = word(&bytes, 0x1d0)? as usize;
                add_bank(
                    &mut inventory,
                    source,
                    id,
                    EffectBankBinding::EnemySlot { base: 2 },
                    start,
                    bytes
                        .get(start..if end > start { end } else { bytes.len() })
                        .context("invalid enemy effect bank range")?,
                );
            }
            Ok(())
        })();
        if let Err(error) = result {
            issue(
                &mut inventory,
                source,
                Some(id),
                offsets[index],
                EffectInventoryIssueKind::Package,
                error.to_string(),
            );
        }
    }

    let mut party = fs::read_dir(extracted.join("files/BTL"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    party.sort();
    for path in party
        .into_iter()
        .filter(|p| p.extension().is_some_and(|v| v.eq_ignore_ascii_case("cab")))
    {
        let bytes = fs::read(&path)?;
        let file = format!(
            "BTL/{}",
            path.file_name()
                .context("unnamed party cabinet")?
                .to_str()
                .context("non-UTF8 party cabinet")?
        );
        let source = add_source(&mut inventory, &file, &bytes, EffectArchiveKind::Party, 0)?;
        match MapArchive::decode(&bytes) {
            Ok(package) => {
                inventory.sources[usize::from(source)].entries =
                    package.sections.len().try_into()?;
                for (index, range) in package.sections.iter().enumerate() {
                    let Some(range) = range else { continue };
                    if package.bytes[range.clone()].starts_with(b"ef1\0") {
                        add_bank(
                            &mut inventory,
                            source,
                            EffectSourceBank::Unclassified {
                                source,
                                member: index as u16,
                            },
                            EffectBankBinding::Unresolved,
                            range.start,
                            &package.bytes[range.clone()],
                        );
                    }
                }
            }
            Err(error) => issue(
                &mut inventory,
                source,
                None,
                0,
                EffectInventoryIssueKind::Package,
                error.to_string(),
            ),
        }
    }
    inventory.validate()?;
    Ok(inventory)
}

fn add_source(
    out: &mut EffectInventory,
    path: &str,
    bytes: &[u8],
    kind: EffectArchiveKind,
    entries: usize,
) -> Result<u16> {
    let index = out.sources.len().try_into()?;
    out.sources.push(EffectInventorySource {
        path: if kind == EffectArchiveKind::Catalog {
            path.into()
        } else {
            format!("files/{path}")
        },
        sha256: digest(bytes),
        kind,
        entries: entries.try_into()?,
    });
    Ok(index)
}

fn issue(
    out: &mut EffectInventory,
    source: u16,
    bank: Option<EffectSourceBank>,
    offset: usize,
    kind: EffectInventoryIssueKind,
    detail: String,
) {
    out.unresolved.push(EffectInventoryIssue {
        source,
        bank,
        offset: offset as u32,
        kind,
        detail,
    });
}

fn add_bank(
    out: &mut EffectInventory,
    source: u16,
    id: EffectSourceBank,
    binding: EffectBankBinding,
    offset: usize,
    bytes: &[u8],
) {
    let mut parser = Parser {
        bytes,
        source,
        id,
        issues: Vec::new(),
    };
    match parser.bank(binding, offset) {
        Ok(bank) => out.banks.push(bank),
        Err(error) => parser.issue(0, EffectInventoryIssueKind::Bank, error.to_string()),
    }
    out.unresolved.extend(parser.issues);
}

struct Parser<'a> {
    bytes: &'a [u8],
    source: u16,
    id: EffectSourceBank,
    issues: Vec<EffectInventoryIssue>,
}
impl Parser<'_> {
    fn issue(&mut self, offset: usize, kind: EffectInventoryIssueKind, detail: impl Into<String>) {
        self.issues.push(EffectInventoryIssue {
            source: self.source,
            bank: Some(self.id),
            offset: offset as u32,
            kind,
            detail: detail.into(),
        });
    }

    fn bank(&mut self, binding: EffectBankBinding, offset: usize) -> Result<EffectBankInventory> {
        let b = self.bytes;
        ensure!(
            b.starts_with(b"ef1\0") && b.len() >= 20,
            "invalid ef1 header"
        );
        let actor_start = usize::from(half(b, 8)?);
        let actor_end = usize::from(half(b, 12)?);
        let program_table = usize::from(half(b, 16)?);
        let uv_table = usize::from(half(b, 18)?);
        ensure!(
            actor_end >= actor_start && (actor_end - actor_start) % ACTOR_BYTES == 0,
            "invalid ef1 actor region"
        );
        ensure!(
            uv_table == program_table + usize::from(b[4]) * 2
                && uv_table + usize::from(b[5]) * 2 <= b.len(),
            "invalid ef1 program/UV table extent"
        );
        self.bytes = &b[..uv_table + usize::from(b[5]) * 2];
        let b = self.bytes;
        let mut programs = Vec::new();
        let mut modifiers = BTreeSet::new();
        for id in 0..u16::from(b[4]) {
            match self.program(id, &mut modifiers) {
                Ok(program) => programs.push(program),
                Err(error) => {
                    self.issue(
                        program_table + usize::from(id) * 2,
                        EffectInventoryIssueKind::Program,
                        error.to_string(),
                    );
                    programs.push(EffectProgramInventory {
                        id,
                        offset: 0,
                        timeline: Vec::new(),
                        actors: Vec::new(),
                        terminated: false,
                    });
                }
            }
        }
        let mut actors = Vec::new();
        let mut uv = BTreeSet::new();
        for index in 0..(actor_end - actor_start) / ACTOR_BYTES {
            let at = actor_start + index * ACTOR_BYTES;
            match self.actor(index as u16, at) {
                Ok(actor) => {
                    if let Some(index) = actor.uv_track {
                        uv.insert(index);
                    }
                    actors.push(actor);
                }
                Err(error) => self.issue(at, EffectInventoryIssueKind::Actor, error.to_string()),
            }
        }
        for program in &programs {
            for actor in &program.actors {
                if !actors.iter().any(|a| a.id == *actor) {
                    self.issue(
                        program.offset as usize,
                        EffectInventoryIssueKind::Actor,
                        format!("program {} references missing actor {actor}", program.id),
                    );
                }
            }
        }
        for actor in &actors {
            for secondary in &actor.secondary {
                if secondary.program >= b[4] {
                    self.issue(
                        actor.offset as usize,
                        EffectInventoryIssueKind::Program,
                        format!("secondary program {} exceeds bank count", secondary.program),
                    );
                }
            }
        }
        for index in 0..usize::from(b[5]) {
            let at = uv_table + index * 2;
            let offset = usize::from(half(b, at)?);
            if offset % 10 == 0 && offset / 10 < 128 {
                uv.insert((offset / 10) as u8);
            } else {
                self.issue(
                    at,
                    EffectInventoryIssueKind::UvTrack,
                    "UV table entry is not a signed-byte row index",
                );
            }
        }
        let modifiers = modifiers.into_iter().map(|at| self.modifiers(at)).collect();
        let uv_tracks = uv.into_iter().map(|id| self.uv(id)).collect();
        if matches!(binding, EffectBankBinding::Unresolved) {
            self.issue(
                0,
                EffectInventoryIssueKind::UnknownBankBinding,
                "ef1 member has no verified runtime registration",
            );
        }
        Ok(EffectBankInventory {
            id: self.id,
            source: self.source,
            offset: offset as u32,
            sha256: digest(b),
            binding,
            alias_of: None,
            declared_programs: b[4],
            declared_uv_tracks: b[5],
            declared_actors: ((actor_end - actor_start) / ACTOR_BYTES).try_into()?,
            programs,
            actors,
            modifiers,
            uv_tracks,
        })
    }

    fn program(
        &mut self,
        id: u16,
        modifiers: &mut BTreeSet<u16>,
    ) -> Result<EffectProgramInventory> {
        let table = usize::from(half(self.bytes, 16)?);
        let relative = half(self.bytes, table + usize::from(id) * 2)? as i16;
        let start = i32::from(half(self.bytes, 10)?) + i32::from(relative);
        ensure!(start >= 0, "negative ef1 program address");
        let start = start as usize;
        let mut stream =
            events::Timeline::new(self.bytes.get(start..).context("missing effect timeline")?);
        let mut timeline = Vec::new();
        let mut actors = BTreeSet::new();
        let mut terminated = false;
        for _ in 0..RECORD_LIMIT {
            let at = start + stream.offset();
            let event = stream.next()?;
            let command_at = at + usize::from(event.repeat.is_some()) * events::RECORD_BYTES;
            let command = self.command(event.command, command_at, &mut actors, modifiers);
            let command = if let Some(repeat) = event.repeat {
                EffectTimelineCommand::Repeat {
                    count: repeat.count,
                    interval: repeat.interval,
                    command: Box::new(command),
                }
            } else {
                command
            };
            terminated = matches!(command, EffectTimelineCommand::End);
            timeline.push(EffectTimelineRecord {
                offset: at as u32,
                tick: event.tick,
                command,
            });
            if terminated {
                break;
            }
        }
        if !terminated {
            self.issue(
                start + stream.offset(),
                EffectInventoryIssueKind::Program,
                "effect timeline exceeds record limit",
            );
        }
        Ok(EffectProgramInventory {
            id,
            offset: start as u32,
            timeline,
            actors: actors.into_iter().collect(),
            terminated,
        })
    }

    fn command(
        &mut self,
        command: events::Command,
        at: usize,
        actors: &mut BTreeSet<u16>,
        modifiers: &mut BTreeSet<u16>,
    ) -> EffectTimelineCommand {
        match command {
            events::Command::Sound { sound, priority } => EffectTimelineCommand::Sound {
                sound: u16::from(sound),
                priority: priority as u8,
            },
            events::Command::ModifyRetained { slot, modifier } => {
                modifiers.insert(modifier);
                EffectTimelineCommand::ModifyRetained { slot, modifier }
            }
            events::Command::End { .. } => EffectTimelineCommand::End,
            events::Command::Repeat { .. } => {
                self.issue(
                    at,
                    EffectInventoryIssueKind::Program,
                    "nested repeat requires native actor-dispatch resolution",
                );
                EffectTimelineCommand::Unresolved { opcode: 255 }
            }
            events::Command::Emit {
                actor,
                attachment,
                modifier,
            } => {
                actors.insert(u16::from(actor));
                if modifier != 0 {
                    modifiers.insert(modifier);
                }
                EffectTimelineCommand::Emit {
                    actor: u16::from(actor),
                    modifier: (modifier != 0).then_some(modifier),
                    attachment,
                }
            }
        }
    }

    fn actor(&mut self, id: u16, offset: usize) -> Result<EffectActorInventory> {
        let record = actor::Record::read(
            self.bytes
                .get(offset..offset + ACTOR_BYTES)
                .context("truncated effect actor")?,
        )?;
        let row = &record.prefix;
        // Controller actors return before allocation; their payload is not render state.
        let geometric = row.kind < 22;
        let flags = if geometric {
            row.flags_or_shake_amplitude
        } else {
            0
        };
        let geometry = match row.kind {
            3 => EffectGeometryRequirement::Model,
            4 => EffectGeometryRequirement::Ring,
            5 => EffectGeometryRequirement::Quad,
            6 => EffectGeometryRequirement::SphereBands,
            7 => EffectGeometryRequirement::BillboardRing,
            8 => EffectGeometryRequirement::BillboardTrail,
            9 => EffectGeometryRequirement::RadialQuads,
            10 => EffectGeometryRequirement::Spiral,
            11 => EffectGeometryRequirement::Disc,
            12 => EffectGeometryRequirement::CurvedShell,
            13 => EffectGeometryRequirement::Ellipsoid,
            0 | 14 | 16 | 17 => EffectGeometryRequirement::NoDraw,
            19 => EffectGeometryRequirement::TriangleBand,
            18 => EffectGeometryRequirement::JitterRibbon,
            22 => EffectGeometryRequirement::Shake,
            23 => EffectGeometryRequirement::Camera,
            24 => EffectGeometryRequirement::StageColor,
            25 => EffectGeometryRequirement::Caption,
            15 => EffectGeometryRequirement::VertexQuad,
            native_type => {
                self.issue(
                    offset,
                    EffectInventoryIssueKind::UnknownGeometry,
                    format!("native actor type {native_type} requires semantic recovery"),
                );
                EffectGeometryRequirement::Unresolved { native_type }
            }
        };
        use EffectActorRequirement::*;
        let bits = [
            (1, Bounce),
            (2, BottomAnchor),
            (4, ModelAnimation),
            (8, ColorGradient),
            (0x10, RepeatedUv),
            (0x20, GeometryMode),
            (0x40, Billboard),
            (0x80, AlignToVelocity),
            (0x100, DuringPause),
            (0x400, FollowEmitter),
            (0x800, FlaredRing),
            (0x1000, GroundRelative),
            (0x2000, OwnerAfter),
            (0x40000, RetainedActor),
            (0x80000, NoDepthTest),
            (0x100000, FineTessellation),
            (0x200000, OwnerBefore),
            (0x800000, DepthWrite),
            (0x1000000, CoarseTessellation),
            (0x2000000, CameraRelative),
            (0x4000000, DualTexture),
            (0x10000000, CullBack),
            (0x20000000, FollowBone),
            (0x400000, ElementPalette),
            (0x8000000, ClampToGround),
        ];
        let mut requirements = Vec::new();
        let mut unknown = flags;
        if flags & 0x200 != 0 && matches!(row.kind, 3 | 5) {
            if row.kind == 5 {
                requirements.push(BoneGroupCopies);
            }
            // The model draw path leaves this quad-only flag unused. Preserve the raw bit.
            unknown &= !0x200;
        }
        if row.kind == 3 && flags & 0x20000 != 0 {
            requirements.push(RetainedModelJoint);
            unknown &= !0x20000;
        }
        if flags & 0x40000000 != 0 && matches!(row.kind, 3 | 4) {
            requirements.push(OwnerBoneTransform);
            if row.kind == 3 {
                requirements.push(IndependentModelHeading);
            }
            unknown &= !0x40000000;
        }
        // Billboard quads do not read this ring/model-only transform flag. Keep its raw bit.
        if row.kind == 5 && flags & 0x40000040 == 0x40000040 {
            unknown &= !0x40000000;
        }
        for (bit, requirement) in bits {
            if flags & bit != 0 {
                requirements.push(requirement);
                unknown &= !bit;
            }
        }
        if unknown != 0 {
            requirements.push(UnresolvedFlags);
            self.issue(
                offset + 0x14,
                EffectInventoryIssueKind::UnknownFlags,
                format!("unresolved actor flags {unknown:#010x}"),
            );
        }
        let model = if let actor::Body::Particle { geometry, .. } = &record.body
            && let actor::GeometryOperands::Model {
                index, animation, ..
            } = geometry.as_ref()
        {
            Some(EffectModelReference {
                slot: EffectModelSlot::Authored(row.resource_slot),
                index: *index,
                animation: (flags & 4 != 0).then_some(*animation),
                loop_animation: row.secondary.flags & 2 != 0,
            })
        } else {
            None
        };
        let texture =
            (geometric && !matches!(row.kind, 0 | 3 | 14 | 16 | 17) && row.resource_slot != 255)
                .then_some(EffectTextureReference {
                    slot: row.resource_slot,
                    color_palette: row.palettes[0],
                    alpha_palette: (flags & 0x4000000 != 0).then_some(row.palettes[1]),
                    palette_stride: row.palette_stride,
                });
        let common_update = geometric && row.kind != 17;
        let uv_track = (common_update && row.uv_track != -1).then_some(row.uv_track as u8);
        let mut secondary = Vec::new();
        if common_update && row.secondary.flags & 4 != 0 {
            secondary.push(EffectSecondaryReference {
                trigger: EffectSecondaryTrigger::GroundContact,
                program: row.secondary.ground_program,
            });
        }
        if common_update && row.secondary.flags & 8 != 0 {
            secondary.push(EffectSecondaryReference {
                trigger: EffectSecondaryTrigger::Periodic,
                program: row.secondary.periodic_program,
            });
        }
        if common_update && row.secondary.flags & !15 != 0 {
            self.issue(
                offset + 0x91,
                EffectInventoryIssueKind::UnknownSecondaryEmission,
                format!(
                    "unresolved actor control flags {:#04x}",
                    row.secondary.flags & !15
                ),
            );
        }
        Ok(EffectActorInventory {
            id,
            offset: offset as u32,
            native_type: row.kind,
            geometry,
            flags,
            requirements,
            model,
            texture,
            uv_track,
            secondary,
        })
    }

    fn modifiers(&mut self, offset: u16) -> EffectModifierInventory {
        let mut out = EffectModifierInventory {
            offset,
            operations: Vec::new(),
            terminated: false,
        };
        let result = (|| -> Result<()> {
            let mut at = usize::from(offset);
            for _ in 0..RECORD_LIMIT {
                let opcode = half(self.bytes, at)? as i16;
                let (operation, destination, model, next) = if !matches!(opcode, -1..=30) {
                    // Unknown widths cannot be walked; retain their diagnostic header only.
                    (
                        EffectModifierOperation::Unresolved { opcode },
                        half(self.bytes, at + 2)?,
                        None,
                        at,
                    )
                } else {
                    let (record, next) = modifiers::read(self.bytes, at)?;
                    let modifiers::Record::Instruction { instruction } = record else {
                        out.terminated = true;
                        return Ok(());
                    };
                    let model = match &instruction.operation {
                        modifiers::Operation::ModelAnimation {
                            model, clip, flags, ..
                        } => Some(EffectModelReference {
                            slot: EffectModelSlot::EffectContext,
                            index: *model as u8,
                            animation: Some(*clip as u8),
                            loop_animation: *flags == 0 || flags & 1 != 0,
                        }),
                        _ => None,
                    };
                    (
                        modifier_operation(&instruction.operation),
                        instruction.destination as u16,
                        model,
                        next,
                    )
                };
                let destination = match destination {
                    i @ 0x7ffc..=0x7fff => {
                        EffectModifierDestination::IntegerTemporary((i - 0x7ffc) as u8)
                    }
                    i @ 0x7ff8..=0x7ffb => {
                        EffectModifierDestination::FloatTemporary((i - 0x7ff8) as u8)
                    }
                    i => EffectModifierDestination::ActorField(i),
                };
                out.operations.push(EffectModifierUse {
                    offset: at as u32,
                    operation,
                    destination,
                    model,
                });
                if matches!(operation, EffectModifierOperation::Unresolved { .. }) {
                    self.issue(
                        at,
                        EffectInventoryIssueKind::UnknownModifier,
                        format!("native modifier opcode {opcode}"),
                    );
                    return Ok(());
                }
                at = next;
            }
            anyhow::bail!("effect modifier stream exceeds record limit")
        })();
        if let Err(error) = result {
            self.issue(
                usize::from(offset),
                EffectInventoryIssueKind::Modifier,
                error.to_string(),
            );
        }
        out
    }

    fn uv(&mut self, id: u8) -> EffectUvInventory {
        let mut out = EffectUvInventory {
            id,
            offset: 0,
            commands: Vec::new(),
            terminated: false,
        };
        let result = (|| -> Result<()> {
            ensure!(id < 128, "negative UV row index");
            let start = usize::from(half(self.bytes, 14)?) + usize::from(id) * 10;
            out.offset = start as u32;
            for i in 0..RECORD_LIMIT {
                let row = self
                    .bytes
                    .get(start + i * 10..start + (i + 1) * 10)
                    .context("unterminated UV track")?;
                let command = match row[0] {
                    254 => EffectUvCommand::End,
                    255 => EffectUvCommand::Loop { target: row[1] },
                    opcode @ 128..=253 => EffectUvCommand::Scroll { opcode },
                    duration => {
                        if half(row, 2)? as i16 == -32000 {
                            EffectUvCommand::Palette { duration }
                        } else {
                            EffectUvCommand::Frame { duration }
                        }
                    }
                };
                out.commands.push(command);
                if matches!(
                    command,
                    EffectUvCommand::End
                        | EffectUvCommand::Loop { .. }
                        | EffectUvCommand::Scroll { .. }
                ) {
                    out.terminated = true;
                    return Ok(());
                }
            }
            anyhow::bail!("UV track exceeds record limit")
        })();
        if let Err(error) = result {
            self.issue(
                out.offset as usize,
                EffectInventoryIssueKind::UvTrack,
                error.to_string(),
            );
        }
        out
    }
}

fn modifier_operation(operation: &modifiers::Operation) -> EffectModifierOperation {
    use EffectModifierOperation::*;
    use modifiers::Operation as Physical;
    match operation {
        Physical::Integer {
            width, operation, ..
        } => Integer {
            width: *width,
            operation: *operation,
        },
        Physical::Float { operation, .. } => Float {
            operation: *operation,
        },
        Physical::SetVector { .. } => SetVector,
        Physical::RandomInteger { .. } => RandomInteger,
        Physical::RandomPolarVector { .. } => RandomPolarVector,
        Physical::RandomFloat { .. } => RandomFloat,
        Physical::RotateVector { .. } => RotateVector,
        Physical::PolarVector { .. } => PolarVector,
        Physical::SetColor { .. } => SetColor,
        Physical::ClearFlags { .. } => ClearFlags,
        Physical::TranslateVertices { .. } => TranslateVertices,
        Physical::SetFlags { .. } => SetFlags,
        Physical::ModelAnimation { .. } => ModelAnimation,
        Physical::AnimationPosition { .. } => AnimationPosition,
        Physical::AnimationRate { .. } => AnimationRate,
        Physical::EmitterAxisVector { axis, .. } => EmitterAxisVector { axis: *axis },
    }
}

#[test]
fn inventory_keeps_repeat_dependencies_and_unknown_modifier_boundaries() -> Result<()> {
    let mut bytes = vec![0; 24];
    bytes[10..12].copy_from_slice(&24u16.to_be_bytes());
    bytes[16..18].copy_from_slice(&20u16.to_be_bytes());
    bytes.extend(
        [
            [255, 255, 255, 2, 255, 253], // Signed repeat time/interval remain inventory data.
            [128, 0, 252, 9, 171, 205],   // Payload time is inactive; priority uses its low byte.
            [0, 1, 255, 1, 0, 2],
            [0, 0, 255, 3, 0, 4], // Nested repeat is unresolved at the payload offset.
            [0, 2, 255, 2, 0, 1],
            [0, 0, 254, 1, 0, 0], // Repeated End does not terminate the outer timeline.
            [0, 3, 7, 255, 0, 0],
            [0, 4, 253, 4, 0, 0], // Retained modifiers may reference zero.
            [0, 5, 254, 255, 171, 205],
        ]
        .concat(),
    );
    let mut parser = Parser {
        bytes: &bytes,
        source: 0,
        id: EffectSourceBank::Common,
        issues: vec![],
    };
    let mut modifiers = BTreeSet::new();
    let program = parser.program(0, &mut modifiers)?;
    assert!(program.terminated);
    assert_eq!(program.actors, [7]);
    assert_eq!(modifiers, BTreeSet::from([0]));
    assert_eq!(
        program
            .timeline
            .iter()
            .map(|row| (row.offset, row.tick))
            .collect::<Vec<_>>(),
        [(24, -1), (36, 1), (48, 2), (60, 3), (66, 4), (72, 5)]
    );
    assert!(matches!(&program.timeline[0].command,
        EffectTimelineCommand::Repeat { count: 2, interval: -3, command }
        if matches!(**command, EffectTimelineCommand::Sound { sound: 9, priority: 205 })));
    assert_eq!(parser.issues.len(), 1);
    assert_eq!(parser.issues[0].offset, 42);
    assert_eq!(
        parser.issues[0].detail,
        "nested repeat requires native actor-dispatch resolution"
    );

    let bytes = [
        0, 22, 255, 255, 255, 255, 1, 2, 0, 0, 0, 0, 0, 0, 171, 205, 128, 0, 127, 250, 255, 255,
    ];
    parser.bytes = &bytes;
    parser.issues.clear();
    let stream = parser.modifiers(0);
    assert!(!stream.terminated);
    assert_eq!(stream.operations.len(), 2);
    assert!(matches!(
        stream.operations[0].destination,
        EffectModifierDestination::ActorField(65535)
    ));
    let model = stream.operations[0].model.as_ref().unwrap();
    assert_eq!(
        (model.index, model.animation, model.loop_animation),
        (255, Some(2), true)
    );
    assert!(matches!(
        stream.operations[1].operation,
        EffectModifierOperation::Unresolved { opcode: -32768 }
    ));
    assert!(matches!(
        stream.operations[1].destination,
        EffectModifierDestination::FloatTemporary(2)
    ));
    assert_eq!(parser.issues.len(), 1);
    assert_eq!(parser.issues[0].offset, 16);
    assert!(matches!(
        parser.issues[0].kind,
        EffectInventoryIssueKind::UnknownModifier
    ));
    parser.bytes = &bytes[..18];
    parser.issues.clear();
    assert_eq!(parser.modifiers(0).operations.len(), 1);
    assert_eq!(parser.issues[0].offset, 0);
    assert!(matches!(
        parser.issues[0].kind,
        EffectInventoryIssueKind::Modifier
    ));
    Ok(())
}

#[test]
fn recovered_draw_types_keep_actor_identity_and_unknown_types_remain_unresolved() {
    let mut row = [0u8; ACTOR_BYTES];
    row[0x32] = 255;
    for (native_type, id, expected) in [
        (0, 0, EffectGeometryRequirement::NoDraw),
        (16, 16, EffectGeometryRequirement::NoDraw),
        (17, 17, EffectGeometryRequirement::NoDraw),
        (19, 55, EffectGeometryRequirement::TriangleBand),
        (
            21,
            56,
            EffectGeometryRequirement::Unresolved { native_type: 21 },
        ),
    ] {
        row[0] = native_type;
        row[0x32] = if native_type == 17 { 0 } else { 255 };
        row[0x91..0x96].fill(0);
        if native_type == 17 {
            row[0x91..0x96].copy_from_slice(&[12, 253, 0, 0, 254]);
        }
        let mut parser = Parser {
            bytes: &row,
            source: 0,
            id: EffectSourceBank::Common,
            issues: vec![],
        };
        let actor = parser.actor(id, 0).unwrap();
        assert_eq!(
            (actor.id, actor.native_type, actor.geometry),
            (id, native_type, expected)
        );
        assert_eq!(parser.issues.is_empty(), native_type != 21);
        assert_eq!(actor.texture.is_none(), matches!(native_type, 0 | 16 | 17));
        assert!(actor.uv_track.is_none());
        assert!(actor.secondary.is_empty());
    }
}

#[test]
fn bone_group_flag_is_classified_only_for_its_quad_draw_consumer() {
    let mut row = [0u8; ACTOR_BYTES];
    row[0x32] = 255;
    row[0x14..0x18].copy_from_slice(&0x240u32.to_be_bytes());
    for native in [5, 3, 4] {
        row[0] = native;
        let mut parser = Parser {
            bytes: &row,
            source: 0,
            id: EffectSourceBank::Enemy { monster: 62 },
            issues: vec![],
        };
        let actor = parser.actor(0, 0).unwrap();
        assert_eq!(
            (actor.id, actor.native_type, actor.flags),
            (0, native, 0x240)
        );
        assert_eq!(
            actor
                .requirements
                .contains(&EffectActorRequirement::BoneGroupCopies),
            native == 5
        );
        assert_eq!(
            actor
                .requirements
                .contains(&EffectActorRequirement::UnresolvedFlags),
            native == 4
        );
    }
}

#[test]
fn retained_joint_flag_requires_the_model_draw_consumer() {
    let mut row = [0u8; ACTOR_BYTES];
    row[0x32] = 255;
    row[0x14..0x18].copy_from_slice(&0x20000u32.to_be_bytes());
    for (package, id, native, model, slot, joint) in [
        (86, 5, 3, 4, 0, 0),
        (86, 8, 3, 6, 2, 0),
        (90, 5, 3, 1, 0, 0),
        (90, 6, 3, 2, 0, 1),
        (92, 5, 3, 1, 0, 0),
        (93, 7, 3, 3, 0, 2),
        (93, 8, 3, 3, 0, 3),
        (90, 5, 5, 1, 0, 0),
    ] {
        row[0] = native;
        row[2] = 6;
        row[0xd4] = model;
        row[0x8d] = slot;
        row[0x8c] = joint;
        let mut parser = Parser {
            bytes: &row,
            source: 0,
            id: EffectSourceBank::Magic { package },
            issues: vec![],
        };
        let actor = parser.actor(id, 0).unwrap();
        assert_eq!(
            (actor.id, actor.native_type, actor.flags),
            (id, native, 0x20000)
        );
        assert_eq!(
            actor
                .requirements
                .contains(&EffectActorRequirement::RetainedModelJoint),
            native == 3
        );
        assert_eq!(
            actor
                .requirements
                .contains(&EffectActorRequirement::UnresolvedFlags),
            native != 3
        );
        assert_eq!(parser.issues.is_empty(), native == 3);
    }
}

#[test]
fn original_model_group_flags_are_inert_but_camera_space_stays_unresolved() {
    use EffectActorRequirement::*;
    for (bank, id, offset, hash, encoded, flags, native, requirements) in [
        (
            EffectSourceBank::Enemy { monster: 72 },
            1,
            384,
            "adf48b2040733cefd1ee32f1a385d158dc7e8f629eb33bdb7c50b400040f7b86",
            "0300ff0000000000000000000000000000200000000002400050004000600080000000000000000000000000000000000000ff00000000004248000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004348000043480000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            576u32,
            3,
            vec![Billboard],
        ),
        (
            EffectSourceBank::Enemy { monster: 76 },
            2,
            736,
            "1fdee5237360ec7a43bd00edbe50e68c40c3d09bb5fd5cf0eac406527492cc35",
            "03000200000000000000000000000000003c000000000600004000400040000000000000000000000000001000000010102cff00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000c039999ac039999a4039999a000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000000000000003f8000003f8000003f8000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            1536u32,
            3,
            vec![FollowEmitter],
        ),
        (
            EffectSourceBank::Magic { package: 37 },
            1,
            384,
            "e0c1b70dc7991fbda65616fb586a085806de269bf1600e6d7f395cfafe10bfca",
            "050106000100200000010001003e003e003c00008400000000800080008000ff000000000000000000000000000000000000ff0043a000004370000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004280000042800000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            2214592512u32,
            5,
            vec![DualTexture, UnresolvedFlags],
        ),
    ] {
        let row = encoded
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(row.len(), ACTOR_BYTES);
        assert_eq!(digest(&row), hash);
        let bytes = [vec![0; offset], row].concat();
        let mut parser = Parser {
            bytes: &bytes,
            source: 0,
            id: bank,
            issues: vec![],
        };
        let actor = parser.actor(id, offset).unwrap();
        assert_eq!(
            (actor.id, actor.offset, actor.native_type, actor.flags),
            (id, offset as u32, native, flags)
        );
        assert_eq!(actor.requirements, requirements);
        if native == 3 {
            assert!(parser.issues.is_empty());
            let model = actor.model.unwrap();
            let wanted_slot = if flags == 0x240 { 255 } else { 2 };
            assert!(matches!(model.slot, EffectModelSlot::Authored(slot) if slot == wanted_slot));
            assert_eq!(model.index, 0);
            assert!(actor.texture.is_none());
        } else {
            assert_eq!(parser.issues.len(), 1);
            assert_eq!(parser.issues[0].offset, (offset + 0x14) as u32);
            assert!(matches!(
                parser.issues[0].kind,
                EffectInventoryIssueKind::UnknownFlags
            ));
            assert_eq!(parser.issues[0].detail, "unresolved actor flags 0x80000000");
        }
    }
}

#[test]
fn owner_bone_flag_is_geometry_dependent_and_unknown_consumers_stay_unresolved() {
    use EffectActorRequirement::*;
    for (native, flags, bone_transform, independent_heading, unresolved) in [
        (3, 0x40000000u32, true, true, false),
        (4, 0x40000000, true, false, false),
        (5, 0x40000000, false, false, true),
        (5, 0x44000040, false, false, false),
        (6, 0x44000040, false, false, true),
    ] {
        let mut row = [0; ACTOR_BYTES];
        row[0] = native;
        row[0x14..0x18].copy_from_slice(&flags.to_be_bytes());
        let mut parser = Parser {
            bytes: &row,
            source: 0,
            id: EffectSourceBank::Enemy { monster: 172 },
            issues: vec![],
        };
        let actor = parser.actor(10, 0).unwrap();
        assert_eq!(
            actor.requirements.contains(&OwnerBoneTransform),
            bone_transform
        );
        assert_eq!(
            actor.requirements.contains(&IndependentModelHeading),
            independent_heading
        );
        assert_eq!(actor.requirements.contains(&UnresolvedFlags), unresolved);
        assert_eq!(parser.issues.is_empty(), !unresolved);
    }
}
