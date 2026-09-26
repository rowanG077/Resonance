//! Load original particle commands into the shared VM. This translates original
//! input only; maintained native battle behavior is compiled from `.sym` source.
use anyhow::{Context, Result, ensure};
use resonance_battle::{
    ActionDefinition, ActionPhase, ParticleDefinition, ResourceBinding, SoundBinding,
};
use resonance_content::battle_effect::ProgramSource;
use std::sync::Arc;
use symphonia_script::{
    Op,
    authored::{BinaryOp, Conversion, Function, Module, ValueLayout},
};

/// Load requested original members from the immutable verified snapshot. The
/// caller supplies the ID of the prepared presentation resource generation.
/// A failed member leaves the entire bank inactive.
pub fn load(
    files: &resonance_content::prepared::Files,
    path: &str,
    resource: u32,
    members: &[u16],
    sound: &mut impl FnMut(u16) -> Result<SoundBinding>,
) -> Result<resonance_battle::EffectBank> {
    let source: resonance_content::battle_effect::SourceBank = files.json(path)?;
    let mut definitions = std::collections::BTreeMap::new();
    for &member in members {
        let input = source
            .program(usize::from(member))
            .with_context(|| format!("prepare effect {path} member {member}"))?;
        let definition = prepare(&input, resource, member, sound)
            .with_context(|| format!("prepare effect {path} member {member}"))?;
        ensure!(
            definitions.insert(member, Arc::new(definition)).is_none(),
            "duplicate effect member {member}"
        );
    }
    Ok(resonance_battle::EffectBank {
        models: Default::default(),
        resource,
        members: definitions,
    })
}

/// `resource` identifies the already-prepared rendering recipes for this bank.
/// Complete resource verification/preparation precedes activation of the result.
pub fn prepare(
    source: &ProgramSource,
    resource: u32,
    member: u16,
    sound: &mut impl FnMut(u16) -> Result<SoundBinding>,
) -> Result<ActionDefinition> {
    let mut module = Module::default();
    let mut resources = Vec::new();
    let mut commands = Vec::new();
    for (index, record) in source.records.iter().enumerate() {
        if record.command >= 254 {
            commands.push(None);
            continue;
        }
        ensure!(
            record.command != 253 && (record.command == 252 || record.argument == 0),
            "effect command {index} needs an unprepared retained particle or attachment"
        );
        commands.push(Some(u16::try_from(module.functions.len())?));
        module.functions.push(Function {
            name: format!("original_effect::command_{index}"),
            entry: module.code.len() as u32,
            parameters: 0,
            parameter_layout: ValueLayout::Sequence(vec![]),
            locals: if record.command == 252 { 0 } else { 5 },
            results: 0,
            is_task: false,
        });
        if record.command == 252 {
            if record.argument != 0 {
                let binding = resources.len();
                resources.push(ResourceBinding::Sound(sound(u16::from(record.argument))?));
                argument(&mut module, binding as i32);
                // 418B4 narrows the source halfword to the sound priority byte.
                argument(&mut module, i32::from(record.operand as u8));
                native(&mut module, "battle::sound");
            }
            module.code.push(Op::ReturnValues(0));
            continue;
        }
        let data = source
            .particles
            .get(&record.command)
            .context("unbound effect particle")?;
        data.validate()?;
        let binding = resources.len();
        resources.push(ResourceBinding::Particle(Arc::new(ParticleDefinition {
            model: source.models.get(&record.command).copied(),
            resource,
            member: u16::from(record.command),
            data: data.clone(),
        })));
        argument(&mut module, binding as i32);
        native(&mut module, "battle::spawn_particle");
        module.code.push(Op::StoreLocal(0));
        module.code.extend([Op::LoadLocal(0), Op::ArgumentValue]);
        native(&mut module, "battle::particle_alive");
        let allocation_failed = module.code.len();
        module.code.push(Op::BranchFalseStack(0));
        if record.operand != 0 {
            let words = source
                .modifiers
                .get(&record.operand)
                .context("unbound effect modifier")?;
            modifiers(
                &mut module,
                words,
                &data.state.geometry,
                &mut resources,
                resource,
            )?;
        }
        module.code.extend([Op::LoadLocal(0), Op::ArgumentValue]);
        native(&mut module, "battle::apply_effect_appearance");
        module.code[allocation_failed] = Op::BranchFalseStack(module.code.len() as u32);
        module.code.push(Op::ReturnValues(0));
    }
    let (program, entry) = super::effect_timeline::prepare(&source.records, &commands, module)?;
    Ok(ActionDefinition {
        id: member,
        phase: ActionPhase::Effect,
        program,
        entry,
        duration: 0,
        tp_cost: 0,
        resources,
    })
}

fn native(module: &mut Module, name: &str) {
    let declaration = resonance_battle::native_declarations()
        .into_iter()
        .find(|n| n.name == name)
        .unwrap();
    if !module.natives.contains(&declaration) {
        module.natives.push(declaration);
    }
    module.code.push(Op::Native(declaration.opcode));
}

fn argument(module: &mut Module, value: i32) {
    module.code.extend([Op::Push(value), Op::ArgumentValue]);
}

fn get_value(module: &mut Module, selector: u16) {
    argument(module, i32::from(selector - 0x7ff8));
    native(module, "battle::effect_value");
}

// Source offsets/selectors are confined to this original-input translator. The
// simulation sees named particle vectors and values, never packed memory offsets.
fn modifiers(
    module: &mut Module,
    words: &[u16],
    geometry: &resonance_battle::ParticleGeometry,
    resources: &mut Vec<ResourceBinding>,
    bank: u32,
) -> Result<()> {
    let mut at = 0;
    loop {
        let opcode = *words.get(at).context("unterminated effect modifier")?;
        if opcode == 0xffff {
            ensure!(
                at + 1 == words.len(),
                "effect modifier has records after end"
            );
            return Ok(());
        }
        let count = match opcode {
            22 => 8,
            21 => 4,
            0 | 2 | 3 | 5 | 10..=12 => 4,
            9 => 12,
            7 | 8 | 13..=15 => 6,
            _ => anyhow::bail!("effect modifier opcode {opcode} is not prepared"),
        };
        let row = words
            .get(at..at + count)
            .context("truncated effect modifier")?;
        if opcode == 22 {
            ensure!(
                row[4..7] == [0; 3] && row[2] <= 127,
                "effect model playback parameters are not prepared"
            );
            let binding = resources.len();
            resources.push(ResourceBinding::EffectMotion(
                resonance_battle::EffectMotionBinding {
                    bank,
                    model: row[2] as u8,
                    clip: row[3],
                },
            ));
            argument(module, binding as i32);
            native(module, "battle::play_effect_model");
            at += count;
            continue;
        }
        if opcode == 21 {
            let mask = u32::from(row[2]) << 16 | u32::from(row[3]);
            ensure!(
                mask & !0x1000_0000 == 0,
                "effect flag modifier is not prepared"
            );
            if mask != 0 {
                module.code.extend([Op::LoadLocal(0), Op::ArgumentValue]);
                argument(module, 1);
                native(module, "battle::set_particle_cull_back");
            }
            at += count;
            continue;
        }
        if opcode == 9 {
            polar_modifier(module, row)?;
            at += count;
            continue;
        }
        if matches!(opcode, 0 | 2 | 3 | 5 | 10 | 12) {
            integer_modifier(module, row, geometry)?;
            at += count;
            continue;
        }
        let target = row[1];
        if (0xb0..=0xc4).contains(&target) {
            ensure!(
                matches!(geometry, resonance_battle::ParticleGeometry::Size { .. }),
                "size modifier requires particle dimensions"
            );
        }
        let slot = (0x7ff8..0x7ffc).contains(&target);
        let vector = match target {
            0x34..=0x3c if target % 4 == 0 => Some(("offset", (target - 0x34) / 4)),
            0x40..=0x48 if target % 4 == 0 => Some(("velocity", (target - 0x40) / 4)),
            0x58..=0x60 if target % 4 == 0 => Some(("angles", (target - 0x58) / 4)),
            0x64..=0x6c if target % 4 == 0 => Some(("angular_velocity", (target - 0x64) / 4)),
            0xa4..=0xac if target % 4 == 0 => Some(("orbit_velocity", (target - 0xa4) / 4)),
            0xb0..=0xb8 if target % 4 == 0 => Some(("size", (target - 0xb0) / 4)),
            0xbc..=0xc4 if target % 4 == 0 => Some(("size_velocity", (target - 0xbc) / 4)),
            _ if slot => None,
            0x84 if matches!(
                geometry,
                resonance_battle::ParticleGeometry::BillboardTrail { .. }
            ) =>
            {
                None
            }
            _ => anyhow::bail!("effect modifier destination {target:#x} is not prepared"),
        };
        if opcode == 11 {
            ensure!(
                (row[3] as i16) < 0x7ff8 && row[2] != 0,
                "effect random divisor is not prepared or is zero"
            );
            native(module, "battle::random_signed");
            module.code.extend([
                Op::Push(i32::from(row[2] as i16)),
                Op::Binary(BinaryOp::RemI32),
                Op::Convert(Conversion::I32ToF32),
                Op::Push(0.1_f32.to_bits() as i32),
                Op::Binary(BinaryOp::MulF32),
            ]);
        } else if (row[4] as i16) >= 0x7ff8 {
            ensure!(
                row[4] < 0x7ffc,
                "integer effect value needs a prepared conversion"
            );
            get_value(module, row[4]);
        } else {
            let bits = u32::from(row[2]) << 16 | u32::from(row[3]);
            ensure!(
                f32::from_bits(bits).is_finite(),
                "nonfinite effect modifier literal"
            );
            module.code.push(Op::Push(bits as i32));
        }
        module.code.push(Op::StoreLocal(4));
        let arithmetic = match opcode {
            8 => Some(BinaryOp::AddF32),
            13 => Some(BinaryOp::SubF32),
            14 => Some(BinaryOp::MulF32),
            15 => Some(BinaryOp::DivF32),
            _ => None,
        };
        if let Some((name, axis)) = vector {
            module.code.extend([Op::LoadLocal(0), Op::ArgumentValue]);
            native(module, &format!("battle::particle_{name}"));
            module
                .code
                .extend([Op::StoreLocal(3), Op::StoreLocal(2), Op::StoreLocal(1)]);
            module.code.extend([Op::LoadLocal(0), Op::ArgumentValue]);
            for i in 0..3 {
                if i == axis {
                    if let Some(op) = arithmetic {
                        module.code.extend([
                            Op::LoadLocal(i + 1),
                            Op::LoadLocal(4),
                            Op::Binary(op),
                        ]);
                    } else {
                        module.code.push(Op::LoadLocal(4));
                    }
                } else {
                    module.code.push(Op::LoadLocal(i + 1));
                }
                module.code.push(Op::ArgumentValue);
            }
            native(module, &format!("battle::set_particle_{name}"));
        } else if target == 0x84 {
            if let Some(op) = arithmetic {
                module.code.extend([Op::LoadLocal(0), Op::ArgumentValue]);
                native(module, "battle::particle_segment_angle_step");
                module
                    .code
                    .extend([Op::LoadLocal(4), Op::Binary(op), Op::StoreLocal(4)]);
            }
            module.code.extend([
                Op::LoadLocal(0),
                Op::ArgumentValue,
                Op::LoadLocal(4),
                Op::ArgumentValue,
            ]);
            native(module, "battle::set_particle_segment_angle_step");
        } else {
            // Evaluate before constructing the setter's argument list: getter
            // calls consume their own native arguments in the shared VM.
            if let Some(op) = arithmetic {
                get_value(module, target);
                module
                    .code
                    .extend([Op::LoadLocal(4), Op::Binary(op), Op::StoreLocal(4)]);
            }
            argument(module, i32::from(target - 0x7ff8));
            module.code.extend([Op::LoadLocal(4), Op::ArgumentValue]);
            native(module, "battle::set_effect_value");
        }
        at += count;
    }
}

fn integer_value(module: &mut Module, value: u16) -> Result<()> {
    if (value as i16) >= 0x7ffc {
        argument(module, i32::from(value - 0x7ffc));
        native(module, "battle::effect_integer");
    } else {
        ensure!(
            (value as i16) < 0x7ff8,
            "float-to-integer effect selector is not prepared"
        );
        module.code.push(Op::Push(i32::from(value as i16)));
    }
    Ok(())
}

fn polar_modifier(module: &mut Module, row: &[u16]) -> Result<()> {
    let target = match row[1] {
        0x34 => "offset",
        0x40 => "velocity",
        _ => anyhow::bail!("polar effect destination {:#x} is not prepared", row[1]),
    };
    let float = |at| f32::from_bits(u32::from(row[at]) << 16 | u32::from(row[at + 1]));
    ensure!(
        [float(2), float(4), float(6), float(8)]
            .iter()
            .all(|v| v.is_finite()),
        "invalid polar effect operand"
    );
    let random = |module: &mut Module, divisor: u16| {
        if divisor == 0 {
            module.code.push(Op::Push(0_f32.to_bits() as i32));
        } else {
            native(module, "battle::random_signed");
            module.code.extend([
                Op::Push(i32::from(divisor as i16)),
                Op::Binary(BinaryOp::RemI32),
                Op::Convert(Conversion::I32ToF32),
                Op::Push(0.1_f32.to_bits() as i32),
                Op::Binary(BinaryOp::MulF32),
            ]);
        }
    };
    // The two source draws are radius, then Z rotation; zero ranges draw nothing.
    random(module, row[10]);
    module.code.extend([
        Op::Push(float(8).to_bits() as i32),
        Op::Binary(BinaryOp::AddF32),
        Op::StoreLocal(4),
    ]);
    random(module, row[11]);
    module.code.extend([
        Op::Push(float(6).to_bits() as i32),
        Op::Binary(BinaryOp::AddF32),
        Op::StoreLocal(3),
    ]);
    argument(module, float(2).to_bits() as i32);
    argument(module, float(4).to_bits() as i32);
    module.code.extend([
        Op::LoadLocal(3),
        Op::ArgumentValue,
        Op::LoadLocal(4),
        Op::ArgumentValue,
    ]);
    native(module, "battle::polar_point");
    module.code.extend([
        Op::StoreLocal(3),
        Op::StoreLocal(2),
        Op::StoreLocal(1),
        Op::LoadLocal(0),
        Op::ArgumentValue,
    ]);
    for local in 1..=3 {
        module
            .code
            .extend([Op::LoadLocal(local), Op::ArgumentValue]);
    }
    native(module, &format!("battle::set_particle_{target}"));
    Ok(())
}

fn integer_modifier(
    module: &mut Module,
    row: &[u16],
    geometry: &resonance_battle::ParticleGeometry,
) -> Result<()> {
    let target = row[1];
    if row[0] == 10 && target == 0xd4 {
        module.code.extend([Op::LoadLocal(0), Op::ArgumentValue]);
        integer_value(module, row[2])?;
        module.code.extend([
            Op::Push(255),
            Op::Binary(BinaryOp::BitAnd),
            Op::ArgumentValue,
        ]);
        native(module, "battle::set_particle_model");
        return Ok(());
    }
    if matches!(row[0], 0 | 2 | 3 | 5) && (0x7ffc..=0x7fff).contains(&target) {
        if matches!(row[0], 3 | 5) {
            integer_value(module, target)?;
        }
        if row[0] == 2 {
            ensure!(
                (row[2] as i16) > 0 && row[2] < 0x7ff8,
                "invalid integer random range"
            );
            native(module, "battle::random_signed");
            module
                .code
                .extend([Op::Push(65535), Op::Binary(BinaryOp::BitAnd)]);
        }
        integer_value(module, row[2])?;
        if row[0] != 0 {
            module.code.push(Op::Binary(match row[0] {
                2 => BinaryOp::RemI32,
                3 => BinaryOp::AddI32,
                5 => BinaryOp::MulI32,
                _ => unreachable!(),
            }));
        }
        // The original scratch cells are signed halfwords, including wraparound.
        module.code.extend([
            Op::Push(16),
            Op::Binary(BinaryOp::Shl),
            Op::Push(16),
            Op::Binary(BinaryOp::Shr),
            Op::StoreLocal(4),
        ]);
        argument(module, i32::from(target - 0x7ffc));
        module.code.extend([Op::LoadLocal(4), Op::ArgumentValue]);
        native(module, "battle::set_effect_integer");
        return Ok(());
    }
    if row[0] == 3 && (0x08..=0x0e).contains(&target) && target.is_multiple_of(2) {
        module.code.extend([Op::LoadLocal(0), Op::ArgumentValue]);
        argument(module, i32::from((target - 8) / 2));
        native(module, "battle::particle_uv");
        integer_value(module, row[2])?;
        module.code.extend([
            Op::Binary(BinaryOp::AddI32),
            Op::Push(16),
            Op::Binary(BinaryOp::Shl),
            Op::Push(16),
            Op::Binary(BinaryOp::Shr),
            Op::StoreLocal(4),
            Op::LoadLocal(0),
            Op::ArgumentValue,
        ]);
        argument(module, i32::from((target - 8) / 2));
        module.code.extend([Op::LoadLocal(4), Op::ArgumentValue]);
        native(module, "battle::set_particle_uv");
        return Ok(());
    }
    if matches!(row[0], 10 | 12) && matches!(target, 0x12 | 0x13) {
        ensure!(
            target != 0x13 || matches!(geometry, resonance_battle::ParticleGeometry::Ribbon { .. }),
            "phase modifier requires a ribbon particle"
        );
        let name = if target == 0x12 {
            "geometry_count"
        } else {
            "phase"
        };
        if row[0] == 12 {
            module.code.extend([Op::LoadLocal(0), Op::ArgumentValue]);
            native(module, &format!("battle::particle_{name}"));
        }
        integer_value(module, row[2])?;
        if row[0] == 12 {
            module.code.push(Op::Binary(BinaryOp::AddI32));
        }
        module.code.extend([
            Op::Push(255),
            Op::Binary(BinaryOp::BitAnd),
            Op::StoreLocal(4),
            Op::LoadLocal(0),
            Op::ArgumentValue,
            Op::LoadLocal(4),
            Op::ArgumentValue,
        ]);
        native(module, &format!("battle::set_particle_{name}"));
        return Ok(());
    }
    ensure!(
        (row[2] as i16) < 0x7ff8,
        "integer effect value selector is not prepared"
    );
    let short = i32::from(row[2] as i16);
    let byte = i32::from(row[2] as u8);
    let (name, values) = match (row[0], target) {
        (0, 0x18..=0x26) if target.is_multiple_of(2) => (
            "battle::set_particle_color",
            vec![
                i32::from((target - 0x18) / 8),
                i32::from((target % 8) / 2),
                short,
            ],
        ),
        (0, 0x08..=0x0e) if target.is_multiple_of(2) => (
            "battle::set_particle_uv",
            vec![i32::from((target - 8) / 2), short],
        ),
        (10, 0x28..=0x2b) => (
            "battle::set_particle_brighten",
            vec![i32::from(target - 0x28), byte],
        ),
        (10, 0x30) => ("battle::set_particle_brighten_until", vec![byte]),
        _ => anyhow::bail!("integer effect destination {target:#x} is not prepared"),
    };
    module.code.extend([Op::LoadLocal(0), Op::ArgumentValue]);
    for value in values {
        argument(module, value);
    }
    native(module, name);
    Ok(())
}
