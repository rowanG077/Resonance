use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn files(directory: &Path) -> Vec<PathBuf> {
    let mut files: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    files.sort();
    files
}

fn embed(directory: &Path, root: &Path, code: &mut String) {
    for path in files(directory) {
        if path.is_dir() {
            embed(&path, root, code);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "ktx2")
        {
            let name = path.strip_prefix(root).unwrap().with_extension("");
            let name = name.to_str().unwrap().replace('\\', "/");
            code.push_str(&format!(
                "({name:?}, include_bytes!({path:?}), include_str!({:?})),\n",
                path.with_extension("json")
            ));
        }
    }
}

fn main() {
    // Assets are already packed. Building never needs an art tool or converter.
    println!("cargo:rerun-if-changed=assets");
    let root = Path::new(&env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("assets");
    let mut code = String::from("static ATLASES: &[(&str, &[u8], &str)] = &[\n");
    embed(&root, &root, &mut code);
    code.push_str("];\n");
    code.push_str("static EVENTS: &[(&str, &str)] = &[\n");
    for path in files(&root.join("events")) {
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            let name = format!("events/{}", path.file_name().unwrap().to_str().unwrap());
            code.push_str(&format!("({name:?}, include_str!({path:?})),\n"));
        }
    }
    code.push_str("];\n");
    fs::write(
        Path::new(&env::var_os("OUT_DIR").unwrap()).join("assets.rs"),
        code,
    )
    .unwrap();
}
