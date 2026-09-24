//! Target selection only. Opening, reward notices and closing are authored events.
use anyhow::{Context, Result};
use resonance_events::EventRuntime;
use std::sync::Arc;

#[derive(Default)]
pub(super) struct Treasures {
    pub event: Option<Arc<crate::authored::PreparedEvent>>,
}
impl Treasures {
    pub fn step(&self, events: &mut EventRuntime, confirm: bool) -> Result<()> {
        if confirm && events.player_has_control() {
            let world = &events.world;
            let player = world
                .actors
                .get(&world.controlled_actor)
                .context("treasure interaction lacks player")?;
            let heading = player.heading.to_radians();
            let target = world
                .treasures
                .iter()
                .enumerate()
                .filter(|(_, chest)| {
                    !world
                        .party
                        .as_ref()
                        .is_some_and(|party| party.travel.opened_treasures.contains(&chest.flag))
                })
                .filter_map(|(index, chest)| {
                    let position = world.actors.get(&chest.actor)?.position;
                    let dx = position[0] - player.position[0];
                    let dy = position[1] - player.position[1];
                    let distance = dx.hypot(dy);
                    (distance < 140.
                        && (position[2] - player.position[2]).abs() < 145.
                        && (distance == 0.
                            || (dx * heading.sin() - dy * heading.cos()) / distance > 0.4))
                        .then_some((index, distance))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(index, _)| index);
            if let Some(index) = target {
                self.event
                    .as_ref()
                    .context("treasure script was not prepared")?
                    .start_with_arguments(events, &[i32::try_from(index)?])?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_events::{
        Actor, Animation, GameWorld, ResourceLibrary, TreasureChest, party::Party,
    };
    use std::sync::Arc;

    fn prepared_treasures() -> Treasures {
        struct Resources;
        impl crate::authored::Resources for Resources {
            fn asset(&mut self, _: &crate::authored::AssetReference) -> Result<()> {
                anyhow::bail!("unexpected treasure asset")
            }
            fn message(&mut self, _: &str) -> Result<()> {
                Ok(())
            }
            fn substitution(&mut self, _: symphonia_script::authored::Type) -> Result<()> {
                Ok(())
            }
        }
        let sources = symphonia_script_tools::SourceTree::load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../scripts"
        ))
        .unwrap();
        let event = crate::authored::PreparedEvent::prepare(
            &mut Default::default(),
            &sources,
            crate::authored::Entry {
                module: "field::treasure",
                task: "open",
                arguments: &[0],
            },
            &mut Resources,
        )
        .unwrap();
        Treasures {
            event: Some(Arc::new(event)),
        }
    }

    fn fixture(full: bool) -> EventRuntime {
        let data = Arc::new(serde_json::from_value(serde_json::json!({
            "version":1,"executable_sha256":"0".repeat(64),"experience":[0,0,10],
            "items":[{"equipment_kind":null,"allowed_characters":511,"stack_limit":20}],
            "characters":vec![serde_json::json!({"level":1,"experience":0,"affinity":0,
                "base_stats":[100,20,30,40,50,60,70],"luck":10,"overlimit":0,
                "equipment":vec![0;6],"techniques":[],"allowed_techniques":[],"shortcuts":vec![0;4],
                "growth":vec![serde_json::json!({"base":1,"random":1,"title_bonus":0});7],"level_techniques":{}});9]
        })).unwrap());
        let mut party = Party::new(&data, Default::default()).unwrap();
        if full {
            party.items.insert(0, 20);
        }
        let mut world = GameWorld::default();
        world.controlled_actor = 1;
        world.input_enabled = true;
        world.party = Some(party);
        world.actors.insert(1, Actor::new(1, [0.; 3]));
        let mut chest = Actor::new(2, [0., -100., 0.]);
        let mut animation = Animation::new(2, 12, 4, 0);
        animation.rate = 0.;
        animation.repeat = false;
        chest.animation = Some(animation);
        world.actors.insert(2, chest);
        world.treasures.push(TreasureChest {
            actor: 2,
            flag: 9,
            reward: resonance_events::TreasureReward::Item(0),
            kind: resonance_events::TreasureKind::UnknownId0,
        });
        let program = Arc::new(
            symphonia_script::Program::decode(&[0, 4, 0, 0, 0, 0, 0, 0, 0x20, 0xff]).unwrap(),
        );
        let mut text = resonance_content::session::GameText::default();
        text.items.insert(0, "Apple Gel".into());
        EventRuntime::with_state(
            program,
            Arc::new(ResourceLibrary {
                session_data: Some(data),
                text: Arc::new(text),
                ..Default::default()
            }),
            world,
            Default::default(),
        )
        .unwrap()
    }
    #[test]
    fn reward_waits_for_opening_and_acknowledgement_then_persists_once() {
        let mut events = fixture(false);
        let service = prepared_treasures();
        service.step(&mut events, true).unwrap();
        events.step().unwrap();
        assert!(!events.world.input_enabled);
        assert!(events.world.party.as_ref().unwrap().items.is_empty());
        events.world.tick = 10;
        events.step().unwrap();
        assert_eq!(events.world.party.as_ref().unwrap().items[&0], 1);
        assert!(
            !events
                .world
                .party
                .as_ref()
                .unwrap()
                .travel
                .opened_treasures
                .contains(&9)
        );
        events.world.dialogue[&0].operation.complete(None).unwrap();
        events.step().unwrap();
        assert!(events.world.input_enabled);
        assert!(
            events
                .world
                .party
                .as_ref()
                .unwrap()
                .travel
                .opened_treasures
                .contains(&9)
        );
        service.step(&mut events, true).unwrap();
        events.step().unwrap();
        assert!(events.player_has_control());
        assert_eq!(events.world.party.as_ref().unwrap().items[&0], 1);
    }
    #[test]
    fn full_inventory_closes_the_chest_and_leaves_it_available() {
        let mut events = fixture(true);
        let service = prepared_treasures();
        service.step(&mut events, true).unwrap();
        events.step().unwrap();
        events.world.tick = 10;
        events.step().unwrap();
        events.world.dialogue[&0].operation.complete(None).unwrap();
        events.step().unwrap();
        assert!(!events.world.input_enabled);
        assert_eq!(
            events.world.actors[&2].animation.as_ref().unwrap().rate,
            -0.8
        );
        for _ in 0..6 {
            events.step().unwrap();
        }
        assert!(events.world.input_enabled);
        assert!(
            events
                .world
                .party
                .as_ref()
                .unwrap()
                .travel
                .opened_treasures
                .is_empty()
        );
        assert_eq!(events.world.party.as_ref().unwrap().items[&0], 20);
    }

    #[test]
    fn gald_rewards_use_the_same_script_and_persist_after_acknowledgement() {
        let mut events = fixture(false);
        events.world.treasures[0].reward = resonance_events::TreasureReward::Gald(200);
        events.world.treasures[0].kind = resonance_events::TreasureKind::CustomModel1;
        let service = prepared_treasures();
        service.step(&mut events, true).unwrap();
        for _ in 0..12 {
            events.step().unwrap();
        }
        assert_eq!(events.world.party.as_ref().unwrap().gald, 200);
        events.world.dialogue[&0].operation.complete(None).unwrap();
        events.step().unwrap();
        assert!(events.player_has_control());
        assert!(
            events
                .world
                .party
                .as_ref()
                .unwrap()
                .travel
                .opened_treasures
                .contains(&9)
        );
    }
}
