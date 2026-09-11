//! Resource declarations in every script branch, independent of story progress.
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};
use symphonia_script::{NativeCall, Op, Program, scenario};

#[derive(Debug)]
pub(crate) struct Declarations {
    pub resources: BTreeSet<u32>,
    pub save_point: bool,
}

pub(crate) fn validate_cooked(
    required: &BTreeSet<u32>,
    cooked: impl IntoIterator<Item = u32>,
) -> Result<()> {
    let cooked: BTreeSet<_> = cooked.into_iter().collect();
    let missing: Vec<_> = required.difference(&cooked).collect();
    ensure!(
        missing.is_empty(),
        "uncooked declared resources {missing:#x?}; add their model/animation bindings"
    );
    Ok(())
}

/// Resolve standalone files and grouped archives through the source catalog.
pub(crate) fn source_path(executable: &[u8], files: &Path, id: u32) -> Result<String> {
    let group = id >> 16;
    let address = if group == 0 {
        ensure!((1..53).contains(&id), "unknown standalone resource {id:#x}");
        0x801f85e4 + id * 4
    } else {
        ensure!(group <= 14, "unknown resource group {group}");
        0x801f86b8 + (group - 1) * 12
    };
    let pointer = crate::read::u32(crate::dol::slice(executable, address, 4)?, 0)?;
    let path = crate::dol::text(executable, pointer)?;
    resonance_content::validate_asset_path(&path)?;
    if files.join(&path).is_file() {
        return Ok(path);
    }
    // Disc lookups ignore case, but the extracted filesystem preserves it.
    let mut actual = PathBuf::new();
    for component in path.split('/') {
        let entries = fs::read_dir(files.join(&actual))?
            .map(|entry| entry.map(|e| e.file_name()))
            .collect::<std::io::Result<Vec<_>>>()?;
        let matches: Vec<_> = entries
            .iter()
            .filter(|name| {
                name.to_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case(component))
            })
            .collect();
        ensure!(
            matches.len() == 1,
            "resource {id:#x} path {path:?} is missing or ambiguous"
        );
        actual.push(matches[0]);
    }
    Ok(actual
        .to_str()
        .context("non-UTF8 resource path")?
        .replace('\\', "/"))
}

pub(crate) fn declarations(bytes: &[u8]) -> Result<Declarations> {
    let resources = literal_calls(bytes, NativeCall::ResolveScriptResource, 1)?;
    ensure!(
        resources.iter().all(|id| *id > 0),
        "invalid resource declaration"
    );
    Ok(Declarations {
        resources: resources.into_iter().map(|id| id as u32).collect(),
        save_point: scenario::analyze(bytes)?.instructions.values().any(|i| {
            i.mnemonic == "proc" && i.operands[0] == i64::from(NativeCall::CreateSavePoint as u8)
        }),
    })
}

/// First argument of calls whose complete argument list consists of literals.
/// Complex expressions require an explicit recipe instead of a guessed value.
pub(crate) fn literal_calls(
    bytes: &[u8],
    call: NativeCall,
    arguments: usize,
) -> Result<BTreeSet<i32>> {
    literal_arguments(bytes, call, arguments)?
        .into_iter()
        .map(|args| {
            ensure!(
                args.iter().all(Option::is_some),
                "dynamic {call:?} arguments need a cooking recipe"
            );
            Ok(args[0].unwrap())
        })
        .collect()
}

/// Resolve literal arguments independently; a dynamic duration need not obscure
/// a static resource ID. None never means a guessed/default resource.
pub(crate) fn literal_arguments(
    bytes: &[u8],
    call: NativeCall,
    arguments: usize,
) -> Result<BTreeSet<Vec<Option<i32>>>> {
    ensure!(
        (1..=16).contains(&arguments),
        "invalid native argument count"
    );
    let program = Program::decode(bytes)?;
    let analysis = scenario::analyze(bytes)?;
    let mut values = BTreeSet::new();
    for block in analysis.basic_blocks() {
        let ops: Vec<_> = block
            .instruction_pcs
            .iter()
            .map(|pc| (*pc, program.instruction(*pc).unwrap().0))
            .collect();
        for (index, &(pc, op)) in ops.iter().enumerate() {
            if op != Op::Native(call as u8) {
                continue;
            }
            let start = ops[..index]
                .iter()
                .rposition(|(_, op)| matches!(op, Op::Native(_)))
                .map_or(0, |i| i + 1);
            let ends: Vec<_> = (start..index)
                .filter(|&i| ops[i].1 == Op::Argument)
                .collect();
            ensure!(
                ends.len() == arguments,
                "dynamic {call:?} arguments at PC {pc:#x} need a cooking recipe"
            );
            values.insert(
                ends.into_iter()
                    .map(|end| match &ops[end.saturating_sub(2)..end] {
                        [(_, Op::Push(value)), (_, Op::Calculate(0))] => Some(*value),
                        _ => None,
                    })
                    .collect(),
            );
        }
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventories_both_branches_and_rejects_dynamic_resource_ids() {
        let source = r".scenario
.code_base 4
.word 4
.word 0
.word 0
.word 0
 push.s8 0
 calc 0
 branch_false other
 push.s32 0x2000e
 calc 0
 arg
 proc 0x98
 calc 0
 jump done
other:
 push.s32 0x2001c
 calc 0
 arg
 proc 0x98
 calc 0
done:
 end
";
        let bytes = scenario::assemble(source).unwrap();
        let declared = declarations(&bytes).unwrap();
        assert_eq!(declared.resources, [0x2000e, 0x2001c].into());
        validate_cooked(&declared.resources, [0x2000e, 0x2001c]).unwrap();
        assert!(
            validate_cooked(&declared.resources, [0x2000e])
                .unwrap_err()
                .to_string()
                .contains("0x2001c")
        );
        let dynamic =
            scenario::assemble(&source.replace("push.s32 0x2001c", "load.s32 0x800")).unwrap();
        assert!(
            declarations(&dynamic)
                .unwrap_err()
                .to_string()
                .contains("dynamic ResolveScriptResource")
        );
        let audio = scenario::assemble(
            &source.replace("proc 0x98", "push.s8 64\n calc 0\n arg\n proc 0x52"),
        )
        .unwrap();
        assert_eq!(
            literal_calls(&audio, NativeCall::PlaySoundSimple, 2).unwrap(),
            [0x2000e, 0x2001c].into()
        );
        let expression = scenario::assemble(&source.replace(
            "proc 0x98",
            "push.s8 25\n push.s8 7\n calc 0x33\n calc 0\n arg\n proc 0x52",
        ))
        .unwrap();
        assert_eq!(
            literal_arguments(&expression, NativeCall::PlaySoundSimple, 2).unwrap(),
            [vec![Some(0x2000e), None], vec![Some(0x2001c), None]].into()
        );
        assert!(literal_calls(&expression, NativeCall::PlaySoundSimple, 2).is_err());
    }
}
