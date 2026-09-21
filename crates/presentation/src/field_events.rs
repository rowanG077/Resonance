//! Enumerate live field interactions from independent exploration checkpoints.
//! Run discovered choice branches through scripts, dialogue and the real silent mixer.
use super::{field_audio::validation::Playback, new_game};
use anyhow::{Context, Result, ensure};
use resonance_content::{
    effect::FieldEffects,
    field::FieldAssets,
    font::{BitmapFont, DialogueArt},
    prepared::Files,
    session::SessionData,
};
use resonance_events::{PersistentState, party::Party};
use resonance_game::{
    clock::{UPDATE_RATE_DENOMINATOR, UPDATE_RATE_NUMERATOR},
    field::{FieldCheckpoint, FieldEntry, FieldInput, FieldSession},
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    path::Path,
    sync::Arc,
};

#[derive(Clone, Copy, Debug, serde::Serialize)]
enum Target {
    Entry,
    Actor(i32),
    Trigger { key: u32, confirmed: bool },
}

// Voiced multi-scene branches may exceed the old hundred-second replay limit.
const EVENT_LIMIT: u32 = (600 * UPDATE_RATE_NUMERATOR / UPDATE_RATE_DENOMINATOR) as u32;
const STALL_LIMIT: u32 = (30 * UPDATE_RATE_NUMERATOR / UPDATE_RATE_DENOMINATOR) as u32;

pub fn check_field_events(root: &Path, map: u32, story: i32, output: &Path) -> Result<()> {
    let manifest = resonance_content::field::preload_path(map);
    let files = Arc::new(Files::load(
        root,
        &[&manifest],
        &mut Default::default(),
        || false,
    )?);
    let package = new_game::FieldPackage::load(root, files.clone(), map, &mut Default::default())?;
    let effects: FieldEffects = files.json(&package.assets.effects)?;
    effects.validate()?;
    let data: Arc<SessionData> = Arc::new(files.json("game/session-data.json")?);
    let art: DialogueArt = files.json("ui/dialogue.json")?;
    let font: BitmapFont = files.json(&art.font)?;
    let available_fields = new_game::available_fields(root)?;
    // Registry probes need a valid floor point even for rooms whose authored
    // entrances have not yet been captured by a navigation replay.
    let floor = package
        .assets
        .ground
        .iter()
        .flat_map(|group| {
            group
                .triangles
                .iter()
                .map(move |triangle| triangle.map(|i| group.vertices[usize::from(i)]))
        })
        .max_by(|a, b| floor_area(a).total_cmp(&floor_area(b)))
        .context("field has no ground triangle")?;
    let position = std::array::from_fn(|axis| floor.iter().map(|p| p[axis]).sum::<f32>() / 3.);
    let new = |checkpoint: Option<&FieldCheckpoint>| -> Result<(FieldSession, Playback)> {
        let mut party = Party::new(&data, Default::default())?;
        party.formation = if story >= 2000 {
            vec![1, 2, 3]
        } else {
            vec![1]
        };
        let mut persistent = PersistentState {
            party: Some(party),
            ..Default::default()
        };
        persistent
            .memory
            .write(0x40, symphonia_script::Width::S32, story)?;
        let mut entry = if let Some(checkpoint) = checkpoint {
            checkpoint
                .clone()
                .entry(&package.assets, data.clone(), available_fields.clone())?
        } else {
            FieldEntry {
                persistent,
                data: Some(data.clone()),
                position,
                available_fields: available_fields.clone(),
                ..Default::default()
            }
        };
        entry.skits = Some(Arc::new(files.json("game/skits.json")?));
        let mut field = package.enter(entry)?;
        let mut audio = Playback::new((*package.audio).clone(), &mut field);
        for _ in 0..1000 {
            check_effects(&field, &package.assets, &effects, &files)?;
            audio.step(&mut field)?;
            if field.player_has_control()
                || !field.dialogue.is_empty()
                || field.menu.is_some()
                || field.shop.is_some()
            {
                return Ok((field, audio));
            }
            field.step(FieldInput::default())?;
        }
        anyhow::bail!("field {map} story {story} initialization stalled")
    };
    let mut targets = VecDeque::from([Target::Entry]);
    let mut checkpoint = None;
    let mut reports = Vec::new();
    while let Some(target) = targets.pop_front() {
        let mut pending = VecDeque::from([Vec::<u8>::new()]);
        let mut tried = BTreeSet::new();
        while let Some(path) = pending.pop_front() {
            if !tried.insert(path.clone()) {
                continue;
            }
            ensure!(
                tried.len() <= 128,
                "event choice search exceeds 128 branches"
            );
            let (mut field, mut audio) = new(if matches!(target, Target::Entry) {
                None
            } else {
                checkpoint.as_ref()
            })?;
            let origin = match target {
                Target::Entry => serde_json::json!({"initialization":true}),
                Target::Actor(id) => {
                    let actor = &field.events.world.actors[&id];
                    serde_json::json!({"position":actor.position,"resource":actor.resource,"visible":actor.visible,"model_hidden":actor.appearance.model_hidden})
                }
                Target::Trigger { key, .. } => {
                    serde_json::json!({"shape":format!("{:?}",field.events.world.triggers.iter().find(|t|t.key==key).unwrap().shape)})
                }
            };
            let mut pages = BTreeMap::new();
            let mut choices = BTreeSet::new();
            let mut menus = BTreeSet::new();
            let mut shops = BTreeSet::new();
            let mut chosen = Vec::new();
            let mut ticks = 0;
            let mut last_progress = None;
            let mut progress_tick = 0;
            let result = (|| -> Result<()> {
                ensure!(
                    match target {
                        Target::Entry => true,
                        Target::Actor(id) => field.events.interact(id)?,
                        Target::Trigger { key, confirmed } =>
                            field.events.trigger(key, confirmed)?,
                    },
                    "registered field target did not start"
                );
                if matches!(target, Target::Entry) && field.checkpoint().is_ok() {
                    return audio.finish();
                }
                while ticks < EVENT_LIMIT {
                    for dialogue in field.dialogue.values().filter(|p| !p.closed) {
                        let text: String = dialogue
                            .current()
                            .glyphs
                            .iter()
                            .map(|g| g.character)
                            .collect();
                        ensure!(
                            text.chars().all(|c| matches!(c, '\n' | '\r' | '\u{c}')
                                || font.glyphs.contains_key(&c)),
                            "uncooked glyph in {text:?}"
                        );
                        pages.insert((dialogue.operation.id(), dialogue.page), text);
                    }
                    for choice in field
                        .events
                        .world
                        .choices
                        .values_mut()
                        .filter(|c| c.operation.is_pending())
                    {
                        if choices.insert(choice.operation.id()) {
                            let depth = chosen.len();
                            ensure!(depth < 32, "event choice path exceeds 32 decisions");
                            let selected = path.get(depth).copied().unwrap_or(choice.first_line);
                            ensure!(
                                (choice.first_line..=choice.last_line).contains(&selected),
                                "choice branch changed unexpectedly"
                            );
                            if depth >= path.len() {
                                for alternative in choice.first_line..=choice.last_line {
                                    if alternative != selected {
                                        let mut next = chosen.clone();
                                        next.push(alternative);
                                        pending.push_back(next);
                                    }
                                }
                            }
                            chosen.push(selected);
                            choice.selected_line = selected;
                        }
                    }
                    if let Some(menu) = &field.menu {
                        menus.insert(format!("{:?}", menu.page));
                    }
                    if let Some(shop) = &field.shop {
                        shops.insert(shop.id);
                    }
                    let in_menu = field.menu.is_some() || field.shop.is_some();
                    let dialogue_ready = field.dialogue.values().any(|page| {
                        !page.closed
                            && !page.persistent
                            && page.fully_revealed()
                            && page.voice_finished()
                    });
                    let choosing = field
                        .events
                        .world
                        .choices
                        .values()
                        .any(|choice| choice.operation.is_pending());
                    field.step(FieldInput {
                        interact: !in_menu && (dialogue_ready || choosing) && ticks % 30 == 10,
                        cancel: in_menu && ticks % 30 == 10,
                        ..Default::default()
                    })?;
                    ticks += 1;
                    check_effects(&field, &package.assets, &effects, &files)?;
                    audio.step(&mut field)?;
                    // Looping music and idle animations do not count as event
                    // progress. Spoken PCM, text reveal and scripted motion do.
                    let world = &field.events.world;
                    let progress = (
                        chosen.len(),
                        field.story_progress()?,
                        world.input_enabled,
                        field
                            .dialogue
                            .values()
                            .filter(|page| !page.closed)
                            .map(|page| {
                                (
                                    page.operation.id(),
                                    page.page,
                                    page.visible,
                                    page.voice_finished(),
                                )
                            })
                            .collect::<Vec<_>>(),
                        audio.voice_position(),
                        world
                            .field_camera
                            .as_ref()
                            .and_then(|camera| camera.motion.as_ref())
                            .map(|motion| {
                                (motion.position.value, motion.angles.value, motion.fov.value)
                            }),
                        world
                            .actors
                            .iter()
                            .filter(|(_, actor)| !world.input_enabled && actor.motion.is_some())
                            .map(|(&id, actor)| (id, actor.position, actor.heading))
                            .collect::<Vec<_>>(),
                    );
                    if last_progress.as_ref() != Some(&progress) {
                        last_progress = Some(progress);
                        progress_tick = ticks;
                    }
                    ensure!(
                        ticks - progress_tick < STALL_LIMIT,
                        "event made no presentation progress for thirty seconds: {:?}",
                        field.events.pending_operations()
                    );
                    if let Some(camera) = &field.events.world.field_camera {
                        ensure!(
                            camera
                                .position
                                .iter()
                                .chain(&camera.target)
                                .all(|v| v.is_finite())
                                && (1. ..179.).contains(&camera.fov_degrees())
                                && camera.position != camera.target,
                            "invalid field camera at tick {}",
                            field.events.tick()
                        );
                    }
                    if (field.player_has_control()
                        && (!matches!(target, Target::Entry) || field.checkpoint().is_ok()))
                        || field.events.world.field_transition.is_some()
                    {
                        audio.finish()?;
                        return Ok(());
                    }
                }
                anyhow::bail!(
                    "event exceeded ten minutes: {:?}",
                    field.events.pending_operations()
                )
            })();
            if matches!(target, Target::Entry)
                && path.is_empty()
                && result.is_ok()
                && field.player_has_control()
            {
                checkpoint = Some(field.checkpoint()?);
                targets.extend(
                    field
                        .events
                        .world
                        .actors
                        .keys()
                        .filter(|&&id| field.events.has_interaction(id))
                        .map(|&id| Target::Actor(id)),
                );
                targets.extend(
                    field
                        .events
                        .world
                        .triggers
                        .iter()
                        .map(|trigger| Target::Trigger {
                            key: trigger.key,
                            confirmed: trigger.transition.is_some(),
                        }),
                );
            }
            reports.push(serde_json::json!({"target":target,"origin":origin,"choices":chosen,"ticks":ticks,"error":result.err().map(|e|format!("{e:#}")),
                "audio":audio.report(),
                "menus":menus,"shops":shops,
                "story":field.story_progress()?,"event_flags":field.events.world.event_flags,
                "transition":field.events.world.field_transition.as_ref().map(|t|serde_json::json!({"map":t.map,"position":t.position,"heading":t.heading})),
                "pages":pages.values().collect::<Vec<_>>(),"items":field.events.world.party.as_ref().map(|p|&p.items),
                "titles":field.events.world.party.as_ref().map(|p|p.members.iter().map(|m|&m.titles).collect::<Vec<_>>())}));
        }
    }
    let failures = reports.iter().filter(|r| !r["error"].is_null()).count();
    fs::write(
        output,
        serde_json::to_vec_pretty(
            &serde_json::json!({"map":map,"story":story,"branches":reports,"failures":failures,
        "preload":manifest,"audio_banks":files.manifests[&map].inputs.audio,
        "budgets":{"event_updates":EVENT_LIMIT,"stalled_updates":STALL_LIMIT},
        "dialogue_policy":"Wait for complete text and voice before confirming; enumerate every choice; cancel menus through normal input.",
        "scope":"All entry choices, then active actors/triggers from the default completed entry and their discovered choices. Real field initialization, native services, dialogue glyphs, emitted effect recipes/textures and PCM synthesis against the active verified preload. Registry invocation; does not establish natural navigation, destination entry or image/audio equivalence."}),
        )?,
    )?;
    ensure!(
        failures == 0,
        "{failures} field event branches failed; see report"
    );
    Ok(())
}

fn check_effects(
    field: &FieldSession,
    assets: &FieldAssets,
    effects: &FieldEffects,
    files: &Files,
) -> Result<()> {
    let world = &field.events.world;
    for emote in world.emotes.values() {
        ensure!(
            effects.emotes.contains_key(&emote.kind),
            "uncooked emitted emote {} for actor {} at tick {}",
            emote.kind,
            emote.actor,
            world.tick
        );
        files.read(&effects.emote_texture)?;
    }
    for billboard in world.billboards.values() {
        let recipe = effects.sprites.get(&billboard.recipe).with_context(|| {
            format!(
                "uncooked emitted billboard {} at tick {}",
                billboard.recipe, world.tick
            )
        })?;
        files.read(&recipe.texture)?;
    }
    for particle in &world.particles {
        let recipe = assets.particles.get(&particle.kind).with_context(|| {
            format!(
                "uncooked emitted particle {} at tick {}",
                particle.kind, world.tick
            )
        })?;
        ensure!(
            particle.flutter.is_some(),
            "emitted particle {} has no renderer motion at tick {}",
            particle.kind,
            world.tick
        );
        files.read(&recipe.texture)?;
    }
    Ok(())
}

fn floor_area(points: &[[f32; 3]; 3]) -> f32 {
    let [a, b, c] = *points;
    ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs()
}
