use super::*;

#[test]
fn inclusive_stat_ranges_preserve_fixed_point_steps_and_limits() -> Result<()> {
    for stat in Statistic::ALL {
        for base in [0, 1, 38, 255] {
            for random in [0, 1, 12, 255] {
                let gain = Gain::from_parts(stat, base, random);
                let prepared = gain.prepare(stat, 3)?;
                assert_eq!(
                    (prepared.base, prepared.random, prepared.title_bonus),
                    (base, random, 3)
                );
            }
        }
        assert!(Gain::Range([2., 1.]).prepare(stat, 0).is_err());
        assert!(Gain::Fixed(-1.).prepare(stat, 0).is_err());
        assert!(Gain::Fixed(f64::NAN).prepare(stat, 0).is_err());
    }
    assert!(Gain::Fixed(1.5).prepare(Statistic::Hp, 0).is_err());
    assert!(Gain::Fixed(0.15).prepare(Statistic::Strength, 0).is_err());
    assert!(
        Gain::Range([0., 25.6])
            .prepare(Statistic::Strength, 0)
            .is_err()
    );
    Ok(())
}

#[test]
fn named_initial_state_preserves_nondefault_values_and_sparse_slots() -> Result<()> {
    let initial: Initial = serde_json::from_value(serde_json::json!({
        "stats":{"hp":250,"tp":30,"strength":44.1,"accuracy":70.5},
        "equipment":{"weapon":"blade","accessory2":"ring"},
        "level":3,"luck":61,"experience":300,"affinity":42,"overlimit":17,"technique_balance":-9,
        "techniques":["demon-fang","sonic-thrust"],"shortcuts":{"side":"sonic-thrust","up":"demon-fang"},
        "cooking":{"sandwich":2,"paella":8},"ex_gems":{"1":{"level":1,"skill":"strong"},"4":{"level":3}},
        "compound_ex_skills":["l-unison","over-limit"],"recent_compound_ex_skills":["over-limit"]
    }))?;
    let items =
        Mappings::new([(0, "empty".into()), (1, "blade".into()), (2, "ring".into())].into())?;
    let techniques = Mappings::new([(1, "demon-fang".into()), (4, "sonic-thrust".into())].into())?;
    let recipes = Mappings::new([(0, "sandwich".into()), (23, "paella".into())].into())?;
    let recipe_ids = recipes.iter().map(|(id, name)| (name, id)).collect();
    let character = crate::overworld::PartyCharacter::Lloyd;
    let ex: crate::ex_skills::Document = serde_json::from_value(serde_json::json!({
        "mappings":{"1":"strong"},"skills":{},"characters":{"lloyd":{"levels":{},"compounds":{"2":"l-unison","5":"over-limit"}}},
        "gem_items":{"1":"gem1","2":"gem2","3":"gem3","4":"gem4","max":"max"}
    }))?;
    let item_ids = items.iter().map(|(id, name)| (name, id)).collect();
    let tech_ids = techniques.iter().map(|(id, name)| (name, id)).collect();
    let result = initial.prepare(&item_ids, &tech_ids, &recipe_ids, &ex, character)?;
    assert_eq!(result.base_stats, [250, 30, 441, 0, 0, 0, 705]);
    assert_eq!(result.equipment, [1, 0, 0, 0, 2, 0]);
    assert_eq!(result.shortcuts, [0, 1, 0, 4]);
    assert_eq!(result.ex_gems, [1, 0, 0, 3]);
    assert_eq!(result.ex_skills, [1, 0, 0, 0]);
    assert_eq!((result.cooking[0], result.cooking[23]), (2, 8));
    let rebuilt = Initial::from_parts(&result, &items, &techniques, &recipes, &ex, character)?;
    assert_eq!(
        serde_json::to_value(&rebuilt)?,
        serde_json::to_value(&initial)?
    );
    let mut bad = initial.clone();
    bad.equipment.insert(Slot::Weapon, "missing".into());
    assert!(
        bad.prepare(&item_ids, &tech_ids, &recipe_ids, &ex, character)
            .is_err()
    );
    let mut bad = initial.clone();
    bad.ex_gems.insert(
        0,
        Gem {
            level: 1,
            skill: None,
        },
    );
    assert!(
        bad.prepare(&item_ids, &tech_ids, &recipe_ids, &ex, character)
            .is_err()
    );
    let mut bad = initial;
    bad.cooking.insert("missing-recipe".into(), 1);
    assert!(
        bad.prepare(&item_ids, &tech_ids, &recipe_ids, &ex, character)
            .is_err()
    );
    Ok(())
}

#[test]
fn level_numbers_are_explicit_and_optional_full_names_are_isolated() -> Result<()> {
    let mut document = Document {
        mappings: Mappings::new((1..=10).map(|id| (id, format!("character-{id}"))).collect())?,
        characters: (1..=10)
            .map(|id| {
                (
                    format!("character-{id}"),
                    Entry {
                        name: format!("Name{id}"),
                        full_name: Value::Null,
                        initial: None,
                        growth: None,
                    },
                )
            })
            .collect(),
        experience: [(1, 0), (2, 107), (3, 291)].into(),
        growth_profiles: BTreeMap::new(),
    };
    assert_eq!(document.experience_curve()?, vec![0, 0, 107, 291]);
    assert_eq!(document.names()?[&10], "Name10");
    assert_eq!(
        document.full_names(&Diagnostics::new(true))?.unwrap(),
        vec!["{name}"; 9]
    );
    document
        .characters
        .get_mut("character-1")
        .unwrap()
        .full_name = false.into();
    assert!(document.full_names(&Diagnostics::new(false))?.is_none());
    assert!(document.full_names(&Diagnostics::new(true)).is_err());
    assert_eq!(document.initial_names()?[0], "Name1");
    document.experience.remove(&2);
    assert!(document.experience_curve().is_err());
    Ok(())
}
