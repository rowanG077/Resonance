//! Enumerate live field interactions from independent exploration checkpoints.
//! Run every discovered choice branch with normal dialogue advancement; no devices.
use anyhow::{Context, Result, ensure};
use resonance_content::{
    field::FieldAssets,
    font::{BitmapFont, DialogueArt},
    prepared::Files,
    session::SessionData,
};
use resonance_events::{PersistentState, party::Party};
use resonance_game::field::{FieldEntry, FieldInput, FieldSession};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    path::Path,
    sync::Arc,
};

#[derive(Clone, Copy, Debug, serde::Serialize)]
enum Target {
    Actor(i32),
    Trigger { key: u32, confirmed: bool },
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let map: u32 = args
        .next()
        .context("MAP STORY OUTPUT.json [COOKED_ROOT]")?
        .parse()?;
    let story: i32 = args.next().context("STORY")?.parse()?;
    let output = args.next().context("OUTPUT.json")?;
    let root = args.next().unwrap_or_else(|| "local/cooked".into());
    let root = Path::new(&root);
    let manifest = if map == 340 {
        "fields/iselia-classroom.preload.json".into()
    } else {
        format!("fields/map-{map}.preload.json")
    };
    let files = Files::load(root, &[&manifest], &mut Default::default(), || false)?;
    let assets: FieldAssets = files.json(&files.manifests[&map].inputs.field)?;
    let data: Arc<SessionData> = Arc::new(files.json("game/session-data.json")?);
    let art: DialogueArt = files.json("ui/dialogue.json")?;
    let font: BitmapFont = files.json(&art.font)?;
    let new = || -> Result<FieldSession> {
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
        let mut field = FieldSession::enter(
            &files.read(&assets.script.path)?,
            files.json(&assets.messages)?,
            &assets,
            FieldEntry {
                persistent,
                data: Some(data.clone()),
                skits: Some(Arc::new(files.json("game/skits.json")?)),
                text: Arc::new(files.json("game/text.json")?),
                position: if map == 340 {
                    [-52., -619., 0.]
                } else {
                    [1950., 850., 0.]
                },
                available_fields: [330, 332, 340].into(),
                ..Default::default()
            },
        )?;
        field.prepare_skits(&files)?;
        for _ in 0..1000 {
            if field.player_has_control() {
                return Ok(field);
            }
            field.step(FieldInput::default())?;
        }
        anyhow::bail!("field {map} story {story} did not grant control")
    };
    let field = new()?;
    let targets: Vec<_> = field
        .events
        .world
        .actors
        .keys()
        .filter(|&&id| field.events.has_interaction(id))
        .map(|&id| Target::Actor(id))
        .chain(field.events.world.triggers.iter().map(|t| Target::Trigger {
            key: t.key,
            confirmed: t.transition.is_some(),
        }))
        .collect();
    let mut reports = Vec::new();
    for target in targets {
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
            let mut field = new()?;
            let origin = match target {
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
            let mut chosen = Vec::new();
            let mut ticks = 0;
            let result = (|| -> Result<()> {
                ensure!(
                    match target {
                        Target::Actor(id) => field.events.interact(id)?,
                        Target::Trigger { key, confirmed } =>
                            field.events.trigger(key, confirmed)?,
                    },
                    "registered field target did not start"
                );
                while ticks < 6000 {
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
                    field.step(FieldInput {
                        interact: ticks % 30 == 10,
                        ..Default::default()
                    })?;
                    ticks += 1;
                    field.events.world.audio_commands.clear();
                    if field.player_has_control() || field.events.world.field_transition.is_some() {
                        return Ok(());
                    }
                }
                anyhow::bail!(
                    "event did not finish: {:?}",
                    field.events.pending_operations()
                )
            })();
            reports.push(serde_json::json!({"target":target,"origin":origin,"choices":chosen,"ticks":ticks,"error":result.err().map(|e|format!("{e:#}")),
                "pages":pages.values().collect::<Vec<_>>(),"items":field.events.world.party.as_ref().map(|p|&p.items),
                "titles":field.events.world.party.as_ref().map(|p|p.members.iter().map(|m|&m.titles).collect::<Vec<_>>())}));
        }
    }
    let failures = reports.iter().filter(|r| !r["error"].is_null()).count();
    fs::write(
        output,
        serde_json::to_vec_pretty(
            &serde_json::json!({"map":map,"story":story,"branches":reports,"failures":failures,
        "scope":"Active actors/triggers and dynamically discovered choice branches. Registry invocation; does not establish navigation or image/audio equivalence."}),
        )?,
    )?;
    ensure!(
        failures == 0,
        "{failures} field event branches failed; see report"
    );
    Ok(())
}
