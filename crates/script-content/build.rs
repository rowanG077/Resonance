use std::{collections::BTreeMap, env, fs, io, path::Path};

fn collect(directory: &Path, files: &mut Vec<std::path::PathBuf>) -> io::Result<()> {
    println!("cargo:rerun-if-changed={}", directory.display());
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            collect(&path, files)?;
        } else if kind.is_symlink() {
            return Err(io::Error::other(format!(
                "embedded scripts cannot be symlinks: {}",
                path.display()
            )));
        } else if kind.is_file()
            && path
                .extension()
                .is_some_and(|ext| ext == "sym" || ext == "md")
        {
            println!("cargo:rerun-if-changed={}", path.display());
            files.push(path);
        }
    }
    Ok(())
}

fn main() -> io::Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts");
    let mut files = Vec::new();
    for directory in ["std", "preview"] {
        collect(&root.join(directory), &mut files)?;
    }
    files.sort();
    let mut modules = String::from("pub const MODULES: &[(&str, &str)] = &[\n");
    let mut published = String::from("pub const FILES: &[(&str, &str)] = &[\n");
    let mut nodes = BTreeMap::new();
    for (index, path) in files.into_iter().enumerate() {
        let relative = path
            .strip_prefix(&root)
            .expect("collected beneath source root");
        let name = relative
            .components()
            .map(|component| component.as_os_str().to_str())
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| io::Error::other("embedded script paths must be UTF-8"))?
            .join("/");
        let source = format!("include_str!({:?})", path);
        published.push_str(&format!("({name:?}, {source}),\n"));
        if name.starts_with("std/nodes/") && name.ends_with(".sym") {
            let source = fs::read_to_string(&path)?;
            if let Some(digest) = source
                .lines()
                .find_map(|line| line.strip_prefix("// Skeleton: "))
            {
                if digest.len() != 64
                    || !digest
                        .bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                {
                    return Err(io::Error::other(format!(
                        "invalid skeleton signature in {name}"
                    )));
                }
                if let Some(previous) = nodes.insert(digest.to_owned(), name.clone()) {
                    return Err(io::Error::other(format!(
                        "duplicate skeleton {digest} in {previous} and {name}"
                    )));
                }
            }
        }
        if let Some(module) = name.strip_suffix(".sym") {
            modules.push_str(&format!(
                "({:?}, FILES[{index}].1),\n",
                module.replace('/', "::")
            ));
        }
    }
    modules.push_str("];\n");
    published.push_str("];\n");
    published.push_str("pub const NODE_MODULES: &[(&str, &str)] = &[\n");
    for (digest, path) in nodes {
        published.push_str(&format!("({digest:?}, {path:?}),\n"));
    }
    published.push_str("];\n");
    let output = env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("missing OUT_DIR"))?;
    fs::write(Path::new(&output).join("sources.rs"), modules + &published)
}
