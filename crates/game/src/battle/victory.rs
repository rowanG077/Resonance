//! Prepare victory clips and result postures before battle activation.
use super::{lifecycle::victory_selection::ColetteState, party::Character};
use anyhow::{Context, Result, ensure};
use resonance_battle::{ActorAvailability, ActorId, Battle, Cue, ModelDefinition, MotionBinding};
use resonance_content::{
    animation::Motion,
    battle_model,
    battle_victory::{PATH, Performances},
    prepared::{Cache, Files},
};
use std::{path::Path, sync::Arc};

const FIRST_VICTORY_CLIP: u16 = 0x100;
const WEAK_HP_PERCENT: u8 = 25;

#[derive(Debug, Clone, Copy)]
pub struct Performance {
    pub character: u8,
    pub selector: u8,
    pub motion: MotionBinding,
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
        group_archive_sha256: raw.group_archive_sha256,
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
        });
    }
    Ok((Arc::new(candidate), result))
}
