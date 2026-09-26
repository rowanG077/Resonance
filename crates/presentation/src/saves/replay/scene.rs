//! Observe real application owners after a fatal field has been retired.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Phase {
    Field,
    Battle,
    GameOver,
    Load,
    Loading,
    Title,
}

#[derive(Default)]
struct Owners {
    session: bool,
    battle: bool,
    game_over_loading: Option<bool>,
    load: bool,
    starting: bool,
    field_loading: bool,
    title: bool,
}

impl Owners {
    fn phase(self) -> Result<Phase> {
        ensure!(
            self.session
                || !self.battle && !self.field_loading && self.game_over_loading != Some(false),
            "recording lost a retained field session"
        );
        if self.starting || self.field_loading {
            return Ok(Phase::Loading);
        }
        if self.load {
            return Ok(Phase::Load);
        }
        ensure!(
            self.game_over_loading != Some(true),
            "Game Over load lost its destination owner"
        );
        if self.game_over_loading.is_some() {
            return Ok(Phase::GameOver);
        }
        if self.battle {
            return Ok(Phase::Battle);
        }
        if self.session {
            return Ok(Phase::Field);
        }
        // A stale root Menu is always present. The live title VM is removed
        // by new_game::activate and recreated only by the real title return.
        ensure!(
            self.title,
            "recording has neither a field session nor a live scene owner"
        );
        Ok(Phase::Title)
    }
}

pub(super) fn phase(world: &World) -> Result<Phase> {
    Owners {
        session: world.contains_resource::<new_game::Session>(),
        battle: world.contains_resource::<crate::battle::Owner>(),
        game_over_loading: world
            .get_resource::<crate::game_over::Active>()
            .map(|active| active.loading()),
        load: world.contains_resource::<super::super::title::LoadMenu>(),
        starting: world.contains_resource::<loading::Pending>()
            || world.contains_resource::<new_game::Request>()
            || world.contains_resource::<crate::game_over::Returning>()
            || super::super::menu::loading(world),
        field_loading: world.contains_resource::<loading::FieldPending>(),
        title: world.contains_resource::<crate::Events>(),
    }
    .phase()
}

pub(super) fn ready(world: &mut World, phase: Phase) -> bool {
    match phase {
        Phase::Loading => false,
        Phase::Title => world.resource::<crate::timing::Ready>().0,
        Phase::Load => {
            world
                .get_resource::<crate::field_ui::MenuOverlay>()
                .is_some_and(|art| art.ready(world.resource::<Assets<Image>>()))
                && world
                    .get_resource::<super::super::title::LoadMenu>()
                    .is_some_and(|menu| !menu.0.busy)
        }
        Phase::Field | Phase::Battle | Phase::GameOver => {
            field_view::ready(world)
                && world.get_resource::<crate::battle::Owner>().map_or_else(
                    || {
                        world
                            .resource::<loading::Resident>()
                            .active
                            .load(Ordering::Acquire)
                    },
                    |battle| battle.capture_ready(),
                )
                && world.get_resource::<new_game::Session>().is_some_and(|s| {
                    s.ready_for_field
                        && s.audio.is_none()
                        && s.field.events.world.field_transition.is_none()
                        && s.field.menu.as_ref().is_none_or(|m| !m.busy)
                })
        }
    }
}

pub(crate) fn diagnostic(world: &World) -> Result<serde_json::Value> {
    let phase = phase(world)?;
    let field = world
        .get_resource::<new_game::Session>()
        .map(|session| {
            let field = &session.field;
            Ok::<_, anyhow::Error>(serde_json::json!({
                "map_id":field.map_id, "story":field.story_progress()?,
                "tick":field.events.tick(), "effect_tick":field.effect_clock.tick(),
                "battle_pending":field.events.battle_pending(),
                "played_ticks":field.play_time.total(), "session_ticks":field.play_time.session(),
            }))
        })
        .transpose()?;
    Ok(serde_json::json!({
        "phase":phase, "session_retained":field.is_some(), "field":field,
        "battle_entry":world.get_resource::<crate::battle::Owner>()
            .and_then(|battle| battle.entry_diagnostic()),
        "game_over":world.get_resource::<crate::game_over::Active>().map(|active|active.diagnostic()),
        "load":world.get_resource::<super::super::title::LoadMenu>().map(|load| {
            let menu = &load.0;
            serde_json::json!({"page":format!("{:?}",menu.page), "bank":menu.bank,
                "slot":menu.slot, "focus":format!("{:?}",menu.focus), "tick":menu.tick,
                "notice":menu.notice, "confirmation":menu.confirmation,
                "busy":menu.busy, "closed":menu.closed,
                "activating":super::super::menu::loading(world)})
        }),
        "title":world.get_resource::<crate::Events>().map(|events| serde_json::json!({
            "menu":world.resource::<crate::Menu>().0, "script_tick":events.0.tick(),
            "preparing":world.contains_resource::<crate::game_over::Returning>()
        })),
        "audio":{
            "battle_owner":world.contains_resource::<crate::battle_audio::Playback>(),
            "field_source_frame":world.get_resource::<crate::field_audio::Control>().map(|control|control.rendered_frames()),
            "title_owner":world.get_resource::<crate::audio::MenuSounds>().is_some_and(|sounds|sounds.control.is_some()),
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retired_field_requires_an_actual_destination_owner() {
        for (owners, expected) in [
            (
                Owners {
                    game_over_loading: Some(true),
                    load: true,
                    ..Default::default()
                },
                Phase::Load,
            ),
            (
                Owners {
                    starting: true,
                    ..Default::default()
                },
                Phase::Loading,
            ),
            (
                Owners {
                    title: true,
                    ..Default::default()
                },
                Phase::Title,
            ),
            (
                Owners {
                    session: true,
                    game_over_loading: Some(false),
                    ..Default::default()
                },
                Phase::GameOver,
            ),
        ] {
            assert_eq!(owners.phase().unwrap(), expected);
        }
        assert!(Owners::default().phase().is_err());
        assert!(
            Owners {
                battle: true,
                ..Default::default()
            }
            .phase()
            .is_err()
        );
        assert!(
            Owners {
                field_loading: true,
                ..Default::default()
            }
            .phase()
            .is_err()
        );
        assert!(
            Owners {
                game_over_loading: Some(false),
                ..Default::default()
            }
            .phase()
            .is_err()
        );
        assert!(
            Owners {
                game_over_loading: Some(true),
                ..Default::default()
            }
            .phase()
            .is_err()
        );
    }

    #[test]
    fn a_stale_title_menu_does_not_authorize_a_missing_field() {
        let mut world = World::new();
        world.insert_resource(crate::Menu(resonance_game::TitleState::default()));
        assert!(phase(&world).is_err());
        world.insert_resource(new_game::Request(None));
        assert_eq!(phase(&world).unwrap(), Phase::Loading);
        world.remove_resource::<new_game::Request>();
        world.insert_resource(super::super::super::title::LoadMenu::new());
        assert_eq!(phase(&world).unwrap(), Phase::Load);
        let metadata = diagnostic(&world).unwrap();
        assert_eq!(metadata["phase"], "load");
        assert_eq!(metadata["session_retained"], false);
        assert!(metadata["field"].is_null());
        assert!(metadata["title"].is_null());
        assert_eq!(metadata["load"]["slot"], 0);
        assert_eq!(metadata["load"]["tick"], 0);
    }
}
