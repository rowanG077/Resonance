//! Title loading is an input scenario; assertions inspect its held captures.
use super::*;
use replay::{Event, Key, MenuPage, SlotMode, SlotState, Step, assert_checkpoint, capture_from};
use std::{fs, path::Path};

pub fn run_title_load_probe(root: &Path, directory: &Path, output: &Path) -> Result<()> {
    let bytes = Store::new(directory).read(Kind::Save, &SlotId::new("a-001")?)?;
    let (identity, _) =
        new_game::save_context(root, resonance_content::diagnostics::Diagnostics::new(true))?;
    let (_, expected): (_, FieldCheckpoint) =
        resonance_persistence::decode(&bytes)?.admit(&identity)?;
    let app = probe::app_with_saves(
        root,
        output,
        SaveOptions {
            directory: Some(directory.into()),
            ..Default::default()
        },
        crate::Resolution::default(),
    )?;
    let mut steps = vec![Step::wait(Event::Title, 600)];
    steps.push(Step::select(Event::TitleLoad, Key::Down));
    steps.extend(Step::tap(Key::Interact));
    steps.push(slots(SlotState::Bank));
    steps.push(settled());
    steps.push(Step::capture("title-load-banks"));
    steps.push(Step::Hold {
        keys: vec![],
        updates: 30,
    });
    steps.push(Step::capture("title-load-held"));
    steps.extend(Step::tap(Key::Cancel));
    steps.push(Step::wait(Event::Title, 600));
    steps.push(Step::capture("title-cancelled"));
    steps.extend(Step::tap(Key::Interact));
    steps.push(slots(SlotState::Bank));
    steps.extend(Step::tap(Key::Interact));
    steps.push(slots(SlotState::List));
    steps.push(settled());
    steps.push(Step::capture("title-load-slot"));
    steps.extend(Step::tap(Key::Interact));
    steps.push(slots(SlotState::Confirm));
    steps.push(settled());
    steps.push(Step::capture("title-load-confirm"));
    steps.extend(Step::tap(Key::Interact));
    steps.push(field(expected.map_id));
    steps.push(Step::capture("loaded-field"));
    steps.push(Step::Hold {
        keys: vec![],
        updates: 8,
    });
    steps.push(Step::capture("field-continued"));
    replay::record_app(app, output, &CheckpointReplay::new(steps), false)?;
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("recording.json"))?)?;
    let banks = capture_from(&report, "title-load-banks")?;
    let held = capture_from(&report, "title-load-held")?;
    ensure!(
        banks["quicksave_available"] == false,
        "title Load allowed quicksave"
    );
    ensure!(
        banks["scene"]["title"] == held["scene"]["title"],
        "title advanced underneath Load"
    );
    ensure!(
        capture_from(&report, "title-cancelled")?["scene"]["title"]["menu"]["selected"] == 1,
        "cancel lost title selection"
    );
    let loaded = capture_from(&report, "loaded-field")?;
    ensure!(
        loaded["scene"]["phase"] == "field" && loaded["scene"]["load"].is_null(),
        "load menu survived field activation"
    );
    let restored: FieldCheckpoint = serde_json::from_value(loaded["restored_checkpoint"].clone())?;
    assert_checkpoint(&restored, &expected)?;
    let state: FieldCheckpoint =
        serde_json::from_value(capture_from(&report, "field-continued")?["checkpoint"].clone())?;
    ensure!(
        state.map_id == restored.map_id
            && state.progress.tick > restored.progress.tick
            && state.played_ticks > restored.played_ticks,
        "loaded field did not resume ordinary updates"
    );
    fs::write(
        output.join("result.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "saved":expected, "restored":restored, "checkpoint":state,
            "title_cancel_reopen":true, "loaded_previous_process_save":true,
            "keyboard_input":true, "audio_device":false, "unprepared_reads":report["unprepared_reads"]
        }))?,
    )?;
    Ok(())
}

fn slots(state: SlotState) -> Step {
    Step::wait(
        Event::Slots {
            mode: SlotMode::Load,
            state,
        },
        600,
    )
}
fn settled() -> Step {
    Step::wait(
        Event::MenuSettled {
            page: MenuPage::Load,
        },
        600,
    )
}
pub(super) fn field(map_id: u32) -> Step {
    Step::wait(
        Event::Field {
            map_id,
            free_control: true,
            story: None,
            max_x: None,
        },
        600,
    )
}
