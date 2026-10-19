fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let root = std::env::var_os("RESONANCE_TEST_ASSETS")
        .map_or_else(|| "local/all-assets".into(), std::path::PathBuf::from);
    let output = args.next().expect("output directory");
    let size = args
        .next()
        .unwrap_or_else(|| "1920x1080".into())
        .parse()
        .map_err(anyhow::Error::msg)?;
    let seconds = args.next().unwrap_or_else(|| "120".into()).parse()?;
    let profile = args.next().is_some_and(|arg| arg == "profile");
    let save = args.next().map(std::path::PathBuf::from);
    let encounter = save
        .as_deref()
        .map(|save| -> anyhow::Result<_> {
            use resonance_events::battle::{DefeatPolicy, Encounter, Setup};
            Ok((
                save,
                Setup {
                    encounter: Encounter::Formation(args.next().expect("FORMATION").parse()?),
                    arena: args.next().expect("ARENA").parse()?,
                    defeat: DefeatPolicy::ResumeEvent,
                    music: None,
                    route: [0; 5],
                },
            ))
        })
        .transpose()?;
    let replay = args
        .next()
        .map(|path| -> anyhow::Result<_> { Ok(serde_json::from_slice(&std::fs::read(path)?)?) })
        .transpose()?;
    anyhow::ensure!(args.next().is_none(), "unexpected argument");
    resonance_presentation::run_frame_benchmark(
        &root,
        std::path::Path::new(&output),
        size,
        seconds,
        profile,
        encounter,
        replay.as_ref(),
    )
}
