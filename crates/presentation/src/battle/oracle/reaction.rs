//! Compare independently witnessed contact stuns without equating random draws.
//!
//! Ordinary hurt poses and a pinned delayed vertical recoil are checked per run.
//! Horizontal movement, heading, knockdown and the selected owner's pose remain
//! exact comparisons; an unqualified trace gets no exemption.
use super::{Trace, exactly_one};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const STUN_BANK: &str = "2c72d54fcdf809b93f10d0c9213790710b4f0144a4f55253ebab415f4e69a48b";
const STUN_CLIP: u64 = 21;
const STUN_TICKS: usize = 180;

#[derive(Default)]
pub(super) struct QualifiedReactions {
    stun_member: Option<u64>,
    landing_member: Option<u64>,
    poses: BTreeSet<(usize, u64)>,
    heights: BTreeSet<(usize, u64)>,
    declarations: [BTreeSet<(u64, u64, u64)>; 2],
    landings: [BTreeSet<(u64, u64, u64)>; 2],
}

impl QualifiedReactions {
    pub(super) fn permits(&self, frame: usize, actor: u64) -> bool {
        self.poses.contains(&(frame, actor))
    }

    pub(super) fn permits_height(&self, frame: usize, actor: u64) -> bool {
        self.heights.contains(&(frame, actor))
    }

    /// Conditional feedback is removed only after its complete own lifetime was checked.
    pub(super) fn retain_event(&self, side: usize, event: &Value) -> bool {
        if event["bank"] != STUN_BANK {
            return true;
        }
        let Some(key) = event["owner"]
            .as_u64()
            .zip(event["clock"].as_u64())
            .zip(event["presentation_clock"].as_u64())
            .map(|((owner, clock), presentation)| (owner, clock, presentation))
        else {
            return true;
        };
        let stun = event["kind"] == "declaration_started"
            && event["member"].as_u64() == self.stun_member
            && self.declarations[side].contains(&key);
        let landing = self.landings[side].contains(&key)
            && (event["kind"] == "program_started" && event["program"] == 17
                || event["kind"] == "declaration_started"
                    && event["member"].as_u64() == self.landing_member);
        !(stun || landing)
    }
}

#[derive(Clone, Copy)]
struct Rule {
    hurt_clip: u64,
    ordinary: bool,
    hitstun: usize,
    vertical: Option<Vertical>,
}

#[derive(Clone, Copy)]
struct Vertical {
    delay: usize,
    impulse: f64,
    projectile: u64,
    hitstun: usize,
}

// Use the selected action's pinned authoring. Requiring a shared ordinary
// reaction across all its hit rules avoids guessing which scene table row a
// callback used. Mixed reactions can qualify only bound stun feedback when all
// body poses and motion remain identical. Zero-stun tables stay strict.
fn authored_rule(source: &Trace, native: &Trace) -> Result<Option<Rule>> {
    if source.setup["catalogue"].is_null() {
        return Ok(None);
    }
    ensure!(
        source.setup["catalogue"] == native.setup["catalogue"],
        "reaction catalogue differs"
    );
    let row: usize = source
        .case
        .rsplit(':')
        .next()
        .context("missing action row")?
        .parse()?;
    let catalogue = super::content_evidence::action_catalogue(source)?;
    let party_spell = source.case.starts_with("party:")
        && catalogue["definitions"][row]["capabilities"]["spell"] == true
        && source.setup["input"]["selection_arte"].as_u64() == Some(row as u64);
    if !source.case.starts_with("enemy:") && !party_spell {
        return Ok(None);
    }
    let execution = &catalogue["actions"]["rows"][row]["execution"];
    let mut rules = Vec::new();
    let mut vertical = None;
    if party_spell || execution["spell"].as_u64().is_some() {
        let Some(registrations) = source.setup["stored_scenes"]["registrations"].as_array() else {
            return Ok(None);
        };
        let Some(registration) = exactly_one(registrations.iter().filter(|entry| {
            entry["kind"] == "spell"
                && entry["physical_release"] == false
                && (party_spell || entry["native"] == execution["spell"])
        })) else {
            return Ok(None);
        };
        ensure!(
            source.setup["stored_scenes"] == native.setup["stored_scenes"],
            "reaction scene differs"
        );
        if !registered_owner(source, registration) {
            return Ok(None);
        }
        let (scene, _) = super::content_evidence::registered_scene(registration)?;
        let scene = serde_json::to_value(scene)?;
        vertical = vertical_rule(source, native, &scene)?;
        if let Some(projectiles) = scene["projectiles"].as_array() {
            rules.extend(
                projectiles
                    .iter()
                    .map(|projectile| projectile["hit"].clone()),
            );
        }
    } else {
        let attack = execution
            .get("attack")
            .unwrap_or(&execution["physical"]["attack"]);
        if let Some(events) = attack["events"].as_array() {
            for event in events {
                for kind in ["contact", "projectile"] {
                    if let Some(hit) = event[1][kind].get("hit") {
                        rules.push(hit.clone());
                    }
                }
            }
        }
    }
    let Some(first) = rules.first() else {
        return Ok(None);
    };
    let alternate = first["alternate_motion"].as_bool();
    if alternate.is_none()
        || !rules.iter().all(|rule| {
            rule["stun_chance"]
                .as_u64()
                .is_some_and(|chance| (1..100).contains(&chance))
                && rule["suppress_reaction"] == false
                && rule["hitstun"]
                    .as_u64()
                    .is_some_and(|duration| duration > 0)
        })
    {
        return Ok(None);
    }
    let ordinary = rules.iter().all(|rule| {
        rule["launch"] == false
            && rule["knock_down"] == false
            && rule["alternate_motion"].as_bool() == alternate
    });
    Ok(Some(Rule {
        hurt_clip: if alternate == Some(true) { 4 } else { 3 },
        ordinary,
        hitstun: rules
            .iter()
            .map(|rule| rule["hitstun"].as_u64().unwrap() as usize)
            .min()
            .unwrap(),
        vertical,
    }))
}

fn registered_owner(trace: &Trace, registration: &Value) -> bool {
    trace.setup["source_actor_map"]
        .as_array()
        .and_then(|actors| {
            actors
                .iter()
                .find(|actor| actor["actor"] == trace.setup["owner"])
        })
        .is_some_and(|owner| {
            trace.setup["initial_battle"]["actors"]
                .as_array()
                .is_some_and(|actors| {
                    actors.iter().any(|actor| {
                        actor["slot"] == owner["source_slot"]
                            && actor["address"] == registration["owner"]
                    })
                })
        })
}

/// Bind enemy reaction roles to its formation, captured package and cooked body.
/// Missing clips use the same ordinary-hurt fallback as model preparation.
fn recipient_rule(
    source: &Trace,
    native: &Trace,
    target: u64,
    mut rule: Rule,
) -> Result<Option<Rule>> {
    if source.setup["party"][target as usize] == 1
        && source.setup["actors"].as_array().is_some_and(|actors| {
            actors
                .iter()
                .any(|actor| actor["actor"] == target && actor["side"] == 0)
        })
    {
        return Ok(Some(rule));
    }
    let Some(proof) = source.setup.get("reaction_recipients") else {
        return Ok(None);
    };
    ensure!(
        native.setup.get("reaction_recipients") == Some(proof),
        "reaction recipient proof differs"
    );
    let Some(binding) = proof["actors"]
        .as_array()
        .and_then(|actors| actors.iter().find(|actor| actor["actor"] == target))
    else {
        return Ok(None);
    };
    let read = |value: &Value| -> Result<Value> {
        let pin: super::random_inputs::Pin = serde_json::from_value(value.clone())?;
        Ok(serde_json::from_slice(&pin.read_bytes()?)?)
    };
    let formations: resonance_content::battle_rules::Formations =
        serde_json::from_value(read(&proof["formations"])?)?;
    let formation_enemies = serde_json::from_value(read(&proof["formation_enemies"])?)?;
    let formations = serde_json::to_value(formations.prepare(&formation_enemies)?)?;
    let formation = source.setup["formation"]
        .as_u64()
        .context("missing reaction formation")? as usize;
    let source_slot = source.setup["source_actor_map"]
        .as_array()
        .and_then(|actors| actors.iter().find(|actor| actor["actor"] == target))
        .and_then(|actor| actor["source_slot"].as_u64())
        .context("missing reaction recipient source slot")?;
    let enemies: Vec<_> = source.setup["initial_battle"]["actors"]
        .as_array()
        .context("missing initial reaction actors")?
        .iter()
        .filter(|actor| actor["side"] == 1)
        .collect();
    let Some(index) = enemies
        .iter()
        .position(|actor| actor["slot"].as_u64() == Some(source_slot))
    else {
        return Ok(None);
    };
    let spawns = &formations["records"][formation]["actors"];
    let Some(resource) = spawns[index]["resource"].as_u64() else {
        return Ok(None);
    };
    ensure!(
        spawns.as_array().map(Vec::len) == Some(enemies.len())
            && enemies[index]["kind"] == resource
            && formations["records"][formation]["resources"][resource as usize]["enemy"]
                == binding["enemy"],
        "reaction recipient differs from observed formation"
    );
    let evidence = super::content_evidence::Evidence::parse(&binding["content"])?;
    let enemy = binding["enemy"]
        .as_u64()
        .and_then(|id| u8::try_from(id).ok())
        .context("invalid reaction enemy ID")?;
    let (definition, preview) = evidence.enemy(enemy)?;
    let package_digest = evidence.source_digest()?;
    ensure!(
        source.setup["detached_contacts"]["enemy_packages"]
            .as_array()
            .is_some_and(|packages| packages.iter().any(|package| {
                package["enemy"] == enemy && package["package"]["sha256"] == package_digest
            })),
        "reaction recipient package differs"
    );
    let definition = serde_json::to_value(definition)?;
    let profile = &definition["profile"];
    if !["immunities", "intrinsic_conditions"]
        .iter()
        .all(|key| profile[*key].as_array().is_some_and(Vec::is_empty))
        || profile["stun_resistance"] != 0
        || [
            "body_motion_disabled",
            "suppress_hurt_motion",
            "flying",
            "fixed_height",
        ]
        .iter()
        .any(|key| profile["traits"][*key] != false)
    {
        return Ok(None);
    }
    let clips = &preview.parts[0].scene.clips;
    let has_clip = |id: u64| {
        clips.iter().any(|clip| u64::from(clip.resource_slot) == id)
            && definition["null_motions"]
                .as_array()
                .is_some_and(|nulls| !nulls.iter().any(|clip| clip == id))
    };
    if !has_clip(STUN_CLIP) || !has_clip(3) {
        return Ok(None);
    }
    if !has_clip(rule.hurt_clip) {
        rule.hurt_clip = 3;
    }
    Ok(Some(rule))
}

fn vertical_rule(source: &Trace, native: &Trace, scene: &Value) -> Result<Option<Vertical>> {
    let Some(pin) = source.setup.get("reaction_recoil") else {
        return Ok(None);
    };
    ensure!(
        native.setup.get("reaction_recoil") == Some(pin),
        "reaction recoil pin differs"
    );
    let evidence = super::content_evidence::Evidence::parse(pin)?;
    let recoil = evidence.recoil()?;
    ensure!(
        source.setup["stored_scenes"]["module"]["sha256"] == evidence.source_digest()?,
        "reaction recoil module differs"
    );
    let hits = contacts(source);
    let Some(projectile) = hits.first().and_then(|hit| hit["projectile"].as_u64()) else {
        return Ok(None);
    };
    if hits
        .iter()
        .any(|hit| hit["projectile"] != projectile || hit["target"] != hits[0]["target"])
    {
        return Ok(None);
    }
    let samples = |trace: &Trace| -> Vec<usize> {
        trace
            .frames
            .iter()
            .enumerate()
            .filter(|(_, frame)| {
                frame["projectiles"]
                    .as_array()
                    .is_some_and(|entries| entries.iter().any(|entry| entry["id"] == projectile))
            })
            .map(|(index, _)| index)
            .collect()
    };
    let life = samples(source);
    if life.is_empty()
        || life != samples(native)
        || life.len() != life[life.len() - 1] - life[0] + 1
        || life[life.len() - 1] + 1 >= source.frames.len()
    {
        return Ok(None);
    }
    // A complete, uniquely matching projectile lifetime binds the actual hit
    // table row; short detached volumes must not borrow another row's impulse.
    let Some(entries) = scene["projectiles"].as_array() else {
        return Ok(None);
    };
    let Some(entry) =
        exactly_one(entries.iter().filter(|entry| {
            entry["projectile"]["lifetime"].as_u64() == Some(life.len() as u64 - 1)
        }))
    else {
        return Ok(None);
    };
    let hit = &entry["hit"];
    let Some(row) = hit["recoil"].as_u64() else {
        return Ok(None);
    };
    let Some(delay) = hit["recoil_delay"].as_u64().filter(|delay| *delay > 0) else {
        return Ok(None);
    };
    let impulse = &recoil["impulses"][row as usize];
    if impulse[0].as_f64() != Some(0.) {
        return Ok(None);
    }
    let Some(impulse) = impulse[1]
        .as_f64()
        .filter(|value| value.is_finite() && *value > 0.)
    else {
        return Ok(None);
    };
    Ok(Some(Vertical {
        delay: delay as usize,
        impulse,
        projectile,
        hitstun: hit["hitstun"]
            .as_u64()
            .context("missing vertical hitstun")? as usize,
    }))
}

fn actor(frame: &Value, id: u64) -> Option<&Value> {
    frame["actors"]
        .as_array()?
        .iter()
        .find(|actor| actor["actor"] == id)
}

fn clip(frame: &Value, id: u64) -> Option<u64> {
    frame["models"]
        .as_array()?
        .iter()
        .find(|model| model["actor"] == id)?["clip"]
        .as_u64()
}

fn height(frame: &Value, id: u64) -> Option<f64> {
    actor(frame, id)?["position"][1].as_f64()
}

fn contacts(trace: &Trace) -> Vec<&Value> {
    let mut hits: Vec<_> = trace
        .events
        .iter()
        .filter(|event| event["kind"] == "hit")
        .collect();
    hits.sort_by_key(|hit| {
        (
            hit["presentation_clock"].as_u64(),
            hit["target"].as_u64(),
            hit["projectile"].as_u64(),
        )
    });
    hits
}

fn at(trace: &Trace, event: &Value) -> Option<usize> {
    let presentation = event["presentation_clock"].as_u64()?;
    let combat = event["clock"].as_u64()?;
    let first = trace.frames.first()?;
    trace.frames.iter().position(|frame| {
        frame["presentation_tick"]
            .as_u64()
            .zip(first["presentation_tick"].as_u64())
            .and_then(|(now, start)| now.checked_sub(start))
            == Some(presentation)
            && frame["combat_tick"]
                .as_u64()
                .zip(first["combat_tick"].as_u64())
                .and_then(|(now, start)| now.checked_sub(start))
                == Some(combat)
    })
}

#[derive(Debug)]
struct Stun {
    hit: usize,
    end: usize,
    pose_start: usize,
    declaration: (u64, u64, u64),
}

fn witnessed_stuns(trace: &Trace, target: u64, native: bool) -> Result<Vec<Stun>> {
    let stun_member = super::content_evidence::template_member(trace, STUN_BANK, 19)?;
    let hits: Vec<_> = contacts(trace)
        .into_iter()
        .filter(|hit| hit["target"] == target)
        .collect();
    let hit_frames: Vec<_> = hits
        .iter()
        .map(|hit| at(trace, hit).context("stun contact lacks observed clock"))
        .collect::<Result<_>>()?;
    let mut particles: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    for (index, frame) in trace.frames.iter().enumerate() {
        for particle in frame["particles"]
            .as_array()
            .context("missing reaction particles")?
        {
            if particle["owner"] == target
                && particle["bank"] == STUN_BANK
                && particle["member"] == stun_member
                && particle["initialized"] == true
            {
                let id = particle["id"]
                    .as_u64()
                    .context("missing stun particle identity")?;
                let life = particles.entry(id).or_default();
                ensure!(
                    particle["age"].as_u64() == Some(life.len() as u64 + 1),
                    "stun particle age differs"
                );
                life.push(index);
            }
        }
    }
    let mut stuns = Vec::new();
    for life in particles.values() {
        let first = life[0];
        let last = *life.last().unwrap();
        ensure!(
            first > 0 && life.len() == last - first + 1,
            "stun particle lifetime is discontinuous"
        );
        let (hit_index, &hit) = exactly_one(
            hit_frames
                .iter()
                .enumerate()
                .filter(|(_, frame)| **frame + 1 == first),
        )
        .context("stun effect has no unique attributed contact")?;
        let event = hits[hit_index];
        ensure!(
            event["guard"].is_null()
                && event["protection"].is_null()
                && actor(&trace.frames[hit], target)
                    .and_then(|actor| actor["hp"].as_u64())
                    .is_some_and(|hp| hp > 0),
            "stun contact is guarded, protected or defeated"
        );
        let next = hit_frames
            .iter()
            .copied()
            .find(|&frame| frame > hit)
            .unwrap_or(trace.frames.len());
        let mut ticks = 0;
        let mut end = trace.frames.len();
        let mut pose_start = None;
        for index in first..next {
            if height(&trace.frames[index - 1], target).context("missing stun height")? <= 0. {
                pose_start.get_or_insert(index);
                ticks += 1;
            }
            if ticks == STUN_TICKS {
                end = index;
                break;
            }
        }
        end = end.min(next);
        ensure!(
            last + 1 == end,
            "stun effect retirement differs from contact interruption or 180 grounded updates"
        );
        let pose_start = pose_start.unwrap_or(end);
        // The retirement update preserves the last requested pose. A contact
        // instead interrupts it in that same update.
        let pose_end = if end < next {
            (end + 1).min(trace.frames.len())
        } else {
            end
        };
        for frame in pose_start..pose_end {
            ensure!(
                clip(&trace.frames[frame], target) == Some(STUN_CLIP),
                "stun recipient pose differs at frame {frame}"
            );
        }
        let declaration = exactly_one(trace.events.iter().filter(|declaration| {
            declaration["kind"] == "declaration_started"
                && declaration["owner"] == target
                && declaration["bank"] == STUN_BANK
                && declaration["member"] == stun_member
                && at(trace, declaration) == Some(first)
        }))
        .context("stun effect lacks unique bound declaration")?;
        stuns.push(Stun {
            hit,
            end: pose_end,
            pose_start,
            declaration: (
                target,
                declaration["clock"].as_u64().unwrap(),
                declaration["presentation_clock"].as_u64().unwrap(),
            ),
        });
    }
    if native {
        let outcomes = trace.setup["contact_stun"]
            .as_array()
            .context("native stun outcomes missing")?;
        for (hit, frame) in hits.iter().zip(&hit_frames) {
            let outcome =
                exactly_one(outcomes.iter().filter(|outcome| {
                    outcome["target"] == target && outcome["clock"] == hit["clock"]
                }))
                .context("native stun outcome lacks unique contact")?;
            ensure!(
                outcome["stunned"].as_bool() == Some(stuns.iter().any(|stun| stun.hit == *frame)),
                "native stun cue and visible lifetime disagree"
            );
        }
    }
    Ok(stuns)
}

pub(super) fn validate(source: &Trace, native: &Trace) -> Result<QualifiedReactions> {
    let Some(rule) = authored_rule(source, native)? else {
        return Ok(QualifiedReactions::default());
    };
    qualify(source, native, rule)
}

fn qualify(source: &Trace, native: &Trace, rule: Rule) -> Result<QualifiedReactions> {
    let mut result = QualifiedReactions::default();
    if !source.setup["random_inputs"].is_null()
        || !native.setup["random_inputs"].is_null()
        || source.frames.len() != native.frames.len()
        || source.frames.is_empty()
        || source.setup["common_effect_bank"] != STUN_BANK
        || native.setup["common_effect_bank"] != STUN_BANK
        || source.setup["recipient_input"]
            .get("mode")
            .is_some_and(|mode| mode != "none")
        || source.setup["recipient_input"] != native.setup["recipient_input"]
    {
        return Ok(result);
    }
    let stun_member = super::content_evidence::template_member(source, STUN_BANK, 19)?;
    let landing_member = super::content_evidence::template_member(source, STUN_BANK, 25)?;
    ensure!(
        super::content_evidence::template_member(native, STUN_BANK, 19)? == stun_member
            && super::content_evidence::template_member(native, STUN_BANK, 25)? == landing_member,
        "reaction template bindings differ"
    );
    result.stun_member = Some(stun_member);
    result.landing_member = Some(landing_member);
    let hits = [contacts(source), contacts(native)];
    if !(hits[0].len() == hits[1].len()
        && hits[0].iter().zip(&hits[1]).all(|(a, b)| {
            [
                "owner",
                "target",
                "action",
                "projectile",
                "clock",
                "presentation_clock",
                "protection",
                "guard",
            ]
            .iter()
            .all(|key| a[key] == b[key])
        }))
    {
        return Ok(result);
    }
    if hits[0]
        .iter()
        .any(|hit| hit["owner"] != source.setup["owner"])
    {
        return Ok(result);
    }
    // A neutral recipient must have a known reaction model and ordinary stun
    // duration. A visible clip alone does not establish either property.
    let Some(initial) = source.setup["actors"].as_array() else {
        return Ok(result);
    };
    for target in hits[0]
        .iter()
        .filter_map(|hit| hit["target"].as_u64())
        .collect::<BTreeSet<_>>()
    {
        if source.setup["owner"] == target
            || !initial.iter().any(|actor| {
                actor["actor"] == target
                    && actor["conditions"].as_array().is_some_and(Vec::is_empty)
                    && actor["time_stop"].as_u64().unwrap_or(0) == 0
                    && actor["overlimit_remaining"].as_u64().unwrap_or(0) == 0
            })
        {
            continue;
        }
        let Some(rule) = recipient_rule(source, native, target, rule)? else {
            continue;
        };
        let roots_match = source.frames.iter().zip(&native.frames).all(|(a, b)| {
            actor(a, target)
                .zip(actor(b, target))
                .is_some_and(|(a, b)| {
                    a["position"]
                        .as_array()
                        .zip(b["position"].as_array())
                        .is_some_and(|(a, b)| {
                            a.len() == 3
                                && b.len() == 3
                                && a.iter().zip(b).all(|(a, b)| {
                                    a.as_f64()
                                        .zip(b.as_f64())
                                        .is_some_and(|(a, b)| (a - b).abs() <= 0.001)
                                })
                        })
                })
        });
        if !roots_match && rule.vertical.is_none() {
            continue;
        }
        if [source, native].iter().any(|trace| {
            trace.frames[0]["particles"]
                .as_array()
                .is_some_and(|particles| {
                    particles.iter().any(|particle| {
                        particle["owner"] == target
                            && particle["bank"] == STUN_BANK
                            && particle["member"] == stun_member
                    })
                })
        }) {
            // A carried ready-state stun needs its earlier contact history.
            continue;
        }
        // Time-stop and recipient directional input need their own clock proof.
        if [source, native].iter().any(|trace| {
            trace.frames.windows(2).any(|frames| {
                ["combat_tick", "presentation_tick"].iter().any(|key| {
                    frames[1][key]
                        .as_u64()
                        .zip(frames[0][key].as_u64())
                        .is_none_or(|(b, a)| b != a + 1)
                })
            })
        }) {
            continue;
        }
        let stuns = [
            witnessed_stuns(source, target, false)?,
            witnessed_stuns(native, target, true)?,
        ];
        if !rule.ordinary {
            // A later launch or knockdown in the same action must not invalidate
            // an earlier interrupted stun's feedback. This branch changes no
            // pose or position assertion: every recipient pose must already match.
            if roots_match
                && source
                    .frames
                    .iter()
                    .zip(&native.frames)
                    .all(|(a, b)| clip(a, target).is_some_and(|a| clip(b, target) == Some(a)))
            {
                for (side, stuns) in stuns.iter().enumerate() {
                    result.declarations[side].extend(stuns.iter().map(|stun| stun.declaration));
                }
            }
            continue;
        }
        if !roots_match {
            if !vertical_motion(source, native, target, rule, &stuns)? {
                continue;
            }
            for (side, trace) in [source, native].iter().enumerate() {
                result.landings[side].insert(landing_feedback(trace, target)?);
            }
            for (index, (a, b)) in source.frames.iter().zip(&native.frames).enumerate() {
                if height(a, target) != height(b, target) {
                    result.heights.insert((index, target));
                }
                if clip(a, target) != clip(b, target) {
                    result.poses.insert((index, target));
                }
            }
        }
        let traces = [source, native];
        for side in 0..2 {
            for stun in &stuns[side] {
                // Only retire conditional feedback when every differing pose in
                // this entire stun is justified. Partial masks must not hide a
                // later unsupported recovery or airborne phase.
                let mut poses = Vec::new();
                let qualified = (stun.hit + 1..stun.end).all(|index| {
                    let a = clip(&traces[side].frames[index], target);
                    let b = clip(&traces[1 - side].frames[index], target);
                    if a == b {
                        return true;
                    }
                    let ordinary_hit = hits[1 - side]
                        .iter()
                        .filter(|hit| hit["target"] == target)
                        .filter_map(|hit| at(traces[1 - side], hit))
                        .filter(|&hit| hit < index)
                        .max();
                    if a != Some(STUN_CLIP)
                        || index < stun.pose_start
                        || b != Some(rule.hurt_clip)
                        || height(&traces[side].frames[index - 1], target) != Some(0.)
                        || !ordinary_hit.is_some_and(|hit| index - hit <= rule.hitstun)
                    {
                        return false;
                    }
                    poses.push((index, target));
                    true
                });
                if qualified {
                    result.poses.extend(poses);
                    result.declarations[side].insert(stun.declaration);
                }
            }
        }
    }
    Ok(result)
}

// The only admitted motion variation is an ordinary delayed vertical impulse.
// Every contact resets vertical speed to zero. Hurt waits for its authored delay
// then releases the impulse; an airborne stun instead falls immediately. Check
// each complete interval, including the next contact's position and final floor,
// so a prior height error cannot become the next interval's accepted origin.
fn vertical_motion(
    source: &Trace,
    native: &Trace,
    target: u64,
    rule: Rule,
    stuns: &[Vec<Stun>; 2],
) -> Result<bool> {
    let Some(vertical) = rule.vertical else {
        return Ok(false);
    };
    let traces = [source, native];
    let hits: Vec<_> = contacts(source)
        .into_iter()
        .filter(|hit| hit["target"] == target)
        .collect();
    if hits.is_empty()
        || hits.iter().any(|hit| {
            hit["projectile"] != vertical.projectile
                || !hit["guard"].is_null()
                || !hit["protection"].is_null()
        })
    {
        return Ok(false);
    }
    let hit_frames: Vec<_> = hits
        .iter()
        .map(|hit| at(source, hit).context("vertical contact clock missing"))
        .collect::<Result<_>>()?;
    if hit_frames
        .windows(2)
        .any(|pair| pair[1] - pair[0] <= vertical.delay || pair[1] - pair[0] > rule.hitstun)
        || stuns[0]
            .iter()
            .map(|stun| stun.hit)
            .collect::<BTreeSet<_>>()
            == stuns[1]
                .iter()
                .map(|stun| stun.hit)
                .collect::<BTreeSet<_>>()
    {
        return Ok(false);
    }
    // The model remains Hurt during these airborne interruptions. Grounded or
    // final-contact stuns require the separate ordinary stun/recovery contract.
    if stuns.iter().flatten().any(|stun| {
        stun.hit == hit_frames[0] || !hit_frames.contains(&stun.end) || stun.pose_start != stun.end
    }) {
        return Ok(false);
    }
    for (a, b) in source.frames.iter().zip(&native.frames) {
        let a = actor(a, target).context("vertical recipient missing")?;
        let b = actor(b, target).context("vertical recipient missing")?;
        if a["heading"] != b["heading"]
            || [0, 2]
                .iter()
                .any(|&axis| a["position"][axis] != b["position"][axis])
        {
            return Ok(false);
        }
    }
    for side in 0..2 {
        let trace = traces[side];
        if trace.frames[..=hit_frames[0]]
            .iter()
            .any(|frame| height(frame, target) != Some(0.))
        {
            return Ok(false);
        }
        let mut expected = 0.;
        for (number, &hit) in hit_frames.iter().enumerate() {
            ensure!(
                height(&trace.frames[hit], target) == Some(expected),
                "vertical reaction lost contact continuity"
            );
            let stunned = stuns[side].iter().any(|stun| stun.hit == hit);
            let next = hit_frames.get(number + 1).copied();
            let end = next.unwrap_or(trace.frames.len() - 1);
            let origin = expected;
            let mut landing = None;
            for index in hit + 1..=end {
                let elapsed = index - hit;
                let count = if stunned {
                    elapsed
                } else {
                    elapsed.saturating_sub(vertical.delay - 1)
                } as f64;
                let speed = if stunned { 0. } else { vertical.impulse };
                expected = (origin + count * speed - count * (count - 1.) / 2.).max(0.);
                ensure!(
                    height(&trace.frames[index], target) == Some(expected),
                    "vertical reaction recurrence differs at frame {index}"
                );
                if stunned {
                    ensure!(
                        expected > 0.,
                        "vertical stun reached an unqualified grounded phase"
                    );
                }
                if next.is_none()
                    && elapsed >= vertical.delay
                    && expected == 0.
                    && landing.is_none()
                {
                    ensure!(
                        elapsed > vertical.hitstun,
                        "vertical landing precedes qualified hurt expiry"
                    );
                    landing = Some(index);
                }
                let expected_clip = if landing.is_some_and(|landed| index > landed) {
                    0
                } else {
                    rule.hurt_clip
                };
                ensure!(
                    clip(&trace.frames[index], target) == Some(expected_clip),
                    "vertical reaction pose differs at frame {index}"
                );
            }
            if next.is_none() {
                ensure!(
                    landing.is_some(),
                    "vertical reaction lacks observed final landing"
                );
            }
        }
    }
    Ok(true)
}

// In this exact common bank, program17 immediately spawns member25: a fixed
// 13-update, nonfollowing ground-relative dust ring with offset[0,1,0]. Its
// origin retains the pre-clamp landing height. The vertical proof above makes
// that height independently recoverable from the preceding two gravity steps.
fn landing_feedback(trace: &Trace, target: u64) -> Result<(u64, u64, u64)> {
    let landing_member = super::content_evidence::template_member(trace, STUN_BANK, 25)?;
    let landing = exactly_one((2..trace.frames.len()).filter(|&index| {
        height(&trace.frames[index - 1], target).is_some_and(|y| y > 0.)
            && height(&trace.frames[index], target) == Some(0.)
    }))
    .context("landing feedback lacks a unique proved floor crossing")?;
    let previous = height(&trace.frames[landing - 1], target).unwrap();
    let unclamped = 2. * previous
        - height(&trace.frames[landing - 2], target).context("missing landing gravity sample")?
        - 1.;
    ensure!(
        unclamped <= 0. && landing + 13 < trace.frames.len(),
        "landing dust retirement not observed"
    );
    let position =
        &actor(&trace.frames[landing], target).context("missing landing recipient")?["position"];
    let origin = [
        position[0].as_f64().context("missing landing X")?,
        unclamped,
        position[2].as_f64().context("missing landing Z")?,
    ];
    let vector_matches = |value: &Value, expected: [f64; 3]| {
        value.as_array().is_some_and(|values| {
            values.len() == 3
                && values
                    .iter()
                    .zip(expected)
                    .all(|(value, expected)| value.as_f64() == Some(expected))
        })
    };
    let feedback: Vec<_> = trace
        .events
        .iter()
        .filter(|event| {
            event["owner"] == target
                && event["bank"] == STUN_BANK
                && (event["kind"] == "program_started" && event["program"] == 17
                    || event["kind"] == "declaration_started" && event["member"] == landing_member)
        })
        .collect();
    ensure!(
        feedback.len() == 2
            && feedback
                .iter()
                .all(|event| at(trace, event) == Some(landing))
            && feedback
                .iter()
                .filter(|event| event["kind"] == "program_started")
                .count()
                == 1,
        "landing dust lacks its exact own program and declaration clocks"
    );
    let mut samples = 0;
    let mut identity = None;
    for (index, frame) in trace.frames.iter().enumerate() {
        for particle in frame["particles"]
            .as_array()
            .context("missing landing particles")?
        {
            if particle["owner"] != target
                || particle["bank"] != STUN_BANK
                || particle["member"] != landing_member
            {
                continue;
            }
            let id = particle["id"]
                .as_u64()
                .context("missing landing particle identity")?;
            ensure!(
                index == landing + samples
                    && particle["age"].as_u64() == Some(samples as u64 + 1)
                    && particle["initialized"] == true
                    && particle["model"].is_null()
                    && particle["owner_bone"].is_null()
                    && vector_matches(&particle["origin"], origin)
                    && vector_matches(&particle["offset"], [0., 1., 0.])
                    && identity.is_none_or(|identity| identity == id),
                "landing dust lifetime or geometry differs"
            );
            identity = Some(id);
            samples += 1;
        }
    }
    ensure!(
        samples == 13,
        "landing dust must retain its complete authored13update lifetime"
    );
    Ok((
        target,
        feedback[0]["clock"].as_u64().unwrap(),
        feedback[0]["presentation_clock"].as_u64().unwrap(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn trace(stunned: bool) -> Trace {
        let frames = (0..8).map(|index| json!({
            "presentation_tick": index, "combat_tick":index,
            "actors":[{"actor":0,"hp":100,"position":[0.,0.,0.]},{"actor":1,"hp":100,"position":[100.,0.,0.]}],
            "models":[{"actor":0,"clip":if stunned && (3..6).contains(&index) {21} else {3}},{"actor":1,"clip":30}],
            "particles":if stunned && (3..6).contains(&index) { vec![json!({"id":1,"owner":0,"bank":STUN_BANK,"member":19,"initialized":true,"age":index-2})] } else {vec![]}
        })).collect();
        let mut events = vec![
            json!({"kind":"hit","owner":1,"target":0,"action":1,"clock":2,"presentation_clock":2}),
            json!({"kind":"hit","owner":1,"target":0,"action":1,"clock":6,"presentation_clock":6}),
        ];
        if stunned {
            events.push(json!({"kind":"declaration_started","owner":0,"bank":STUN_BANK,"member":19,"clock":3,"presentation_clock":3}));
        }
        Trace {
            case: "enemy:1:0".into(),
            setup: json!({"owner":1,"party":[1],"common_effect_bank":STUN_BANK,"effect_inputs":{"common":{"source":{"sha256":STUN_BANK},"effect_templates":(0..26).map(|id|vec![id]).collect::<Vec<_>>() }},
            "actors":[{"actor":0,"side":0,"conditions":[]}],"contact_stun":[{"clock":2,"target":0,"stunned":stunned},{"clock":6,"target":0,"stunned":false}]}),
            frames,
            events,
            assessment: Value::Null,
            incomplete_observations: vec![],
        }
    }

    #[test]
    fn only_proved_recipient_stun_pose_and_declaration_can_differ() {
        let source = trace(true);
        let native = trace(false);
        let result = qualify(
            &source,
            &native,
            Rule {
                hurt_clip: 3,
                ordinary: true,
                hitstun: 20,
                vertical: None,
            },
        )
        .unwrap();
        assert!(result.permits(3, 0));
        assert!(result.permits(5, 0));
        assert!(!result.permits(6, 0));
        assert!(!result.permits(3, 1));
        assert!(!result.retain_event(0, &source.events[2]));
        assert!(result.retain_event(0, &source.events[0]));
        let mut missing = trace(false);
        missing.events.remove(0);
        assert!(
            !qualify(
                &source,
                &missing,
                Rule {
                    hurt_clip: 3,
                    ordinary: true,
                    hitstun: 20,
                    vertical: None,
                }
            )
            .unwrap()
            .permits(3, 0)
        );
        let mut wrong_pose = trace(true);
        wrong_pose.frames[4]["models"][0]["clip"] = json!(4);
        assert!(
            qualify(
                &wrong_pose,
                &native,
                Rule {
                    hurt_clip: 3,
                    ordinary: true,
                    hitstun: 20,
                    vertical: None,
                }
            )
            .is_err()
        );
        let mut short = trace(true);
        short.frames[5]["particles"] = json!([]);
        assert!(
            qualify(
                &short,
                &native,
                Rule {
                    hurt_clip: 3,
                    ordinary: true,
                    hitstun: 20,
                    vertical: None,
                }
            )
            .is_err()
        );
        let mut wrong_owner = trace(false);
        wrong_owner.frames[3]["models"][1]["clip"] = json!(99);
        assert!(
            !qualify(
                &source,
                &wrong_owner,
                Rule {
                    hurt_clip: 3,
                    ordinary: true,
                    hitstun: 20,
                    vertical: None,
                }
            )
            .unwrap()
            .permits(3, 1)
        );
        let mut moved = trace(false);
        moved.frames[3]["actors"][0]["position"][0] = json!(1.);
        assert!(
            !qualify(
                &source,
                &moved,
                Rule {
                    hurt_clip: 3,
                    ordinary: true,
                    hitstun: 20,
                    vertical: None,
                }
            )
            .unwrap()
            .permits(3, 0)
        );
    }

    #[test]
    fn enemy_recipient_cannot_borrow_party_reaction_roles_without_pins() {
        let mut source = trace(true);
        let mut native = trace(false);
        for trace in [&mut source, &mut native] {
            trace.case = "party:3:67".into();
            trace.setup["actors"][0]["side"] = json!(1);
        }
        let qualified = qualify(
            &source,
            &native,
            Rule {
                hurt_clip: 3,
                ordinary: true,
                hitstun: 20,
                vertical: None,
            },
        )
        .unwrap();
        assert!(!qualified.permits(3, 0));
        assert!(qualified.retain_event(0, &source.events[2]));
    }

    #[test]
    fn uninterrupted_stun_requires_180_grounded_updates() {
        let mut source = trace(true);
        source.events.remove(1);
        source.frames = (0..185).map(|index| json!({
            "presentation_tick":index,"combat_tick":index,
            "actors":[{"actor":0,"hp":100,"position":[0.,0.,0.]}],
            "models":[{"actor":0,"clip":if (3..183).contains(&index) {21} else {3}}],
            "particles":if (3..182).contains(&index) {vec![json!({"id":1,"owner":0,"bank":STUN_BANK,"member":19,"initialized":true,"age":index-2})]} else {vec![]}
        })).collect();
        assert_eq!(witnessed_stuns(&source, 0, false).unwrap()[0].end, 183);
        source.frames[181]["particles"] = json!([]);
        assert!(witnessed_stuns(&source, 0, false).is_err());
    }

    fn vertical_trace(stunned: bool) -> Trace {
        let mut trace = trace(false);
        let hits = [2, 10, 18, 26];
        trace.events = hits.iter().map(|&at| json!({"kind":"hit","owner":1,"target":0,"action":1,"projectile":1,"clock":at,"presentation_clock":at})).collect();
        trace.setup["contact_stun"] = hits
            .iter()
            .map(|&at| json!({"clock":at,"target":0,"stunned":stunned && at==18}))
            .collect();
        if stunned {
            trace.events.push(json!({"kind":"declaration_started","owner":0,"bank":STUN_BANK,"member":19,"clock":19,"presentation_clock":19}));
        }
        let (mut y, mut speed, mut delay, mut falling, mut active, mut idle) =
            (0_f64, 0_f64, 0_usize, false, false, false);
        trace.frames = (0..75).map(|index| {
            if active && !idle {
                if !falling && delay > 0 { delay -= 1; if delay==0 { speed=10.; } }
                if falling || delay==0 { y+=speed; speed-=1.; if y < 0. { y=0.; speed=0.; } }
            }
            let pose = if !active || idle { 0 } else { 3 };
            if index > 46 && y==0. { idle=true; }
            let pose = if hits.contains(&index) {
                active=true; idle=false; delay=7; speed=0.; falling=stunned && index==18; 3
            } else { pose };
            json!({"presentation_tick":index,"combat_tick":index,
                "actors":[{"actor":0,"hp":100,"heading":90.,"position":[0.,y,0.]},{"actor":1,"hp":100,"heading":-90.,"position":[100.,0.,0.]}],
                "models":[{"actor":0,"clip":pose},{"actor":1,"clip":30}],
                "particles":if stunned && (19..26).contains(&index) {vec![json!({"id":1,"owner":0,"bank":STUN_BANK,"member":19,"initialized":true,"age":index-18})]} else {vec![]}})
        }).collect();
        let landing = (27..trace.frames.len())
            .find(|&index| height(&trace.frames[index], 0) == Some(0.))
            .unwrap();
        let y = 2. * height(&trace.frames[landing - 1], 0).unwrap()
            - height(&trace.frames[landing - 2], 0).unwrap()
            - 1.;
        trace.events.push(json!({"kind":"program_started","owner":0,"bank":STUN_BANK,"program":17,"clock":landing,"presentation_clock":landing}));
        trace.events.push(json!({"kind":"declaration_started","owner":0,"bank":STUN_BANK,"member":25,"clock":landing,"presentation_clock":landing}));
        for age in 1..=13 {
            trace.frames[landing+age-1]["particles"].as_array_mut().unwrap().push(json!({"id":2,"owner":0,"bank":STUN_BANK,"member":25,"initialized":true,"age":age,"origin":[0.,y,0.],"offset":[0.,1.,0.],"model":null,"owner_bone":null}));
        }
        trace
    }

    #[test]
    fn delayed_vertical_recoil_requires_complete_own_recurrence_and_landing() {
        let source = vertical_trace(false);
        let native = vertical_trace(true);
        let rule = Rule {
            hurt_clip: 3,
            ordinary: true,
            hitstun: 20,
            vertical: Some(Vertical {
                delay: 7,
                impulse: 10.,
                projectile: 1,
                hitstun: 20,
            }),
        };
        let qualified = qualify(&source, &native, rule).unwrap();
        assert!(qualified.permits_height(20, 0));
        assert!(!qualified.permits_height(20, 1));
        assert!(!qualified.retain_event(1, native.events.last().unwrap()));
        for (index, side) in [(8, 0), (20, 1), (33, 1)] {
            let mut wrong = vertical_trace(side == 1);
            let y = height(&wrong.frames[index], 0).unwrap();
            wrong.frames[index]["actors"][0]["position"][1] = json!(y + 1.);
            assert!(
                if side == 0 {
                    qualify(&wrong, &native, rule)
                } else {
                    qualify(&source, &wrong, rule)
                }
                .is_err()
            );
        }
        let mut heading = vertical_trace(true);
        heading.frames[20]["actors"][0]["heading"] = json!(180.);
        assert!(
            !qualify(&source, &heading, rule)
                .unwrap()
                .permits_height(20, 0)
        );
        let mut recorded = vertical_trace(false);
        recorded.setup["random_inputs"] = json!({"path":"recorded","sha256":"recorded"});
        assert!(
            !qualify(&recorded, &native, rule)
                .unwrap()
                .permits_height(20, 0)
        );
        let mut idle = vertical_trace(true);
        let landed = (27..70)
            .find(|&index| height(&idle.frames[index], 0) == Some(0.))
            .unwrap();
        idle.frames[landed + 1]["models"][0]["clip"] = json!(3);
        assert!(qualify(&source, &idle, rule).is_err());
    }

    #[test]
    fn landing_feedback_requires_own_crossing_recipe_and_retirement() {
        let source = vertical_trace(false);
        let native = vertical_trace(true);
        let rule = Rule {
            hurt_clip: 3,
            ordinary: true,
            hitstun: 20,
            vertical: Some(Vertical {
                delay: 7,
                impulse: 10.,
                projectile: 1,
                hitstun: 20,
            }),
        };
        let qualified = qualify(&source, &native, rule).unwrap();
        for (side, trace) in [&source, &native].iter().enumerate() {
            assert!(!qualified.retain_event(side, trace.events.last().unwrap()));
            assert!(!qualified.retain_event(side, &trace.events[trace.events.len() - 2]));
            assert!(qualified.retain_event(side, &trace.events[0]));
        }
        let mut clock = vertical_trace(true);
        let last = clock.events.len() - 1;
        clock.events[last]["clock"] = json!(0);
        assert!(qualify(&source, &clock, rule).is_err());
        let mut missing = vertical_trace(true);
        missing.events.pop();
        assert!(qualify(&source, &missing, rule).is_err());
        let mut short = vertical_trace(true);
        let landing = landing_feedback(&short, 0).unwrap().1 as usize;
        short.frames[landing + 12]["particles"] = json!([]);
        assert!(qualify(&source, &short, rule).is_err());
        let mut origin = vertical_trace(true);
        origin.frames[landing]["particles"][0]["origin"][1] = json!(0.);
        assert!(qualify(&source, &origin, rule).is_err());
    }

    #[test]
    fn mixed_reactions_only_qualify_feedback_with_identical_poses_and_motion() {
        let mut source = trace(true);
        let mut native = trace(false);
        for trace in [&mut source, &mut native] {
            for frame in &mut trace.frames {
                frame["actors"][0]["position"][1] = json!(100.);
                frame["models"][0]["clip"] = json!(3);
            }
        }
        let rule = Rule {
            hurt_clip: 3,
            ordinary: false,
            hitstun: 20,
            vertical: None,
        };
        let result = qualify(&source, &native, rule).unwrap();
        assert!(!result.retain_event(0, &source.events[2]));
        assert!(result.poses.is_empty() && result.heights.is_empty());
        let mut changed_pose = trace(false);
        for frame in &mut changed_pose.frames {
            frame["actors"][0]["position"][1] = json!(100.);
        }
        changed_pose.frames[4]["models"][0]["clip"] = json!(4);
        assert!(
            qualify(&source, &changed_pose, rule)
                .unwrap()
                .retain_event(0, &source.events[2])
        );
        native.frames[4]["actors"][0]["position"][0] = json!(1.);
        assert!(
            qualify(&source, &native, rule)
                .unwrap()
                .retain_event(0, &source.events[2])
        );
    }
}
