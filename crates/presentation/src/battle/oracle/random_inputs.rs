use super::Trace;
use anyhow::{Context, Result, ensure};
use resonance_battle::{
    ActorId, HammerRing, ProjectileScatter,
    random_inputs::{
        Ailment, Contact, Critical, CriticalRole, HammerRingLaunch, Key, Positioning,
        RecordedInputs, Scatter, Seal, Stun, TargetChoice, Velocity,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Pin {
    path: String,
    pub(super) sha256: String,
}

impl Pin {
    pub(super) fn read_bytes(&self) -> Result<Vec<u8>> {
        let bytes = std::fs::read(&self.path)?;
        ensure!(
            format!("{:x}", Sha256::digest(&bytes)) == self.sha256,
            "input evidence changed: {}",
            self.path
        );
        Ok(bytes)
    }

    fn read(&self) -> Result<Value> {
        Ok(serde_json::from_slice(&self.read_bytes()?)?)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Distribution {
    position_range: u16,
    position_step: f32,
    lateral_range: u16,
    lateral_step: f32,
}

#[derive(Deserialize)]
struct InputKey {
    clock: u32,
    owner: usize,
    target: usize,
    action: u64,
    projectile: Option<u64>,
    contact_source: Option<String>,
}

impl TryFrom<InputKey> for Key {
    type Error = anyhow::Error;

    fn try_from(key: InputKey) -> Result<Self> {
        let contact = match (key.projectile, key.contact_source.as_deref()) {
            (Some(id), None) if id != 0 => Contact::Projectile(id),
            (None, Some("attached")) => Contact::Attached,
            _ => anyhow::bail!("random input contact identity is not witnessed"),
        };
        Ok(Self {
            clock: key.clock,
            owner: ActorId::from_index(key.owner)?,
            target: ActorId::from_index(key.target)?,
            action: key.action,
            contact,
        })
    }
}

fn input_key(row: &Value) -> Result<Key> {
    InputKey::deserialize(row)?.try_into()
}

pub(super) type Inputs = RecordedInputs;

pub(super) fn admit(source: &Trace) -> Result<Option<Inputs>> {
    let Some(fixture) = source.setup.get("random_inputs") else {
        return Ok(None);
    };
    let fixture: Pin = serde_json::from_value(fixture.clone())?;
    let proof = fixture.read()?;
    ensure!(
        proof["format"] == "source-observed-random-input-proof-v1"
            && proof["case"] == source.case
            && proof["status"] == "source_role_attribution_complete",
        "unqualified random input evidence"
    );
    qualify(source, &proof).map(Some)
}

pub(super) fn qualify(source: &Trace, proof: &Value) -> Result<Inputs> {
    ensure!(
        proof["input_types"]["stun_return"] == "unsigned_u16_modulo_100",
        "random input return type is not witnessed"
    );
    let launches = proof["launches"]
        .as_array()
        .context("missing source launches")?;
    let contacts = proof["contacts"]
        .as_array()
        .context("missing source contacts")?;
    let event_matches = |row: &Value, kind: &str| {
        source
            .events
            .iter()
            .filter(|event| {
                event["kind"] == kind
                    && event["presentation_clock"] == row["clock"]
                    && ["owner", "target", "action", "projectile"]
                        .iter()
                        .all(|key| event[key] == row[key])
            })
            .count()
            == 1
    };
    let mut inputs = Inputs::default();
    if let Some(rows) = proof.get("target_choices") {
        ensure!(
            proof["input_types"]["target_return"] == "unsigned_u16_modulo_roster",
            "target choice distribution is not witnessed"
        );
        let mut choices = Vec::new();
        let mut previous_release = None;
        let mut remaining = Vec::new();
        let mut roster = Vec::new();
        for row in rows.as_array().context("invalid target choices")? {
            ensure!(
                event_matches(row, "projectile_started"),
                "target choice has no unique source launch"
            );
            let key = input_key(row)?;
            let observed_roster: Vec<usize> = serde_json::from_value(row["roster"].clone())?;
            let release = (key.clock, key.owner, key.action);
            if previous_release != Some(release) {
                let actors = source.setup["actors"]
                    .as_array()
                    .context("missing actors")?;
                let owner = actors
                    .get(key.owner.index())
                    .context("missing target-choice owner")?;
                roster = actors
                    .iter()
                    .enumerate()
                    .filter(|(_, actor)| {
                        actor["side"] != owner["side"]
                            && actor["hp"].as_u64().is_some_and(|hp| hp > 0)
                    })
                    .map(|(index, _)| index)
                    .collect();
                remaining = roster.clone();
                previous_release = Some(release);
            }
            let candidates: Vec<usize> = serde_json::from_value(row["candidates"].clone())?;
            let values: Vec<u16> = serde_json::from_value(row["values"].clone())?;
            ensure!(
                !roster.is_empty()
                    && observed_roster == roster
                    && candidates == remaining
                    && !values.is_empty(),
                "target choice roster or draw sequence differs"
            );
            for (index, value) in values.iter().enumerate() {
                let target = roster[usize::from(*value) % roster.len()];
                ensure!(
                    if index + 1 == values.len() {
                        target == key.target.index() && remaining.contains(&target)
                    } else {
                        !remaining.contains(&target)
                    },
                    "target choice accepted or rejected draw differs"
                );
            }
            let index = remaining
                .iter()
                .position(|target| *target == key.target.index())
                .unwrap();
            remaining.swap_remove(index);
            choices.push(TargetChoice {
                key,
                candidates: candidates
                    .into_iter()
                    .map(ActorId::from_index)
                    .collect::<Result<_>>()?,
            });
        }
        inputs.targets = Some(choices);
    }
    if let Some(rows) = proof.get("positioning") {
        ensure!(
            proof["input_types"]["positioning_return"] == "unsigned_u16_modulo_neighborhood",
            "positioning distribution is not witnessed"
        );
        let mut recorded = Vec::new();
        let origin = source.frames[0]["presentation_tick"]
            .as_u64()
            .context("missing source clock")?;
        for row in rows.as_array().context("invalid positioning inputs")? {
            let clock: u32 = serde_json::from_value(row["clock"].clone())?;
            let owner: usize = serde_json::from_value(row["owner"].clone())?;
            let target: usize = serde_json::from_value(row["target"].clone())?;
            let action: u64 = serde_json::from_value(row["action"].clone())?;
            let index = source
                .frames
                .iter()
                .position(|frame| {
                    frame["presentation_tick"].as_u64() == Some(origin + u64::from(clock))
                })
                .context("positioning clock is absent")?;
            ensure!(index > 0, "positioning interval lacks its preceding frame");
            let before = &source.frames[index - 1];
            let after = &source.frames[index];
            let interval = &row["source_interval"];
            ensure!(
                before["source_random_state"] == interval["before"]
                    && after["source_random_state"] == interval["after"]
                    && row["strategy"] == 1
                    && source
                        .events
                        .iter()
                        .any(|event| event["kind"] == "recovery_started"
                            && event["owner"] == owner
                            && event["action"] == action
                            && event["presentation_clock"]
                                .as_u64()
                                .is_some_and(|at| at < u64::from(clock))),
                "positioning input lacks an observed recovery interval"
            );
            let model = |frame: &Value| {
                frame["models"]
                    .as_array()
                    .and_then(|models| models.iter().find(|model| model["actor"] == owner))
                    .cloned()
            };
            let previous = model(before).context("missing prior return pose")?;
            let current = model(after).context("missing return pose")?;
            ensure!(
                previous["clip"] != current["clip"]
                    && current["clip"] == row["motion"]
                    && current["frame"] == 0.,
                "positioning input lacks an observed return pose entry"
            );
            let all: Vec<u16> = serde_json::from_value(interval["values"].clone())?;
            let mut state: u32 = serde_json::from_value(interval["before"].clone())?;
            for value in &all {
                state = state.wrapping_mul(0x41c64e6d).wrapping_add(0x12d687);
                ensure!(
                    (state >> 16) as u16 == *value,
                    "positioning RNG interval differs"
                );
            }
            ensure!(
                interval["after"] == state,
                "positioning RNG interval is incomplete"
            );
            let count: usize = serde_json::from_value(row["draw_count"].clone())?;
            let values: Vec<u16> = serde_json::from_value(row["values"].clone())?;
            ensure!(
                count > 0
                    && count.is_multiple_of(2)
                    && values.len() == count
                    && all.get(..count) == Some(values.as_slice()),
                "positioning inputs differ from their attributed source prefix"
            );
            recorded.push(Positioning {
                clock,
                owner: ActorId::from_index(owner)?,
                target: ActorId::from_index(target)?,
                action,
                values,
            });
        }
        inputs.positioning = Some(recorded);
    }
    for row in launches {
        let distribution: Distribution =
            serde_json::from_value(proof["input_types"]["scatter"].clone())?;
        ensure!(
            distribution.position_range != 0
                && distribution.lateral_range != 0
                && distribution.position_step.is_finite()
                && distribution.lateral_step.is_finite()
                && proof["input_types"]["scatter_return"] == "signed_i16_remainder",
            "invalid random input distribution"
        );
        ensure!(
            row["matching_source_triples"] == 1 && event_matches(row, "projectile_started"),
            "scatter input has no unique source launch"
        );
        let values: [u16; 3] = serde_json::from_value(row["values"].clone())?;
        let ranges = [
            distribution.position_range,
            distribution.position_range,
            distribution.lateral_range,
        ];
        let remainders =
            std::array::from_fn::<_, 3, _>(|i| i32::from(values[i] as i16) % i32::from(ranges[i]));
        ensure!(
            serde_json::to_value(remainders)? == row["signed_remainders"],
            "scatter input signed range differs"
        );
        let key = input_key(row)?;
        ensure!(
            matches!(key.contact, Contact::Projectile(_)),
            "scatter input is not a projectile"
        );
        inputs.scatter.push(Scatter {
            key,
            values,
            distribution: ProjectileScatter {
                position_range: distribution.position_range,
                position_step: distribution.position_step,
                lateral_range: distribution.lateral_range,
                lateral_step: distribution.lateral_step,
            },
        });
    }
    if let Some(rows) = proof.get("hammer_ring_launches") {
        for row in rows.as_array().context("invalid hammer ring inputs")? {
            ensure!(
                proof["input_types"]["hammer_ring_return"] == "signed_i16_remainder"
                    && row["matching_source_triples"] == 1
                    && event_matches(row, "projectile_started"),
                "hammer ring input has no unique source launch"
            );
            let distribution: HammerRing = serde_json::from_value(row["distribution"].clone())?;
            let index: u8 = serde_json::from_value(row["ring_index"].clone())?;
            let values: [u16; 3] = serde_json::from_value(row["values"].clone())?;
            ensure!(
                index < distribution.count && distribution.jitter_range > 0,
                "invalid hammer ring distribution"
            );
            let remainders =
                values.map(|value| i32::from(value as i16) % i32::from(distribution.jitter_range));
            ensure!(
                serde_json::to_value(remainders)? == row["signed_remainders"],
                "hammer ring signed range differs"
            );
            inputs.hammer_ring.push(HammerRingLaunch {
                key: input_key(row)?,
                index,
                values,
                distribution,
            });
        }
    }
    if let Some(rows) = proof.get("velocity_launches") {
        for row in rows.as_array().context("invalid velocity jitter inputs")? {
            ensure!(
                proof["input_types"]["velocity_jitter_return"] == "signed_i16_remainder"
                    && row["matching_source_values"] == 1
                    && event_matches(row, "projectile_started"),
                "velocity jitter input has no unique source launch"
            );
            let key = input_key(row)?;
            let axis: usize = serde_json::from_value(row["axis"].clone())?;
            let amplitude: f32 = serde_json::from_value(row["amplitude"].clone())?;
            let value: u16 = serde_json::from_value(row["value"].clone())?;
            ensure!(
                matches!(key.contact, Contact::Projectile(_))
                    && axis < 3
                    && amplitude.is_finite()
                    && amplitude.abs() >= 1.,
                "invalid velocity jitter distribution"
            );
            let steps = (amplitude * 10.) as i32;
            ensure!(
                row["steps"] == steps
                    && row["scale"] == 0.1
                    && row["signed_remainder"] == i32::from(value as i16) % steps,
                "velocity jitter input signed range differs"
            );
            inputs.velocity.push(Velocity {
                key,
                axis,
                amplitude,
                value,
            });
        }
    }
    for row in contacts {
        ensure!(
            event_matches(row, "hit"),
            "stun input has no unique source contact"
        );
        let value: u16 = serde_json::from_value(row["stun_value"].clone())?;
        let chance_percent: u16 = serde_json::from_value(row["effective_chance"].clone())?;
        ensure!(
            u64::from(value % 100)
                == row["stun_remainder"]
                    .as_u64()
                    .context("missing stun range witness")?,
            "stun input range differs"
        );
        inputs.stun.push(Stun {
            key: input_key(row)?,
            value,
            chance_percent,
        });
    }
    if let Some(rows) = proof.get("critical_draws") {
        ensure!(
            proof["input_types"]["critical_chance_return"] == "unsigned_u16_modulo_4"
                && proof["input_types"]["critical_return"] == "unsigned_u16_modulo_100",
            "critical input return types are not witnessed"
        );
        let mut criticals = Vec::new();
        for row in rows.as_array().context("invalid critical inputs")? {
            ensure!(
                event_matches(row, "hit"),
                "critical input has no unique source contact"
            );
            let key = input_key(row)?;
            for (role, name) in [
                (CriticalRole::Chance, "chance_value"),
                (CriticalRole::Roll, "value"),
            ] {
                criticals.push(Critical {
                    key,
                    role,
                    value: serde_json::from_value(row[name].clone())?,
                });
            }
        }
        inputs.criticals = Some(criticals);
    }
    if let Some(rows) = proof.get("ailment_draws") {
        ensure!(
            proof["input_types"]["ailment_return"] == "unsigned_u16_modulo_100",
            "ailment input return type is not witnessed"
        );
        let mut ailments = Vec::new();
        for row in rows.as_array().context("invalid ailment inputs")? {
            ensure!(
                event_matches(row, "hit"),
                "ailment input has no unique source contact"
            );
            let value: u16 = serde_json::from_value(row["value"].clone())?;
            ensure!(
                row["remainder"] == value % 100,
                "ailment input range differs"
            );
            ailments.push(Ailment {
                key: input_key(row)?,
                condition: serde_json::from_value(row["condition"].clone())?,
                resistance: serde_json::from_value(row["resistance"].clone())?,
                value,
            });
        }
        inputs.ailments = Some(ailments);
    }
    if let Some(rows) = proof.get("seal_draws") {
        ensure!(
            proof["input_types"]["seal_return"] == "unsigned_u16_reverse_percentile",
            "seal input distribution is not witnessed"
        );
        let mut seals = Vec::new();
        for row in rows.as_array().context("invalid seal inputs")? {
            let mut contact = row.clone();
            contact["clock"] = row["contact_clock"].clone();
            ensure!(
                event_matches(&contact, "hit"),
                "seal input has no unique source contact"
            );
            let key = input_key(row)?;
            let contact_clock: u32 = serde_json::from_value(row["contact_clock"].clone())?;
            ensure!(
                contact_clock < key.clock
                    && (!matches!(key.contact, Contact::Projectile(_))
                        || key.clock - contact_clock == 1),
                "seal input does not follow its contact"
            );
            let value: u16 = serde_json::from_value(row["value"].clone())?;
            let percentile = 99 - value % 100;
            ensure!(
                row["remainder"] == value % 100 && row["percentile"] == percentile,
                "seal input percentile differs"
            );
            seals.push(Seal {
                key,
                condition: serde_json::from_value(row["condition"].clone())?,
                chance_percent: serde_json::from_value(row["chance_percent"].clone())?,
                percentile,
            });
        }
        inputs.seals = Some(seals);
    }
    let outcomes = source.setup["contact_stun"]
        .as_array()
        .context("missing contact stun outcomes")?;
    let protected = proof
        .get("protected_contacts")
        .map(|rows| rows.as_array().context("invalid protected contacts"))
        .transpose()?
        .map_or(&[][..], Vec::as_slice);
    for row in protected {
        ensure!(
            event_matches(row, "hit")
                && outcomes.iter().any(|outcome| {
                    outcome["clock"] == row["clock"]
                        && outcome["target"] == row["target"]
                        && outcome["stunned"] == false
                }),
            "protected contact lacks its observed unstunned outcome"
        );
    }
    ensure!(
        outcomes.len() == inputs.stun.len() + protected.len(),
        "random input fixture omits an observed stun attempt"
    );
    // Keep the source observations intact; inputs never supply expected outcomes.
    ensure!(
        proof["contact_stun"] == source.setup["contact_stun"],
        "random input proof contacts differ from observations"
    );
    Ok(inputs)
}
