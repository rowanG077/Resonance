use super::*;

#[test]
fn combo_probability_gate_preserves_short_circuit_draw_order() -> Result<()> {
    // Level, completed normal links, TP, throwing, seed, observed residues,
    // admission. Residues exercise both sides of each probability boundary.
    for (level, links, tp, throwing, seed, residues, admitted) in [
        (30, 2, 100, false, 0, &[][..], true),
        (30, 2, 30, false, 52, &[(100, 24)][..], true),
        (30, 2, 30, false, 11, &[(100, 25)][..], false),
        (15, 0, 100, false, 0, &[(8, 0)][..], true),
        (15, 0, 100, false, 14, &[(8, 1)][..], false),
        (30, 0, 100, false, 0, &[][..], false),
        (30, 0, 100, true, 0, &[][..], true),
        (30, 2, 30, true, 158, &[(100, 49)][..], true),
        (30, 2, 30, true, 3462, &[(100, 50), (100, 24)][..], true),
        (
            30,
            2,
            30,
            true,
            220734,
            &[(100, 50), (100, 25), (100, 49)][..],
            true,
        ),
        (
            30,
            2,
            30,
            true,
            586229,
            &[(100, 50), (100, 25), (100, 50)][..],
            false,
        ),
        (15, 0, 30, true, 484, &[(100, 50), (8, 0)][..], true),
        (
            15,
            0,
            30,
            true,
            113054,
            &[(100, 50), (8, 1), (100, 49)][..],
            true,
        ),
        (
            15,
            0,
            30,
            true,
            280646,
            &[(100, 50), (8, 1), (100, 50)][..],
            false,
        ),
    ] {
        let mut prepared = prepared_combo_arte(false)?;
        prepared.actors[2].equipment.stats.level = level;
        prepared.actors[2].equipment.max_tp = 100;
        prepared.actors[2].tp = tp;
        let definition = prepared.resources.actor_setup[2]
            .companion
            .as_mut()
            .unwrap();
        definition.character = if throwing {
            crate::Character::Colette
        } else {
            crate::Character::Lloyd
        };
        definition.limits[6].tp_reserve_percent = 30;
        let mut battle = prepared.finish()?;
        battle.runtime[2].combo.normal_links = links;
        let mut random = crate::Random::new(seed);
        let mut expected = random;
        for &(modulus, residue) in residues {
            assert_eq!(expected.next_u16() % modulus, residue, "seed {seed}");
        }
        let strategy = battle.prepared.actor_setup[2]
            .companion
            .as_ref()
            .unwrap()
            .resolve([1, 6, 2])?;
        assert_eq!(
            selector(&battle, ActorId(2), &mut random, Some(strategy)).offensive_combo_admitted(
                Some(crate::control::NormalState::Attack(if throwing {
                    NormalAttack::Thrust
                } else {
                    NormalAttack::Neutral
                }))
            ),
            admitted,
            "seed {seed}, level {level}, links {links}, TP {tp}, throwing {throwing}"
        );
        assert_eq!(
            random.state(),
            expected.state(),
            "seed {seed}: draw count/order"
        );
    }
    Ok(())
}
