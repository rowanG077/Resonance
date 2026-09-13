//! Offline classroom event capture with quality presets; invoked by capture-classroom.py.
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let output = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("expected output directory and settings JSON"))?;
    let settings = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("expected settings JSON"))?;
    anyhow::ensure!(args.next().is_none(), "unexpected argument");
    let spec: resonance_presentation::ClassroomShowcase =
        serde_json::from_slice(&std::fs::read(settings)?)?;
    if !spec.plan_only {
        resonance_presentation::prepare_ray_tracing_process()?;
    }
    resonance_presentation::capture_classroom_showcase(
        std::path::Path::new("local/cooked"),
        std::path::Path::new(&output),
        &spec,
    )
}
