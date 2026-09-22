use crate::{Error, SourceTree, module_id};
use std::{fs, io::Write};
use symphonia_script::authored::NativeDeclaration;
use symphonia_script_compiler::{SourceResolver, check, format, native_reference};

const USAGE: &str = "usage: symphonia-script check ROOT MODULE...\n       symphonia-script fmt [--check] ROOT [MODULE...]\n       symphonia-script api";

/// Shared CLI entry point. Game tools pass their actual declaration table; the
/// standalone binary checks language-only modules with an empty native API.
pub fn run(
    args: impl IntoIterator<Item = String>,
    natives: &[NativeDeclaration],
    output: &mut impl Write,
) -> Result<(), Error> {
    let mut args = args.into_iter();
    let command = args.next().ok_or_else(|| Error::Usage(USAGE.into()))?;
    if command == "api" {
        if args.next().is_some() {
            return Err(Error::Usage(USAGE.into()));
        }
        output.write_all(native_reference(natives).as_bytes())?;
        return Ok(());
    }
    if command != "check" && command != "fmt" {
        return Err(Error::Usage(USAGE.into()));
    }
    let mut root = args.next().ok_or_else(|| Error::Usage(USAGE.into()))?;
    let check_format = command == "fmt" && root == "--check";
    if check_format {
        root = args.next().ok_or_else(|| Error::Usage(USAGE.into()))?;
    }
    let tree = SourceTree::load(root)?;
    let mut modules = args.collect::<Vec<_>>();
    if modules.is_empty() {
        if command == "check" {
            return Err(Error::Usage(USAGE.into()));
        }
        modules.extend(tree.modules().map(str::to_owned));
    }
    for module in &modules {
        module_id(module)?;
    }
    if command == "check" {
        for module in &modules {
            check(module, &tree, natives)?;
        }
        writeln!(output, "checked {} module(s)", modules.len())?;
    } else {
        let mut changes = Vec::new();
        for module in modules {
            let source = tree
                .source(&module)
                .ok_or_else(|| Error::Usage(format!("module not found: {module}")))?;
            let formatted = format(&module, source)?;
            if formatted != source {
                changes.push((module, formatted));
            }
        }
        if check_format && !changes.is_empty() {
            return Err(Error::Formatting(
                changes
                    .iter()
                    .map(|(module, _)| module.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }
        for (module, formatted) in changes {
            let path = tree
                .path(&module)
                .expect("formatted module came from source tree");
            fs::write(path, formatted).map_err(|source| Error::Io {
                path: path.into(),
                source,
            })?;
            writeln!(output, "formatted {module}")?;
        }
    }
    Ok(())
}
