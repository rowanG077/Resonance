//! Title loading is an input scenario; assertions inspect its held captures.
use super::*;
use replay::{Event, Key, MenuPage, SlotMode, SlotState, Step, assert_checkpoint, capture_from};
use std::{fs, path::Path};

pub fn run_title_load_probe(root: &Path, directory: &Path, output: &Path) -> Result<()> {
    let bytes = Store::new(directory).read(Kind::Save, &SlotId::new("a-001")?)?;
    let (_, SceneCheckpoint::Field(expected)) = resonance_persistence::decode(&bytes)?.into_parts()
    else {
        anyhow::bail!("probe requires a field save");
    };
    let mut app = harness::app_with_saves(
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
    steps.extend(Step::tap(Key::Right));
    steps.extend(Step::tap(Key::Interact));
    steps.push(slots(SlotState::List));
    for _ in 0..8 {
        steps.extend(Step::tap(Key::Down));
    }
    steps.push(Step::capture("title-load-selected"));
    steps.extend(Step::tap(Key::Cancel));
    steps.extend(Step::tap(Key::Cancel));
    steps.push(Step::wait(Event::Title, 600));
    steps.push(Step::capture("title-cancelled"));
    steps.extend(Step::tap(Key::Interact));
    steps.push(slots(SlotState::Bank));
    steps.extend(Step::tap(Key::Interact));
    steps.push(slots(SlotState::List));
    steps.push(settled());
    steps.push(Step::capture("title-load-reopened"));
    for _ in 0..8 {
        steps.extend(Step::tap(Key::Up));
    }
    steps.extend(Step::tap(Key::Cancel));
    steps.extend(Step::tap(Key::Left));
    steps.extend(Step::tap(Key::Interact));
    steps.push(slots(SlotState::List));
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
    replay::record_app(&mut app, output, &CheckpointReplay::new(steps), false)?;
    // Compare the previous release's slot after applying just its outfit migration.
    let mut recovered = expected.clone();
    let session = app
        .world()
        .get_resource::<new_game::Session>()
        .context("loaded session missing")?;
    recovered
        .progress
        .party
        .recover_story_costumes(session.data(), &recovered.progress.event_flags);
    drop(app);
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("recording.json"))?)?;
    let banks = capture_from(&report, "title-load-banks")?;
    let held = capture_from(&report, "title-load-held")?;
    ensure!(
        banks["checkpoint_available"] == false,
        "title Load allowed a field checkpoint"
    );
    ensure!(
        banks["scene"]["title"] == held["scene"]["title"],
        "title advanced underneath Load"
    );
    ensure!(
        capture_from(&report, "title-cancelled")?["scene"]["title"]["menu"]["selected"] == 1,
        "cancel lost title selection"
    );
    ensure!(
        banks["scene"]["load"]["bank"] == 0
            && banks["scene"]["load"]["slot"] == 0
            && banks["scene"]["load"]["first_slot"] == 0,
        "a fresh process retained an earlier save/load position"
    );
    for name in ["title-load-selected", "title-load-reopened"] {
        let menu = &capture_from(&report, name)?["scene"]["load"];
        ensure!(
            menu["bank"] == 1 && menu["slot"] == 8 && menu["first_slot"] == 3,
            "{name} lost the shared save/load position: {menu}"
        );
    }
    let loaded = capture_from(&report, "loaded-field")?;
    ensure!(
        loaded["scene"]["phase"] == "field" && loaded["scene"]["load"].is_null(),
        "load menu survived field activation"
    );
    let restored: FieldCheckpoint = serde_json::from_value(loaded["restored_checkpoint"].clone())?;
    assert_checkpoint(&restored, &recovered)?;
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
