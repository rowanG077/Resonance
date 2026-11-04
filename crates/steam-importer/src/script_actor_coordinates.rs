//! Reflect source actor X/Y tilt into the same basis as the cooked models.
//! Geometry and animation already reflect Steam's model Z axis. Native actor
//! properties 35/36 use that model basis too; world position/heading do not.
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;
use symphonia_script::{NativeCall, Op, Program, scenario, semantics::NativeRegistry};

const RESULT: u16 = 0x20;

#[derive(Clone)]
struct Replay {
    words: Vec<u16>,
    end: u32,
    memory: bool,
}

pub(super) fn normalize(bytes: &[u8]) -> Result<Vec<u8>> {
    let analysis = scenario::analyze(bytes)?;
    let program = Program::decode(bytes)?;
    let registry = NativeRegistry::gqseaf();
    let calls = [
        (NativeCall::GetActorProperty, 2),
        (NativeCall::SetActorProperty, 3),
    ];
    let known = calls
        .iter()
        .map(|&(call, arity)| {
            Ok((
                call as u8,
                scenario::calls::analyzed_arguments(&program, &analysis, call, arity)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let words = |start: u32, end: u32| {
        let base = analysis.header.code_base();
        bytes[base + start as usize * 2..base + end as usize * 2]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|w| u16::from_be_bytes(*w))
            .collect::<Vec<_>>()
    };
    let mut replacements = BTreeMap::new();
    for block in analysis.basic_blocks() {
        let mut values: Vec<Option<Replay>> = Vec::new();
        let mut expression = None;
        let mut expression_end = None;
        let mut arguments = Vec::new();
        for pc in block.instruction_pcs {
            let (op, next) = program.instruction(pc).unwrap();
            match op {
                Op::Push(_) | Op::Load(_, _) => values.push(Some(Replay {
                    words: words(pc, next),
                    end: next,
                    memory: matches!(op, Op::Load(_, _)),
                })),
                Op::Calculate(0) => {
                    expression = values.pop().flatten();
                    expression_end = Some(pc);
                }
                Op::Calculate(op @ 1..=8) => {
                    if op <= 4 {
                        for value in &mut values {
                            if value.as_ref().is_some_and(|v| v.memory) {
                                *value = None;
                            }
                        }
                    }
                    let span = values
                        .pop()
                        .flatten()
                        .filter(|s| op >= 5 && s.end == pc)
                        .map(|mut s| {
                            s.words.extend(words(pc, next));
                            s.end = next;
                            s
                        });
                    values.push(span);
                }
                Op::Calculate(op) => {
                    if (0x10..=0x1a).contains(&op) {
                        for value in &mut values {
                            if value.as_ref().is_some_and(|v| v.memory) {
                                *value = None;
                            }
                        }
                    }
                    let right = values.pop().flatten();
                    let left = values.pop().flatten();
                    let pure =
                        op == 0x0f || (0x20..=0x27).contains(&op) || (0x31..=0x3a).contains(&op);
                    values.push(left.zip(right).filter(|(_, r)| pure && r.end == pc).map(
                        |(mut l, r)| {
                            l.words.extend(r.words);
                            l.words.extend(words(pc, next));
                            l.end = next;
                            l.memory |= r.memory;
                            l
                        },
                    ));
                }
                Op::Argument => arguments.push((pc, expression.clone(), expression_end)),
                Op::Native(call) => {
                    let Some(signature) = registry.get(call) else {
                        values.clear();
                        arguments.clear();
                        expression = None;
                        expression_end = None;
                        continue;
                    };
                    if let Some(&(_, arity)) =
                        calls.iter().find(|(native, _)| *native as u8 == call)
                    {
                        let start = arguments
                            .len()
                            .checked_sub(arity)
                            .context("actor property arguments cross a branch")?;
                        let property = known[&call][&pc][1];
                        if property.is_none_or(|p| matches!(p, 35 | 36)) {
                            let (argument, origin, _) = &arguments[start + 1];
                            let factor =
                                if property.is_some() {
                                    vec![0x00ff]
                                } else {
                                    let source = origin.as_ref().with_context(|| {
                                    format!("actor property at PC {pc:#x} is not a pure expression")
                                })?.words.clone();
                                    // 1 - 2 * (property == 35 || property == 36).
                                    let mut code = vec![1];
                                    code.extend_from_slice(&source);
                                    code.extend([35, 0x3020]);
                                    code.extend(source);
                                    code.extend([36, 0x3020, 0x3027, 2, 0x3033, 0x3032]);
                                    code
                                };
                            // Keep a return-cell reference and the sampled reflection
                            // factor below subsequent source expressions. No source
                            // memory or expression register is used as scratch.
                            let mut code = vec![0x4000, 0, 0x1200, RESULT];
                            code.extend_from_slice(&factor);
                            if arity == 3 {
                                code.extend_from_slice(&factor);
                            }
                            ensure!(
                                replacements.insert(*argument, code).is_none(),
                                "actor property argument has conflicting owners"
                            );
                            if arity == 3 {
                                let (value_arg, value_origin, end) = &arguments[start + 2];
                                // A following expression-end immediately replaces
                                // the argument expression with the native return.
                                // In that case even an impure source value needs no
                                // replay: reflect its existing scalar exactly once.
                                let expression_discarded = matches!(
                                    program.instruction(next),
                                    Some((Op::Calculate(0), _))
                                );
                                let original = value_origin.as_ref().map(|v| &v.words);
                                ensure!(
                                    original.is_some() || expression_discarded,
                                    "actor tilt value at PC {pc:#x} is not a pure expression and its expression remains live"
                                );
                                let end = end.context("actor tilt value has no expression end")?;
                                ensure!(
                                    program.instruction(end)
                                        == Some((Op::Calculate(0), *value_arg)),
                                    "actor tilt value is not adjacent to its argument"
                                );
                                ensure!(
                                    replacements.insert(end, vec![0x3033, 0x3000]).is_none(),
                                    "actor tilt expression has conflicting owners"
                                );
                                let mut code = vec![0x4000];
                                if let Some(original) = original {
                                    code.extend_from_slice(original);
                                    code.push(0x3000);
                                }
                                ensure!(
                                    replacements.insert(*value_arg, code).is_none(),
                                    "actor tilt argument has conflicting owners"
                                );
                            }
                            // Return the source-basis previous value both on the
                            // value stack and in the VM's implicit return cell.
                            replacements
                                .insert(pc, vec![0x2000 | u16::from(call), 0x3033, 0x3010, 0x3031]);
                        }
                    }
                    arguments.truncate(arguments.len().saturating_sub(signature.arguments.len()));
                    // A native's return cell can replay its scalar without
                    // repeating that native (for example ConfigureSession*15).
                    // A later native can modify memory, invalidating older
                    // memory-dependent values still being assembled.
                    for value in &mut values {
                        if value.as_ref().is_some_and(|v| v.memory) {
                            *value = None;
                        }
                    }
                    match signature.returns_value {
                        Some(true) => values.push(Some(Replay {
                            words: vec![0x1200, RESULT],
                            end: next,
                            memory: true,
                        })),
                        Some(false) => (),
                        None => values.clear(),
                    }
                    expression = None;
                    expression_end = None;
                }
                _ => {
                    values.clear();
                    arguments.clear();
                    expression = None;
                    expression_end = None;
                }
            }
        }
    }
    crate::script::rewrite(bytes, &analysis, replacements)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use symphonia_script::Width;
    use symphonia_script_vm::{Host, Memory, NativeBindings, NativeResult, RunEvent, Vm};

    #[derive(Default)]
    struct ActorHost {
        value: i32,
        calls: Vec<Vec<i32>>,
        session_reads: usize,
        random_reads: usize,
    }
    impl Host for ActorHost {
        const NATIVES: NativeBindings<Self> = NativeBindings::<Self>::new()
            .register(
                NativeCall::SetActorProperty as u8,
                3,
                true,
                |host, args, _| {
                    host.calls.push(args.to_vec());
                    let old = std::mem::replace(&mut host.value, args[2]);
                    Ok(NativeResult::Continue(Some(old)))
                },
            )
            .register(
                NativeCall::GetActorProperty as u8,
                2,
                true,
                |host, args, _| {
                    host.calls.push(args.to_vec());
                    Ok(NativeResult::Continue(Some(host.value)))
                },
            )
            .register(NativeCall::ConfigureSession as u8, 2, true, |host, _, _| {
                host.session_reads += 1;
                Ok(NativeResult::Continue(Some(3)))
            })
            .register(NativeCall::RandomMod as u8, 1, true, |host, _, _| {
                host.random_reads += 1;
                Ok(NativeResult::Continue(Some(4)))
            });
    }
    fn fixture(code: Vec<u16>) -> Vec<u8> {
        [4u16, 0, 0, 0]
            .into_iter()
            .chain(code)
            .flat_map(u16::to_be_bytes)
            .collect()
    }
    fn run(code: Vec<u16>, property: i32, old: i32) -> Result<(ActorHost, Memory, Vm)> {
        let bytes = normalize(&fixture(code))?;
        let program = Arc::new(Program::decode(&bytes)?);
        let mut vm = Vm::new(program.clone(), program.entry())?;
        let mut memory = Memory::default();
        memory.write(0x100, Width::S32, 1234)?;
        memory.write(0x104, Width::S32, property)?;
        memory.write(0x108, Width::S32, -75)?;
        let mut host = ActorHost {
            value: old,
            ..Default::default()
        };
        assert_eq!(
            vm.run(&mut host, &mut memory, 1000)?.event,
            RunEvent::Halted
        );
        Ok((host, memory, vm))
    }
    fn source(set: bool, dynamic: bool, property: u16) -> Vec<u16> {
        let mut code = vec![
            99, 0x3000, 0x4000, // pending outer native argument
            0x1200, 0x100, // pending outer assignment reference
            7, 0x3000, 0x4000,
        ];
        if dynamic {
            code.extend([0x1200, 0x104]);
        } else {
            code.push(property);
        }
        code.extend([0x3000, 0x4000]);
        if set {
            code.extend([0x1200, 0x108, 1, 0x3033, 0x3000, 0x4000]);
        }
        code.extend([if set { 0x201d } else { 0x201c }, 0x20ff]);
        code
    }
    #[test]
    fn reflected_setter_preserves_source_return_expression_and_caller_stacks() -> Result<()> {
        for dynamic in [false, true] {
            for property in [35, 36, 8] {
                let reflected = matches!(property, 35 | 36);
                let (host, memory, vm) =
                    run(source(true, dynamic, property), i32::from(property), 12)?;
                assert_eq!(
                    host.calls,
                    [vec![
                        7,
                        i32::from(property),
                        if reflected { 75 } else { -75 }
                    ]]
                );
                assert_eq!(host.value, if reflected { 75 } else { -75 });
                assert_eq!(
                    memory.read(RESULT, Width::S32)?,
                    if reflected { -12 } else { 12 }
                );
                assert_eq!(memory.read(0x100, Width::S32)?, 1234);
                assert_eq!(memory.read(0x108, Width::S32)?, -75);
                assert_eq!(vm.expression(), Some(-75));
                assert_eq!((vm.value_depth(), vm.argument_depth()), (2, 1));
            }
        }
        Ok(())
    }
    #[test]
    fn reflected_getter_preserves_source_return_and_property_expression() -> Result<()> {
        for dynamic in [false, true] {
            for property in [35, 36, 8] {
                let (host, memory, vm) =
                    run(source(false, dynamic, property), i32::from(property), 75)?;
                assert_eq!(host.calls, [vec![7, i32::from(property)]]);
                assert_eq!(
                    memory.read(RESULT, Width::S32)?,
                    if matches!(property, 35 | 36) { -75 } else { 75 }
                );
                assert_eq!(vm.expression(), Some(i32::from(property)));
                assert_eq!(memory.read(0x100, Width::S32)?, 1234);
                assert_eq!((vm.value_depth(), vm.argument_depth()), (2, 1));
            }
        }
        Ok(())
    }
    #[test]
    fn known_noncoordinate_property_is_byte_identical() -> Result<()> {
        let bytes = fixture(source(true, false, 8));
        assert_eq!(normalize(&bytes)?, bytes);
        Ok(())
    }
    #[test]
    fn impure_dynamic_property_is_rejected_without_repeating_side_effects() {
        let mut code = source(false, true, 35);
        let at = code.windows(2).position(|w| w == [0x1200, 0x104]).unwrap();
        code.insert(at + 2, 0x3001); // source property++ must never execute twice
        assert!(
            normalize(&fixture(code))
                .unwrap_err()
                .to_string()
                .contains("not a pure expression")
        );
    }
    #[test]
    fn native_value_is_sampled_once_before_coordinate_conversion() -> Result<()> {
        // Installed field270: ConfigureSession(11,0)*15%360.
        let mut code = source(true, false, 35);
        let start = code.windows(2).position(|w| w == [0x1200, 0x108]).unwrap();
        code.splice(
            start..start + 4,
            [
                11, 0x3000, 0x4000, 0, 0x3000, 0x4000, 0x206d, 15, 0x3033, 0x0100, 360, 0x3035,
            ],
        );
        let (host, memory, vm) = run(code, 35, 12)?;
        assert_eq!(host.session_reads, 1);
        assert_eq!(host.calls, [vec![7, 35, -45]]);
        assert_eq!(memory.read(RESULT, Width::S32)?, -12);
        assert_eq!(vm.expression(), Some(45));
        assert_eq!((vm.value_depth(), vm.argument_depth()), (2, 1));
        Ok(())
    }
    #[test]
    fn nested_native_value_with_discarded_expression_is_evaluated_once() -> Result<()> {
        // Installed field408: SetActorProperty(actor,35,
        // GetActorProperty(actor,35)+RandomMod(10)); expression-end.
        let mut code = source(true, false, 35);
        let start = code.windows(2).position(|w| w == [0x1200, 0x108]).unwrap();
        code.splice(
            start..start + 4,
            [
                7, 0x3000, 0x4000, 35, 0x3000, 0x4000, 0x201c, 10, 0x3000, 0x4000, 0x2065, 0x3031,
            ],
        );
        code.insert(code.len() - 1, 0x3000);
        let (host, memory, vm) = run(code, 35, 12)?;
        assert_eq!(host.random_reads, 1);
        assert_eq!(host.calls, [vec![7, 35], vec![7, 35, 8]]);
        assert_eq!(host.value, 8);
        assert_eq!(memory.read(RESULT, Width::S32)?, -12);
        assert_eq!(memory.read(0x100, Width::S32)?, 1234);
        assert_eq!(vm.expression(), Some(-12));
        assert_eq!((vm.value_depth(), vm.argument_depth()), (1, 1));
        Ok(())
    }
    #[test]
    #[ignore = "requires installed Steam and library; CPU-only raw field source census/private184 script"]
    fn installed_actor_coordinate_source_census_and_private184() -> Result<()> {
        use crate::{package::Package, pe, source::Source};
        use std::{fs, path::PathBuf};
        let source = Source::open(
            &PathBuf::from(
                std::env::var_os("RESONANCE_TEST_STEAM").context("set RESONANCE_TEST_STEAM")?,
            ),
            crate::Language::English,
        )?;
        let root = PathBuf::from(
            std::env::var_os("RESONANCE_TEST_LIBRARY").context("set RESONANCE_TEST_LIBRARY")?,
        );
        let fields: resonance_content::library::Mappings =
            serde_json::from_slice(&fs::read(root.join(resonance_content::library::FIELDS))?)?;
        let image = pe::Image::parse(&source.executable)?;
        let mut changed = Vec::new();
        let mut failures = Vec::new();
        for (id, name) in fields.iter() {
            let pointer = pe::word(image.slice(0x8cb908 + id * 24, 4)?, 0)?;
            let package_bytes = source.require(image.text(pointer)?)?;
            let package = Package::parse(&package_bytes)?;
            let native = package.reference(6)?.context("field has no scenario")?;
            let (bytes, encoding) = source.scenario(native)?;
            let decoded = crate::script::decode(&bytes, encoding)?;
            let normalized = match normalize(&decoded.bytes) {
                Ok(normalized) => normalized,
                Err(error) => {
                    failures.push(format!("field{id} {name} {native}: {error:#}"));
                    continue;
                }
            };
            if normalized != decoded.bytes {
                changed.push((id, name.to_owned()));
            }
            if id == 184
                && let Some(out) = std::env::var_os("RESONANCE_ACTOR_COORDINATES_OUTPUT")
            {
                let bytes = crate::script_input::normalize(&decoded.bytes)?;
                crate::output::write(
                    &PathBuf::from(out).join(format!("fields/{name}/scripts/scenario.ssb")),
                    &bytes,
                )?;
            }
        }
        eprintln!(
            "actor-coordinate source fields changed: {}",
            serde_json::to_string(&changed)?
        );
        ensure!(
            failures.is_empty(),
            "actor-coordinate source census failures ({}):\n{}",
            failures.len(),
            failures.join("\n")
        );
        Ok(())
    }
}
