//! Compare a cooked source library against its normalized original binaries.
//! Uses only the library's published constants; never regenerates source.
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use symphonia_script_compiler::scenario::{self, Definitions};

fn files(root: &Path, extension: &str) -> std::io::Result<Vec<PathBuf>> {
    let mut pending = vec![root.to_owned()];
    let mut result = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() && entry.path().extension().is_some_and(|e| e == extension) {
                result.push(entry.path());
            }
        }
    }
    result.sort();
    Ok(result)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let original = PathBuf::from(
        args.next()
            .ok_or("expected normalized original binary root")?,
    );
    let cooked = PathBuf::from(args.next().ok_or("expected cooked source root")?);
    if args.next().is_some() {
        return Err("expected exactly two root paths".into());
    }
    let mut sources = BTreeMap::new();
    for path in files(&cooked.join("scripts/std"), "sym")? {
        let module = path
            .strip_prefix(cooked.join("scripts"))?
            .with_extension("");
        let module = module
            .iter()
            .map(|p| p.to_str().ok_or("non-UTF-8 module path"))
            .collect::<Result<Vec<_>, _>>()?
            .join("::");
        sources.insert(module, fs::read_to_string(path)?);
    }
    if sources.is_empty() {
        return Err("cooked library has no published standard definitions".into());
    }
    let definitions = Definitions::from_sources(&sources)?;
    let originals = files(&original, "ssb")?;
    if originals.is_empty() {
        return Err("original root has no normalized scenario binaries".into());
    }
    let mut failures = Vec::new();
    let mut compared = 0;
    let mut bytes_compared = 0;
    for binary in &originals {
        let relative = binary.strip_prefix(&original)?;
        let source_path = cooked.join(relative).with_extension("sym");
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let source = fs::read_to_string(&source_path)?;
            let compiled =
                scenario::compile(&relative.display().to_string(), &source, &definitions)?;
            let expected = fs::read(binary)?;
            if compiled != expected {
                return Err(format!(
                    "compiled bytes differ ({} versus {} bytes)",
                    compiled.len(),
                    expected.len()
                )
                .into());
            }
            bytes_compared += expected.len();
            compared += 1;
            Ok(())
        })();
        if let Err(error) = result {
            failures.push(format!("{}: {error}", source_path.display()));
        }
    }
    // A fresh source publication must not leave runnable binary scenarios as
    // alternate roots. Explicit binary inspection/export is a separate action.
    let binaries = files(&cooked, "ssb")?;
    for binary in binaries {
        failures.push(format!(
            "cooked publication contains binary scenario {}",
            binary.display()
        ));
    }
    if !failures.is_empty() {
        for failure in failures.iter().take(10) {
            eprintln!("{failure}");
        }
        return Err(format!(
            "{} source-publication failures; {compared}/{} scripts matched",
            failures.len(),
            originals.len()
        )
        .into());
    }
    println!(
        "{compared} cooked .sym files match normalized originals ({bytes_compared} bytes); published constants only; no cooked .ssb files"
    );
    Ok(())
}
