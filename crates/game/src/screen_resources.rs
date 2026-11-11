//! Session-screen dependencies discovered from current native scripts before
//! field activation. No generated dependency list needs updating after an edit.
use anyhow::{Context, Result, ensure};
use resonance_content::{
    credits, diagnostics::Diagnostics, game_over, library::Snapshot, preparation,
};
use std::{collections::BTreeMap, sync::Arc};
use symphonia_script::{NativeCall, Program, scenario::calls};

/// Immutable code belongs to the captured inputs which were inspected.
#[derive(Clone, Default)]
pub struct Programs(BTreeMap<String, (Arc<[u8]>, Arc<Program>)>);
impl Programs {
    /// Combine independently captured scripts from the same preparation.
    pub fn extend(&mut self, programs: Self) {
        self.0.extend(programs.0);
    }

    pub fn program(
        &self,
        files: &resonance_content::prepared::Files,
        path: &str,
    ) -> Result<Arc<Program>> {
        let bytes = files.read(path)?;
        if let Some((captured, program)) = self.0.get(path)
            && resonance_content::same_bytes(captured, &bytes)
        {
            return Ok(program.clone());
        }
        Ok(Arc::new(Program::decode(&bytes)?))
    }
}

pub fn capture<'a>(
    snapshot: &mut Snapshot,
    scripts: impl IntoIterator<Item = &'a str>,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<Programs> {
    // A fatal battle can happen without a ConfigureSession instruction in the
    // field. Keep this small shared screen ready alongside other shared UI.
    game_over::document::capture(snapshot, diagnostics, cancelled)?;
    let mut programs = Programs::default();
    let mut captured_credits = false;
    for path in scripts {
        if programs.0.contains_key(path) {
            continue;
        }
        let (bytes, program, credits) = inspect(snapshot, path, cancelled)?;
        if credits && !captured_credits {
            credits::document::capture(snapshot, cancelled)
                .with_context(|| format!("credits requested by {path}"))?;
            captured_credits = true;
        }
        programs
            .0
            .insert(path.to_owned(), (bytes, Arc::new(program)));
    }
    ensure!(!cancelled(), "session-screen preparation cancelled");
    Ok(programs)
}

/// Capture one field's screen dependencies and retain its validated bytecode.
pub fn capture_program(
    snapshot: &mut Snapshot,
    path: &str,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<Program> {
    game_over::document::capture(snapshot, diagnostics, cancelled)?;
    let (_, program, credits) = inspect(snapshot, path, cancelled)?;
    if credits {
        credits::document::capture(snapshot, cancelled)
            .with_context(|| format!("credits requested by {path}"))?;
    }
    ensure!(!cancelled(), "session-screen preparation cancelled");
    Ok(program)
}
fn inspect(
    snapshot: &mut Snapshot,
    path: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<(Arc<[u8]>, Program, bool)> {
    ensure!(!cancelled(), "session-screen preparation cancelled");
    let bytes = snapshot.read(path, preparation::JSON_LIMIT)?;
    let (program, calls) = calls::program_arguments(&bytes, NativeCall::ConfigureSession, 2)
        .with_context(|| format!("discover session screens in {path}"))?;
    Ok((
        bytes,
        program,
        calls
            .values()
            .any(|args| args[0].is_none_or(|setting| setting == 18)),
    ))
}
