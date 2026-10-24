//! Publish one complete editable generation; scratch never becomes a library.
use super::*;

pub(super) struct Prepared {
    pub report: Report,
    pub sources: BTreeMap<String, Vec<String>>,
}

pub(super) fn cook(options: &Options<'_>) -> Result<Report> {
    fresh_output(options.output)?;
    let output = std::path::absolute(options.output)?;
    let parent = output.parent().context("cook destination has no parent")?;
    fs::create_dir_all(parent)?;
    // A sibling directory lets the final rename be atomic on this filesystem.
    let working = tempfile::Builder::new()
        .prefix(".resonance-cook-")
        .tempdir_in(parent)?;
    let scratch = working.path().join("scratch");
    let candidate = working.path().join("library");
    let result = (|| -> Result<Report> {
        let publications = crate::publication::Session::start(working.path())?;
        let mut prepared = super::cook_staged(&Options {
            discs: options.discs,
            output: &scratch,
            jobs: options.jobs,
            memory_budget: options.memory_budget,
        })?;
        // Collect diagnostics for source accounting, then keep them outside
        // the installed game library. Other exports come from named writers.
        for tree in ["resources/physical", "scripts", "reports"] {
            crate::publication::export_tree(&scratch.join(tree))?;
        }
        let scratch = scratch.canonicalize()?;
        let exports = publications
            .exports()?
            .into_iter()
            .map(|path| crate::relative_source_path(&scratch, &path))
            .collect::<Result<BTreeSet<_>>>()?;
        let sources = source_outputs(&mut prepared.report, prepared.sources, &exports);
        write_reports(&scratch, &prepared.report, &sources)?;
        if !prepared.report.failures.is_empty() {
            return Ok(prepared.report);
        }
        let exports = exports
            .into_iter()
            .filter(|path| !path.starts_with("reports/"))
            .collect::<BTreeSet<_>>();
        fs::create_dir(&candidate)?;
        copy_exports(&scratch, &candidate, &exports)?;
        let mut publisher = crate::named::Publisher::new(&candidate, &candidate)?;
        publisher.descriptor("GQSEAF", 0)?;
        if let Err(error) = super::admission::validate(&candidate) {
            fs::remove_file(candidate.join(resonance_content::library::DESCRIPTOR))?;
            return Err(error).context("completed named generation has invalid references");
        }
        fs::rename(scratch.join("reports"), working.path().join("reports"))?;
        fs::remove_dir_all(&scratch)?;
        // Refuse any files or edits that appeared at the requested destination
        // while cooking. No previous generation is removed or overwritten.
        fresh_output(&output)?;
        fs::rename(&candidate, &output).context("install complete cooked generation")?;
        Ok(prepared.report)
    })();
    match result {
        Ok(report) if report.failures.is_empty() => {
            let retained = working.keep();
            eprintln!(
                "Import diagnostics retained outside the library at {}",
                retained.join("reports").display()
            );
            Ok(report)
        }
        Ok(report) => {
            let retained = working.keep();
            eprintln!(
                "Cook failed; incomplete scratch and reports retained at {}",
                retained.display()
            );
            Ok(report)
        }
        Err(error) => {
            let retained = working.keep();
            Err(error).with_context(|| {
                format!(
                    "cook failed; incomplete scratch retained at {}",
                    retained.display()
                )
            })
        }
    }
}

fn fresh_output(output: &Path) -> Result<()> {
    match fs::symlink_metadata(output) {
        Ok(metadata) => ensure!(
            metadata.is_dir()
                && !metadata.file_type().is_symlink()
                && fs::read_dir(output)?.next().is_none(),
            "cook output must be a fresh directory; existing content and edits are never overwritten"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn source_outputs(
    report: &mut Report,
    sources: BTreeMap<String, Vec<String>>,
    exports: &BTreeSet<String>,
) -> BTreeMap<String, Vec<String>> {
    sources
        .into_iter()
        .map(|(source, paths)| {
            let mut selected = BTreeSet::new();
            for path in paths {
                if exports.contains(&path) {
                    selected.insert(path.clone());
                }
                let prefix = format!("{path}/");
                selected.extend(
                    exports
                        .range(prefix.clone()..)
                        .take_while(|path| path.starts_with(&prefix))
                        .cloned(),
                );
            }
            if selected.is_empty() && !report.excluded.contains_key(&source) {
                report.record(
                    &source,
                    Err(anyhow::anyhow!("source produced no named authoring output")),
                );
            }
            (source, selected.into_iter().collect())
        })
        .collect()
}

fn copy_exports(source: &Path, destination: &Path, paths: &BTreeSet<String>) -> Result<()> {
    let mut spelling = BTreeMap::new();
    for path in paths {
        resonance_content::library::validate_path(path)?;
        let mut prefix = String::new();
        for component in path.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(component);
            if let Some(previous) = spelling.insert(prefix.to_lowercase(), prefix.clone()) {
                ensure!(
                    previous == prefix,
                    "case-conflicting authoring paths: {previous} and {prefix}"
                );
            }
        }
        let input = source.join(path);
        let metadata = fs::symlink_metadata(&input)?;
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "authoring export is not an ordinary file: {path}"
        );
        let output = destination.join(path);
        fs::create_dir_all(output.parent().context("export has no parent")?)?;
        // Separate files preserve independent mod ownership, including when
        // scratch deduplicated payloads through hard links.
        fs::copy(&input, &output).with_context(|| format!("export {path}"))?;
    }
    Ok(())
}

fn write_reports(
    root: &Path,
    report: &Report,
    sources: &BTreeMap<String, Vec<String>>,
) -> Result<()> {
    crate::write_atomic(
        &root.join("reports/coverage.json"),
        &serde_json::to_vec_pretty(report)?,
    )?;
    crate::write_atomic(
        &root.join("reports/import-sources.json"),
        &serde_json::to_vec_pretty(sources)?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_omit_scratch_and_keep_equal_owners_independently_editable() -> Result<()> {
        let working = tempfile::tempdir()?;
        let scratch = working.path().join("scratch");
        let candidate = working.path().join("library");
        fs::create_dir_all(scratch.join("meshes"))?;
        fs::create_dir_all(scratch.join("fields/first"))?;
        fs::create_dir_all(scratch.join("fields/second"))?;
        let session = crate::publication::Session::start(working.path())?;
        let raw = scratch.join("meshes/private-converter-payload");
        fs::write(&raw, b"shared native bytes")?;
        let first = "fields/first/mesh.glb";
        let second = "fields/second/mesh.glb";
        fs::hard_link(&raw, scratch.join(first))?;
        fs::hard_link(&raw, scratch.join(second))?;
        crate::publication::export(&scratch.join(first))?;
        crate::publication::export(&scratch.join(second))?;
        let exports = session
            .exports()?
            .iter()
            .map(|path| crate::relative_source_path(&scratch, path))
            .collect::<Result<BTreeSet<_>>>()?;
        let mut report = Report::default();
        let sources = source_outputs(
            &mut report,
            BTreeMap::from([(
                "disc1/native.bin".into(),
                vec!["fields/first".into(), "fields/second".into()],
            )]),
            &exports,
        );
        assert!(report.failures.is_empty());
        assert_eq!(sources["disc1/native.bin"], [first, second]);
        copy_exports(&scratch, &candidate, &exports)?;
        assert!(!candidate.join("meshes").exists());
        fs::write(candidate.join(first), b"modded first owner")?;
        assert_eq!(fs::read(candidate.join(second))?, b"shared native bytes");
        assert_eq!(fs::read(raw)?, b"shared native bytes");
        Ok(())
    }

    #[test]
    fn failed_preparation_keeps_the_requested_destination_unpublished() -> Result<()> {
        let root = tempfile::tempdir()?;
        let disc = root.path().join("disc");
        fs::create_dir_all(disc.join("sys"))?;
        fs::write(disc.join("sys/boot.bin"), b"GQSEAF\0\0")?;
        let output = root.path().join("cooked");
        let error = cook(&Options {
            discs: &[disc],
            output: &output,
            jobs: 1,
            memory_budget: 64 * 1024 * 1024,
        })
        .unwrap_err();
        assert!(format!("{error:#}").contains("incomplete scratch retained"));
        assert!(!output.exists());
        let retained = fs::read_dir(root.path())?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?
            .into_iter()
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".resonance-cook-")
            })
            .collect::<Vec<_>>();
        assert_eq!(retained.len(), 1);
        assert!(!retained[0].join("library/content.json").exists());
        Ok(())
    }
}
