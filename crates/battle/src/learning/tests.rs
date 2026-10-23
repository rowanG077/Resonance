use super::*;
use crate::TechniqueId;
use resonance_content::arte::{Definition, LearningPrerequisite};

fn catalogue() -> Catalogue {
    let mut data = Catalogue {
        definitions: vec![Definition::default(); 5],
        learning: vec![vec![]],
    };
    data.learning[0] = vec![1, 2, 3];
    for row in &mut data.definitions[1..=3] {
        row.required_level = 1;
        row.action_range = 400.;
    }
    data.definitions[1].learning.technical_successor = Some(2);
    data.definitions[1].learning.strike_successor = Some(3);
    for row in &mut data.definitions[2..=3] {
        row.learning.parent = Some(1);
        row.learning.parent_uses = 50;
    }
    data
}

fn member(data: Catalogue, current: &[u16], balance: i8, counts: &[(u16, u16)]) -> LearningMember {
    LearningCatalogue::new(Arc::new(data))
        .prepare_member(LearningEntry {
            character: crate::Character::Lloyd,
            level: 10,
            balance,
            story_unlocked: true,
            current: current.iter().copied().map(TechniqueId).collect(),
            counts: counts
                .iter()
                .map(|&(id, count)| (TechniqueId(id), count))
                .collect(),
        })
        .unwrap()
}

fn attempt(current: Option<u16>) -> LearningAttempt {
    LearningAttempt {
        mode: LearningMode::Martial,
        current: current.map(TechniqueId),
        airborne: false,
    }
}

#[test]
fn both_paths_learn_the_missing_successor_on_the_next_eligible_use() -> Result<()> {
    let mut data = catalogue();
    data.definitions[2].learning.route = Some(LearningRoute::Technical);
    let other = &mut data.definitions[3];
    other.required_level = 11;
    other.learning.route = Some(LearningRoute::Strike);
    other.learning.requires_story_unlock = true;
    other.learning.excludes = vec![2];
    other.learning.prerequisites = vec![LearningPrerequisite {
        any_of: vec![2],
        minimum_uses: 25,
    }];
    for balance in [-40, 40] {
        let mut learner = member(data.clone(), &[1, 2], balance, &[(1, 50), (2, 25)]);
        learner.catalogue = learner.catalogue.with_both_arte_paths(true);
        let select = |learner: &LearningMember| {
            learner.select_after_count(
                attempt(Some(1)),
                |_| true,
                || panic!("learning both paths must not wait for a random roll"),
            )
        };
        assert_eq!(learner.current(), &[TechniqueId(1), TechniqueId(2)].into());
        assert_eq!(select(&learner)?, None);
        learner.entry.level = 11;
        learner.entry.story_unlocked = false;
        assert_eq!(select(&learner)?, None);
        learner.entry.story_unlocked = true;
        learner.entry.counts.insert(TechniqueId(1), 49);
        assert_eq!(select(&learner)?, None);
        learner.record_use(TechniqueId(1));
        learner.entry.counts.insert(TechniqueId(2), 24);
        assert_eq!(select(&learner)?, None);
        learner.record_use(TechniqueId(2));
        assert_eq!(
            learner.select_after_count(attempt(Some(1)), |_| false, || 0)?,
            None
        );
        assert_eq!(
            learner.select_after_count(
                LearningAttempt {
                    airborne: true,
                    ..attempt(Some(1))
                },
                |_| true,
                || 0
            )?,
            None
        );
        assert_eq!(select(&learner)?, Some(TechniqueId(3)));
        assert_eq!(learner.count(TechniqueId(1)), 50);
        assert_eq!(learner.count(TechniqueId(2)), 25);
        learner.mark_acquired(TechniqueId(3))?;
        assert_eq!(select(&learner)?, None);
    }
    Ok(())
}

#[test]
fn both_paths_only_relax_opposing_path_exclusions() -> Result<()> {
    for excluded_route in [
        None,
        Some(LearningRoute::Strike),
        Some(LearningRoute::Technical),
    ] {
        let mut data = catalogue();
        data.definitions[1].learning.route = Some(LearningRoute::Strike);
        data.definitions[1].learning.excludes = vec![2];
        data.definitions[1].learning.prerequisites = vec![LearningPrerequisite {
            any_of: vec![2],
            minimum_uses: 50,
        }];
        data.definitions[2].learning.route = excluded_route;
        let mut learner = member(data, &[2], -40, &[(2, 49)]);
        assert_eq!(
            learner.select_after_count(attempt(None), |_| true, || 0)?,
            None
        );
        learner.catalogue = learner.catalogue.with_both_arte_paths(true);
        assert_eq!(
            learner.select_after_count(attempt(None), |_| true, || 0)?,
            None
        );
        learner.record_use(TechniqueId(2));
        assert_eq!(
            learner.select_after_count(attempt(None), |_| true, || panic!("random roll"))?,
            (excluded_route == Some(LearningRoute::Technical)).then_some(TechniqueId(1))
        );
    }
    Ok(())
}

#[test]
fn elemental_variant_learns_on_its_fiftieth_use_only_when_enabled() -> Result<()> {
    use resonance_content::arte::elemental::{LEARNING_USES, LIGHTNING_TIGER};
    let mut data = Catalogue {
        definitions: vec![Definition::default(); usize::from(LIGHTNING_TIGER) + 1],
        learning: vec![vec![6, LIGHTNING_TIGER]],
    };
    data.definitions[6].required_level = 1;
    let data = Arc::new(data);
    for enabled in [false, true] {
        let mut learner = LearningCatalogue::new(data.clone())
            .with_elemental_learning(enabled)
            .prepare_member(LearningEntry {
                character: crate::Character::Lloyd,
                level: 1,
                balance: 0,
                story_unlocked: false,
                current: [TechniqueId(6)].into(),
                counts: [
                    (TechniqueId(6), 999),
                    (TechniqueId(LIGHTNING_TIGER), LEARNING_USES - 1),
                ]
                .into(),
            })?;
        let select = |learner: &LearningMember| {
            learner.select_after_count(
                attempt(Some(LIGHTNING_TIGER)),
                |_| true,
                || panic!("elemental learning must not depend on chance"),
            )
        };
        assert_eq!(select(&learner)?, None);
        learner.record_use(TechniqueId(LIGHTNING_TIGER));
        assert_eq!(
            select(&learner)?,
            enabled.then_some(TechniqueId(LIGHTNING_TIGER))
        );
        if enabled {
            learner.mark_acquired(TechniqueId(LIGHTNING_TIGER))?;
            assert_eq!(select(&learner)?, None);
        }
    }
    Ok(())
}

#[test]
fn learns_bases_and_successors_from_eligible_usage() -> Result<()> {
    let mut data = catalogue();
    data.definitions[2].learning.requires_story_unlock = true;
    let mut learner = member(data, &[], 0, &[(1, 49)]);
    assert_eq!(
        learner.select_after_count(attempt(None), |_| true, || 0)?,
        Some(TechniqueId(1))
    );
    learner.mark_acquired(TechniqueId(1))?;
    assert_eq!(
        learner.select_after_count(attempt(Some(1)), |_| true, || 0)?,
        None
    );
    assert_eq!(learner.record_use(TechniqueId(1)), Some(()));
    learner.entry.story_unlocked = false;
    assert_eq!(
        learner.select_after_count(attempt(Some(1)), |_| true, || 0)?,
        None
    );
    learner.entry.story_unlocked = true;
    assert_eq!(
        learner.select_after_count(attempt(Some(1)), |_| true, || 0)?,
        Some(TechniqueId(2))
    );
    assert_eq!(
        member(catalogue(), &[1], 10, &[(1, 50)]).select_after_count(
            attempt(Some(1)),
            |_| true,
            || 0
        )?,
        Some(TechniqueId(3))
    );
    learner.mark_acquired(TechniqueId(2))?;
    assert_eq!(
        learner.select_after_count(attempt(Some(1)), |_| true, || 0)?,
        None
    );
    assert!(learner.forget(TechniqueId(2))?);
    assert!(!learner.current().contains(&TechniqueId(2)));
    Ok(())
}

#[test]
fn learning_requires_eligible_actions() -> Result<()> {
    for (level, airborne, mode) in [
        (20, false, LearningMode::Martial),
        (1, true, LearningMode::Martial),
        (1, false, LearningMode::Casting),
    ] {
        let mut data = catalogue();
        data.definitions[1].required_level = level;
        let learner = member(data, &[], 0, &[]);
        assert_eq!(
            learner.select_after_count(
                LearningAttempt {
                    mode,
                    airborne,
                    current: None
                },
                |_| true,
                || 0
            )?,
            None
        );
    }
    let learner = member(catalogue(), &[], 0, &[]);
    assert_eq!(
        learner.select_after_count(attempt(None), |_| true, || 1)?,
        None
    );
    for current in [None, Some(1)] {
        let learner = member(
            catalogue(),
            &current.into_iter().collect::<Vec<_>>(),
            0,
            &[(1, 50)],
        );
        assert_eq!(
            learner.select_after_count(attempt(current), |_| false, || 0)?,
            None
        );
    }
    Ok(())
}

#[test]
fn excludes_opposite_successor_and_missing_prerequisites() -> Result<()> {
    assert_eq!(
        member(catalogue(), &[1, 3], 0, &[(1, 50)]).select_after_count(
            attempt(Some(1)),
            |_| true,
            || 0
        )?,
        None
    );
    let mut data = catalogue();
    data.definitions[1].learning.prerequisites = vec![LearningPrerequisite {
        any_of: vec![2],
        minimum_uses: 50,
    }];
    assert_eq!(
        member(data.clone(), &[], 0, &[(2, 50)]).select_after_count(
            attempt(None),
            |_| true,
            || 0
        )?,
        None
    );
    assert_eq!(
        member(data.clone(), &[2], 0, &[(2, 49)]).select_after_count(
            attempt(None),
            |_| true,
            || 0
        )?,
        None
    );
    assert_eq!(
        member(data, &[2], 0, &[(2, 50)]).select_after_count(attempt(None), |_| true, || 0)?,
        Some(TechniqueId(1))
    );
    Ok(())
}

#[test]
fn combination_accepts_either_qualified_successor() -> Result<()> {
    let mut data = catalogue();
    data.learning[0].push(4);
    data.definitions[4] = Definition {
        required_level: 1,
        learning: LearningRules {
            prerequisites: vec![LearningPrerequisite {
                any_of: vec![2, 3],
                minimum_uses: 50,
            }],
            ..Default::default()
        },
        ..Default::default()
    };
    // Both routes are known; either sufficiently used successor satisfies the combination.
    assert_eq!(
        member(data, &[1, 2, 3], 0, &[(3, 50)]).select_after_count(
            attempt(None),
            |_| true,
            || 0
        )?,
        Some(TechniqueId(4))
    );
    Ok(())
}

#[test]
fn airborne_techniques_require_airborne_use() -> Result<()> {
    let mut data = catalogue();
    data.definitions[1].capabilities.regal_family = Some(crate::RegalArteFamily::Aerial);
    let learner = member(data, &[], 0, &[]);
    assert_eq!(
        learner.select_after_count(attempt(None), |_| true, || 0)?,
        None
    );
    assert_eq!(
        learner.select_after_count(
            LearningAttempt {
                airborne: true,
                ..attempt(None)
            },
            |_| true,
            || 0
        )?,
        Some(TechniqueId(1))
    );
    Ok(())
}
