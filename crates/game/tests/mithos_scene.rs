//! Run the original final-scene script with a read-only copy of late-game progress.
mod common;
use anyhow::{Context, Result, ensure};
use resonance_content::{field::FieldAssets, session::SessionData};
use resonance_events::{
    Face, SavedProgress,
    input::{Button, Buttons},
};
use resonance_game::field::{FieldEntry, FieldInput, FieldSession};
use std::{collections::BTreeSet, fs, sync::Arc};

#[test]
#[ignore = "requires a full cook and late-game RESONANCE_TEST_SAVE; CPU only, no save writes"]
fn final_scene_visible_mithos_duplicates_receive_mouth_animation() -> Result<()> {
    let root = common::asset_root();
    let save = std::env::var_os("RESONANCE_TEST_SAVE").context("set RESONANCE_TEST_SAVE")?;
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(save)?)?;
    let progress: SavedProgress =
        serde_json::from_value(saved["state"]["field"]["progress"].clone())?;
    let files = Arc::new(resonance_content::preparation::capture(
        &resonance_content::library::Library::open(&root, &[])?,
        &[136],
        &mut Default::default(),
        || false,
        resonance_content::diagnostics::Diagnostics::new(true),
    )?);
    let assets: FieldAssets = files.field(136)?;
    let data = Arc::new(SessionData::load(&files)?);
    // This prepares a scene fixture from progress. It does not bypass or test the
    // production full-save admission path, and never rewrites the source save.
    let entry = FieldEntry {
        persistent: progress.into_state(&data)?,
        data: Some(data),
        text: Arc::new(files.json("game/text.json")?),
        menu_data: Some(Arc::new(resonance_content::menu_data::MenuData::load(
            &files,
        )?)),
        menu_files: files.clone(),
        available_fields: (0..600).collect(),
        attachments: resonance_game::field::attachments::prepare(&assets, |path| files.read(path))?,
        ..Default::default()
    };
    let mut scene = FieldSession::enter(
        &files.read(&assets.script)?,
        files.json(&assets.messages)?,
        &assets,
        entry,
    )?;
    let effects: resonance_content::effect::FieldEffects = files.json(&assets.effects)?;
    ensure!(
        !effects.mouth_cycle.is_empty(),
        "missing authored mouth cycle"
    );
    let mut triggered = false;
    let mut frames = BTreeSet::new();
    let mut lines = BTreeSet::new();
    for _ in 0..30000 {
        if scene.player_has_control() && !triggered {
            ensure!(
                scene.events.trigger(100, true)?,
                "final scene did not start"
            );
            triggered = true;
        }
        let accept = scene
            .dialogue
            .values()
            .any(|line| !line.closed && line.fully_revealed() && line.voice_finished());
        scene.step(FieldInput {
            pressed_buttons: Buttons::default().with(Button::Accept, accept),
            ..Default::default()
        })?;
        for (slot, line) in &scene.dialogue {
            if !line.is_talking() {
                continue;
            }
            let request = &scene.events.world.dialogue[slot];
            let Some(speaker) = request.speaker_actor else {
                continue;
            };
            if ![200, 202].contains(&speaker) {
                continue;
            }
            lines.insert(line.operation.id());
            for (&id, actor) in &scene.events.world.actors {
                if ![131072, 131073].contains(&actor.resource)
                    || !actor.visible
                    || actor.appearance.model_hidden
                {
                    continue;
                }
                let start = scene.talking.get(&id).with_context(|| {
                    format!("visible Mithos {id} is missing its dialogue clock")
                })?;
                let frame = match actor.appearance.mouth {
                    Some(Face::Frame(frame)) => frame,
                    _ => {
                        effects.mouth_cycle
                            [(scene.events.tick() - start) as usize % effects.mouth_cycle.len()]
                    }
                };
                frames.insert((id, actor.resource, frame));
            }
        }
        if scene.events.battle_pending() {
            break;
        }
    }
    ensure!(
        triggered && scene.events.battle_pending(),
        "final scene did not reach battle"
    );
    ensure!(!lines.is_empty(), "Mithos did not speak");
    for frame in [1, 2] {
        ensure!(
            frames
                .iter()
                .any(|&(id, _, observed)| id < 0 && observed == frame),
            "visible duplicate Mithos never received mouth frame {frame}"
        );
    }
    ensure!(
        !files.diagnostics().has_errors(),
        "final scene has preparation diagnostics"
    );
    eprintln!(
        "Validated {} Mithos lines and visible mouth samples {frames:?}",
        lines.len()
    );
    Ok(())
}
