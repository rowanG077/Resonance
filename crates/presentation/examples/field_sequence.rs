//! Silent consecutive-frame rendering diagnostic, including uncapped redraws.
fn main() -> anyhow::Result<()> {
    use anyhow::Context;
    use std::{
        fs,
        io::{BufRead, Write},
        path::Path,
    };
    let mut args = std::env::args().skip(1);
    let spec = args
        .next()
        .context("SEQUENCE.json OUTPUT_DIR [COOKED_ROOT]")?;
    if spec == "--worker" {
        #[derive(serde::Deserialize)]
        struct Request {
            sequence: std::path::PathBuf,
            output: std::path::PathBuf,
        }
        let root = args.next().context("--worker COOKED_ROOT")?;
        let mut renderer = resonance_presentation::FieldSequenceRenderer::new(Path::new(&root))?;
        for line in std::io::stdin().lock().lines() {
            let result = (|| -> anyhow::Result<()> {
                let request: Request = serde_json::from_str(&line?)?;
                let sequence = serde_json::from_slice(&fs::read(request.sequence)?)?;
                renderer.capture(&request.output, &sequence)
            })();
            println!(
                "ORACLE {}",
                serde_json::json!({"error": result.err().map(|e| format!("{e:#}"))})
            );
            std::io::stdout().flush()?;
        }
        return Ok(());
    }
    let output = args.next().context("OUTPUT_DIR")?;
    let root = args.next().unwrap_or_else(|| "local/cooked".into());
    let sequence = serde_json::from_slice(&fs::read(spec)?)?;
    resonance_presentation::capture_field_sequence(Path::new(&root), Path::new(&output), &sequence)
}
