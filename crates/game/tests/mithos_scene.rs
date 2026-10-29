//! Exercise original angel scenes with read-only save progress and mapped joints.
mod common;
use anyhow::{Context, Result, ensure};
use resonance_content::{field::FieldAssets, session::SessionData};
use resonance_events::{
    Face, SavedProgress,
    input::{Button, Buttons},
};
use resonance_game::field::{FieldEntry, FieldInput, FieldSession};
use std::{collections::BTreeSet, fs, sync::Arc};

fn enter_scene(
    map_id: u32,
    story: Option<i32>,
) -> Result<(
    FieldSession,
    Arc<resonance_content::prepared::Files>,
    FieldAssets,
)> {
    let root = common::asset_root();
    let save = std::env::var_os("RESONANCE_TEST_SAVE").context("set RESONANCE_TEST_SAVE")?;
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(save)?)?;
    let mut progress: SavedProgress =
        serde_json::from_value(saved["state"]["field"]["progress"].clone())?;
    if let Some(story) = story {
        progress.script_globals[16] = story;
    }
    let files = Arc::new(resonance_content::preparation::capture(
        &resonance_content::library::Library::open(&root, &[])?,
        &[map_id],
        || false,
        resonance_content::diagnostics::Diagnostics::new(true),
    )?);
    let assets: FieldAssets = files.field(map_id)?;
    let data = Arc::new(SessionData::load(&files)?);
    // This prepares a scene fixture from progress. It does not bypass or test the
    // production full-save admission path, and never rewrites the source save.
    let entry = FieldEntry {
        persistent: progress.into_state(&data)?,
        data: Some(data),
        text: Arc::new(resonance_content::session::GameText::load(&files)?),
        menu_data: Some(Arc::new(resonance_content::menu_data::MenuData::load(
            &files,
        )?)),
        menu_files: files.clone(),
        available_fields: (0..600).collect(),
        attachments: resonance_game::field::attachments::prepare(&assets, |path| files.read(path))?,
        ..Default::default()
    };
    let scene = FieldSession::enter(
        &files.read(&assets.script)?,
        files.json_or_default(assets.messages.as_deref())?,
        &assets,
        entry,
    )?;
    Ok((scene, files, assets))
}

#[test]
#[ignore = "requires a full cook and late-game RESONANCE_TEST_SAVE; CPU only, no save writes"]
fn final_scene_visible_mithos_duplicates_receive_mouth_animation() -> Result<()> {
    let (mut scene, files, assets) = enter_scene(136, None)?;
    let effects: resonance_content::effect::FieldEffects = files
        .json::<resonance_content::field::settings::Settings>(&assets.effects)?
        .effects;
    ensure!(
        !effects.mouth_cycle.is_empty(),
        "missing authored mouth cycle"
    );
    let mut triggered = false;
    let mut wings_attached = false;
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
        if !wings_attached
            && let Some(wings) = scene.events.world.actors.get(&90026)
            && let Some(attachment) = &wings.attachment
        {
            ensure!(
                attachment.actor == 200 && attachment.bone == "spine-03",
                "Yggdrasill wings attached to the wrong actor or joint"
            );
            ensure!(
                scene.events.world.warnings.is_empty(),
                "scene has missing node warnings"
            );
            wings_attached = true;
        }
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
    ensure!(
        wings_attached,
        "Yggdrasill wing attachment was never exercised"
    );
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

#[test]
#[ignore = "requires a full cook and RESONANCE_TEST_SAVE; CPU only, no save writes"]
fn first_salvation_colette_and_kratos_wings_use_mapped_joints() -> Result<()> {
    // First Salvation scene, before the Remiel fight. Battle simulation is
    // outside this fixture: complete requests as victories to resume the scene.
    let (mut scene, files, _) = enter_scene(535, Some(2_302_000))?;
    let mut attached = BTreeSet::new();
    for _ in 0..30000 {
        let accept = scene
            .dialogue
            .values()
            .any(|line| !line.closed && line.fully_revealed() && line.voice_finished());
        scene.step(FieldInput {
            pressed_buttons: Buttons::default().with(Button::Accept, accept),
            ..Default::default()
        })?;
        for (wings_id, actor_id) in [(90021, 2), (90024, 9)] {
            if let Some(wings) = scene.events.world.actors.get(&wings_id)
                && let Some(attachment) = &wings.attachment
            {
                ensure!(
                    attachment.actor == actor_id && attachment.bone == "spine-03",
                    "wings {wings_id} attached to the wrong actor or joint"
                );
                attached.insert(actor_id);
            }
        }
        if attached.len() == 2 {
            break;
        }
        if scene.events.battle_pending() {
            scene
                .events
                .world
                .skip_battle_as_victory()
                .map_err(anyhow::Error::msg)?;
        }
    }
    ensure!(
        attached == BTreeSet::from([2, 9]),
        "missing wing attachments: {attached:?}"
    );
    ensure!(
        scene.events.world.warnings.is_empty(),
        "scene has missing node warnings"
    );
    ensure!(
        !files.diagnostics().has_errors(),
        "Salvation scene has preparation diagnostics"
    );
    eprintln!("Validated first Salvation wing attachments for Colette and Kratos");
    Ok(())
}
