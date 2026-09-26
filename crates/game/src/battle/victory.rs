//! Bind every possible opening victory before activation. Each selector gets a
//! private immutable clip slot; the original replaces slot23 in place at runtime.
use anyhow::{Context, Result, ensure};
use resonance_battle::{
    ActionPhase, ActorAvailability, ActorId, Battle, Cue, ModelDefinition, MotionBinding,
};
use resonance_content::{
    animation::Motion,
    battle_model,
    battle_victory::{PATH, Performances},
    prepared::{Cache, Files},
};
use std::{path::Path, sync::Arc};

#[derive(Debug, Clone, Copy)]
pub struct Performance {
    pub character: u8,
    pub selector: u8,
    pub action: u16,
    pub motion: MotionBinding,
}

#[derive(Debug, Clone, Copy)]
pub struct PostureBinding {
    pub character: u8,
    /// Healthy, weak, then Colette's sealed healthy/weak/petrified callbacks.
    pub actions: [u16; 5],
}

pub fn prepare_posture(
    files: &Files,
    character: u8,
    model: &ModelDefinition,
    actions: [u16; 5],
) -> Result<PostureBinding> {
    let source: Performances = files.json(PATH)?;
    let mut rows = source
        .postures
        .iter()
        .filter(|row| row.character == character);
    let row = rows.next().context("missing result posture")?;
    ensure!(rows.next().is_none(), "duplicate result posture");
    ensure!(
        (1..=3).contains(&character)
            && row.rate.to_bits() == 0.5f32.to_bits()
            && row.healthy_expression == [0; 4]
            && row.weak_expression == [10, 5, 0, 0],
        "result posture differs from maintained source"
    );
    for clip in [0, 26] {
        model
            .motions
            .get(&clip)
            .context("missing result posture motion")?
            .validate(&model.skeleton)?;
    }
    ensure!(
        source.groups.iter().all(|group| group.descriptor[2] == 0),
        "special group result posture is not prepared"
    );
    Ok(PostureBinding { character, actions })
}

pub fn posture_bindings(character: u8, ids: [u16; 5]) -> Result<Vec<super::ActionBinding>> {
    ensure!(
        (1..=3).contains(&character),
        "result character is not prepared"
    );
    let names = [
        "healthy",
        "weak",
        "sealed_healthy",
        "sealed_weak",
        "sealed_petrified",
    ];
    Ok(ids
        .into_iter()
        .zip(names)
        .take(if character == 2 { 5 } else { 2 })
        .map(|(id, name)| super::ActionBinding {
            id,
            phase: ActionPhase::Controller,
            module: "battle::result_posture".into(),
            entry: format!("character_{character}_{name}"),
            duration: 0,
            tp_cost: 0,
        })
        .collect())
}

/// 57718 runs after growth and layout. Select the pose before the source rewrites
/// availability, then replace all ordinary callbacks with the result controller.
pub fn construct_result_actors(
    battle: &mut Battle,
    leader: ActorId,
    roster: &[(ActorId, u8)],
    postures: &[PostureBinding],
    story: u32,
) -> Result<Vec<Cue>> {
    let mut cues = Vec::new();
    for &(id, character) in roster {
        let binding = postures
            .iter()
            .find(|row| row.character == character)
            .context("result posture was not prepared")?;
        let actor = battle
            .actors()
            .get(id.index())
            .context("missing result actor")?;
        let petrified = actor.availability == ActorAvailability::Petrified;
        let weak = !petrified && (!actor.available() || (id != leader && actor.hp_percent() <= 25));
        let sealed = character == 2 && story == 1000;
        cues.extend(battle.reset_result_actor(id, !weak)?);
        if !petrified && !weak {
            battle.end_overlimit(id)?;
        }
        let action = match (petrified, sealed, weak) {
            (true, false, _) => None,
            (true, true, _) => Some(binding.actions[4]),
            (false, true, false) => Some(binding.actions[2]),
            (false, true, true) => Some(binding.actions[3]),
            (false, false, false) => Some(binding.actions[0]),
            (false, false, true) => Some(binding.actions[1]),
        };
        if let Some(action) = action {
            cues.extend(battle.start_result_controller(id, action, !weak)?);
        }
    }
    Ok(cues)
}

pub fn load_files(
    root: &Path,
    files: Files,
    cache: &mut Cache,
    cancelled: impl Fn() -> bool,
) -> Result<Files> {
    let performances: Performances = files.json(PATH)?;
    files.with_dependencies(root, performances.files, cache, cancelled)
}

pub fn prepare(
    files: &Files,
    character: u8,
    model: &ModelDefinition,
    ids: [u16; 5],
) -> Result<(Arc<ModelDefinition>, Vec<Performance>)> {
    ensure!(
        (1..=3).contains(&character),
        "victory character is not prepared"
    );
    let source: Performances = files.json(PATH)?;
    let body: battle_model::Party = files.json(&battle_model::party_path(character))?;
    let mut candidate = model.clone();
    let mut result = Vec::new();
    for (selector, action) in ids.into_iter().enumerate() {
        let mut selected = source
            .ordinary
            .iter()
            .filter(|row| row.character == character && usize::from(row.selector) == selector);
        let row = selected.next().context("missing victory selector")?;
        ensure!(selected.next().is_none(), "duplicate victory selector");
        validate_rows(row)?;
        ensure!(
            row.body_sha256 == body.body_sha256,
            "victory body identity differs from actor"
        );
        ensure!(
            source.files.contains_key(&row.motion),
            "victory motion is absent from dependency inventory"
        );
        let motion = Motion::decode(&files.read(&row.motion)?)?;
        motion.validate(&candidate.skeleton)?;
        let clip = 0x100 + selector as u16;
        ensure!(
            candidate.motions.insert(clip, motion).is_none(),
            "victory clip conflicts with actor bank"
        );
        // 2C05C mirrors original body23 into an owner-linked weapon's83,
        // falling back to60 only when83 is absent. Preserve that source lookup
        // after giving the body's five candidate packages private clip slots.
        for weapon in &mut candidate.weapons {
            if let resonance_battle::WeaponPlayback::Owner {
                offset, fallback, ..
            } = weapon.playback
            {
                let original = 23u16
                    .checked_add(offset)
                    .context("victory weapon slot overflow")?;
                let motion = weapon
                    .motions
                    .get(&original)
                    .or_else(|| weapon.motions.get(&fallback))
                    .context("missing victory weapon motion")?
                    .clone();
                let mapped = clip
                    .checked_add(offset)
                    .context("victory weapon slot overflow")?;
                ensure!(
                    Arc::make_mut(weapon)
                        .motions
                        .insert(mapped, motion)
                        .is_none(),
                    "victory weapon slot conflicts with bank"
                );
            }
        }
        result.push(Performance {
            character,
            selector: selector as u8,
            action,
            motion: MotionBinding {
                model: model.resource,
                clip,
            },
        });
    }
    Ok((Arc::new(candidate), result))
}

fn validate_rows(row: &resonance_content::battle_victory::Ordinary) -> Result<()> {
    use resonance_content::battle_action::Animation;
    let record = |row: &Animation| {
        (
            row.time,
            row.clip,
            row.blend,
            row.start,
            row.end,
            row.layer_flags,
            row.resource,
            row.rate.bits(),
        )
    };
    let fast = matches!((row.character, row.selector), (1, 3) | (2, 2 | 3));
    let blend = if matches!((row.character, row.selector), (1, 2) | (2, 2 | 3)) {
        12
    } else {
        15
    };
    let rate = if fast { 1f32 } else { 0.5f32 }.to_bits();
    let face = if row.character == 3 { 0 } else { 4 };
    let mut expected = vec![
        (0, 23, blend, 0, 0, 8, -1, rate),
        (0, 255, face, 0, 0, 0, 0, 0),
    ];
    let timed = match (row.character, row.selector) {
        (1, 0) => Some((111, 96)),
        (1, 1 | 4) => Some((91, 76)),
        (1, 2) => None,
        (1, 3) => Some((231, 216)),
        (2, 0) => Some((79, 64)),
        (2, 1) => Some((215, 200)),
        (2, 2 | 3) => {
            expected.push((
                -3,
                0,
                if row.selector == 2 { 4 } else { 0 },
                0,
                0,
                72,
                -1,
                0.5f32.to_bits(),
            ));
            None
        }
        (2, 4) => Some((163, 148)),
        (3, 0) => Some((239, 224)),
        (3, 1) => Some((231, 216)),
        (3, 2) => Some((115, 100)),
        (3, 3) => Some((95, 80)),
        (3, 4) => Some((63, 48)),
        _ => anyhow::bail!("unsupported ordinary victory selector"),
    };
    if let Some((age, start)) = timed {
        expected.push((age, 23, 0, start, 0, 72, -1, rate));
    }
    expected.push((-2, 0, 0, 0, 0, 0, 0, 0));
    ensure!(
        row.animations.iter().map(record).eq(expected),
        "victory source differs from maintained performance"
    );
    Ok(())
}

pub fn bindings(character: u8, ids: [u16; 5]) -> Result<Vec<super::ActionBinding>> {
    ensure!(
        (1..=3).contains(&character),
        "victory character is not prepared"
    );
    Ok(ids
        .into_iter()
        .enumerate()
        .map(|(selector, id)| super::ActionBinding {
            id,
            phase: ActionPhase::Controller,
            module: "battle::victory_performance".into(),
            entry: format!("character_{character}_{selector}"),
            duration: 0,
            tp_cost: 0,
        })
        .collect())
}

pub fn motion(path: &str, performances: &[Performance]) -> Result<MotionBinding> {
    let suffix = path
        .strip_prefix("battle/motions/victory/")
        .context("unknown victory motion")?;
    let (character, selector) = suffix
        .split_once('/')
        .context("invalid victory motion path")?;
    let character: u8 = character.parse()?;
    let selector: u8 = selector.parse()?;
    Ok(performances
        .iter()
        .find(|row| row.character == character && row.selector == selector)
        .context("victory motion was not prepared")?
        .motion)
}
