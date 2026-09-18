//! Resource declarations in every script branch, independent of story progress.
pub(crate) mod binding;
mod expressions;
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
use symphonia_script::{NativeCall, Op, Program, scenario, semantics::NativeRegistry};

#[derive(Debug)]
pub(crate) struct Declarations {
    pub resources: BTreeSet<u32>,
    pub save_point: bool,
    /// Native call PCs whose resource selector depends on runtime input.
    pub dynamic: BTreeSet<u32>,
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

/// Disc lookups ignore case, but the extracted filesystem preserves it.
pub(crate) fn resolve_path(files: &Path, path: &str) -> Result<String> {
    find_path(files, path)?.with_context(|| format!("missing resource path {path:?}"))
}

/// Missing declarations are distinct from unreadable or ambiguous source paths.
pub(crate) fn find_path(files: &Path, path: &str) -> Result<Option<String>> {
    resonance_content::validate_asset_path(path)?;
    ensure!(
        path.split('/').all(|part| !part.is_empty()),
        "empty resource path component"
    );
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
        if matches.is_empty() {
            return Ok(None);
        }
        ensure!(matches.len() == 1, "ambiguous resource path {path:?}");
        actual.push(matches[0]);
    }
    Ok(Some(
        actual
            .to_str()
            .context("non-UTF8 resource path")?
            .replace('\\', "/"),
    ))
}

pub(crate) fn declarations(bytes: &[u8]) -> Result<Declarations> {
    let program = Program::decode(bytes)?;
    let analysis = scenario::analyze(bytes)?;
    let call = NativeCall::ResolveScriptResource;
    let arguments = call_arguments(&program, &analysis, call, 1)?;
    let finite = if arguments.values().any(|args| args[0].is_none()) {
        expressions::finite_arguments(&program, &analysis, call)
    } else {
        BTreeMap::new()
    };
    let mut resources = BTreeSet::new();
    let mut dynamic = BTreeSet::new();
    for (pc, args) in arguments {
        if let Some(value) = args[0] {
            resources.insert(value);
        } else if let Some(values) = finite.get(&pc) {
            resources.extend(values);
        } else {
            dynamic.insert(pc);
        }
    }
    ensure!(
        resources.iter().all(|id| *id > 0),
        "invalid resource declaration"
    );
    Ok(Declarations {
        resources: resources.into_iter().map(|id| id as u32).collect(),
        dynamic,
        save_point: analysis.instructions.keys().any(|&pc| {
            program
                .instruction(pc)
                .is_some_and(|(op, _)| op == Op::Native(NativeCall::CreateSavePoint as u8))
        }),
    })
}

/// Resolve constant arguments independently; a dynamic duration need not obscure
/// a static resource ID. None never means a guessed/default resource.
pub(crate) fn literal_arguments(
    bytes: &[u8],
    call: NativeCall,
    arguments: usize,
) -> Result<BTreeSet<Vec<Option<i32>>>> {
    let program = Program::decode(bytes)?;
    let analysis = scenario::analyze(bytes)?;
    Ok(call_arguments(&program, &analysis, call, arguments)?
        .into_values()
        .collect())
}

fn call_arguments(
    program: &Program,
    analysis: &scenario::Analysis,
    call: NativeCall,
    arguments: usize,
) -> Result<BTreeMap<u32, Vec<Option<i32>>>> {
    ensure!(
        (1..=16).contains(&arguments),
        "invalid native argument count"
    );
    let registry = NativeRegistry::gqseaf();
    ensure!(
        registry.get(call as u8).unwrap().arguments.len() == arguments,
        "incorrect {call:?} argument count"
    );
    let mut values = BTreeMap::new();
    for block in analysis.basic_blocks() {
        let mut expressions = expressions::Expressions::default();
        for pc in block.instruction_pcs {
            let op = program.instruction(pc).unwrap().0;
            if op == Op::Native(call as u8) {
                values.insert(
                    pc,
                    expressions.arguments(arguments).with_context(|| {
                        format!("dynamic {call:?} arguments at PC {pc:#x} need a cooking recipe")
                    })?,
                );
            }
            expressions.step(op, &registry);
        }
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires both original discs; checks every declared map without cooking or playback"]
    fn original_field_resource_declarations_cover_both_discs() -> Result<()> {
        let mut unique = BTreeSet::new();
        let mut failures = Vec::new();
        let mut dynamic = BTreeMap::new();
        for disc in [1, 2] {
            let extracted = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../local/extracted/disc{disc}"));
            for path in crate::field_catalogue::map_paths(&extracted)? {
                let source = extracted.join("files").join(&path);
                let bytes = fs::read(&source)?;
                if !unique.insert(crate::digest(&bytes)) {
                    continue;
                }
                let result = crate::field::MapArchive::open(&source)
                    .and_then(|map| declarations(map.section(6)?));
                match result {
                    Ok(declared) if !declared.dynamic.is_empty() => {
                        dynamic.insert(path, declared.dynamic);
                    }
                    Ok(_) => (),
                    Err(error) => failures.push(format!("disc {disc} {path}: {error:#}")),
                }
            }
        }
        println!(
            "Checked {} distinct field archives; runtime selectors: {dynamic:#x?}",
            unique.len()
        );
        ensure!(
            dynamic.len() == 2,
            "unexpected runtime resource selector coverage"
        );
        ensure!(failures.is_empty(), "{}", failures.join("\n"));
        Ok(())
    }

    #[test]
    fn source_lookup_distinguishes_missing_files_from_invalid_paths() -> Result<()> {
        let root = crate::temporary_path(&std::env::temp_dir().join("resonance-source-paths"));
        fs::create_dir_all(root.join("BTL"))?;
        fs::write(root.join("BTL/Model.bin"), [])?;
        assert_eq!(
            find_path(&root, "btl/model.bin")?,
            Some("BTL/Model.bin".into())
        );
        assert_eq!(find_path(&root, "missing/model.bin")?, None);
        assert!(resolve_path(&root, "BTL/missing.bin").is_err());
        assert!(find_path(&root, "../model.bin").is_err());
        assert!(find_path(&root, "BTL//Model.bin").is_err());
        fs::write(root.join("BTL/MODEL.bin"), [])?;
        assert!(find_path(&root, "btl/model.bin").is_err());
        assert!(find_path(&root, "BTL/Model.bin").is_err());
        assert!(find_path(&root.join("BTL/Model.bin"), "model.bin").is_err());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn inventories_static_branches_and_preserves_runtime_resource_selectors() {
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
        assert!(!declared.save_point);
        let with_save_point = scenario::assemble(&source.replace(
            "done:\n end",
            &format!("done:\n proc {}\n end", NativeCall::CreateSavePoint as u8),
        ))
        .unwrap();
        assert!(declarations(&with_save_point).unwrap().save_point);
        validate_cooked(&declared.resources, [0x2000e, 0x2001c]).unwrap();
        assert!(
            validate_cooked(&declared.resources, [0x2000e])
                .unwrap_err()
                .to_string()
                .contains("0x2001c")
        );
        let dynamic =
            scenario::assemble(&source.replace("push.s32 0x2001c", "load.s32 0x800")).unwrap();
        let dynamic = declarations(&dynamic).unwrap();
        assert_eq!(dynamic.resources, [0x2000e].into());
        assert_eq!(dynamic.dynamic.len(), 1);
        assert!(declared.dynamic.is_empty());
        let audio = scenario::assemble(
            &source.replace("proc 0x98", "push.s8 64\n calc 0\n arg\n proc 0x52"),
        )
        .unwrap();
        assert_eq!(
            literal_arguments(&audio, NativeCall::PlaySoundSimple, 2).unwrap(),
            [vec![Some(0x2000e), Some(64)], vec![Some(0x2001c), Some(64)]].into()
        );
        let expression = scenario::assemble(&source.replace(
            "proc 0x98",
            "push.s8 25\n push.s8 7\n calc 0x33\n calc 0\n arg\n proc 0x52",
        ))
        .unwrap();
        assert_eq!(
            literal_arguments(&expression, NativeCall::PlaySoundSimple, 2).unwrap(),
            [
                vec![Some(0x2000e), Some(175)],
                vec![Some(0x2001c), Some(175)]
            ]
            .into()
        );
    }

    #[test]
    fn constants_follow_native_integer_and_nested_argument_semantics() {
        let header = ".scenario\n.code_base 4\n.word 4\n.word 0\n.word 0\n.word 0\n";
        for (expression, expected) in [
            (
                "push.s32 196608\n push.s8 37\n calc 0x31\n push.s32 65536\n push.s8 0\n calc 0x33\n calc 0x31",
                Some(0x30025),
            ),
            (
                "push.s32 196608\n push.s8 73\n calc 0x31\n push.s32 65536\n push.s8 2\n calc 0x33\n calc 0x31",
                Some(0x50049),
            ),
            ("push.s32 68400\n push.s32 68400\n calc 0x10", Some(68400)),
            ("push.s32 68400\n calc 0x10", None),
            (
                "push.s32 2147483647\n push.s8 1\n calc 0x31",
                Some(i32::MIN),
            ),
            ("push.s8 -7\n push.s8 2\n calc 0x34", Some(-3)),
            ("push.s8 -7\n push.s8 2\n calc 0x35", Some(-1)),
            ("push.s8 1\n push.s8 32\n calc 0x39", Some(0)),
            ("push.s8 -1\n push.s8 32\n calc 0x3a", Some(-1)),
            ("push.s8 1\n push.s8 64\n calc 0x39", Some(1)),
            ("push.s8 1\n push.s8 0\n calc 0x34", None),
            ("load.s32 0x800\n push.s32 68400\n calc 0x10", Some(68400)),
            (
                "load.s32 0x800\n push.s8 2\n calc 0x10\n push.s8 0\n calc 0x0f",
                None,
            ),
            ("load.s32 0x800\n push.s8 0\n calc 0x33", None),
        ] {
            let bytes = scenario::assemble(&format!(
                "{header}{expression}\n calc 0\n arg\n proc 0x50\n end\n"
            ))
            .unwrap();
            assert_eq!(
                literal_arguments(&bytes, NativeCall::AudioCommand, 1).unwrap(),
                [vec![expected]].into(),
                "{expression}"
            );
        }
        let nested = scenario::assemble(&format!(
            "{header}push.s8 105\n calc 0\n arg\n push.s8 7\n calc 0\n arg\n \
             proc 0x6a\n calc 0\n arg\n proc 0x52\n end\n"
        ))
        .unwrap();
        assert_eq!(
            literal_arguments(&nested, NativeCall::PlaySoundSimple, 2).unwrap(),
            [vec![Some(105), None]].into()
        );
    }

    #[test]
    #[ignore = "requires both original discs; reads scripts without cooking or playback"]
    fn original_computed_resource_declarations_are_constant_on_both_discs() -> Result<()> {
        for disc in [1, 2] {
            let root = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../local/extracted/disc{disc}/files/MAP"));
            for (name, expected) in [
                ("elc_d14.bin", &[0x30025, 0x50049][..]),
                ("faa_d03.bin", &[68400][..]),
                ("tre_d05.bin", &[68352][..]),
                ("fac_d03.bin", &[68240][..]),
            ] {
                let map = crate::field::MapArchive::open(&root.join(name))?;
                let resources = declarations(map.section(6)?)?.resources;
                ensure!(
                    expected.iter().all(|id| resources.contains(id)),
                    "missing computed declarations in disc {disc} {name}"
                );
            }
        }
        Ok(())
    }

    fn party_lookup(helper: bool) -> String {
        let resource = "push.s32 0x30060\n push.s32 65536\n load.s32 0x904\n calc 0x33\n calc 0x31\n calc 0\n arg\n proc 0x98\n calc 0\n end\n";
        let mut source =
            ".scenario\n.code_base 4\n.word 4\n.word 0\n.word 0\n.word 0\nstart:\n".to_owned();
        if helper {
            source.push_str(&format!("call lookup\nresource:\n{resource}"));
        }
        let continuation = if helper { "returned" } else { "resource" };
        source.push_str(&format!(
            "lookup:\n load.s32 0x900\n push.s8 -1\n calc 0\n arg\n proc 0x22\n calc 0x10\n calc 0\n jump case1\njoin:\n jump {continuation}\n"
        ));
        for member in 1..=9 {
            let next = if member == 9 {
                continuation.to_owned()
            } else {
                format!("case{}", member + 1)
            };
            source.push_str(&format!(
                "case{member}:\n load.s32 0x900\n push.s8 {member}\n calc 0x20\n calc 0\n branch_false {next}\n load.s32 0x904\n push.s8 {}\n calc 0x10\n calc 0\n jump join\n",
                member - 1
            ));
        }
        source.push_str(&format!("{continuation}:\n"));
        source.push_str(if helper { "ret\n" } else { resource });
        source
    }

    #[test]
    fn finite_party_lookups_require_complete_paths_and_preserve_aliases() -> Result<()> {
        let expected: BTreeSet<_> = (0..9).map(|member| 0x30060 + 65536 * member).collect();
        for helper in [false, true] {
            let source = party_lookup(helper);
            let bytes = scenario::assemble(&source)?;
            let declared = declarations(&bytes)?;
            assert_eq!(declared.resources, expected);
            assert!(declared.dynamic.is_empty());
            assert_eq!(
                literal_arguments(&bytes, NativeCall::ResolveScriptResource, 1)?,
                [vec![None]].into()
            );

            // A known byte write changes the later signed word read too.
            let aliased = source.replace(
                "resource:\n",
                "resource:\n load.s8 0x907\n push.s8 3\n calc 0x10\n calc 0\n",
            );
            assert_eq!(
                declarations(&scenario::assemble(&aliased)?)?.resources,
                [0x60060].into()
            );

            // One party member takes a separate literal resource call.
            let split = format!(
                "{}\nfallback:\n push.s32 0x10302\n calc 0\n arg\n proc 0x98\n calc 0\n end\n",
                source.replace("resource:\n", "resource:\n load.s32 0x904\n push.s8 3\n calc 0x21\n calc 0\n branch_false fallback\n")
            );
            let bytes = scenario::assemble(&split)?;
            let subset: BTreeSet<_> = expected
                .iter()
                .copied()
                .filter(|id| *id != 0x60060)
                .collect();
            assert_eq!(
                declarations(&bytes)?.resources,
                subset.iter().copied().chain([0x10302]).collect()
            );
            let finite = expressions::finite_arguments(
                &Program::decode(&bytes)?,
                &scenario::analyze(&bytes)?,
                NativeCall::ResolveScriptResource,
            );
            assert_eq!(
                finite.into_values().collect::<BTreeSet<_>>(),
                [
                    subset.into_iter().map(|id| id as i32).collect(),
                    [0x10302].into()
                ]
                .into()
            );

            for uncertain in [
                // The ninth party member can reach the call without an assignment.
                source.replace("push.s8 9\n", "push.s8 10\n"),
                source.replace("resource:\n", "resource:\n load.s8 0x907\n load.s32 0x1000\n calc 0x10\n calc 0\n"),
                source.replace("resource:\n", "resource:\n load.s32 0x904\n load.s32 0x1000\n calc 0x0f\n push.s8 0\n calc 0x10\n calc 0\n"),
                source.replace("resource:\n", "resource:\n push.s8 0\n calc 0\n arg\n proc 0x50\n"),
                source.replace("resource:\n", "resource:\n load.s32 0x1000\n calc 0\n branch_false uncertain\nuncertain:\n"),
                source.replace("start:\n", "start:\n load.s32 0x1000\n calc 0\n branch_false resource\n"),
            ] {
                let bytes = scenario::assemble(&uncertain.replace(
                    "start:\n",
                    "start:\n push.s32 0x10302\n calc 0\n arg\n proc 0x98\n calc 0\n",
                ))?;
                let declared = declarations(&bytes)?;
                assert_eq!(declared.resources, [0x10302].into(), "guessed uncertain lookup: {uncertain}");
                assert_eq!(declared.dynamic.len(), 1, "lost runtime selector: {uncertain}");
            }
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires both original discs; reads scripts without cooking or playback"]
    fn original_party_declarations_cover_all_registry_kinds_on_both_discs() -> Result<()> {
        use symphonia_script::Width;
        // Numeric viewer input: call PC, lhs PC/address, upper bound, resource
        // offset, and consumer PCs. Actor calls intervene after each widget.
        let numeric = [
            (
                0x55ba,
                0x55a7,
                0x08b8,
                500,
                0x20000,
                [
                    0x55e2, 0x5669, 0x56f0, 0x5777, 0x57fe, 0x5885, 0x590c, 0x5995, 0x59fc,
                ],
            ),
            (
                0x5a44,
                0x5a32,
                0x08bc,
                14,
                15,
                [
                    0x5a6a, 0x5aef, 0x5b74, 0x5bf9, 0x5c7e, 0x5d03, 0x5d88, 0x5e0f, 0x5e74,
                ],
            ),
        ];
        let numeric_pcs: Vec<_> = numeric.iter().flat_map(|group| group.5).collect();
        // Native PC, entry, and selector mask. Kind-zero event records expose
        // eight additional sites beyond the original nine-party entry-60 set.
        type Case<'a> = (&'a str, usize, &'a [u32], &'a [(u32, i32, u16)]);
        let cases: &[Case<'_>] = &[
            (
                "elc_d03.bin",
                8,
                &[0x11ee, 0x1ad4, 0x1d16, 0x1f96, 0x220f, 0x2449, 0x2670],
                &[],
            ),
            (
                "faa_d01.bin",
                98,
                &[
                    0x3640, 0x37e6, 0x398a, 0x3b4e, 0x3cf2, 0x3e95, 0x4059, 0x421c,
                ],
                &[],
            ),
            (
                "yum_d00.bin",
                7,
                &[0x55a7],
                &[(0x4062, 0x79, 7), (0x5237, 0x79, 7)],
            ),
            (
                "yum_d01.bin",
                6,
                &[0x57e6],
                &[
                    (0x4a88, 0x79, 7),
                    (0x55be, 0x79, 7),
                    (0x6e12, 0x8a, 0x1ff),
                    (0x727c, 0x1f, 0x1f7),
                ],
            ),
            (
                "yum_d02.bin",
                4,
                &[0x4822],
                &[(0x4182, 0x79, 7), (0x448b, 0x79, 7)],
            ),
            ("sew_d00.bin", 4, &[0x1168, 0x13c7], &[]),
            ("thu_d01.bin", 3, &[0x3238, 0x3672, 0x3a97], &[]),
            ("moa_d00.bin", 4, &[0x7b1, 0x9e2], &[]),
            (
                "testfield_01.bin",
                8,
                &[0x13b7, 0x13cd, 0x1cbd, 0x2133],
                &[],
            ),
            ("testfield_02.bin", 53, &numeric_pcs, &[]),
        ];
        for disc in [1, 2] {
            let root = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../local/extracted/disc{disc}/files/MAP"));
            let mut recovered = 0;
            for &(name, total, primary, additional) in cases {
                let map = crate::field::MapArchive::open(&root.join(name))?;
                let script = map.section(6)?;
                let program = Program::decode(script)?;
                let analysis = scenario::analyze(script)?;
                let call = NativeCall::ResolveScriptResource;
                let arguments = call_arguments(&program, &analysis, call, 1)?;
                let unknown: BTreeSet<_> = arguments
                    .iter()
                    .filter(|(_, args)| args[0].is_none())
                    .map(|(pc, _)| *pc)
                    .collect();
                let expected: BTreeMap<_, BTreeSet<_>> = primary
                    .iter()
                    .map(|&pc| (pc, 0x3c, 0x1ff))
                    .chain(additional.iter().copied())
                    .map(|(pc, entry, mask)| {
                        (
                            pc,
                            (0..9)
                                .filter(|selector| mask & (1 << selector) != 0)
                                .map(|selector| 0x30000 + entry + 65536 * selector)
                                .collect(),
                        )
                    })
                    .collect();
                ensure!(
                    arguments.len() == total && unknown == expected.keys().copied().collect(),
                    "disc {disc} {name}: expected {total} calls/{} variable declarations at {:x?}, found {} calls/{} variable declarations at {unknown:x?}",
                    expected.len(),
                    expected.keys(),
                    arguments.len(),
                    unknown.len(),
                );
                let finite = expressions::finite_arguments(&program, &analysis, call);
                if name.starts_with("testfield_") {
                    ensure!(
                        unknown.iter().all(|pc| !finite.contains_key(pc)),
                        "guessed viewer input"
                    );
                    ensure!(
                        declarations(script).is_err(),
                        "admitted interactive viewer input"
                    );
                    if name == "testfield_02.bin" {
                        let inputs = call_arguments(&program, &analysis, NativeCall::Unknown71, 5)?;
                        for (input, lhs, memory, maximum, base, sites) in numeric {
                            ensure!(
                                inputs.get(&input)
                                    == Some(&vec![Some(0), None, Some(0), Some(maximum), Some(0)]),
                                "changed numeric input bounds at {input:#x}"
                            );
                            ensure!(
                                program.instruction(lhs).unwrap().0 == Op::Load(memory, Width::S32)
                                    && program.instruction(input + 1).unwrap().0
                                        == Op::Calculate(0x10),
                                "changed numeric input destination/assignment at {input:#x}"
                            );
                            for pc in sites {
                                let mut expression: Vec<_> = analysis
                                    .instructions
                                    .range(..pc)
                                    .rev()
                                    .take(5)
                                    .map(|(&at, _)| program.instruction(at).unwrap().0)
                                    .collect();
                                expression.reverse();
                                ensure!(
                                    expression
                                        == [
                                            Op::Load(memory, Width::S32),
                                            Op::Push(base),
                                            Op::Calculate(0x31),
                                            Op::Calculate(0),
                                            Op::Argument
                                        ],
                                    "changed viewer resource expression at {pc:#x}"
                                );
                            }
                        }
                    }
                } else {
                    for pc in unknown {
                        ensure!(
                            finite.get(&pc) == expected.get(&pc),
                            "missing finite declaration in {name} at {pc:#x}"
                        );
                        recovered += 1;
                    }
                    let declared = declarations(script)?.resources;
                    let expected_resources = expected
                        .values()
                        .flatten()
                        .copied()
                        .chain(arguments.values().filter_map(|args| args[0]))
                        .map(|id| id as u32)
                        .collect();
                    ensure!(
                        declared == expected_resources,
                        "changed party resources in disc {disc} {name}"
                    );
                }
            }
            ensure!(
                recovered == 33,
                "incomplete finite resource census: {recovered}/33"
            );
        }
        Ok(())
    }
}
