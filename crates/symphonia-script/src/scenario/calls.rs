//! Observe native arguments across every decoded event branch. Unknown values
//! remain unknown; this analysis never executes a script or guesses saved state.
use crate::{NativeCall, Op, Program, ProgramError, scenario, semantics::NativeRegistry};
use std::collections::BTreeMap;

mod expressions;
pub use expressions::finite_arguments;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Program(#[from] ProgramError),
    #[error(transparent)]
    Scenario(#[from] scenario::Error),
    #[error("incorrect {call:?} argument count {arguments}")]
    Signature { call: NativeCall, arguments: usize },
    #[error("unresolved {call:?} argument stack at PC {pc:#x}")]
    Arguments { call: NativeCall, pc: u32 },
}

pub fn arguments(
    bytes: &[u8],
    call: NativeCall,
    count: usize,
) -> Result<BTreeMap<u32, Vec<Option<i32>>>, Error> {
    program_arguments(bytes, call, count).map(|(_, arguments)| arguments)
}

/// Return the validated code used by dependency discovery for later execution.
pub fn program_arguments(
    bytes: &[u8],
    call: NativeCall,
    count: usize,
) -> Result<(Program, BTreeMap<u32, Vec<Option<i32>>>), Error> {
    let (program, analysis) = Program::decode_with_analysis(bytes)?;
    let arguments = analyzed_arguments(&program, &analysis, call, count)?;
    Ok((program, arguments))
}

/// `analysis` must describe the same validated legacy program as `program`.
pub fn analyzed_arguments(
    program: &Program,
    analysis: &scenario::Analysis,
    call: NativeCall,
    arguments: usize,
) -> Result<BTreeMap<u32, Vec<Option<i32>>>, Error> {
    let registry = NativeRegistry::gqseaf();
    if !(1..=16).contains(&arguments)
        || registry
            .get(call as u8)
            .is_none_or(|signature| signature.arguments.len() != arguments)
    {
        return Err(Error::Signature { call, arguments });
    }
    let mut values = BTreeMap::new();
    for block in analysis.basic_blocks() {
        let mut expressions = expressions::Expressions::default();
        for pc in block.instruction_pcs {
            let op = program.instruction(pc).unwrap().0;
            if op == Op::Native(call as u8) {
                values.insert(
                    pc,
                    expressions
                        .arguments(arguments)
                        .ok_or(Error::Arguments { call, pc })?,
                );
            }
            expressions.step(op, &registry);
        }
    }
    Ok(values)
}
