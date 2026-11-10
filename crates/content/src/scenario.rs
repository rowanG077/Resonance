//! Compile the selected original-game .sym source at preparation boundaries.
//! Snapshots retain the source and its generated constants; stepping a VM never
//! opens files or recompiles code. Explicit binary resources remain supported.
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
};
use symphonia_script::Program;
use symphonia_script_compiler::SourceResolver;
use symphonia_script_compiler::scenario::Definitions;

#[derive(Clone)]
struct Prepared {
    bytes: Arc<[u8]>,
    program: Arc<Program>,
}

/// One preparation owns its definitions and compiled paths. Clones retain
/// existing programs but invalidate independently when their source bytes change.
#[derive(Default)]
pub(crate) struct Cache {
    definitions: OnceLock<Arc<Definitions>>,
    programs: Mutex<BTreeMap<String, Prepared>>,
}

impl Clone for Cache {
    fn clone(&self) -> Self {
        Self {
            definitions: self.definitions.clone(),
            programs: Mutex::new(self.programs.lock().unwrap().clone()),
        }
    }
}

impl Cache {
    fn define(&self, sources: &impl SourceResolver) -> Result<()> {
        if self.definitions.get().is_none() {
            let definitions = Definitions::from_sources(sources)?;
            let _ = self.definitions.set(Arc::new(definitions));
        }
        Ok(())
    }

    fn prepare(&self, path: &str, bytes: Arc<[u8]>) -> Result<Prepared> {
        let mut programs = self.programs.lock().unwrap();
        if let Some(prepared) = programs.get(path) {
            return Ok(prepared.clone());
        }
        let (bytes, program) = if path.ends_with(".sym") {
            let source = std::str::from_utf8(&bytes)
                .with_context(|| format!("scenario is not UTF-8: {path}"))?;
            let definitions = self
                .definitions
                .get()
                .context("unprepared scenario definitions")?;
            let (bytes, program) =
                symphonia_script_compiler::scenario::compile_program(path, source, definitions)
                    .with_context(|| format!("compile scenario {path}"))?;
            (bytes.into(), program)
        } else {
            let program =
                Program::decode(&bytes).with_context(|| format!("decode scenario {path}"))?;
            (bytes, program)
        };
        let prepared = Prepared {
            bytes,
            program: Arc::new(program),
        };
        programs.insert(path.into(), prepared.clone());
        Ok(prepared)
    }

    pub(crate) fn invalidate(&mut self, path: &str) {
        let programs = self.programs.get_mut().unwrap();
        if path.starts_with("scripts/") {
            self.definitions.take();
            programs.clear();
        } else {
            programs.remove(path);
        }
    }
}

/// Explicit preparation for command-line inspection and asset integration tests.
pub fn read_root(root: &std::path::Path, path: &str) -> Result<Arc<[u8]>> {
    let library = crate::library::Library::open(root, &[])?;
    capture(&mut library.snapshot(), path, &|| false)
}

fn prepared(files: &crate::prepared::Files, path: &str) -> Result<Prepared> {
    let bytes = files.read(path)?;
    if path.ends_with(".sym") && files.scenarios.definitions.get().is_none() {
        files.scenarios.define(&files.script_sources()?)?;
    }
    files.scenarios.prepare(path, bytes)
}

pub fn read(files: &crate::prepared::Files, path: &str) -> Result<Arc<[u8]>> {
    Ok(prepared(files, path)?.bytes)
}

/// Reuse the program already validated during dependency discovery.
pub fn program(files: &crate::prepared::Files, path: &str) -> Result<Arc<Program>> {
    Ok(prepared(files, path)?.program)
}

pub fn capture(
    snapshot: &mut crate::library::Snapshot,
    path: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<Arc<[u8]>> {
    ensure!(!cancelled(), "scenario preparation cancelled");
    let bytes = snapshot.read(path, crate::preparation::JSON_LIMIT)?;
    if path.ends_with(".sym") && snapshot.scenarios.definitions.get().is_none() {
        let sources = crate::preparation::scripts(snapshot, cancelled)?;
        snapshot.scenarios.define(&sources)?;
    }
    Ok(snapshot.scenarios.prepare(path, bytes)?.bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path};

    fn write(root: &Path, path: &str, bytes: &[u8]) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn selected_source_and_constants_share_the_mod_snapshot_and_bad_edits_fail() -> Result<()> {
        let work = tempfile::tempdir()?;
        let root = work.path().join("base");
        let mods = work.path().join("mod");
        write(
            &root,
            "content.json",
            &serde_json::to_vec(&crate::library::Descriptor {
                version: crate::library::VERSION,
                game: crate::library::GAME.into(),
            })?,
        );
        let path = "fields/example/scripts/scenario.sym";
        write(
            &root,
            path,
            b"script scenario; use std::story; code_base = 4;
            data { word(4); pc(entry); word(0); word(0); }
            task entry() { native::set_event_bit(story::StoryFlag::Custom); end; }",
        );
        write(
            &root,
            "scripts/std/story.sym",
            b"script library; pub enum StoryFlag { Custom = 100 }",
        );
        write(
            &mods,
            "scripts/std/story.sym",
            b"script library; pub enum StoryFlag { Custom = 101 }",
        );
        let library = crate::library::Library::open_paths(&root, std::slice::from_ref(&mods))?;
        let mut snapshot = library.snapshot();
        let original = capture(&mut snapshot, path, &|| false)?;
        assert_eq!(&original[8..10], &[0, 101]);
        assert!(Arc::ptr_eq(
            &capture(&mut snapshot, path, &|| false)?,
            &original
        ));
        let files = crate::prepared::Files::from_snapshot(
            snapshot,
            || false,
            crate::diagnostics::Diagnostics::new(true),
        )?;
        assert!(Arc::ptr_eq(&read(&files, path)?, &original));
        let original_program = program(&files, path)?;
        assert!(Arc::ptr_eq(&program(&files, path)?, &original_program));
        let concurrent = library.snapshot_with(&files)?.concurrent();
        let mut view = concurrent.view();
        assert!(Arc::ptr_eq(
            &capture(&mut view, path, &|| false)?,
            &original
        ));
        drop(view);
        let mut inherited = crate::prepared::Files::from_snapshot(
            concurrent.finish()?,
            || false,
            crate::diagnostics::Diagnostics::new(true),
        )?;
        assert!(Arc::ptr_eq(&read(&inherited, path)?, &original));
        inherited.insert(
            "scripts/std/story.sym".into(),
            Arc::from(b"script library; pub enum StoryFlag { Custom = 104 }".as_slice()),
        );
        assert_eq!(&read(&inherited, path)?[8..10], &[0, 104]);
        let mut edited = files.clone();
        edited.insert(
            "scripts/std/story.sym".into(),
            Arc::from(b"script library; pub enum StoryFlag { Custom = 103 }".as_slice()),
        );
        assert_eq!(&read(&edited, path)?[8..10], &[0, 103]);
        assert!(!Arc::ptr_eq(&program(&edited, path)?, &original_program));
        edited.insert(path.into(), Arc::from(b"invalid source".as_slice()));
        assert!(read(&edited, path).is_err());
        edited.remove(path);
        assert!(read(&edited, path).is_err());
        assert!(Arc::ptr_eq(&program(&files, path)?, &original_program));
        write(
            &mods,
            "scripts/std/story.sym",
            b"script library; pub enum StoryFlag { Custom = 102 }",
        );
        assert_eq!(
            read(&files, path)?,
            original,
            "retained preparation is immutable"
        );
        let next = capture(&mut library.snapshot(), path, &|| false)?;
        assert_eq!(&next[8..10], &[0, 102]);
        write(&mods, path, b"script scenario; invalid source");
        let library = crate::library::Library::open_paths(&root, &[mods])?;
        assert!(capture(&mut library.snapshot(), path, &|| false).is_err());
        assert_eq!(
            read(&files, path)?,
            original,
            "invalid replacement cannot alter a running program"
        );
        Ok(())
    }
}
