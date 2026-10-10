use super::*;
use resonance_battle::VerticalRecoil;
use resonance_content::{battle_profile::ProfileTraits, battle_recoil};
use resonance_game::battle::recoil::Parameters;

pub(super) fn source() -> battle_recoil::Table {
    battle_recoil::Table {
        source_sha256: "a".repeat(64),
        light_vertical_scale: 1.15,
        heavy_vertical_scale: 0.75,
        default_guard_recovery_bonuses: vec![10, 0, 5, 25, 10],
    }
}

fn snapshot(source: &battle_recoil::Table) -> Files {
    let mut files = files();
    files.insert(
        battle_recoil::PATH.into(),
        serde_json::to_vec(source).unwrap().into(),
    );
    files
}

#[test]
fn preparation_requires_assets_and_resolves_weight_and_guard_preferences() -> Result<()> {
    assert!(Parameters::load(&Files::default()).is_err());
    let files = snapshot(&source());
    let parameters = Parameters::load(&files)?;
    assert_eq!(parameters.guard_recovery_bonus(0, 0)?, 10);
    assert_eq!(parameters.guard_recovery_bonus(0, 1)?, 0);
    assert_eq!(parameters.guard_recovery_bonus(0, 2)?, 5);
    assert_eq!(parameters.guard_recovery_bonus(0, 3)?, 25);
    assert_eq!(parameters.guard_recovery_bonus(0, 4)?, 10);
    assert!(parameters.guard_recovery_bonus(0, 5).is_err());
    assert_eq!(parameters.guard_recovery_bonus(3, 255)?, 25);
    let ordinary = ProfileTraits::default();
    for (weight, vertical) in [
        (1, VerticalRecoil::Scale(1.15)),
        (2, VerticalRecoil::Scale(0.75)),
        (3, VerticalRecoil::Grounded),
        (255, VerticalRecoil::Unchanged),
    ] {
        assert_eq!(parameters.profile(weight, &ordinary).vertical, vertical);
    }
    let immovable = ProfileTraits {
        knockdown_immune: true,
        ..ordinary
    };
    let grounded = ProfileTraits {
        launch_immune: true,
        ..ordinary
    };
    let interruptible = ProfileTraits {
        clear_pending_on_hit: true,
        ..ordinary
    };
    let immovable = parameters.profile(0, &immovable);
    assert!(!immovable.can_knock_down && !immovable.can_launch);
    assert!(!parameters.profile(0, &grounded).can_launch);
    assert!(!parameters.profile(3, &ordinary).can_launch);
    assert!(parameters.profile(0, &interruptible).clear_pending);
    Ok(())
}
