use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use symphonia_script::authored::{NativeDeclaration, Type};
use symphonia_script_compiler::{ScriptKind, SourceResolver};
use symphonia_script_tools::{PreparationCache, SourceTree, run};

const API: &[NativeDeclaration] = &[NativeDeclaration {
    name: "game::wait",
    opcode: 1,
    parameters: &[Type::Ticks],
    result: None,
    suspends: true,
}];

fn sources() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "main".into(),
            "script field; use shared; pub fn main() -> i32 { return shared::value(); }".into(),
        ),
        (
            "shared".into(),
            "script library; pub fn value() -> i32 { return 1; }".into(),
        ),
        ("other".into(), "script field; pub fn main() {}".into()),
    ])
}

#[test]
fn cache_tracks_transitive_sources_and_keeps_running_generations() {
    let mut sources = sources();
    let mut cache = PreparationCache::default();
    let original = cache.prepare(["main"], &sources, &[]).unwrap();
    assert_eq!(original.module("main").unwrap().kind, ScriptKind::Field);
    let cached = cache.prepare(["main"], &sources, &[]).unwrap();
    assert!(Arc::ptr_eq(&original, &cached));
    sources.insert("other".into(), "script field; pub fn unrelated() {}".into());
    assert!(Arc::ptr_eq(
        &original,
        &cache.prepare(["main"], &sources, &[]).unwrap()
    ));
    cache.prepare(["other"], &sources, &[]).unwrap();
    let revisited = cache.prepare(["main"], &sources, &[]).unwrap();
    assert!(Arc::ptr_eq(
        original.module("main").unwrap(),
        revisited.module("main").unwrap()
    ));
    sources.insert(
        "shared".into(),
        "script library; pub fn value() -> i32 { return 2; }".into(),
    );
    let changed = cache.prepare(["main"], &sources, &[]).unwrap();
    assert!(!Arc::ptr_eq(
        original.module("main").unwrap(),
        changed.module("main").unwrap()
    ));
    assert!(original.module("main").unwrap().sources["shared"].contains("return 1"));
    assert!(changed.module("main").unwrap().sources["shared"].contains("return 2"));
}

#[test]
fn failure_does_not_publish_partial_generation_or_return_stale_code() {
    let mut sources = sources();
    let mut cache = PreparationCache::default();
    let original = cache.prepare(["main", "other"], &sources, &[]).unwrap();
    sources.insert("main".into(), "script field; pub fn main() {}".into());
    sources.insert("other".into(), "script field; pub fn broken(".into());
    assert!(cache.prepare(["main", "other"], &sources, &[]).is_err());
    sources = self::sources();
    assert!(Arc::ptr_eq(
        &original,
        &cache.prepare(["main", "other"], &sources, &[]).unwrap()
    ));
    sources.remove("shared");
    assert!(cache.prepare(["main"], &sources, &[]).is_err());
}

#[test]
fn cache_invalidates_native_signatures_and_new_source_shadowing_native_imports() {
    let mut sources = BTreeMap::from([(
        "main".into(),
        "script field; use game; pub task main() { await game::wait(ticks(1)); }".into(),
    )]);
    let mut cache = PreparationCache::default();
    let original = cache.prepare(["main"], &sources, API).unwrap();
    let mut changed = API.to_vec();
    changed[0].opcode = 2;
    let replaced = cache.prepare(["main"], &sources, &changed).unwrap();
    assert!(!Arc::ptr_eq(
        original.module("main").unwrap(),
        replaced.module("main").unwrap()
    ));
    changed[0].parameters = &[Type::Bool];
    assert!(cache.prepare(["main"], &sources, &changed).is_err());
    // Import resolution probes absent source modules too. Adding one must force
    // a fresh compile, even when every previously loaded source is unchanged.
    sources.insert("game".into(), "invalid new module".into());
    assert!(cache.prepare(["main"], &sources, API).is_err());
}

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "symphonia-script-tools-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn args(&self, command: &[&str]) -> Vec<String> {
        command
            .iter()
            .map(|arg| arg.to_string())
            .chain([self.0.to_str().unwrap().into()])
            .collect()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn source_snapshot_and_cli_preserve_comments_and_report_bad_formatting() {
    let directory = Directory::new();
    fs::create_dir(directory.0.join("field")).unwrap();
    let path = directory.0.join("field/start.sym");
    let original =
        "// Comment café\nscript field;\npub fn start(){/* keep 日本語 */let value = 1;}";
    fs::write(&path, original).unwrap();
    let tree = SourceTree::load(&directory.0).unwrap();
    assert_eq!(tree.modules().collect::<Vec<_>>(), ["field::start"]);
    fs::write(&path, "broken").unwrap();
    assert_eq!(tree.source("field::start"), Some(original));
    fs::write(&path, original).unwrap();
    let mut output = Vec::new();
    assert!(run(directory.args(&["fmt", "--check"]), &[], &mut output).is_err());
    run(directory.args(&["fmt"]), &[], &mut output).unwrap();
    let formatted = fs::read_to_string(&path).unwrap();
    assert!(formatted.contains("// Comment café"));
    assert!(formatted.contains("/* keep 日本語 */"));
    run(directory.args(&["fmt", "--check"]), &[], &mut output).unwrap();
    let mut check = directory.args(&["check"]);
    check.push("field::start".into());
    run(check, &[], &mut output).unwrap();
    output.clear();
    run(["api".into()], API, &mut output).unwrap();
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("await game::wait")
    );
}

#[cfg(unix)]
#[test]
fn filesystem_resolver_rejects_symlinks_and_non_identifier_paths() {
    let directory = Directory::new();
    let other = Directory::new();
    fs::write(
        other.0.join("outside.sym"),
        "script field; pub fn main() {}",
    )
    .unwrap();
    std::os::unix::fs::symlink(&other.0, directory.0.join("escape")).unwrap();
    assert!(SourceTree::load(&directory.0).is_err());
    fs::remove_file(directory.0.join("escape")).unwrap();
    fs::write(
        directory.0.join("not-an-id.sym"),
        "script field; pub fn main() {}",
    )
    .unwrap();
    assert!(SourceTree::load(&directory.0).is_err());
}

#[test]
fn standard_sources_reserve_the_namespace_without_implicit_fallback() {
    use symphonia_script_tools::StandardSources;
    let project = BTreeMap::from([
        (
            "main".into(),
            "script field; use std::flags; pub fn main() {}".into(),
        ),
        ("std::flags".into(), "invalid project override".into()),
        ("std::missing".into(), "must not fall back".into()),
        ("standard".into(), "ordinary project module".into()),
    ]);
    let standard = BTreeMap::from([
        (
            "std::flags".into(),
            "script library; pub const Known: i32 = 41;".into(),
        ),
        ("std".into(), "standard root".into()),
        ("extra".into(), "must not expose".into()),
    ]);
    let sources = StandardSources {
        project: &project,
        standard: &standard,
    };
    assert_eq!(sources.source("std::flags"), standard.source("std::flags"));
    assert_eq!(sources.source("std"), Some("standard root"));
    assert_eq!(sources.source("standard"), Some("ordinary project module"));
    assert_eq!(sources.source("std::missing"), None);
    assert_eq!(sources.source("extra"), None);
    let generation = PreparationCache::default()
        .prepare(["main"], &sources, &[])
        .unwrap();
    assert_eq!(generation.module("main").unwrap().sources.len(), 2);
}

#[test]
fn editable_project_discovery_skips_standard_library_bytes() {
    let directory = Directory::new();
    fs::create_dir(directory.0.join("std")).unwrap();
    fs::write(directory.0.join("std/flags.sym"), [0xff]).unwrap();
    fs::write(directory.0.join("std.sym"), [0xff]).unwrap();
    fs::write(directory.0.join("standard.sym"), "script library;").unwrap();
    let project = SourceTree::load_project(&directory.0).unwrap();
    assert_eq!(project.modules().collect::<Vec<_>>(), ["standard"]);
    assert!(SourceTree::load(&directory.0).is_err());
    fs::write(directory.0.join("std/flags.sym"), "script library;").unwrap();
    fs::write(directory.0.join("std.sym"), "script library;").unwrap();
    let authored = SourceTree::load(&directory.0).unwrap();
    assert_eq!(
        authored.modules().collect::<Vec<_>>(),
        ["standard", "std", "std::flags"]
    );
}

#[test]
fn cli_checks_declaration_only_library_catalogues_and_rejects_bad_values() {
    let directory = Directory::new();
    let path = directory.0.join("catalogue.sym");
    let mut args = directory.args(&["check"]);
    args.push("catalogue".into());
    let mut output = Vec::new();
    fs::write(&path, "script library; pub const Tail: i32 = 147;").unwrap();
    run(args.clone(), &[], &mut output).unwrap();
    assert_eq!(output, b"checked 1 module(s)\n");
    fs::write(&path, "script library; pub const Tail: i32 = \"bad\";").unwrap();
    assert!(
        run(args, &[], &mut output)
            .unwrap_err()
            .to_string()
            .contains("expected")
    );
}
