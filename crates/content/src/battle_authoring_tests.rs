//! Editable battle defaults and model shadow membership.
use crate::{
    animation::Skeleton,
    battle_action::{ActionHit, ActionPose},
    library::{Library, Mod},
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::fs;

fn glb(nodes: Value) -> Result<Vec<u8>> {
    resonance_asset_writer::gltf::pack_glb(&json!({"asset":{"version":"2.0"},"nodes":nodes}), &[])
}

#[test]
fn battle_authoring_names_and_defaults_keep_explicit_zero_and_independent_variants() -> Result<()> {
    let ordinary: ActionPose = serde_json::from_value(json!({"clip":"cast-opening"}))?;
    ensure!((ordinary.clip, ordinary.rate, ordinary.blend) == (13, 0.5, 4));
    let alternate: ActionPose = serde_json::from_value(
        json!({"clip":"slot-55","rate":0,"blend":0,"start":2,"repeat":true}),
    )?;
    ensure!(
        (
            alternate.clip,
            alternate.rate,
            alternate.blend,
            alternate.start,
            alternate.repeat
        ) == (55, 0., 0, 2., true)
    );
    let baseline: ActionHit =
        serde_json::from_value(json!({"kind":"slash","power":{"percent":90}}))?;
    let edited: ActionHit = serde_json::from_value(
        json!({"kind":"slash","power":{"percent":90},"guard_pressure":0,"hitstun":0,"cooldown":0,"stun_chance":0,"stagger":0,"recoil":0,"element":"neutral"}),
    )?;
    ensure!(
        (
            baseline.guard_pressure,
            baseline.hitstun,
            baseline.cooldown,
            baseline.stun_chance,
            baseline.stagger,
            baseline.recoil
        ) == (1, 20, 30, 1, 1, 1)
    );
    ensure!(
        (
            edited.guard_pressure,
            edited.hitstun,
            edited.cooldown,
            edited.stun_chance,
            edited.stagger,
            edited.recoil
        ) == (0, 0, 0, 0, 0, 0)
    );
    let restored: ActionHit = serde_json::from_slice(&serde_json::to_vec(&edited)?)?;
    ensure!(serde_json::to_value(restored)? == serde_json::to_value(edited)?);
    ensure!(serde_json::from_value::<ActionPose>(json!({"clip":13})).is_err());
    ensure!(serde_json::from_value::<ActionPose>(json!({"clip":"unmapped-role"})).is_err());
    Ok(())
}

#[test]
fn battle_authoring_shadow_properties_survive_joint_renames_and_mod_snapshots() -> Result<()> {
    let base = tempfile::tempdir()?;
    let modification = tempfile::tempdir()?;
    fs::write(
        base.path().join("content.json"),
        serde_json::to_vec(
            &json!({"version":crate::library::VERSION,"game":crate::library::GAME}),
        )?,
    )?;
    let nodes = json!([
        {"name":"body","extras":{"resonance_bone":0}},
        {"name":"decoration","extras":{"resonance_bone":1,"shadow_bounds":false}},
        {"name":"hand-custom","extras":{"resonance_bone":2,"shadow_bounds":true}}
    ]);
    fs::write(base.path().join("body.glb"), glb(nodes.clone())?)?;
    let library = Library::open(
        base.path(),
        &[Mod {
            name: "shadow".into(),
            root: modification.path().into(),
        }],
    )?;
    let mut retained = library.snapshot();
    let before = Skeleton::from_glb(&retained.read("body.glb", 65536)?)?;
    let mut renamed = nodes.clone();
    renamed[0]["name"] = "ns00".into();
    renamed[1]["name"] = "hips".into();
    fs::write(modification.path().join("body.glb"), glb(renamed.clone())?)?;
    let flags = |s: Skeleton| {
        s.bones
            .into_iter()
            .map(|b| b.shadow_bounds)
            .collect::<Vec<_>>()
    };
    ensure!(flags(before) == [true, false, true]);
    ensure!(
        flags(Skeleton::from_glb(
            &library.snapshot().read("body.glb", 65536)?
        )?) == [true, false, true]
    );
    renamed[1]["extras"]["shadow_bounds"] = true.into();
    fs::write(modification.path().join("body.glb"), glb(renamed.clone())?)?;
    ensure!(
        flags(Skeleton::from_glb(
            &library.snapshot().read("body.glb", 65536)?
        )?) == [true, true, true]
    );
    ensure!(flags(Skeleton::from_glb(&retained.read("body.glb", 65536)?)?) == [true, false, true]);
    renamed[1]["extras"]["shadow_bounds"] = "false".into();
    ensure!(Skeleton::from_glb(&glb(renamed)?).is_err());
    Ok(())
}
