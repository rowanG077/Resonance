//! Exercise a world landmark through the real asynchronous field loader and GPU warmup.
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let root = args.next().expect("asset root");
    let landmark = args.next().expect("landmark id").parse()?;
    let output = args.next().expect("new output directory");
    let direction = args.next().map(|v| v.parse()).transpose()?.unwrap_or(2);
    let exit_trigger = args
        .next()
        .filter(|v| v != "none")
        .map(|v| v.parse())
        .transpose()?;
    let field_override = args.next().map(|v| v.parse()).transpose()?;
    resonance_presentation::run_overworld_field_probe(
        std::path::Path::new(&root),
        landmark,
        direction,
        exit_trigger,
        field_override,
        std::path::Path::new(&output),
    )
}
