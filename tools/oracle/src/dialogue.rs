//! Appearance matrix over naturally reached dialogue; no playback device.
use super::*;
use resonance_content::menu_data::CustomizeSettings;
use serde_json::json;
use std::{collections::BTreeSet, process::Command};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    version: u32,
    description: String,
    capture: Capture,
    regions: Vec<Region>,
    tolerance: u8,
    max_changed_fraction: f64,
    preferences: CustomizeSettings,
    variants: Vec<Variant>,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Capture {
    Dialogue { prefix: String, hold_ticks: u32 },
    Setup { tick: u32 },
}
impl Capture {
    fn command(&self, native: Option<&Path>, image: &Path, settings: &Path) -> Result<Command> {
        let binary = match self {
            Self::Dialogue { .. } => "target/debug/examples/dialogue_capture",
            Self::Setup { .. } => "target/debug/examples/setup_capture",
        };
        let mut command = Command::new(native.unwrap_or(Path::new(binary)));
        command.arg(image);
        match self {
            Self::Dialogue { prefix, hold_ticks } => {
                ensure!(
                    !prefix.is_empty() && *hold_ticks <= 3600,
                    "invalid dialogue capture"
                );
                command.arg(prefix).arg(hold_ticks.to_string());
            }
            Self::Setup { tick } => {
                ensure!((1..=3600).contains(tick), "invalid setup capture");
                command.arg(tick.to_string());
            }
        }
        command.arg(settings);
        Ok(command)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Region {
    name: String,
    rect: [u32; 4],
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Variant {
    window: u8,
    background: u8,
    reference: PathBuf,
    sha256: String,
    capture: Option<Capture>,
}

pub(super) fn run(case_path: &Path, output: &Path, native: Option<&Path>) -> Result<()> {
    let case: Case = serde_json::from_slice(&fs::read(case_path)?)?;
    ensure!(
        case.version == 2 && !case.regions.is_empty() && (1..=18).contains(&case.variants.len()),
        "invalid dialogue case"
    );
    ensure!(!output.exists(), "dialogue output already exists");
    let mut regions = BTreeSet::new();
    for region in &case.regions {
        ensure!(
            !region.name.is_empty()
                && region
                    .name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-')
                && regions.insert(&region.name),
            "invalid or duplicate region name"
        );
    }
    fs::create_dir_all(output)?;
    let mut seen = BTreeSet::new();
    let mut results = Vec::new();
    for variant in case.variants {
        ensure!(
            seen.insert((variant.window, variant.background)),
            "duplicate appearance"
        );
        ensure!(
            pair::file_hash(&variant.reference)? == variant.sha256,
            "reference image hash differs"
        );
        let mut preferences = case.preferences.clone();
        preferences.window = variant.window;
        preferences.background = variant.background;
        preferences.validate()?;
        let name = format!(
            "window-{}-background-{}",
            variant.window, variant.background
        );
        let image = output.join(format!("{name}.png"));
        let settings = output.join(format!("{name}-preferences.json"));
        fs::write(&settings, serde_json::to_vec_pretty(&preferences)?)?;
        let log = fs::File::create(output.join(format!("{name}.log")))?;
        let mut command = variant
            .capture
            .as_ref()
            .unwrap_or(&case.capture)
            .command(native, &image, &settings)?;
        let native_hash = pair::file_hash(Path::new(command.get_program()))?;
        let status = command.stdout(log.try_clone()?).stderr(log).status()?;
        ensure!(status.success(), "dialogue capture failed: {name}");
        let observation: serde_json::Value =
            serde_json::from_slice(&fs::read(image.with_extension("json"))?)?;
        ensure!(
            observation["audio_device"] == false
                && observation["dialogue_preferences"] == serde_json::to_value(&preferences)?,
            "capture did not apply the requested preferences: {name}"
        );
        let mut gates = Vec::new();
        for region in &case.regions {
            let passed = compare(
                &variant.reference,
                &image,
                &output.join(&name).join(&region.name),
                case.tolerance,
                case.max_changed_fraction,
                Some(region.rect),
            )?;
            gates.push(json!({"region":region.name,"passed":passed}));
        }
        let passed = gates.iter().all(|gate| gate["passed"] == true);
        results.push(
            json!({"window":variant.window,"background":variant.background,
            "native_sha256":native_hash,"gates":gates,"passed":passed}),
        );
    }
    let passed = results.iter().all(|r| r["passed"] == true);
    let report = json!({"description":case.description,
        "audio_device":false,"cases":results,"passed":passed});
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    ensure!(passed, "dialogue appearance comparison failed");
    Ok(())
}
