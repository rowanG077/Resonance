//! Exercise the normal New Game handoff without a window or audio device.
fn main() -> anyhow::Result<()> {
    let output = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "local/native/new-game-development".into());
    let assets = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "local/cooked".into());
    let mode = std::env::args().nth(3).unwrap_or_else(|| "keyboard".into());
    anyhow::ensure!(
        matches!(mode.as_str(), "keyboard" | "gamepad"),
        "expected keyboard or gamepad"
    );
    let stop_at = std::env::args().nth(4);
    let replay: Option<resonance_game::field::replay::InputReplay> = std::env::args()
        .nth(5)
        .map(|path| -> anyhow::Result<_> { Ok(serde_json::from_slice(&std::fs::read(path)?)?) })
        .transpose()?;
    resonance_presentation::record_new_game_until(
        std::path::Path::new(&assets),
        std::path::Path::new(&output),
        mode == "gamepad",
        stop_at.as_deref(),
        replay.as_ref(),
    )
}
