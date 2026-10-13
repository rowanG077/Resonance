//! Prepare victory clips and result postures before battle activation.
use super::{lifecycle::victory_selection::ColetteState, party::Character};
use anyhow::{Context, Result, ensure};
use resonance_battle::{ActorAvailability, ActorId, Battle, Cue, ModelDefinition, MotionBinding};
use resonance_content::{
    animation::Motion,
    battle_model,
    battle_victory::{Event, PATH, Performances},
    prepared::{Cache, Files},
};
use std::{path::Path, sync::Arc};

const FIRST_VICTORY_CLIP: u16 = 0x100;
const WEAK_HP_PERCENT: u8 = 25;

#[derive(Debug, Clone)]
pub struct Performance {
    pub character: u8,
    pub selector: u8,
    pub motion: MotionBinding,
    pub timeline: Option<Vec<(u16, Event)>>,
}

/// A result performance has one update clock and a short, ordered list of changes.
pub struct Playing {
    actor: ActorId,
    performance: Performance,
    elapsed: u16,
    next: usize,
}

impl Playing {
    pub fn start(battle: &mut Battle, actor: ActorId, performance: &Performance) -> Result<Self> {
        let mut playing = Self {
            actor,
            performance: performance.clone(),
            elapsed: 0,
            next: 0,
        };
        if performance.timeline.is_some() {
            playing.apply(battle)?;
        } else {
            battle.play_victory_pose(actor, performance.motion)?;
        }
        Ok(playing)
    }

    pub fn advance(&mut self, battle: &mut Battle) -> Result<()> {
        self.elapsed = self.elapsed.saturating_add(1);
        self.apply(battle)
    }

    fn apply(&mut self, battle: &mut Battle) -> Result<()> {
        let Some(events) = &self.performance.timeline else {
            return Ok(());
        };
        while let Some(&(at, event)) = events.get(self.next) {
            if at > self.elapsed {
                break;
            }
            match event {
                Event::Expression(expression) => {
                    battle.set_result_posture(self.actor, None, expression)?;
                }
                Event::Idle { blend } => {
                    battle.play_result_pose(
                        self.actor,
                        MotionBinding {
                            model: self.performance.motion.model,
                            clip: super::model::MotionRole::Idle.id(),
                        },
                        resonance_battle::Pose {
                            blend,
                            repeat: true,
                            ..Default::default()
                        },
                    )?;
                }
                Event::Pose {
                    start,
                    end,
                    rate,
                    blend,
                    repeat,
                } => {
                    battle.play_result_pose(
                        self.actor,
                        self.performance.motion,
                        resonance_battle::Pose {
                            frame: start,
                            end,
                            rate,
                            blend,
                            repeat,
                            loop_start: start,
                            restart: true,
                        },
                    )?;
                }
            }
            self.next += 1;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PostureBinding {
    character: u8,
    healthy: Option<MotionBinding>,
    weak: Option<MotionBinding>,
}

pub fn prepare_posture(
    files: &Files,
    character: u8,
    model: Option<&ModelDefinition>,
) -> Result<PostureBinding> {
    ensure!((1..=9).contains(&character), "invalid result character");
    let motion = |clip| {
        let Some(model) = model else {
            return Ok(None);
        };
        files.diagnostics().attempt(
            "result posture",
            (|| {
                model.motions.get(&clip).context("missing result posture")?;
                Ok(MotionBinding {
                    model: model.resource,
                    clip,
                })
            })(),
        )
    };
    let healthy = motion(super::model::MotionRole::Idle.id())?;
    let weak = motion(super::model::MotionRole::WeakIdle.id())?.or(healthy);
    Ok(PostureBinding {
        character,
        healthy,
        weak,
    })
}

/// Choose result poses before replacing combat availability and controllers.
pub fn construct_result_actors(
    battle: &mut Battle,
    leader: ActorId,
    roster: &[(ActorId, u8)],
    postures: &[PostureBinding],
    colette_state: u32,
) -> Result<Vec<Cue>> {
    let colette_state = ColetteState::from(colette_state);
    let mut cues = Vec::new();
    for &(id, character) in roster {
        let binding = postures.iter().find(|row| row.character == character);
        let actor = battle
            .actors()
            .get(id.index())
            .context("missing result actor")?;
        let petrified = actor.availability == ActorAvailability::Petrified;
        let weak = !petrified
            && (!actor.available()
                || (id != leader && actor.hp_percent() <= i16::from(WEAK_HP_PERCENT)));
        let sealed = character == Character::Colette as u8 && colette_state == ColetteState::Sealed;
        cues.extend(battle.reset_result_actor(id)?);
        if !petrified && !weak {
            battle.end_overlimit(id)?;
        }
        if !petrified || sealed {
            let motion = binding
                .filter(|_| !petrified)
                .and_then(|binding| if weak { binding.weak } else { binding.healthy });
            let expression = if sealed {
                [15, 0, 0, 0]
            } else if weak {
                [10, 5, 0, 0]
            } else {
                [0; 4]
            };
            battle.set_result_posture(id, motion, expression)?;
        }
    }
    // Hide weapons only for a selected Colette in her alternate state.
    if matches!(
        colette_state,
        ColetteState::Alternate | ColetteState::Silent
    ) && roster
        .iter()
        .any(|&(id, character)| id == leader && character == Character::Colette as u8)
    {
        battle.hide_result_weapons(leader)?;
    }
    Ok(cues)
}

pub fn load(
    root: &Path,
    files: Files,
    characters: &[u8],
    cache: &mut Cache,
    cancelled: impl Fn() -> bool,
) -> Result<(Files, Performances)> {
    use serde_json::Value;
    if characters.is_empty() {
        return Ok((files, Performances::default()));
    }
    let diagnostics = files.diagnostics();
    let Some(raw) = diagnostics.attempt(
        "victory descriptor",
        files.json::<Performances<Value, Value, Value>>(PATH),
    )?
    else {
        return Ok((files, Performances::default()));
    };
    let rows = |scope, value| -> Result<Vec<Value>> {
        Ok(diagnostics
            .attempt(
                scope,
                serde_json::from_value(value).context("decode victory rows"),
            )?
            .unwrap_or_default())
    };
    let mut ordinary = Vec::new();
    for row in rows("victory performances", raw.ordinary)? {
        if let Some(row) = diagnostics.attempt(
            "victory performance",
            serde_json::from_value(row).context("decode victory performance"),
        )? {
            ordinary.push(row);
        }
    }
    let mut groups = Vec::new();
    for row in rows("victory groups", raw.groups)? {
        if let Some(row) = diagnostics.attempt(
            "victory group",
            serde_json::from_value(row).context("decode victory group"),
        )? {
            groups.push(row);
        }
    }
    let mut dependencies = std::collections::BTreeMap::new();
    let dependency_rows: std::collections::BTreeMap<String, Value> = diagnostics
        .attempt(
            "victory dependencies",
            serde_json::from_value(raw.files).context("decode victory dependencies"),
        )?
        .unwrap_or_default();
    for (path, row) in dependency_rows {
        if let Some(row) = diagnostics.attempt(
            "victory dependency",
            serde_json::from_value(row)
                .with_context(|| format!("decode victory dependency {path}")),
        )? {
            dependencies.insert(path, row);
        }
    }
    let performances: Performances = Performances {
        module_sha256: raw.module_sha256,
        archive_sha256: raw.archive_sha256,
        ordinary,
        groups,
        files: dependencies,
    };
    let selected = performances
        .files
        .iter()
        .filter(|(path, _)| {
            performances
                .ordinary
                .iter()
                .any(|row| characters.contains(&row.character) && row.motion == **path)
        })
        .map(|(path, row)| (path.clone(), row.clone()))
        .collect();
    let files = files.with_dependencies(root, selected, cache, cancelled)?;
    Ok((files, performances))
}

pub fn prepare(
    files: &Files,
    source: &Performances,
    body: &battle_model::Party,
    character: u8,
    model: &ModelDefinition,
) -> Result<(Arc<ModelDefinition>, Vec<Performance>)> {
    ensure!(
        (1..=9).contains(&character),
        "victory character is not prepared"
    );
    let mut candidate = model.clone();
    let mut result = Vec::new();
    for row in source
        .ordinary
        .iter()
        .filter(|row| row.character == character)
    {
        let prepared = (|| {
            ensure!(row.selector < 5, "unknown victory selector");
            ensure!(
                source
                    .ordinary
                    .iter()
                    .filter(|other| other.character == character && other.selector == row.selector)
                    .count()
                    == 1,
                "duplicate victory selector"
            );
            ensure!(
                row.body_sha256 == body.body_sha256,
                "victory body identity differs from actor"
            );
            let dependency = source
                .files
                .get(&row.motion)
                .context("victory motion is absent from dependency inventory")?;
            let motion = Motion::decode(
                &files.read_verified(
                    &row.motion,
                    &dependency.sha256,
                    dependency
                        .bytes
                        .try_into()
                        .context("victory motion is too large")?,
                )?,
            )?;
            motion.validate(&candidate.skeleton)?;
            if let Some(events) = &row.timeline {
                ensure!(
                    !events.is_empty() && events[0].0 == 0,
                    "victory timeline has no opening event"
                );
                ensure!(
                    events.windows(2).all(|pair| pair[0].0 <= pair[1].0),
                    "unordered victory timeline"
                );
                for &(_, event) in events {
                    if matches!(event, Event::Idle { .. }) {
                        candidate
                            .motions
                            .get(&super::model::MotionRole::Idle.id())
                            .context("victory completion idle is not prepared")?;
                    }
                    if let Event::Pose {
                        start, end, rate, ..
                    } = event
                    {
                        ensure!(
                            start.is_finite()
                                && (0. ..motion.duration_frames).contains(&start)
                                && end.is_none_or(|end| end.is_finite()
                                    && end > start
                                    && end <= motion.duration_frames)
                                && rate.is_finite()
                                && (0.01..=4.).contains(&rate),
                            "victory pose exceeds its clip"
                        );
                    }
                }
            }
            let clip = FIRST_VICTORY_CLIP + u16::from(row.selector);
            ensure!(
                !candidate.motions.contains_key(&clip),
                "victory clip conflicts with actor bank"
            );
            Ok((clip, motion))
        })();
        let Some((clip, motion)) = files
            .diagnostics()
            .attempt("victory performance", prepared)?
        else {
            continue;
        };
        candidate.motions.insert(clip, motion);
        result.push(Performance {
            character,
            selector: row.selector,
            motion: MotionBinding {
                model: model.resource,
                clip,
            },
            timeline: row.timeline.clone(),
        });
    }
    Ok((Arc::new(candidate), result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::results::item_tests::native_fixture;
    use resonance_battle::{ActorAvailability, BattleInput, ModelRequest, PreparedBattle, Side};

    #[test]
    fn result_timeline_holds_when_paused_and_emits_delayed_pose_once() -> Result<()> {
        let mut fixture = native_fixture(
            &[1],
            |_, _, _| Ok(()),
            |mut actors, setup| {
                setup.style.celebrate = true;
                setup.performances.push(Performance {
                    character: 1,
                    selector: 0,
                    motion: MotionBinding {
                        model: 1,
                        clip: FIRST_VICTORY_CLIP,
                    },
                    timeline: Some(vec![
                        (0, Event::Expression([4, 0, 0, 0])),
                        (
                            2,
                            Event::Pose {
                                start: 3.,
                                end: None,
                                rate: 1.,
                                blend: 0,
                                repeat: true,
                            },
                        ),
                        (4, Event::Idle { blend: 4 }),
                    ]),
                });
                for actor in &mut actors {
                    if actor.side == Side::Enemy {
                        actor.hp = 0;
                        actor.availability = ActorAvailability::Dead;
                    }
                }
                PreparedBattle::new(
                    actors
                        .into_iter()
                        .map(|actor| (actor, Default::default()))
                        .collect(),
                    Default::default(),
                    1,
                )?
                .finish()
            },
        )?;
        let battle = &mut fixture.battle;
        let candidate = &mut fixture.candidate;
        battle.recognize_result();
        battle.retire_combat()?;
        candidate.select_victory(battle, 0, 0)?;
        candidate.prepare_results(battle)?;
        let opening = battle.publish(Vec::new());
        assert!(opening.model_requests.iter().any(|request| matches!(
            request,
            ModelRequest::Expression {
                layers: [4, 0, 0, 0],
                ..
            }
        )));
        for paused in [true, true, false] {
            let cues = candidate.world_update(
                battle,
                BattleInput {
                    paused,
                    ..Default::default()
                },
            )?;
            assert!(battle.publish(cues).model_requests.is_empty());
        }
        let cues = candidate.world_update(battle, BattleInput::default())?;
        assert!(matches!(battle.publish(cues).model_requests.as_slice(),
            [ModelRequest::Play { pose, .. }] if pose.frame == 3. && pose.rate == 1. && pose.repeat
        ));
        let cues = candidate.world_update(battle, BattleInput::default())?;
        assert!(battle.publish(cues).model_requests.is_empty());
        let cues = candidate.world_update(battle, BattleInput::default())?;
        assert!(matches!(battle.publish(cues).model_requests.as_slice(),
            [ModelRequest::Play { motion, pose, .. }]
                if motion.clip == super::super::model::MotionRole::Idle.id()
                    && pose.frame == 0. && pose.rate == 0.5 && pose.repeat && pose.blend == 4
        ));
        let cues = candidate.world_update(battle, BattleInput::default())?;
        assert!(battle.publish(cues).model_requests.is_empty());
        Ok(())
    }
}
