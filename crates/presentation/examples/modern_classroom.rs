//! Capture the HD classroom prototype; the ordinary oracle remains unchanged.
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let output = args
        .next()
        .unwrap_or_else(|| "local/modern-classroom.png".into());
    let ray_tracing = args.next().as_deref() != Some("--raster");
    if ray_tracing {
        resonance_presentation::prepare_ray_tracing_process()?;
    }
    resonance_presentation::capture_modern_classroom(
        std::path::Path::new("local/cooked"),
        std::path::Path::new(&output),
        ray_tracing,
    )
}
