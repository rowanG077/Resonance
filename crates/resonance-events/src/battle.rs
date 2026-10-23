//! Field-to-battle handoff. The field keeps its VM and clocks while the game
//! prepares and runs the encounter, then resumes the original caller once.
use crate::{ACTOR_CONTACT_HEIGHT, Operation};

/// Distance from the player for additional encounters within actor contact height.
pub const COMBINED_BATTLE_RADIUS: f32 = 500.;

/// Script-selected restrictions and percentage adjustments for the next encounter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Rules {
    pub modifiers: u16,
    pub disabled_commands: u8,
    pub coliseum: bool,
    pub attack_adjustment: i8,
    pub defense_adjustment: i8,
    pub intelligence_adjustment: i8,
}

impl Rules {
    pub const EXPERIENCE_DOUBLE: u16 = 1 << 0;
    pub const GALD_DOUBLE: u16 = 1 << 1;
    pub const ITEM_DROP_BONUS: u16 = 1 << 2;
    pub const UNISON_FULL: u16 = 1 << 3;
    pub const ATTACK_UP: u16 = 1 << 4;
    pub const ATTACK_DOWN: u16 = 1 << 5;
    pub const DEFENSE_UP: u16 = 1 << 6;
    pub const DEFENSE_DOWN: u16 = 1 << 7;
    pub const ACCURACY_UP: u16 = 1 << 8;
    pub const ACCURACY_DOWN: u16 = 1 << 9;
    pub const SUPPORTED_MODIFIERS: u16 = (1 << 10) - 1;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefeatPolicy {
    GameOver,
    ResumeEvent,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum Encounter {
    Formation(u16),
    /// Resolved formations fought together, including repeated selections.
    Combined(Vec<u16>),
    /// Weighted BTLusual encounter pool; selection belongs to the combat owner.
    Pool(u16),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setup {
    pub encounter: Encounter,
    pub arena: u16,
    pub defeat: DefeatPolicy,
    /// An explicit music selection; None uses the current world's battle theme.
    pub music: Option<u16>,
    /// Native callback/route overrides retained for the encounter owner.
    pub route: [i32; 5],
}

impl Setup {
    pub fn formation(&self) -> anyhow::Result<u16> {
        Ok(self.formations()?[0])
    }

    pub fn formations(&self) -> anyhow::Result<&[u16]> {
        match &self.encounter {
            Encounter::Formation(id) => Ok(std::slice::from_ref(id)),
            Encounter::Combined(ids) if !ids.is_empty() => Ok(ids),
            Encounter::Combined(_) => anyhow::bail!("empty combined encounter"),
            Encounter::Pool(id) => {
                anyhow::bail!("encounter pool {id} has no prepared formation selector")
            }
        }
    }
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Escaped = 1,
    Victory = 2,
    Defeat = 3,
}

impl TryFrom<i32> for Outcome {
    type Error = String;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Escaped),
            2 => Ok(Self::Victory),
            3 => Ok(Self::Defeat),
            _ => Err(format!("invalid battle result {value}")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub setup: Setup,
    pub additional_encounters: Vec<Encounter>,
    pub hunting_symbol: Option<u32>,
    /// Additional field symbols, paired with their instance to avoid retiring replacements.
    joined_symbols: Vec<(i32, u64)>,
    pub rules: Rules,
    /// Mode 5 overrides the next scene's initial black/white clear.
    pub transition_white: Option<bool>,
    pub(crate) operation: Operation,
}

impl Request {
    pub fn formation_count(&self) -> usize {
        match &self.setup.encounter {
            Encounter::Combined(ids) => ids.len(),
            _ => 1 + self.additional_encounters.len(),
        }
    }

    pub fn id(&self) -> u64 {
        self.operation.id()
    }

    pub fn is_pending(&self) -> bool {
        self.operation.is_pending()
    }

    /// Scene owners retain a clone while combat owns the published request.
    pub fn result(&self) -> Result<Option<Outcome>, String> {
        match self.operation.progress().outcome {
            None => Ok(None),
            Some(crate::Outcome::Completed(Some(value))) => Outcome::try_from(value).map(Some),
            Some(crate::Outcome::Completed(None)) => {
                Err("battle completed without a result".into())
            }
            Some(crate::Outcome::Cancelled) => Err("battle request was cancelled".into()),
        }
    }

    /// Complete after writeback and results presentation. Fatal defeat retires
    /// the field instead, cancelling its caller and outstanding callbacks.
    pub fn complete(&self, outcome: Outcome) -> Result<(), String> {
        if outcome == Outcome::Defeat && self.setup.defeat == DefeatPolicy::GameOver {
            return Err("battle defeat requires game over".into());
        }
        self.operation.complete(Some(outcome as i32))
    }
}

impl crate::GameWorld {
    /// Publish an encounter from a non-scripted scene, with cancellation owned
    /// by the same runtime as scripted services. Combat may take the request;
    /// the world retains the returned clone until results have been applied.
    pub fn request_battle(&mut self, setup: Setup) -> Result<Request, String> {
        if self.battle_request.is_some() {
            return Err("nested battle request".into());
        }
        let rules = self
            .party
            .as_ref()
            .ok_or("battle requires a party")?
            .battle_rules;
        let request = Request {
            setup,
            additional_encounters: Vec::new(),
            hunting_symbol: None,
            joined_symbols: Vec::new(),
            rules,
            transition_white: self.next_transition_white.take(),
            operation: self.operations.begin()?,
        };
        self.battle_request = Some(request.clone());
        Ok(request)
    }

    /// Only ordinary symbol encounters recruit neighbors. Their caller keeps
    /// ownership of scripted victory/escape handling for the contacting symbol.
    pub(crate) fn request_symbol_battle(
        &mut self,
        symbol: i32,
        setup: Setup,
    ) -> Result<Request, String> {
        let mut joined = vec![symbol];
        if self
            .party
            .as_ref()
            .is_some_and(|p| p.settings.preferences.combined_battles)
        {
            let event = self.actors[&symbol].enemy.as_ref().unwrap().event;
            let position = self
                .actors
                .get(&self.controlled_actor)
                .ok_or("battle player is missing")?
                .position;
            for (&id, actor) in &self.actors {
                if id != symbol
                    && actor.visible
                    && !actor.retiring
                    && !actor.appearance.model_hidden
                    && actor.enemy.as_ref().is_some_and(|enemy| {
                        enemy.event == event
                            && enemy.pause_ticks == 0
                            && enemy.event_parameters[1] as u16 == setup.arena
                    })
                    && (actor.position[2] - position[2]).abs() <= ACTOR_CONTACT_HEIGHT
                    && actor
                        .position
                        .iter()
                        .zip(position)
                        .map(|(a, b)| (a - b).powi(2))
                        .sum::<f32>()
                        <= COMBINED_BATTLE_RADIUS.powi(2)
                {
                    joined.push(id);
                }
            }
        }
        let mut request = self.request_battle(setup)?;
        request.hunting_symbol = Some(self.actors[&symbol].resource);
        for id in joined.into_iter().skip(1) {
            let actor = &self.actors[&id];
            request.additional_encounters.push(Encounter::Pool(
                actor.enemy.as_ref().unwrap().event_parameters[0] as u16,
            ));
            request.joined_symbols.push((id, actor.instance));
        }
        self.battle_request = Some(request.clone());
        Ok(request)
    }

    pub fn complete_battle(&mut self, request: &Request, outcome: Outcome) -> Result<(), String> {
        request.complete(outcome)?;
        self.enemy_encounter_cooldown = 60;
        if let Some(white) = request.transition_white {
            self.fade = Some(crate::Fade {
                start_tick: self.tick,
                duration: 0,
                from: 255.,
                to: 255.,
                white,
            });
        }
        for &(id, instance) in &request.joined_symbols {
            if !self
                .actors
                .get(&id)
                .is_some_and(|actor| actor.instance == instance)
            {
                continue;
            }
            if outcome == Outcome::Victory {
                self.actors.remove(&id);
                self.overlays.remove(&id);
                self.emotes.remove(&id);
                self.billboards
                    .retain(|_, particle| particle.owner != Some(id));
                self.refractions
                    .retain(|_, particle| particle.owner != Some(id));
            }
        }
        Ok(())
    }
}

impl crate::GameWorld {
    /// Temporary exploration mode: preserve the victory result expected by the
    /// suspended script without constructing a combat scene or awarding loot.
    pub fn skip_battle_as_victory(&mut self) -> Result<bool, String> {
        let Some(request) = self.battle_request.clone() else {
            return Ok(false);
        };
        self.complete_battle(&request, Outcome::Victory)?;
        if let Some(party) = &mut self.party {
            let active: Vec<_> = party
                .formation
                .iter()
                .take(4)
                .map(|&id| (id, party.members[usize::from(id - 1)].technique_balance))
                .collect();
            party
                .begin_battle(&active)
                .map_err(|error| error.to_string())?;
            party.battles.combat_ticks = 0;
            party.battles.formation_count = u32::try_from(request.formation_count())
                .map_err(|_| "invalid battle formation count")?;
        }
        // Restore after the resumed caller has applied its battle-transition
        // fade, so that fade cannot leave the retained field track muted.
        self.restore_battle_music = true;
        self.battle_request = None;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Actor, Enemy, GameWorld, party::Party};

    fn world(enabled: bool) -> GameWorld {
        let mut world = GameWorld::default();
        let mut party = Party::new(&crate::party::tests::data(), Default::default()).unwrap();
        party.settings.preferences.combined_battles = enabled;
        world.party = Some(party);
        world.controlled_actor = 0;
        world.insert_actor(0, Actor::new(0, [1000., 0., 0.]));
        for (id, x) in [
            (10, 1100.),
            (11, 500.),
            (12, 1500.),
            (13, 1501.),
            (14, 1010.),
        ] {
            let mut actor = Actor::new(0, [x, 0., 0.]);
            actor.visible = id != 14;
            actor.enemy = Some(Enemy {
                event: 42,
                behavior: 0,
                normal_speed: 1.,
                alert_speed: 2.,
                random_turns: 0,
                alert_response: 0,
                sight_angle: 0.,
                sight_distance: 0.,
                alerted: false,
                event_parameters: [id as i16, 2],
                pause_ticks: 0,
                pause_outside_view: false,
                script_flag: false,
                reaction: crate::effect::StunEffect::None,
            });
            world.insert_actor(id, actor);
        }
        world
    }

    fn request(world: &mut GameWorld) -> Request {
        world
            .request_symbol_battle(
                10,
                Setup {
                    encounter: Encounter::Pool(10),
                    arena: 2,
                    defeat: DefeatPolicy::ResumeEvent,
                    music: None,
                    route: [0; 5],
                },
            )
            .unwrap()
    }

    #[test]
    fn combined_battles_use_player_radius_and_retire_only_defeated_neighbors() {
        let mut disabled = world(false);
        assert_eq!(request(&mut disabled).formation_count(), 1);
        for outcome in [Outcome::Victory, Outcome::Escaped, Outcome::Defeat] {
            let mut world = world(true);
            let request = request(&mut world);
            assert_eq!(request.formation_count(), 3);
            assert_eq!(
                request.additional_encounters,
                [Encounter::Pool(11), Encounter::Pool(12)]
            );
            world.complete_battle(&request, outcome).unwrap();
            assert_eq!(request.result().unwrap(), Some(outcome));
            assert!(
                world.actors.contains_key(&10),
                "the initiating script owns its symbol"
            );
            assert!(world.actors.contains_key(&13) && world.actors.contains_key(&14));
            for id in [11, 12] {
                assert_eq!(world.actors.contains_key(&id), outcome != Outcome::Victory);
                if outcome != Outcome::Victory {
                    let enemy = world.actors[&id].enemy.as_ref().unwrap();
                    assert_eq!(enemy.pause_ticks, 0);
                    assert!(enemy.stun_effect().is_none());
                }
            }
            assert_eq!(world.enemy_encounter_cooldown, 60);
        }
        let mut world = world(true);
        let request = request(&mut world);
        world.insert_actor(11, Actor::new(0, [0.; 3]));
        world.complete_battle(&request, Outcome::Victory).unwrap();
        assert!(
            world.actors.contains_key(&11),
            "replacement instances must survive"
        );
    }

    #[test]
    fn combined_battles_exclude_enemies_above_and_below_contact_height() {
        for height in [-200., -151., -150., -100., 0., 100., 150., 151., 200.] {
            let mut world = world(true);
            for actor in world.actors.values_mut() {
                actor.position[2] = 1000.;
            }
            world.actors.get_mut(&11).unwrap().position = [1100., 0., 1000. + height];
            let request = request(&mut world);
            let joins = height.abs() <= 150.;
            assert_eq!(
                request.additional_encounters.contains(&Encounter::Pool(11)),
                joins,
                "enemy height relative to player: {height}"
            );
            assert!(request.additional_encounters.contains(&Encounter::Pool(12)));
            world.complete_battle(&request, Outcome::Victory).unwrap();
            assert_eq!(world.actors.contains_key(&11), !joins);
        }
    }
}
