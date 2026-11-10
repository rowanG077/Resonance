//! Verify and publish readable sources for a complete cooked scenario corpus.
use std::{collections::BTreeMap, fs, path::PathBuf};
use symphonia_script_compiler::scenario::{Context, Definitions, decompile_with_context};

#[derive(Default)]
struct ValueCoverage {
    symbolic: usize,
    unresolved: usize,
    partial_mask: usize,
}

impl ValueCoverage {
    fn add(&mut self, status: &str, uses: usize) {
        match status {
            "symbolic" => self.symbolic += uses,
            "unresolved" => self.unresolved += uses,
            "partial-mask" => self.partial_mask += uses,
            _ => unreachable!("unknown identity status"),
        }
    }

    fn status(&self) -> &'static str {
        if self.unresolved != 0 {
            "unresolved"
        } else if self.partial_mask != 0 {
            "partial-mask"
        } else {
            "symbolic"
        }
    }

    fn uses(&self) -> usize {
        self.symbolic + self.unresolved + self.partial_mask
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let root = PathBuf::from(args.next().ok_or("expected cooked source root")?);
    let target = PathBuf::from(args.next().ok_or("expected output root")?);
    let definitions = Definitions::from_sources(&resonance_script_content::modules())?;
    for (path, source) in resonance_script_content::definitions() {
        let output = target.join("scripts").join(path);
        fs::create_dir_all(output.parent().unwrap())?;
        fs::write(output, source)?;
    }
    let mut pending = vec![root.clone()];
    let (mut files, mut instructions, mut intrinsics) = (0, 0, 0);
    let (mut message_controls, mut promoted_message_controls) = (0, 0);
    let mut failures = Vec::new();
    let mut report = Vec::new();
    let mut identities = BTreeMap::new();
    let mut identity_uses = Vec::new();
    let mut message_uses = Vec::new();
    let mut message_coverage = BTreeMap::<String, BTreeMap<i32, ValueCoverage>>::new();
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(&path)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                pending.push(path);
                continue;
            }
            if !path.extension().is_some_and(|e| e == "ssb") {
                continue;
            }
            let bytes = fs::read(&path)?;
            match decompile_with_context(
                &bytes,
                &definitions,
                Context::from_path(path.strip_prefix(&root)?),
            ) {
                Ok(result) => {
                    for id in &result.identities {
                        let status = if id.partial_mask {
                            "partial-mask"
                        } else if id.symbolic {
                            "symbolic"
                        } else {
                            "unresolved"
                        };
                        let key = (
                            id.domain.clone(),
                            id.native.clone(),
                            id.parameter.clone(),
                            id.value,
                            status,
                        );
                        identity_uses.push(format!(
                            "{}\t{:#X}\t{}\t{}\t{}\t{}\t{}",
                            path.strip_prefix(&root)?.display(),
                            id.pc,
                            id.domain,
                            id.native,
                            id.parameter,
                            id.value,
                            status
                        ));
                        let entry = identities.entry(key).or_insert_with(|| {
                            (
                                0usize,
                                path.strip_prefix(&root).unwrap().display().to_string(),
                                id.pc,
                            )
                        });
                        entry.0 += 1;
                    }
                    for id in &result.message_identities {
                        let status = if id.partial_mask {
                            "partial-mask"
                        } else if id.symbolic {
                            "symbolic"
                        } else {
                            "unresolved"
                        };
                        message_uses.push(format!(
                            "{}\t{:#X}\t{}\t{}\t{}\t{}\t{}\t{}",
                            path.strip_prefix(&root)?.display(),
                            id.byte_offset,
                            id.control,
                            id.domain,
                            id.native,
                            id.parameter,
                            id.value,
                            status
                        ));
                        message_coverage
                            .entry(id.domain.clone())
                            .or_default()
                            .entry(id.value)
                            .or_default()
                            .add(status, 1);
                    }
                    let output = target.join(path.strip_prefix(&root)?).with_extension("sym");
                    fs::create_dir_all(output.parent().unwrap())?;
                    fs::write(&output, result.source)?;
                    report.push(format!(
                        "{}\t{}\t{}\t{}\tmatching",
                        path.strip_prefix(&root)?.display(),
                        result.instructions,
                        result.intrinsic_instructions,
                        bytes.len()
                    ));
                    message_controls += result.message_controls;
                    promoted_message_controls += result.promoted_message_controls;
                    files += 1;
                    instructions += result.instructions;
                    intrinsics += result.intrinsic_instructions;
                }
                Err(e) => failures.push(format!("{}: {e}", path.display())),
            }
        }
    }
    message_uses.sort();
    fs::write(
        target.join("message-identity-uses.tsv"),
        format!(
            "input\tbyte_offset\tcontrol\tdomain\tnative\tparameter\tvalue\tstatus\n{}",
            message_uses
                .into_iter()
                .map(|s| s + "\n")
                .collect::<String>()
        ),
    )?;
    let mut message_report = String::from(
        "domain\tunique_values\tsymbolic_values\tunresolved_values\tpartial_mask_values\tuses\tsymbolic_uses\n",
    );
    for (domain, values) in message_coverage {
        use std::fmt::Write;
        let count = |status| values.values().filter(|v| v.status() == status).count();
        writeln!(
            message_report,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            domain,
            values.len(),
            count("symbolic"),
            count("unresolved"),
            count("partial-mask"),
            values.values().map(ValueCoverage::uses).sum::<usize>(),
            values.values().map(|v| v.symbolic).sum::<usize>()
        )
        .unwrap();
    }
    fs::write(target.join("message-identity-coverage.tsv"), message_report)?;
    println!(
        "{files} matching .sym files; {instructions} instructions, {intrinsics} explicit VM intrinsics; {promoted_message_controls}/{message_controls} parsed message controls"
    );
    report.sort();
    let mut coverage = BTreeMap::<String, BTreeMap<i32, ValueCoverage>>::new();
    let mut identity_report =
        String::from("domain\tnative\tparameter\tvalue\tstatus\tuses\texample\tpc\n");
    for ((domain, native, parameter, value, status), (uses, example, pc)) in identities {
        use std::fmt::Write;
        coverage
            .entry(domain.clone())
            .or_default()
            .entry(value)
            .or_default()
            .add(status, uses);
        writeln!(
            identity_report,
            "{domain}\t{native}\t{parameter}\t{value}\t{status}\t{uses}\t{example}\t{pc:#X}"
        )
        .unwrap();
    }
    fs::write(target.join("identifiers.tsv"), identity_report)?;
    let mut coverage_report = String::from(
        "domain\tunique_values\tsymbolic_values\tunresolved_values\tpartial_mask_values\tuses\tsymbolic_uses\n",
    );
    for (domain, values) in coverage {
        use std::fmt::Write;
        let count = |status| values.values().filter(|v| v.status() == status).count();
        writeln!(
            coverage_report,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            domain,
            values.len(),
            count("symbolic"),
            count("unresolved"),
            count("partial-mask"),
            values.values().map(ValueCoverage::uses).sum::<usize>(),
            values.values().map(|v| v.symbolic).sum::<usize>()
        )
        .unwrap();
    }
    fs::write(target.join("identity-coverage.tsv"), coverage_report)?;
    identity_uses.sort();
    fs::write(
        target.join("identity-uses.tsv"),
        format!(
            "input\tpc\tdomain\tnative\tparameter\tvalue\tstatus\n{}\n",
            identity_uses.join("\n")
        ),
    )?;
    fs::write(
        target.join("verification.tsv"),
        format!(
            "input\tinstructions\tintrinsics\tbytes\tresult\n{}\n",
            report.join("\n")
        ),
    )?;
    for failure in &failures {
        eprintln!("{failure}");
    }
    if !failures.is_empty() {
        return Err(format!("{} failures", failures.len()).into());
    }
    Ok(())
}

#[test]
fn a_named_upper_bound_does_not_hide_an_unnamed_use_of_the_same_value() {
    let mut value = ValueCoverage::default();
    value.add("symbolic", 1);
    value.add("unresolved", 1);
    assert_eq!(value.status(), "unresolved");
    assert_eq!(value.uses(), 2);
    assert_eq!(value.symbolic, 1);

    let mut partial = ValueCoverage::default();
    partial.add("partial-mask", 2);
    partial.add("symbolic", 1);
    assert_eq!(partial.status(), "partial-mask");
    assert_eq!(partial.uses(), 3);
    assert_eq!(partial.symbolic, 1);
}
