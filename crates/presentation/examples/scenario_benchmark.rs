//! Run a saved benchmark case without a window, display server or audio device.
//! `RESONANCE_BENCH_WINDOWED=1` runs it in a window on the desktop instead.
fn main() -> anyhow::Result<()> {
    use resonance_events::battle::{DefeatPolicy, Encounter, Setup};
    use resonance_presentation::{
        ScenarioBenchmark, run_offscreen_benchmark, run_window_benchmark,
    };
    use std::path::PathBuf;
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().expect("ASSETS"));
    let output = PathBuf::from(args.next().expect("OUTPUT"));
    let case: serde_json::Value =
        serde_json::from_slice(&std::fs::read(args.next().expect("CASE"))?)?;
    let resolution = args
        .next()
        .unwrap_or_else(|| "1920x1080".into())
        .parse()
        .map_err(anyhow::Error::msg)?;
    let seconds = args.next().unwrap_or_else(|| "90".into()).parse()?;
    let profile = args.next().is_some_and(|arg| arg == "profile");
    anyhow::ensure!(args.next().is_none(), "unexpected argument");
    let save = PathBuf::from(
        case["save"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("case needs a save path"))?,
    );
    let replay = serde_json::from_value(case["replay"].clone())?;
    let encounter = case
        .get("encounter")
        .map(|battle| -> anyhow::Result<_> {
            Ok(Setup {
                encounter: Encounter::Formation(u16::try_from(
                    battle["formation"]
                        .as_u64()
                        .ok_or_else(|| anyhow::anyhow!("formation"))?,
                )?),
                arena: u16::try_from(
                    battle["arena"]
                        .as_u64()
                        .ok_or_else(|| anyhow::anyhow!("arena"))?,
                )?,
                defeat: DefeatPolicy::ResumeEvent,
                music: None,
                route: [0; 5],
            })
        })
        .transpose()?;
    // `RESONANCE_BENCH_AA` (for example `Off` or `Msaa4`) overrides the
    // default 2x MSAA for comparisons.
    let anti_aliasing = match std::env::var("RESONANCE_BENCH_AA") {
        Ok(mode) => serde_json::from_value(serde_json::Value::String(mode))?,
        Err(_) => resonance_content::menu_data::AntiAliasing::Msaa2,
    };
    // `RESONANCE_BENCH_TITLE` runs the title screen instead of the case.
    if std::env::var_os("RESONANCE_BENCH_TITLE").is_some() {
        return resonance_presentation::run_offscreen_title(
            &root,
            &output,
            resolution,
            anti_aliasing,
        );
    }
    let run = if std::env::var_os("RESONANCE_BENCH_WINDOWED").is_some_and(|v| v != "0") {
        run_window_benchmark
    } else {
        run_offscreen_benchmark
    };
    run(
        &root,
        &output,
        resolution,
        ScenarioBenchmark {
            save: &save,
            encounter,
            replay: &replay,
            seconds,
            profile,
            anti_aliasing,
        },
    )
}
