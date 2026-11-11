use super::*;
use crate::{
    battle_victory::{self, document as victories},
    library::Mappings,
};
use serde_json::json;

fn characters() -> Result<Mappings> {
    Mappings::new(
        crate::ui_art::characters::NAMES
            .iter()
            .enumerate()
            .map(|(id, name)| ((id + 1) as u32, (*name).into()))
            .collect(),
    )
}

#[test]
fn declaration_authoring_names_preserve_random_bits_and_character_identity() -> Result<()> {
    let names = characters()?;
    for (mask, accepted) in [(1, 1), (7, 0), (4, 4), (3, 0)] {
        let gate = battle_victory::RandomGate { mask, accepted };
        let bits = victories::RandomBits::from(gate);
        let restored = bits.prepare()?;
        for sample in 0..=u16::MAX {
            assert_eq!(gate.accepts(sample), restored.accepts(sample));
        }
    }
    for gate in [
        json!({}),
        json!({"set":[1,1]}),
        json!({"set":[1],"clear":[1]}),
        json!({"clear":[16]}),
    ] {
        assert!(
            serde_json::from_value::<victories::RandomBits>(gate)?
                .prepare()
                .is_err()
        );
    }
    let mut group: victories::NamedGroup = serde_json::from_value(
        json!({"id":1,"character":"lloyd","participants":["lloyd","sheena"],"required_leader":"sheena","voice":null,"condition":"sheena_affinity","selection":{"kind":"ordinary","priority":1,"gate":{"set":[0]}}}),
    )?;
    let before = group.clone().prepare(&names)?;
    group.character = "hero".into();
    group.participants[0] = "hero".into();
    let renamed = Mappings::new(
        names
            .iter()
            .map(|(id, name)| (id, if id == 1 { "hero" } else { name }.into()))
            .collect(),
    )?;
    assert_eq!(
        serde_json::to_value(group.prepare(&renamed)?)?,
        serde_json::to_value(before)?
    );
    let recoil: Recoil = serde_json::from_value(
        json!({"impulses":{"1":{"horizontal":4,"vertical":10},"0":{"horizontal":0,"vertical":0}},"guard_speed":3,"suppression_distance":400,"light_vertical_scale":1.15,"heavy_vertical_scale":0.75,"default_guard_recovery_bonuses":{"hero":25,"colette":5}}),
    )?;
    let retained = recoil.clone().prepare(&renamed)?;
    assert_eq!(retained.impulses, [[0., 0.], [4., 10.]]);
    assert_eq!(retained.default_guard_recovery_bonuses[&1], 25);
    let mut edited = recoil;
    edited.impulses.get_mut(&1).unwrap().vertical = 20.;
    assert_eq!(edited.prepare(&renamed)?.impulses[1], [4., 20.]);
    assert_eq!(retained.impulses[1], [4., 10.]);
    Ok(())
}
