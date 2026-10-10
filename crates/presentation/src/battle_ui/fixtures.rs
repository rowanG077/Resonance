//! Condition priorities and prepared HUD rendering.
use super::*;
use resonance_battle::conditions::{Conditions, Layers};

pub(super) fn asset_root() -> std::path::PathBuf {
    std::env::var_os("RESONANCE_TEST_ASSETS").map_or_else(
        || std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets"),
        Into::into,
    )
}

pub(super) fn dialogue_font() -> Result<BitmapFont> {
    let root = asset_root();
    let dialogue: DialogueArt =
        serde_json::from_slice(&std::fs::read(root.join("ui/dialogue.json"))?)?;
    let font: BitmapFont = serde_json::from_slice(&std::fs::read(root.join(dialogue.font))?)?;
    font.validate()?;
    Ok(font)
}

pub(super) fn battle_font() -> Result<BitmapFont> {
    let art: Art = serde_json::from_slice(&std::fs::read(
        asset_root().join(resonance_content::battle_ui::PATH),
    )?)?;
    art.font.validate()?;
    Ok(art.font)
}

pub(super) fn actor(layers: Layers) -> resonance_battle::ActorFrame {
    let mut actor = crate::test_support::actor(Side::Party, 800, 100);
    actor.tp = 7;
    actor.conditions = Conditions::new(layers);
    actor.position = [100., 4., 20.];
    actor.heading = 90.;
    actor.facing_direction = [1., 0., 0.];
    actor.body.center_offset = [0., 50., 0.];
    resonance_battle::ActorFrame {
        state: actor,
        activity: Activity::Idle,
    }
}

pub(super) fn frame(actor: resonance_battle::ActorFrame) -> BattleFrame {
    BattleFrame {
        update: 99,
        targets: vec![None],
        actors: vec![actor],
        camera: Some(resonance_battle::CameraPose {
            eye: [0., 100., 500.],
            focus: [0.; 3],
            pitch: 0.,
            yaw: 0.,
            radius: 500.,
        }),
        ..Default::default()
    }
}
