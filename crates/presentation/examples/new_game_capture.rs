//! Exercise the normal New Game handoff without a window or audio device.
fn main() -> anyhow::Result<()> {
    let output = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "local/native/new-game-development".into());
    let assets = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "local/cooked".into());
    let mode = std::env::args().nth(3).unwrap_or_else(|| "keyboard".into());
    #[cfg(feature = "solari")]
    if matches!(mode.as_str(), "modern" | "modern-fast") {
        resonance_presentation::prepare_ray_tracing_process()?;
        return resonance_presentation::record_modern_new_game(
            std::path::Path::new(&assets),
            std::path::Path::new(&output),
            std::env::args().nth(4).as_deref(),
            mode == "modern-fast",
        );
    }
    if mode == "exploration" {
        let replay = std::env::args()
            .nth(4)
            .ok_or_else(|| anyhow::anyhow!("REPLAY.json required"))?;
        return resonance_presentation::record_new_game_exploration(
            std::path::Path::new(&assets),
            std::path::Path::new(&output),
            &serde_json::from_slice(&std::fs::read(replay)?)?,
        );
    }
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
