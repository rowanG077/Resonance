//! Scene completion and visible speakers, independent of dialogue wording.
use super::Fixture;
use super::{enter, replay};
use anyhow::Result;

#[test]
#[ignore = "requires locally cooked Iselia scenes; no devices"]
fn iselia_scenes_keep_one_lloyd_and_visible_speakers() -> Result<()> {
    for (map, story, end) in [
        (195, 20_303_000, 20_305_000),
        (197, 20_305_000, 20_307_000),
        (193, 20_307_000, 20_308_000),
    ] {
        let mut field = enter(Fixture::Iselia, map, Some(story))?;
        let mut heard_kratos = false;
        let mut full_party = false;
        let battles = replay(&mut field, |f| {
            let actors = &f.events.world.actors;
            full_party |= [1, 2, 3, 9].iter().all(|id| actors.contains_key(id));
            assert!(
                actors
                    .values()
                    .filter(|a| a.resource == 1 && a.visible && !a.appearance.model_hidden)
                    .count()
                    <= 1,
                "duplicate Lloyd in map {map}"
            );
            for dialogue in f.events.world.dialogue.values() {
                if dialogue.speaker_actor == Some(9) {
                    heard_kratos = true;
                    assert!(
                        actors
                            .get(&9)
                            .is_some_and(|a| a.visible && !a.appearance.model_hidden)
                    );
                }
            }
            f.story_progress().unwrap() == end
                && (f.player_has_control() || f.events.world.field_transition.is_some())
        })?;
        if map == 197 {
            assert!(heard_kratos && full_party);
            assert_eq!(battles, 1);
        }
    }
    Ok(())
}
