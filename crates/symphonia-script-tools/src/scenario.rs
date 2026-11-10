//! Readable original-VM source, matching binary export and lossless assembly.
use crate::Error;
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};
use symphonia_script::{Program, scenario};

const USAGE: &str = "usage: symphonia-script scenario decompile INPUT.ssb OUTPUT.sym [--assets ROOT]\n       symphonia-script scenario compile INPUT.sym OUTPUT.ssb [--assets ROOT]\n       symphonia-script scenario disassemble INPUT.ssb OUTPUT.ssasm\n       symphonia-script scenario assemble INPUT.ssasm OUTPUT.ssb";
const BINARY_LIMIT: usize = 4 * 1024 * 1024;
const SOURCE_LIMIT: usize = 64 * 1024 * 1024;

pub(crate) fn run(
    args: impl Iterator<Item = String>,
    output: &mut impl Write,
) -> Result<(), Error> {
    let args = args.collect::<Vec<_>>();
    let (args, assets) = match args.as_slice() {
        [_, _, _, flag, assets] if flag == "--assets" => (&args[..3], Some(assets)),
        [_, _, _] => (args.as_slice(), None),
        _ => return Err(Error::Usage(USAGE.into())),
    };
    let [command, input, target] = args else {
        unreachable!()
    };
    let input = Path::new(input);
    let target = Path::new(target);
    let (input_extension, output_extension, limit) = match command.as_str() {
        "disassemble" => ("ssb", "ssasm", BINARY_LIMIT),
        "assemble" => ("ssasm", "ssb", SOURCE_LIMIT),
        "decompile" => ("ssb", "sym", BINARY_LIMIT),
        "compile" => ("sym", "ssb", SOURCE_LIMIT),
        _ => return Err(Error::Usage(USAGE.into())),
    };
    if !input
        .extension()
        .is_some_and(|extension| extension == input_extension)
        || !target
            .extension()
            .is_some_and(|extension| extension == output_extension)
    {
        return Err(Error::Usage(format!(
            "expected .{input_extension} input and .{output_extension} output\n{USAGE}"
        )));
    }
    let invalid = |message: String| Error::Scenario {
        path: input.into(),
        message,
    };
    let mut bytes = Vec::new();
    let file = fs::File::open(input).map_err(|source| Error::Io {
        path: input.into(),
        source,
    })?;
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| Error::Io {
            path: input.into(),
            source,
        })?;
    if bytes.len() > limit {
        return Err(invalid(format!("input exceeds {limit} bytes")));
    }
    let (encoded, diagnostics) = if command == "decompile" || command == "compile" {
        let generated = resonance_script_content::modules();
        let cooked = assets
            .map(|root| crate::SourceTree::load(Path::new(root).join("scripts")))
            .transpose()?;
        let sources: &dyn symphonia_script_compiler::SourceResolver = cooked
            .as_ref()
            .map(|s| s as &dyn symphonia_script_compiler::SourceResolver)
            .unwrap_or(&generated);
        struct Resolve<'a>(&'a dyn symphonia_script_compiler::SourceResolver);
        impl symphonia_script_compiler::SourceResolver for Resolve<'_> {
            fn source(&self, module: &str) -> Option<&str> {
                self.0.source(module)
            }
        }
        let definitions =
            symphonia_script_compiler::scenario::Definitions::from_sources(&Resolve(sources))?;
        let encoded = if command == "decompile" {
            let context = assets
                .and_then(|root| {
                    let root = fs::canonicalize(root).ok()?;
                    let input = fs::canonicalize(input).ok()?;
                    input
                        .strip_prefix(root)
                        .ok()
                        .map(symphonia_script_compiler::scenario::Context::from_path)
                })
                .unwrap_or_else(|| symphonia_script_compiler::scenario::Context::from_path(input));
            symphonia_script_compiler::scenario::decompile_with_context(
                &bytes,
                &definitions,
                context,
            )?
            .source
            .into_bytes()
        } else {
            let source = std::str::from_utf8(&bytes).map_err(|e| invalid(e.to_string()))?;
            symphonia_script_compiler::scenario::compile(
                &input.display().to_string(),
                source,
                &definitions,
            )?
        };
        (encoded, Vec::new())
    } else if command == "disassemble" {
        let (listing, analysis) =
            scenario::disassemble(&bytes).map_err(|error| invalid(error.to_string()))?;
        if listing.len() > SOURCE_LIMIT {
            return Err(invalid("assembly exceeds source size limit".into()));
        }
        let restored = scenario::assemble(&listing).map_err(|error| invalid(error.to_string()))?;
        if restored != bytes {
            return Err(invalid(
                "disassembly did not preserve every input byte".into(),
            ));
        }
        (listing.into_bytes(), analysis.diagnostics)
    } else {
        let source = std::str::from_utf8(&bytes).map_err(|error| invalid(error.to_string()))?;
        let encoded = scenario::assemble(source).map_err(|error| invalid(error.to_string()))?;
        if encoded.len() > BINARY_LIMIT {
            return Err(invalid("assembled scenario exceeds 4 MiB".into()));
        }
        // Use the actual VM admission contract before replacing a runnable file.
        Program::decode(&encoded).map_err(|error| invalid(error.to_string()))?;
        let analysis = scenario::analyze(&encoded).map_err(|error| invalid(error.to_string()))?;
        (encoded, analysis.diagnostics)
    };
    let output_limit = if command == "decompile" || command == "disassemble" {
        SOURCE_LIMIT
    } else {
        BINARY_LIMIT
    };
    if encoded.len() > output_limit {
        return Err(invalid(format!("output exceeds {output_limit} bytes")));
    }
    let parent = target
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|source| Error::Io {
        path: parent.into(),
        source,
    })?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|source| Error::Io {
        path: target.into(),
        source,
    })?;
    temporary.write_all(&encoded).map_err(|source| Error::Io {
        path: target.into(),
        source,
    })?;
    temporary.as_file().sync_all().map_err(|source| Error::Io {
        path: target.into(),
        source,
    })?;
    temporary.persist(target).map_err(|error| Error::Io {
        path: target.into(),
        source: error.error,
    })?;
    writeln!(
        output,
        "Wrote {} ({} bytes)",
        target.display(),
        encoded.len()
    )?;
    for diagnostic in diagnostics {
        writeln!(
            output,
            "{:?}: {}: {}",
            diagnostic.severity, diagnostic.code, diagnostic.message
        )?;
    }
    Ok(())
}
