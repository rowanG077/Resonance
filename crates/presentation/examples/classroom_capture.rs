//! Capture a prepared checkpoint or ordinary New Game scenario.
#[path = "common/field_capture.rs"]
mod capture;

fn main() -> anyhow::Result<()> {
    capture::run(false)
}
