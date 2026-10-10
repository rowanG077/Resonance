//! Save/load menu coverage through real keyboard input and the shared scenario runner.
use super::*;
use replay::{Event, Key, MenuPage, SlotMode, SlotState, Step, assert_checkpoint, capture_from};
use std::{fs, path::Path};

pub fn run_menu_probe(root: &Path, save: &Path, output: &Path) -> Result<()> {
    let (identity, _) =
        new_game::save_context(root, resonance_content::diagnostics::Diagnostics::new(true))?;
    let (_, expected): (_, FieldCheckpoint) =
        resonance_persistence::decode(&fs::read(save)?)?.admit(&identity)?;
    let app = probe::app(root, save, output, crate::Resolution::default())?;
    let mut steps = vec![
        title_probe::field(expected.map_id),
        Step::wait(Event::SavePoint, 1),
        Step::capture("baseline"),
    ];
    steps.extend(Step::tap(Key::Interact));
    steps.push(slots(SlotMode::Save, SlotState::Bank));
    steps.push(menu(MenuPage::Save));
    steps.push(Step::capture("save-empty"));
    steps.extend(Step::tap(Key::Interact));
    steps.push(slots(SlotMode::Save, SlotState::List));
    steps.extend(Step::tap(Key::Interact));
    steps.push(slots(SlotMode::Save, SlotState::Confirm));
    steps.push(menu(MenuPage::Save));
    steps.push(Step::capture("save-confirm"));
    steps.extend(Step::tap(Key::Interact));
    steps.push(slots(SlotMode::Save, SlotState::Notice));
    steps.push(menu(MenuPage::Save));
    steps.push(Step::capture("save-success"));
    steps.extend(Step::tap(Key::Interact));
    steps.extend(Step::tap(Key::Cancel));
    steps.extend(Step::tap(Key::Cancel));
    steps.push(title_probe::field(expected.map_id));
    steps.push(Step::capture("save-closed"));
    steps.extend(Step::tap(Key::Menu));
    steps.push(menu(MenuPage::Main));
    steps.push(Step::capture("field-menu"));
    steps.push(Step::select(
        Event::MenuSelection {
            page: MenuPage::Main,
            entry: MenuPage::System,
        },
        Key::Left,
    ));
    steps.extend(Step::tap(Key::Interact));
    steps.push(menu(MenuPage::System));
    steps.push(Step::capture("system-menu"));
    steps.push(Step::select(
        Event::MenuSelection {
            page: MenuPage::System,
            entry: MenuPage::Load,
        },
        Key::Down,
    ));
    steps.extend(Step::tap(Key::Interact));
    steps.push(slots(SlotMode::Load, SlotState::Bank));
    steps.extend(Step::tap(Key::Interact));
    steps.push(slots(SlotMode::Load, SlotState::List));
    steps.push(menu(MenuPage::Load));
    steps.push(Step::capture("load-slot"));
    steps.extend(Step::tap(Key::Interact));
    steps.push(slots(SlotMode::Load, SlotState::Confirm));
    steps.extend(Step::tap(Key::Interact));
    steps.push(title_probe::field(expected.map_id));
    steps.push(Step::capture("loaded-field"));
    steps.push(Step::Hold {
        keys: vec![],
        updates: 8,
    });
    steps.push(Step::capture("field-continued"));
    replay::record_app(app, output, &CheckpointReplay::new(steps), false)?;
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("recording.json"))?)?;
    let baseline = capture_from(&report, "baseline")?;
    ensure!(
        baseline["active_save_point"] == true,
        "menu probe requires an activated memory circle"
    );
    ensure!(
        capture_from(&report, "save-empty")?["quicksave_available"] == false,
        "quicksave accepted an open menu"
    );
    ensure!(
        capture_from(&report, "save-success")?["menu"]["slots"][0] == "saved"
            && capture_from(&report, "load-slot")?["menu"]["slots"][0] == "saved",
        "save/load menu lost its saved slot"
    );
    let store = Store::new(output.join("slots"));
    let (_, persisted): (_, FieldCheckpoint) =
        resonance_persistence::decode(&store.read(Kind::Save, &SlotId::new("a-001")?)?)?
            .admit(&identity)?;
    ensure!(
        serde_json::to_value(&persisted)?
            == capture_from(&report, "save-confirm")?["menu"]["checkpoint"],
        "save menu changed the submitted checkpoint"
    );
    let loaded = capture_from(&report, "loaded-field")?;
    let restored: FieldCheckpoint = serde_json::from_value(loaded["restored_checkpoint"].clone())?;
    assert_checkpoint(&restored, &persisted)?;
    let continued: FieldCheckpoint =
        serde_json::from_value(capture_from(&report, "field-continued")?["checkpoint"].clone())?;
    ensure!(
        continued.map_id == restored.map_id
            && continued.progress.tick > restored.progress.tick
            && continued.played_ticks > restored.played_ticks,
        "loaded memory circle did not resume ordinary updates"
    );
    fs::write(
        output.join("result.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "saved":persisted, "restored":restored, "checkpoint":continued, "audio_device":false,
            "unprepared_reads":report["unprepared_reads"], "save_load":true, "quicksave_rejected_in_menu":true
        }))?,
    )?;
    Ok(())
}
fn menu(page: MenuPage) -> Step {
    Step::wait(Event::MenuSettled { page }, 600)
}
fn slots(mode: SlotMode, state: SlotState) -> Step {
    Step::wait(Event::Slots { mode, state }, 600)
}
