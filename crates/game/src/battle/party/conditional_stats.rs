use resonance_battle::CombatStats;

#[allow(clippy::too_many_arguments)]
pub(super) fn project(
    base: CombatStats,
    kills_damage: bool,
    kills: u16,
    unlocked: bool,
    battle_cry: bool,
    chivalry: bool,
    guilt: bool,
    formation: &[u8],
) -> CombatStats {
    let weapon_bonus = if unlocked && kills_damage {
        i64::from(kills)
    } else {
        0
    };
    let mut formation = formation.iter().take(4).take_while(|&&id| id != 0);
    let percent = i64::from(battle_cry) * 10
        + if chivalry {
            5 * formation
                .clone()
                .filter(|&&id| matches!(id, 2 | 4 | 5 | 7))
                .count() as i64
        } else {
            0
        };
    let guilt = guilt && formation.any(|&id| id == 7);
    // Conditional percentages share the saved derived base. Flat weapon and
    // formation bonuses are added afterward, without compounding on refresh.
    let boosted = |base: i32, flat: i64| {
        let base = i64::from(base.max(0));
        (base + base * percent / 100 + flat).min(i64::from(i32::MAX)) as i32
    };
    CombatStats {
        slash: boosted(base.slash, weapon_bonus + i64::from(guilt) * 50),
        thrust: boosted(base.thrust, weapon_bonus + i64::from(guilt) * 50),
        defense: boosted(base.defense, i64::from(guilt) * 10),
        intelligence: boosted(base.intelligence, 0),
        accuracy: base.accuracy.max(0),
        evasion: base.evasion.max(0),
        level: base.level,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn base() -> CombatStats {
        CombatStats {
            slash: 101,
            thrust: 89,
            defense: 57,
            intelligence: 99,
            accuracy: 83,
            evasion: 17,
            level: 20,
        }
    }
    #[test]
    fn saved_base_receives_combined_percentages_and_flat_bonuses() {
        let stats = project(base(), true, 1234, true, true, true, true, &[2, 4, 5, 7]);
        assert_eq!(
            (stats.slash, stats.thrust, stats.defense, stats.intelligence),
            (1415, 1399, 84, 128)
        );
        assert_eq!((stats.accuracy, stats.evasion, stats.level), (83, 17, 20));
    }
    #[test]
    fn formation_is_first_four_until_zero_and_ignores_current_availability() {
        assert_eq!(
            project(base(), false, 0, false, false, true, true, &[1, 3, 6, 8, 7]),
            base()
        );
        assert_eq!(
            project(base(), false, 0, false, false, true, true, &[1, 0, 7]),
            base()
        );
        let stats = project(base(), false, 0, false, false, true, true, &[7]);
        assert_eq!(
            (stats.slash, stats.thrust, stats.defense, stats.intelligence),
            (156, 143, 69, 103)
        );
    }
    #[test]
    fn devils_arms_requires_an_unlocked_matching_weapon_and_kills_never_wrap() {
        for (weapon, unlocked) in [(false, false), (false, true), (true, false)] {
            assert_eq!(
                project(base(), weapon, 5000, unlocked, false, false, false, &[]),
                base()
            );
        }
        let mut previous = base().slash;
        for kills in [0, 255, 256, 32767, 32768, u16::MAX] {
            let projected = project(base(), true, kills, true, false, false, false, &[]);
            assert_eq!(projected.slash, base().slash + i32::from(kills));
            assert!(projected.slash >= previous);
            previous = projected.slash;
        }
    }
    #[test]
    fn stacked_bonuses_are_nonnegative_and_cap_at_the_native_stat_limit() {
        let mut stats = base();
        stats.slash = i32::MAX - 1;
        stats.thrust = i32::MAX;
        stats.defense = i32::MAX;
        stats.intelligence = i32::MAX;
        let projected = project(stats, true, u16::MAX, true, true, true, true, &[2, 4, 5, 7]);
        assert_eq!(
            (
                projected.slash,
                projected.thrust,
                projected.defense,
                projected.intelligence
            ),
            (i32::MAX, i32::MAX, i32::MAX, i32::MAX)
        );
        stats.slash = -1;
        stats.defense = i32::MIN;
        stats.accuracy = -10;
        let projected = project(stats, false, 0, false, false, false, false, &[]);
        assert_eq!(
            (projected.slash, projected.defense, projected.accuracy),
            (0, 0, 0)
        );
    }

    #[test]
    fn refresh_recomputes_from_saved_base_and_new_kills_without_compounding() {
        let first = project(base(), true, 40, true, true, false, false, &[]);
        let refreshed = project(base(), true, 41, true, true, false, false, &[]);
        assert_eq!(refreshed.slash, first.slash + 1);
        assert_eq!(
            project(base(), false, 41, true, true, false, false, &[]).slash,
            111
        );
    }

    #[test]
    #[ignore = "requires cooked menu/session metadata; CPU only"]
    fn active_recipes_project_without_changing_learned_history_or_saved_vitals()
    -> anyhow::Result<()> {
        use resonance_content::{menu_data::MenuData, session::SessionData};
        use resonance_events::party::Party;
        let root = std::env::var_os("RESONANCE_TEST_ASSETS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/cooked")
            });
        let menus: MenuData =
            serde_json::from_slice(&std::fs::read(root.join("game/menu-data.json")).unwrap())
                .unwrap();
        let session: SessionData =
            serde_json::from_slice(&std::fs::read(root.join("game/session-data.json")).unwrap())
                .unwrap();
        let mut party = Party::new(&session, Default::default()).unwrap();
        for (character, skills, percent) in [
            (1, [18, 21, 14, 0], 10),
            (5, [41, 1, 0, 0], 20),
            (8, [41, 1, 0, 0], 20),
        ] {
            let member = &mut party.members[character];
            member.ex_skills = skills;
            member.compound_ex_skills.clear();
            let before = serde_json::to_value(&*member).unwrap();
            let base = member.stats_for(&menus, character);
            let projected = super::super::loadout(&menus, member, character)?.battle_stats(
                &[2, 4, 5, 7],
                false,
                0,
            );
            assert_eq!(
                projected.slash,
                i32::from(base.slash) * (100 + percent) / 100
            );
            assert_eq!(
                projected.intelligence,
                i32::from(base.intelligence) * (100 + percent) / 100
            );
            assert_eq!(serde_json::to_value(&*member).unwrap(), before);
            member.hp = 0;
            assert_eq!(
                super::super::loadout(&menus, member, character)?.battle_stats(
                    &[2, 4, 5, 7],
                    false,
                    0
                ),
                projected
            );
            member.compound_ex_skills.extend(
                (0..menus.ex_skills.characters[character].compounds.len()).map(|index| index as u8),
            );
            assert_eq!(
                super::super::loadout(&menus, member, character)?.battle_stats(
                    &[2, 4, 5, 7],
                    false,
                    0
                ),
                projected
            );
            member.ex_skills[0] = 0;
            let removed = super::super::loadout(&menus, member, character)?.battle_stats(
                &[2, 4, 5, 7],
                false,
                0,
            );
            let base = member.stats_for(&menus, character);
            assert_eq!(removed.slash, i32::from(base.slash));
        }
        Ok(())
    }
}
