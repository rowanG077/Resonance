//! Session-screen dependencies discovered from current native scripts before
//! field activation. No generated dependency list needs updating after an edit.
use anyhow::{Context, Result, ensure};
use resonance_content::{
    credits, diagnostics::Diagnostics, game_over, library::Snapshot, preparation,
};
use symphonia_script::{NativeCall, scenario::calls};

pub fn capture<'a>(
    snapshot: &mut Snapshot,
    scripts: impl IntoIterator<Item = &'a str>,
    diagnostics: &Diagnostics,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    // A fatal battle can happen without a ConfigureSession instruction in the
    // field. Keep this small shared screen ready alongside other shared UI.
    game_over::document::capture(snapshot, diagnostics, cancelled)?;
    for path in scripts {
        ensure!(!cancelled(), "session-screen preparation cancelled");
        let bytes = snapshot.read(path, preparation::JSON_LIMIT)?;
        let calls = calls::arguments(&bytes, NativeCall::ConfigureSession, 2)
            .with_context(|| format!("discover session screens in {path}"))?;
        if calls
            .values()
            .any(|args| args[0].is_none_or(|setting| setting == 18))
        {
            return credits::document::capture(snapshot, cancelled)
                .with_context(|| format!("credits requested by {path}"));
        }
    }
    ensure!(!cancelled(), "session-screen preparation cancelled");
    Ok(())
}
