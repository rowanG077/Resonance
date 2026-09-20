use super::*;

#[test]
#[ignore = "requires privately extracted US assets; source recovery only"]
fn original_icicle_preserves_both_hits_every_alias_and_all_enemy_cast_bindings() {
    use resonance_content::battle::action_program::CommandDependency;
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let mut rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
    let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
    let executable = fs::read(extracted.join("sys/main.dol")).unwrap();
    let catalogue = crate::arte::read(&executable).unwrap();
    let techniques = technique_actions(&extracted, &rel, &usual, &[82]).unwrap();
    let TechniqueProgram::Icicle { casters, recipe } = &techniques[0].program else {
        panic!("lost Icicle identity")
    };
    assert_eq!(techniques[0].enemy_spell(), Some(EnemySpell::Icicle));
    assert!(!EnemySpell::Icicle.stored());
    assert_eq!(casters.len(), 1);
    assert_eq!(
        (
            casters[0].character,
            casters[0].tp,
            casters[0].time_adjustment
        ),
        (3, 10, 60)
    );
    assert_eq!(
        (
            casters[0].voices.begin,
            casters[0].voices.release,
            casters[0].voices.begin_remaining
        ),
        (33016, 33083, 57)
    );
    assert!(matches!(
        casters[0].release,
        Some(AnimationCommand::Play { clip: 12, .. })
    ));
    assert_eq!(recipe.lifetime, 90);
    assert_eq!(
        recipe.origin.capture([0.; 3], [300., 50., 400.]),
        [299.4, 0., 399.2]
    );
    assert_eq!((recipe.effect.id, recipe.effect_scale), (32, 1.));
    assert_eq!(
        (
            recipe.contact.shape.radius,
            recipe.contact.shape.height,
            recipe.contact.lifetime
        ),
        (90., 180., 10)
    );
    assert!(matches!(recipe.contact.shape.kind, HitShapeKind::Cylinder));
    assert!(matches!(
        recipe.contact.knockback,
        KnockbackDirection::Velocity
    ));
    assert!(
        recipe.contact.persist_after_hit
            && recipe.contact.spawn_effect.is_none()
            && recipe.contact.trail_effect.is_none()
            && recipe.contact.ground_effect.is_none()
    );
    assert_eq!(
        recipe.pulses.map(|p| (
            p.tick,
            p.reaction,
            p.rule.flags,
            p.rule.power,
            p.rule.contact_cooldown
        )),
        [(2, 10, 0x21, 80, 30), (26, 7, 0x20, 80, 30)]
    );
    assert_eq!(techniques[0].program.spell_rules().count(), 2);
    let inventory = crate::battle::action_inventory::recover(&extracted).unwrap();
    assert_eq!(inventory.artes.artes.len(), 253);
    assert_eq!(
        inventory
            .artes
            .artes
            .iter()
            .filter(|a| a.native_id == 220)
            .map(|a| (a.id, a.owners.clone()))
            .collect::<Vec<_>>(),
        [(82, vec![3])]
    );
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
    assert_eq!(
        inventory
            .enemies
            .packages
            .iter()
            .flat_map(|p| p
                .actions
                .iter()
                .filter(|a| a.native_technique == Some(220))
                .map(move |a| (p.monster, a.id, p.variant_count)))
            .collect::<Vec<_>>(),
        [
            (43, 3, 2),
            (46, 3, 1),
            (47, 3, 1),
            (48, 3, 2),
            (93, 1, 1),
            (106, 2, 6)
        ]
    );
    assert!(
        !inventory
            .enemies
            .packages
            .iter()
            .flat_map(|p| &p.actions)
            .filter_map(|a| a.commands.as_ref())
            .flat_map(|p| &p.commands)
            .flat_map(|c| &c.dependencies)
            .any(|d| matches!(d, CommandDependency::NativeTechnique { id: 220, .. }))
    );
    let archive = fs::read(extracted.join("files/BTL/BTLenemy.dat")).unwrap();
    let directory = word(&usual, 0x2c).unwrap() as usize;
    for (monster, action, duration, rate) in [
        (43, 3, 180, 0.5),
        (46, 3, 180, 0.5),
        (47, 3, 180, 0.5),
        (48, 3, 560, 0.5),
        (93, 1, 180, 0.5),
        (106, 2, 180, 1.),
    ] {
        let start = word(&usual, directory + monster * 4).unwrap() as usize;
        let end = word(&usual, directory + (monster + 1) * 4).unwrap() as usize;
        let bytes = compression::decode(&archive[start..end]).unwrap();
        let casting = enemy::casting(&bytes, &catalogue, &usual, &rel).unwrap();
        let cast = &casting[&action];
        assert_eq!((cast.duration, cast.tp), (duration, 10));
        assert!(cast.absent_motions.is_empty());
        assert!(
            matches!(cast.release, AnimationCommand::Play { clip:12, rate:r, .. } if r == rate)
        );
        assert!(cast.early_release.is_some());
    }
    let text = rel.sections[1].0;
    rel.bytes[text + 0x72100 + 0x30] ^= 1;
    assert!(
        validate_controller(&rel).is_err(),
        "modified callback cannot retain a stale recipe"
    );
}
