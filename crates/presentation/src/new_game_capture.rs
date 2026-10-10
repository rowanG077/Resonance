//! Held GPU captures and diagnostic sidecars for native replay scenarios.
use super::*;
use bevy::{
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    time::TimeUpdateStrategy,
};
use std::{sync::atomic::AtomicU32, thread};

/// Checkpoint replays can continue through Game Over after retiring the field.
/// Those owners require the same held readback as a live field checkpoint.
pub(super) fn screenshot_held(
    app: &mut App,
    path: PathBuf,
    failed: Arc<AtomicBool>,
    written: Arc<AtomicU32>,
) -> Result<()> {
    let fixed_battle = app
        .world()
        .get_resource::<battle::Owner>()
        .and_then(|battle| battle.diagnostic());
    let fixed_presentation = app.world().resource::<Clock>().0.tick();
    let fixed_tick = app
        .world()
        .get_resource::<new_game::Session>()
        .filter(|s| s.ready_for_field && !app.world().resource::<movie::Playback>().active)
        .map(|s| s.field.events.tick());
    {
        // Readback happens on a later render submission. Keep the gameplay
        // snapshot fixed so an effect/opening image cannot depict tick N+1
        // while its sidecar describes tick N. No audio samples are consumed.
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
        app.update();
        playthrough::check_exit(app)?;
        if fixed_tick.is_some() {
            super::model_preview::synchronize_capture(app)?;
        }
    }
    // Present the movie frame selected by the audio already consumed before
    // holding its image and diagnostics through the GPU readback.
    let fixed_scene = crate::saves::recording_scene(app.world())?;
    let completed = Arc::new(AtomicBool::new(false));
    let captured = completed.clone();
    let failure = failed.clone();
    let framebuffer = app.world().resource::<Framebuffer>().0.clone();
    let secondary = super::secondary_motion::diagnostic(app.world_mut());
    let shadows = field_view::shadow_diagnostic(app.world_mut());
    let mut metadata = app.world().get_resource::<new_game::Session>().map(|session| {
        let field = &session.field;
        let (dialogue, retained_dialogue) =
            diagnostic_dialogue(&field.dialogue, &field.events.world.dialogue);
        serde_json::json!({"tick":field.events.tick(),"input_enabled":field.events.world.input_enabled,
            "checkpoint":field.checkpoint().ok(), "restored_checkpoint":session.restored_checkpoint,
            "output_stage":app.world().resource::<display::OutputStage>(),
            "talking":field.talking, "state_tick_locked":true,
            "battle":fixed_battle.as_ref(),
            "secondary_chains":secondary,
            "contact_shadows":shadows,
            "camera":field.events.world.field_camera.as_ref().map(|c|serde_json::json!({"position":c.position,"target":c.target,"fov_degrees":c.fov_degrees()})),
            "actors":field.events.world.actors.iter().map(|(id,a)|serde_json::json!({"id":id,"resource":a.resource,"position":a.position,"heading":a.heading,"target_heading":a.target_heading,"animation":format!("{:?}",a.animation),"attachment":format!("{:?}",a.attachment),"bone_adjustments":format!("{:?}",a.appearance.bone_adjustments)})).collect::<Vec<_>>(),
            "dialogue_preferences":field.events.world.party.as_ref().map(|p| &p.settings.preferences),
            "dialogue_layouts":app.world().resource::<super::field_ui::Artwork>().diagnostic_layouts(field),
            "drawn_effects":app.world().get_resource::<super::field_effects::Artwork>().map(|art|art.diagnostic(&field.events.world)),
            "save_points":field_view::save_point_diagnostic(app.world()),
            "fade":format!("{:?}",field.events.world.fade),
            "dialogue":dialogue, "retained_dialogue":retained_dialogue})
    });
    {
        let metadata = metadata.get_or_insert_with(|| serde_json::json!({}));
        metadata["scene"] = fixed_scene.clone();
        metadata["state_tick_locked"] = serde_json::json!(true);
        metadata["audio_device"] = serde_json::json!(!app.world().resource::<RunOptions>().silent);
        metadata["presentation_counter"] = serde_json::json!(fixed_presentation);
        metadata["output_stage"] =
            serde_json::to_value(app.world().resource::<display::OutputStage>())?;
    }
    app.world_mut()
        .spawn(Screenshot(framebuffer))
        .observe(move |event: On<ScreenshotCaptured>| {
            if let Err(error) = crate::screenshot::write(&event.image, &path, metadata.as_ref()) {
                error!("New Game screenshot failed: {error:#}");
                failed.store(true, Ordering::Release);
            } else {
                written.fetch_add(1, Ordering::Release);
                captured.store(true, Ordering::Release);
            }
        });
    {
        let began = Instant::now();
        while !completed.load(Ordering::Acquire) {
            anyhow::ensure!(
                began.elapsed() < Duration::from_secs(10) && !failure.load(Ordering::Acquire),
                "checkpoint readback failed"
            );
            app.update();
            playthrough::check_exit(app)?;
            anyhow::ensure!(
                fixed_tick.is_none_or(|tick| app
                    .world()
                    .get_resource::<new_game::Session>()
                    .is_some_and(|session| session.field.events.tick() == tick)),
                "field checkpoint advanced gameplay during readback"
            );
            anyhow::ensure!(
                app.world()
                    .get_resource::<battle::Owner>()
                    .and_then(|battle| battle.diagnostic())
                    == fixed_battle
                    && app.world().resource::<Clock>().0.tick() == fixed_presentation,
                "battle checkpoint changed during readback"
            );
            anyhow::ensure!(
                crate::saves::recording_scene(app.world())? == fixed_scene,
                "checkpoint scene changed during held readback"
            );
            thread::sleep(Duration::from_millis(1));
        }
        app.insert_resource(TimeUpdateStrategy::ManualDuration(
            resonance_game::clock::UPDATE_STEP,
        ));
    }
    Ok(())
}

/// Displayed text follows the same owner/visibility gates as the field renderer.
/// Preserve all retained player text separately while close retirement is pending.
fn diagnostic_dialogue(
    players: &std::collections::BTreeMap<u8, resonance_game::dialogue::DialoguePlayer>,
    requests: &std::collections::BTreeMap<u8, resonance_events::dialogue::Dialogue>,
) -> (Vec<String>, Vec<serde_json::Value>) {
    let mut displayed = Vec::new();
    let mut retained = Vec::new();
    for (&slot, player) in players {
        let text: String = player
            .current()
            .glyphs
            .iter()
            .take(player.visible)
            .map(|g| g.character)
            .collect();
        let owns_request = requests
            .get(&slot)
            .is_some_and(|r| r.operation.id() == player.operation.id());
        let pending = player.operation.is_pending();
        let visible = field_ui::displayed_dialogue(player, requests.get(&slot)).is_some();
        if visible {
            displayed.push(text.clone());
        }
        retained.push(serde_json::json!({
            "slot":slot, "operation":player.operation.id(), "owns_request":owns_request,
            "window_visible":player.window_visible(), "displayed":visible,
            "closed":player.closed, "pending":pending, "text":text
        }));
    }
    (displayed, retained)
}

#[cfg(test)]
mod dialogue_capture_tests {
    use super::diagnostic_dialogue;
    use resonance_events::dialogue::{ResolvedMessage, TextToken, flags};
    use resonance_game::dialogue::step_requests;
    use std::collections::BTreeMap;

    fn notice(world: &mut resonance_events::GameWorld, text: &str, flags: u16) {
        world
            .show_notice(
                ResolvedMessage {
                    tokens: vec![TextToken::Text { text: text.into() }],
                },
                flags,
            )
            .unwrap();
    }

    #[test]
    fn dialogue_capture_retains_text_through_hidden_close_and_retirement() {
        let mut world = resonance_events::GameWorld::default();
        notice(&mut world, "Okay!", 0);
        let mut players = BTreeMap::new();
        step_requests(&mut world, &mut players, false, false).unwrap();
        let (displayed, retained) = diagnostic_dialogue(&players, &world.dialogue);
        assert_eq!(displayed, [""]);
        assert_eq!(retained[0]["window_visible"], true);
        assert_eq!(retained[0]["owns_request"], true);
        assert_eq!(retained[0]["pending"], true);
        for _ in 0..32 {
            step_requests(&mut world, &mut players, false, false).unwrap();
        }
        assert!(players[&0].accepts_input());
        assert_eq!(diagnostic_dialogue(&players, &world.dialogue).0, ["Okay!"]);

        step_requests(&mut world, &mut players, true, false).unwrap();
        let (displayed, retained) = diagnostic_dialogue(&players, &world.dialogue);
        assert_eq!(displayed, ["Okay!"]);
        assert_eq!(retained[0]["displayed"], true);
        // Confirmation draws once, followed by hidden updates while
        // the player text and operation remain retained until retirement.
        for _ in 0..3 {
            step_requests(&mut world, &mut players, false, false).unwrap();
            let (displayed, retained) = diagnostic_dialogue(&players, &world.dialogue);
            assert!(displayed.is_empty());
            assert_eq!(retained[0]["text"], "Okay!");
            assert_eq!(retained[0]["closed"], false);
            assert_eq!(retained[0]["pending"], true);
            assert_eq!(retained[0]["window_visible"], false);
        }
        step_requests(&mut world, &mut players, false, false).unwrap();
        let (displayed, retained) = diagnostic_dialogue(&players, &world.dialogue);
        assert!(displayed.is_empty());
        assert_eq!(retained[0]["text"], "Okay!");
        assert_eq!(retained[0]["closed"], true);
        assert_eq!(retained[0]["pending"], false);
    }

    #[test]
    fn dialogue_capture_admits_concurrent_windows_independently() {
        let mut world = resonance_events::GameWorld::default();
        notice(&mut world, "Still open", flags::PERSISTENT | flags::INSTANT);
        notice(&mut world, "Closing", flags::INSTANT);
        let mut players = BTreeMap::new();
        step_requests(&mut world, &mut players, false, false).unwrap();
        // INSTANT removes expansion and opacity delay, not glyph pacing.
        for _ in 0..64 {
            if players
                .values()
                .all(|p| p.fully_revealed() && p.accepts_input())
            {
                break;
            }
            step_requests(&mut world, &mut players, false, false).unwrap();
        }
        assert!(
            players
                .values()
                .all(|p| p.fully_revealed() && p.accepts_input())
        );
        assert_eq!(
            diagnostic_dialogue(&players, &world.dialogue).0,
            ["Still open", "Closing"]
        );
        step_requests(&mut world, &mut players, true, false).unwrap();
        assert_eq!(
            diagnostic_dialogue(&players, &world.dialogue).0,
            ["Still open", "Closing"]
        );
        for _ in 0..3 {
            step_requests(&mut world, &mut players, false, false).unwrap();
            let (displayed, retained) = diagnostic_dialogue(&players, &world.dialogue);
            assert_eq!(displayed, ["Still open"]);
            assert_eq!(retained.len(), 2);
            assert_eq!(retained[0]["slot"], 0);
            assert_eq!(retained[0]["displayed"], true);
            assert_eq!(retained[1]["slot"], 1);
            assert_eq!(retained[1]["text"], "Closing");
            assert_eq!(retained[1]["displayed"], false);
            assert_eq!(retained[1]["pending"], true);
        }
        step_requests(&mut world, &mut players, false, false).unwrap();
        let (displayed, retained) = diagnostic_dialogue(&players, &world.dialogue);
        assert_eq!(displayed, ["Still open"]);
        assert_eq!(retained[1]["text"], "Closing");
        assert_eq!(retained[1]["closed"], true);
        assert_eq!(retained[1]["pending"], false);
        // A retained player whose request has gone is not renderer-admitted.
        world.dialogue.remove(&0);
        let (displayed, retained) = diagnostic_dialogue(&players, &world.dialogue);
        assert!(displayed.is_empty());
        assert_eq!(retained[0]["text"], "Still open");
        assert_eq!(retained[0]["owns_request"], false);
        assert_eq!(retained[0]["pending"], true);
    }
}
