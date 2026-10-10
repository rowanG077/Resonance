//! Meals share admission with their prompt and publish one complete result transaction.
use super::item_tests::{PreparedFixture, native_fixture};
use super::*;
use resonance_battle::{Control, PreparedBattle, Side, conditions::Condition};
use resonance_content::menu_data::{
    CookingData, FoodPreferences, Ingredient, MealEffect, Recipe, RecipeCook, RecipeGrade,
};
use resonance_events::party::{CookingError, Poison, StatBuff};

fn meal(party: &mut Party, menus: &mut MenuData) {
    menus.cooking = CookingData {
        recipes: vec![Recipe {
            required: vec![Ingredient::Item(1)],
            cooks: vec![
                RecipeCook {
                    base_stars: 1,
                    grades: std::array::from_fn(|_| RecipeGrade {
                        effects: MealEffect::ALL.to_vec(),
                        recovery: 40,
                        extras: vec![],
                    }),
                };
                party.members.len()
            ],
        }],
        groups: vec![],
        preferences: vec![
            FoodPreferences {
                likes: vec![],
                dislikes: vec![]
            };
            party.members.len()
        ],
        bonus_skill: u8::MAX,
    };
    party.formation = vec![1, 2, 3];
    party.items = [(1, 3)].into();
    for member in &mut party.members {
        member.hp = 1;
        member.tp = 0;
        member.luck = 0;
    }
    party.members[1].ailments.poison = Poison::Both;
    party.members[2].hp = 0;
}

#[test]
fn meal_admission_and_outcomes_do_not_depend_on_random_seeds() -> Result<()> {
    let (_, _, _, mut party) = super::super::rewards::tests::reward_fixture();
    let (mut menus, _) = super::super::party::projection_tests::fixture()?;
    meal(&mut party, &mut menus);
    // The chef may be in reserve; formation position is independent of character ID.
    party.formation = vec![2, 3, 4, 5, 6, 7, 8, 1];
    assert_eq!(party.cooking_chef(&menus), Ok(1));
    for reason in [
        CookingError::UnavailableCook,
        CookingError::UnknownRecipe,
        CookingError::MissingIngredients,
        CookingError::Full,
    ] {
        let mut rejected = party.clone();
        match reason {
            CookingError::UnavailableCook => {
                rejected.formation.pop();
            }
            CookingError::UnknownRecipe => rejected.cooking.known = 0,
            CookingError::MissingIngredients => rejected.items.clear(),
            CookingError::Full => rejected.cooking.full = true,
        }
        let before = serde_json::to_value(&rejected)?;
        assert_eq!(rejected.cooking_chef(&menus), Err(reason));
        assert_eq!(
            rejected
                .cook(&menus, || panic!("rejected meal drew randomness"))
                .unwrap_err(),
            reason
        );
        assert_eq!(serde_json::to_value(&rejected)?, before);
    }
    party.members[0].hp = 0;
    assert_eq!(
        party.cooking_chef(&menus),
        Err(CookingError::UnavailableCook)
    );
    party.members[0].hp = 1;
    party.members[0].ailments.petrified = true;
    assert_eq!(
        party.cooking_chef(&menus),
        Err(CookingError::UnavailableCook)
    );
    party.members[0].ailments.petrified = false;
    party.members[1].ailments.petrified = true;
    for (rolls, success, has_effects) in [
        ([0, 0], true, true),
        ([99, 50], false, true),
        ([99, 0], false, false),
    ] {
        let mut attempt = party.clone();
        let mut rolls = rolls.into_iter();
        let dish = attempt
            .cook(&menus, || rolls.next().unwrap_or(0))
            .map_err(|error| anyhow::anyhow!("{error:?}"))?;
        assert_eq!(dish.success, success);
        assert_eq!(dish.effects.is_empty(), !has_effects);
        assert_eq!(attempt.items[&1], 2);
        assert_eq!(attempt.members[0].cooking[0], if success { 1 } else { 2 });
        assert_eq!(attempt.members[0].hp > 1, has_effects);
        assert_eq!(attempt.members[1].ailments.petrified, !has_effects);
        assert_eq!(
            attempt.members[1].ailments.poison == Poison::None,
            has_effects
        );
        assert_eq!(attempt.members[2].hp > 0, has_effects);
        assert_eq!(attempt.members[1].queued_buffs.is_empty(), !has_effects);
        assert_eq!(attempt.cooking_chef(&menus), Err(CookingError::Full));
        assert_eq!(
            attempt
                .cook(&menus, || panic!("second meal drew randomness"))
                .unwrap_err(),
            CookingError::Full
        );
        assert_eq!(attempt.items[&1], 2);
    }
    Ok(())
}

#[test]
fn result_meal_publishes_once_after_all_recipients_are_ready() -> Result<()> {
    let PreparedFixture {
        mut candidate,
        mut battle,
        field,
        ..
    } = native_fixture(
        &[1, 2, 3],
        |party, _, menus| {
            meal(party, menus);
            // A practiced, lucky chef guarantees success without fixing a random sequence.
            party.members[0].cooking[0] = 6;
            party.members[0].luck = 200;
            party.members[0].base_stats[1] = 250;
            Ok(())
        },
        |mut actors, _| {
            for actor in &mut actors {
                if actor.side == Side::Enemy {
                    actor.hp = 0;
                    actor.availability = ActorAvailability::Dead;
                }
            }
            // Combat-only conditions must not become persistent meal effects.
            actors[0].conditions = resonance_battle::conditions::Conditions::new(
                resonance_battle::conditions::Layers {
                    base: Condition::Flare.into(),
                    intrinsic: Condition::Guard.into(),
                    ..Default::default()
                },
            );
            PreparedBattle::new(
                (actors)
                    .into_iter()
                    .map(|actor| (actor, Default::default()))
                    .collect(),
                Default::default(),
                1,
            )?
            .finish()
        },
    )?;
    assert_eq!(battle.recognize_result(), Some(BattleResult::Victory));
    battle.retire_combat()?;
    let (chef, character) = candidate.setup.actors[0];
    candidate.selection = Some(Selection {
        actor: chef,
        character,
        pose: None,
        group: None,
    });
    candidate.construct_rewards(&mut battle)?;
    assert_eq!(candidate.party.members[0].tp, 20);
    assert_eq!(battle.actors()[chef.index()].tp, 20);
    assert_eq!(
        candidate.results.as_ref().unwrap().cook_prompt,
        Some(character)
    );
    let notices = candidate.results.as_ref().unwrap().notices.clone();
    let field_before = serde_json::to_value(&field)?;

    // A bad later recipient rejects every write, including spending and random advancement.
    let maximum = candidate.party.members[2].base_stats[0];
    candidate.party.members[2].base_stats[0] = 0;
    let before = serde_json::to_value(&candidate.party)?;
    let random = candidate.gameplay_random;
    let frame = battle.snapshot();
    assert!(
        candidate
            .cook(&mut battle)
            .unwrap_err()
            .to_string()
            .contains("invalid cooking reload vitals")
    );
    assert_eq!(serde_json::to_value(&candidate.party)?, before);
    assert_eq!(candidate.gameplay_random, random);
    assert_eq!(battle.snapshot(), frame);
    assert_eq!(candidate.results.as_ref().unwrap().notices, notices);
    candidate.party.members[2].base_stats[0] = maximum;

    candidate.update_cooking(&mut battle)?;
    assert!(candidate.party.cooking.full);
    assert_eq!(candidate.party.items[&1], 2);
    assert_eq!(candidate.party.members[0].overlimit, 5);
    assert!(candidate.party.members[0].tp > 20);
    assert_eq!(battle.actors()[chef.index()].overlimit.saved_percent(), 5);
    let results = candidate.results.as_ref().unwrap();
    assert_eq!(&results.notices[..notices.len()], notices);
    assert_eq!(
        results.notices.last(),
        Some(&ResultNotice::Cooking {
            character,
            recipe: 0,
            success: true
        })
    );
    assert_eq!(results.cook_prompt, None);
    for &(id, character) in &candidate.setup.actors {
        let actor = &battle.actors()[id.index()];
        let member = &candidate.party.members[usize::from(character - 1)];
        assert_eq!(actor.hp, i32::from(member.hp));
        assert_eq!(actor.tp, member.tp);
        assert!(actor.hp > 0);
        assert_eq!(actor.availability, ActorAvailability::Active);
        assert_eq!(member.ailments, Default::default());
        assert!(member.queued_buffs.contains(&StatBuff::AttackUp));
    }
    assert!(
        !battle.actors()[chef.index()]
            .conditions
            .effective()
            .intersects(ConditionSet::of(&[Condition::Flare, Condition::Guard]))
    );
    let party = serde_json::to_value(&candidate.party)?;
    let random = candidate.gameplay_random;
    let frame = battle.snapshot();
    let notices = candidate.results.as_ref().unwrap().notices.clone();
    candidate.update_cooking(&mut battle)?;
    assert_eq!(serde_json::to_value(&candidate.party)?, party);
    assert_eq!(candidate.gameplay_random, random);
    assert_eq!(battle.snapshot(), frame);
    assert_eq!(candidate.results.as_ref().unwrap().notices, notices);
    assert_eq!(serde_json::to_value(&field)?, field_before);

    let menus = candidate.menus.clone();
    candidate.accept_victory()?;
    let outcome = battle.finish_result()?.outcome.context("missing result")?;
    let completed = candidate.finish(&battle, &outcome)?;
    assert_eq!(completed.gameplay_random, random);
    assert_eq!(completed.party.items[&1], 2);
    assert!(completed.party.cooking.full);
    assert_eq!(completed.party.members[0].cooking[0], 7);
    let member = &completed.party.members[1];
    let loadout = super::super::party::loadout(&menus, member, 1)?;
    let next = super::super::party::actor(&loadout, member, Control::Manual, [0.; 3], 0.)?;
    assert!(next.hp > 0 && !next.is_petrified());
    assert!(next.conditions.base().contains(Condition::AttackUp));
    assert_eq!(member.ailments.poison, Poison::None);
    Ok(())
}
