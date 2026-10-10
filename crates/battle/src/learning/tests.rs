use super::*;
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
            character: 1,
            level: 10,
            balance,
            story_unlocked: true,
            current: current.iter().copied().collect(),
            counts: counts.iter().copied().collect(),
        })
        .unwrap()
}

fn attempt(current: Option<u16>) -> LearningAttempt {
    LearningAttempt {
        mode: LearningMode::Martial,
        current,
        airborne: false,
    }
}

#[test]
fn learns_bases_and_successors_from_eligible_usage() -> Result<()> {
    let mut data = catalogue();
    data.definitions[2].learning.requires_story_unlock = true;
    let mut learner = member(data, &[], 0, &[(1, 49)]);
    assert_eq!(
        learner.select_after_count(attempt(None), |_| true, || 0)?,
        Some(1)
    );
    learner.mark_acquired(1)?;
    assert_eq!(
        learner.select_after_count(attempt(Some(1)), |_| true, || 0)?,
        None
    );
    assert_eq!(learner.record_use(1), Some(()));
    learner.entry.story_unlocked = false;
    assert_eq!(
        learner.select_after_count(attempt(Some(1)), |_| true, || 0)?,
        None
    );
    learner.entry.story_unlocked = true;
    assert_eq!(
        learner.select_after_count(attempt(Some(1)), |_| true, || 0)?,
        Some(2)
    );
    assert_eq!(
        member(catalogue(), &[1], 10, &[(1, 50)]).select_after_count(
            attempt(Some(1)),
            |_| true,
            || 0
        )?,
        Some(3)
    );
    learner.mark_acquired(2)?;
    assert_eq!(
        learner.select_after_count(attempt(Some(1)), |_| true, || 0)?,
        None
    );
    assert!(learner.forget(2)?);
    assert!(!learner.current().contains(&2));
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
        Some(1)
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
        Some(4)
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
        Some(1)
    );
    Ok(())
}

#[test]
fn validates_only_participating_learning_rules_and_membership() {
    let entry = || LearningEntry {
        character: 1,
        level: 1,
        balance: 0,
        story_unlocked: true,
        current: [1].into(),
        counts: BTreeMap::new(),
    };
    let mut data = catalogue();
    data.learning.push(vec![99]);
    data.definitions[4].learning.technical_successor = Some(300);
    assert!(data.validate().is_err());
    assert!(
        LearningCatalogue::new(Arc::new(data))
            .prepare_member(entry())
            .is_ok()
    );
    for invalid in 0..4 {
        let mut data = catalogue();
        match invalid {
            0 => data.learning[0].push(1),
            1 => data.definitions[1].learning.technical_successor = Some(300),
            2 => data.definitions[1]
                .learning
                .prerequisites
                .push(LearningPrerequisite {
                    any_of: vec![300],
                    minimum_uses: 0,
                }),
            _ => data.definitions[1].learning.excludes.push(300),
        }
        assert!(
            LearningCatalogue::new(Arc::new(data))
                .prepare_member(entry())
                .is_err()
        );
    }
    let validated = LearningCatalogue::new(Arc::new(catalogue()));
    for counts in [[(0, 0)], [(1, 1000)], [(99, 1)]] {
        assert!(
            validated
                .prepare_member(LearningEntry {
                    counts: counts.into(),
                    ..entry()
                })
                .is_err()
        );
    }
    let mut learner = member(catalogue(), &[1], 0, &[]);
    assert!(learner.mark_acquired(99).is_err());
    assert!(
        learner
            .select_after_count(attempt(Some(99)), |_| true, || 0)
            .is_err()
    );
}

#[test]
fn invalid_prepared_technique_range_is_rejected_before_activation() {
    for range in [[0., 0.], [-1., 120.], [0., f32::INFINITY]] {
        let mut prepared = crate::tests::prepared(vec![crate::tests::actor(crate::Side::Party)], 1);
        prepared.resources.actor_setup[0].techniques = vec![crate::PreparedTechnique {
            player_range: range,
            ..crate::tests::technique(crate::ActionKey(0), 1)
        }];
        assert!(
            prepared
                .finish()
                .err()
                .expect("invalid technique range was accepted")
                .to_string()
                .contains("invalid prepared technique range")
        );
    }
}
