//! Load-time translation of original effect timelines into the gameplay VM.
//!
//! Each emitting record is bound to a synchronous function in the supplied module.
//! Those functions perform the prepared particle/sound/modification operation.
//! Control records become ordinary waits, owned tasks and calls, with no effect
//! command interpreter left in the active battle.
use anyhow::{Result, ensure};
use resonance_content::battle_effect::Record;
use std::sync::Arc;
use symphonia_script::{
    Op, Program,
    authored::{BinaryOp, Function, Module, ValueLayout},
};

/// Add a parameterless timeline task to a module of resolved command functions.
/// `commands` has one slot per source record; control records must have no binding.
/// Executable output is process-local and must never be published by cooking.
pub fn prepare(
    records: &[Record],
    commands: &[Option<u16>],
    mut module: Module,
) -> Result<(Arc<Program>, u32)> {
    ensure!(
        !records.is_empty() && records.len() == commands.len(),
        "effect command bindings differ"
    );
    let mut repeats = Vec::new();
    let mut index = 0;
    while index < records.len() {
        let record = records[index];
        match record.command {
            254 => {
                ensure!(
                    commands[index].is_none() && index + 1 == records.len(),
                    "effect end must be the final record"
                );
                break;
            }
            255 => {
                ensure!(
                    commands[index].is_none(),
                    "effect repeat cannot bind a command"
                );
                ensure!(
                    records.get(index + 1).is_some_and(|r| r.command < 254),
                    "effect repeat needs a command payload"
                );
                repeats.push(index);
                index += 1;
                validate_command(&module, commands[index])?;
            }
            _ => validate_command(&module, commands[index])?,
        }
        index += 1;
    }
    ensure!(
        records.last().is_some_and(|r| r.command == 254),
        "effect timeline has no end"
    );
    ensure!(
        module.functions.len() + repeats.len() < u16::MAX as usize,
        "too many effect command functions"
    );
    let natives = resonance_battle::native_declarations();
    let mut native = |name| {
        let declaration = *natives.iter().find(|n| n.name == name).unwrap();
        if !module.natives.contains(&declaration) {
            module.natives.push(declaration);
        }
        declaration.opcode
    };
    let at_age = native("battle::at_age");
    let wait = native("battle::wait_ticks");
    let finish = native("battle::finish");
    let root = module.code.len() as u32;
    let root_function = module.functions.len();
    task(&mut module, "original_effect::timeline".into(), 0);
    let mut repeat = 0;
    index = 0;
    while index < records.len() {
        let record = records[index];
        module.code.extend([
            Op::Push(i32::from(record.age.max(0))),
            Op::ArgumentValue,
            Op::Native(at_age),
        ]);
        match record.command {
            254 => module
                .code
                .extend([Op::Native(finish), Op::ReturnValues(0)]),
            255 => {
                // Children run after the parent's due commands, in creation order.
                // A same-visit end cancels them before their first emission.
                repeat += 1;
                module
                    .code
                    .extend([Op::SpawnFunction((root_function + repeat) as u16), Op::Pop]);
                index += 1;
            }
            _ => module.code.push(Op::CallFunction(commands[index].unwrap())),
        }
        index += 1;
    }
    for index in repeats {
        task(&mut module, format!("original_effect::repeat_{index}"), 1);
        let record = records[index];
        // The native counter decrements after emitting, including a zero count.
        module.code.extend([
            Op::Push(i32::from(record.argument.max(1))),
            Op::StoreLocal(0),
        ]);
        let begin = module.code.len() as u32;
        module.code.extend([
            Op::CallFunction(commands[index + 1].unwrap()),
            Op::LoadLocal(0),
            Op::Push(1),
            Op::Binary(BinaryOp::SubI32),
            Op::StoreLocal(0),
            Op::LoadLocal(0),
            Op::Push(0),
            Op::Binary(BinaryOp::GtI32),
            Op::BranchFalseStack(0),
        ]);
        let branch = module.code.len() - 1;
        // Nonpositive intervals consume the remaining count in this visit.
        module.code.extend([
            Op::Push(i32::from((record.operand as i16).max(0))),
            Op::ArgumentValue,
            Op::Native(wait),
            Op::Jump(begin),
        ]);
        module.code[branch] = Op::BranchFalseStack(module.code.len() as u32);
        module.code.push(Op::ReturnValues(0));
    }
    Ok((Arc::new(Program::from_authored(module)?), root))
}

fn validate_command(module: &Module, command: Option<u16>) -> Result<()> {
    ensure!(
        command
            .and_then(|id| module.functions.get(usize::from(id)))
            .is_some_and(|f| !f.is_task && f.parameters == 0 && f.results == 0),
        "effect command needs a bound synchronous function without arguments or result"
    );
    Ok(())
}

fn task(module: &mut Module, name: String, locals: u16) {
    module.functions.push(Function {
        name,
        entry: module.code.len() as u32,
        parameters: 0,
        parameter_layout: ValueLayout::Sequence(vec![]),
        locals,
        results: 0,
        is_task: true,
    });
}
