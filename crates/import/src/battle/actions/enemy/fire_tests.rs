use super::*;

#[test]
#[ignore = "requires privately extracted US assets; parses all original enemy rows without asset encoding"]
fn original_enemy_fire_keeps_every_binding_cast_override_and_resource_dependency() {
    use resonance_content::battle::action_program::CommandDependency;
    use resonance_content::battle::effects::{EffectBank, EffectId};
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let inventory = crate::battle::action_inventory::recover(&extracted).unwrap();
    assert_eq!(inventory.artes.artes.len(), 253);
    assert_eq!(inventory.enemies.packages.len(), 251);
    assert_eq!(
        inventory
            .enemies
            .packages
            .iter()
            .map(|p| p.actions.len())
            .sum::<usize>(),
        1210
    );
    let bindings = inventory
        .enemies
        .packages
        .iter()
        .flat_map(|p| {
            p.actions
                .iter()
                .filter(|a| matches!(a.native_technique, Some(205..=207)))
                .map(move |a| {
                    (
                        p.monster,
                        a.id,
                        a.native_technique.unwrap(),
                        p.variant_count,
                    )
                })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        bindings,
        [
            (26, 3, 205, 1),
            (26, 4, 206, 1),
            (94, 2, 207, 2),
            (109, 2, 206, 1),
            (109, 3, 205, 1),
            (180, 2, 205, 1),
            (191, 4, 207, 3),
            (191, 7, 205, 3),
            (197, 4, 205, 2),
            (197, 6, 206, 2),
            (197, 7, 206, 2),
            (209, 3, 205, 1),
            (209, 4, 207, 1),
            (216, 6, 205, 1),
            (216, 7, 207, 1),
            (216, 8, 207, 1),
            (233, 3, 205, 1),
            (233, 9, 206, 1),
            (234, 14, 206, 1),
            (234, 16, 207, 1),
            (235, 13, 205, 1),
            (240, 7, 207, 1),
            (240, 9, 206, 1),
            (241, 9, 205, 1),
            (249, 2, 206, 1),
            (249, 3, 205, 1),
        ]
    );
    assert_eq!(bindings.iter().map(|b| usize::from(b.3)).sum::<usize>(), 34);
    assert!(
        !inventory
            .enemies
            .packages
            .iter()
            .flat_map(|p| &p.actions)
            .flat_map(|a| [a.commands.as_ref(), a.recovery_commands.as_ref()])
            .flatten()
            .flat_map(|p| &p.commands)
            .flat_map(|c| &c.dependencies)
            .any(|d| matches!(d, CommandDependency::NativeTechnique { id: 205..=207, .. }))
    );

    let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
    let archive = fs::read(extracted.join("files/BTL/BTLenemy.dat")).unwrap();
    let executable = fs::read(extracted.join("sys/main.dol")).unwrap();
    let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
    let directory = word(&usual, 0x2c).unwrap() as usize;
    let mut complete_enemies = vec![];
    // Expected countdowns include zero-row fallback to enemy base + first native menu.
    // Explicit row TP takes precedence; Eruption's player aliases do not alter it.
    for (monster, rows) in [
        (26, vec![(3, 240, 40, 0, 0), (4, 430, 80, 0, 0)]),
        (94, vec![(2, 270, 24, 34306, 34310)]),
        (
            109,
            vec![(2, 390, 55, 34608, 34611), (3, 200, 24, 34608, 34611)],
        ),
        (180, vec![(2, 75, 24, 0, 0)]),
        (191, vec![(4, 90, 24, 0, 0), (7, 100, 24, 0, 0)]),
        (
            197,
            vec![
                (4, 90, 24, 34112, 34117),
                (6, 120, 55, 34112, 34118),
                (7, 120, 55, 34112, 34118),
            ],
        ),
        (209, vec![(3, 75, 24, 0, 0), (4, 75, 24, 0, 0)]),
        (
            216,
            vec![
                (6, 120, 24, 33883, 33892),
                (7, 75, 24, 33883, 33893),
                (8, 75, 24, 33883, 33893),
            ],
        ),
        (
            233,
            vec![(3, 70, 24, 34650, 34657), (9, 240, 55, 34669, 34668)],
        ),
        (
            234,
            vec![(14, 60, 8, 34042, 34046), (16, 60, 8, 34042, 34042)],
        ),
        (235, vec![(13, 90, 12, 33362, 33427)]),
        (
            240,
            vec![(7, 75, 24, 34897, 34013), (9, 100, 55, 34897, 34016)],
        ),
        (241, vec![(9, 90, 10, 33684, 33746)]),
        (
            249,
            vec![(2, 430, 55, 34608, 34611), (3, 240, 24, 34608, 34611)],
        ),
    ] {
        let start = word(&usual, directory + usize::from(monster) * 4).unwrap() as usize;
        let end = word(&usual, directory + (usize::from(monster) + 1) * 4).unwrap() as usize;
        let bytes = compression::decode(&archive[start..end]).unwrap();
        let metadata = &bytes[usize::from(half(&bytes, 4).unwrap())..];
        let casts = casting(
            &bytes,
            &crate::arte::read(&executable).unwrap(),
            &usual,
            &rel,
        )
        .unwrap();
        assert_eq!(metadata[0xa6], 0);
        for (id, duration, tp, begin, release_voice) in rows {
            let cast = &casts[&id];
            assert_eq!(
                (
                    cast.duration,
                    cast.tp,
                    cast.voices.begin,
                    cast.voices.release
                ),
                (duration, tp, begin, release_voice),
                "enemy {monster}/{id}"
            );
            assert_eq!((cast.pulse, cast.release_effect), (3, 7));
            assert!(
                cast.commands.is_empty() && !cast.loop_commands && cast.early_release.is_none()
            );
            assert_eq!(cast.resume_loop_start, if monster == 234 { 10 } else { 0 });
            assert!(matches!(
                cast.animation,
                AnimationCommand::Play {
                    clip: 11,
                    blend: 8,
                    start: 0,
                    rate: 0.5,
                    looping: true,
                    ..
                }
            ));
            assert!(matches!(cast.release, AnimationCommand::Play {
                clip: 13, blend: 4, start: 0, rate: 0.5, looping, ..
            } if looping == (monster == 234)));
            assert!(matches!(cast.resume, AnimationCommand::Play {
                clip: 12, blend: 4, start: 2, rate: 0.5, looping, ..
            } if looping == (monster == 234)));
        }
        if matches!(monster, 26 | 109 | 180 | 249) {
            let mut enemy = enemy_actions(&bytes, monster).unwrap();
            assert_eq!(
                enemy.actions.len(),
                inventory
                    .enemies
                    .packages
                    .iter()
                    .find(|p| p.monster == monster)
                    .unwrap()
                    .actions
                    .len()
            );
            enemy.casting = casts;
            complete_enemies.push(enemy);
        }
    }
    assert_eq!(
        required_techniques(
            &crate::arte::read(&executable).unwrap(),
            &[],
            &complete_enemies
        )
        .unwrap(),
        [67, 68]
    );
    let techniques = technique_actions(&extracted, &rel, &usual, &[67, 68, 69]).unwrap();
    for technique in &techniques {
        assert_eq!(technique.enemy_spell().unwrap() as u16, technique.native_id);
    }
    let actions = BattleActions {
        party: vec![],
        enemies: complete_enemies,
        techniques,
        projectiles: vec![],
        chains: None,
    };
    actions.validate().unwrap();
    let closure = crate::battle::selection::Dependencies::actions(&actions).unwrap();
    for id in [3, 6, 7, 37] {
        assert!(closure.programs.contains(&EffectId {
            bank: EffectBank::Common,
            id
        }));
    }
    for (package, projectiles) in [(5, vec![2, 3]), (6, vec![1, 2]), (7, vec![1, 2])] {
        let bank = EffectBank::Magic(package);
        assert!(closure.programs.contains(&EffectId { bank, id: 1 }));
        for id in projectiles {
            assert!(closure.projectiles.contains(&EffectId { bank, id }));
        }
    }
    let mut missing = actions;
    missing.techniques.retain(|a| a.native_id != 206);
    assert!(
        crate::battle::selection::Dependencies::actions(&missing).is_err(),
        "missing enemy spell resources must fail before battle entry"
    );
}
