use anyhow::{Context, Result, ensure};
use resonance_battle::{ActorId, Battle, BattleFrame, Pose, WeaponPlayback};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
pub(super) struct Binding {
    actor: usize,
    motion_offset: u16,
    bones: Vec<String>,
    ready: Ready,
}

#[derive(Deserialize)]
struct Ready {
    clip: u16,
    frame: f32,
    rate: f32,
    repeat: bool,
    loop_start: f32,
    stopped: bool,
}

pub(super) fn admit(setup: &Value, core: &Battle) -> Result<Vec<Binding>> {
    let bindings: Vec<Binding> = setup
        .get("secondary_bodies")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    let mut count = 0;
    for actor in 0..core.actors().len() {
        let definition = core
            .models()
            .and_then(|models| models.definition(ActorId::from_index(actor).ok()?))
            .context("missing prepared actor model")?;
        for layers in definition
            .weapons
            .iter()
            .filter_map(|weapon| weapon.body_layers())
        {
            count += 1;
            let observed = bindings
                .iter()
                .filter(|binding| binding.actor == actor)
                .collect::<Vec<_>>();
            ensure!(
                observed.len() == 1,
                "secondary rig has no unique independent ready registration"
            );
            let binding = observed[0];
            for layer in layers {
                ensure!(
                    matches!(layer.playback, WeaponPlayback::Body {offset, ..} if offset == binding.motion_offset),
                    "secondary motion mapping differs from its ready registration"
                );
                ensure!(
                    layer
                        .skeleton
                        .bones
                        .iter()
                        .map(|bone| &bone.name)
                        .eq(binding.bones.iter()),
                    "secondary rig differs from its ready registration"
                );
            }
        }
    }
    ensure!(
        count == bindings.len(),
        "secondary ready registration has no prepared rig"
    );
    Ok(bindings)
}

pub(super) fn ready(bindings: &[Binding], actor: usize) -> Option<(u16, Pose)> {
    let ready = &bindings
        .iter()
        .find(|binding| binding.actor == actor)?
        .ready;
    Some((
        ready.clip,
        Pose {
            frame: ready.frame,
            rate: if ready.stopped { 0. } else { ready.rate },
            repeat: ready.repeat,
            loop_start: ready.loop_start,
            ..Default::default()
        },
    ))
}

pub(super) fn observe(bindings: &[Binding], frame: &BattleFrame) -> Result<Vec<Value>> {
    let mut result = Vec::new();
    for binding in bindings {
        let mut layers = frame
            .weapons
            .iter()
            .filter(|layer| layer.owner.index() == binding.actor && layer.slot == u8::MAX);
        let first = layers
            .next()
            .context("secondary rig disappeared from the frame")?;
        ensure!(
            first.clip.is_some() && first.frame.is_finite(),
            "unknown secondary controller"
        );
        ensure!(
            layers.all(|layer| layer.clip == first.clip
                && layer.frame == first.frame
                && layer.visible == first.visible),
            "secondary render layers disagree about their controller"
        );
        result.push(json!({"actor":binding.actor,"clip":first.clip,"frame":first.frame,"visible":first.visible}));
    }
    Ok(result)
}
