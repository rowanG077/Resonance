use super::*;
use EntryRow::{Back, Front, Middle, Random as RandomRow};

fn spawns(count: usize) -> Vec<Spawn> {
    (0..count)
        .map(|index| Spawn {
            resource: 0,
            variant: index,
            texture_variant: 0,
            weapon_variant: 0,
            unsupported_reason: None,
            position: None,
        })
        .collect()
}

#[test]
fn party_formation_positions_match_recorded_entries() {
    for (lanes, expected) in [
        (
            vec![0, 1, 2],
            vec![[-300., 0.], [-500., -150.], [-700., 150.]],
        ),
        (
            vec![0, 0, 0, 0],
            vec![[-300., 0.], [-350., -250.], [-400., 250.], [-450., -450.]],
        ),
        (
            vec![2, 0, 2, 0],
            vec![[-700., 0.], [-300., 200.], [-750., -250.], [-350., -200.]],
        ),
        (
            vec![2, 0, 0, 0],
            vec![[-700., 0.], [-300., 200.], [-350., -200.], [-400., -600.]],
        ),
    ] {
        assert_eq!(party_rows(&lanes), expected);
    }
}

#[test]
fn automatic_enemy_rows_match_recorded_entries() -> Result<()> {
    for (choices, expected) in [
        (vec![Middle], vec![[350., 0.]]),
        (vec![Middle, Middle], vec![[350., 0.], [400., -250.]]),
        (
            vec![Middle, Middle, Middle],
            vec![[350., 0.], [400., -250.], [450., 250.]],
        ),
        (
            vec![Middle, Middle, Middle, Middle],
            vec![[350., 0.], [400., -250.], [450., 250.], [500., -450.]],
        ),
        (
            vec![Middle, Middle, Middle, Front],
            vec![[350., 200.], [400., -50.], [450., -300.], [100., 0.]],
        ),
        (vec![Front, Back], vec![[100., 0.], [600., 150.]]),
        (
            vec![Middle, Front, Front, Front, Front],
            vec![
                [350., -150.],
                [100., 0.],
                [150., -250.],
                [200., 250.],
                [250., -450.],
            ],
        ),
    ] {
        assert_eq!(
            enemy_positions(&spawns(choices.len()), &choices, || panic!("fixed row"))?,
            expected
        );
    }
    let mut rolls = [2, 0].into_iter();
    assert_eq!(
        enemy_positions(&spawns(3), &[RandomRow, Middle, RandomRow], || {
            rolls.next().unwrap()
        })?,
        [[600., 150.], [350., -150.], [100., 0.]]
    );
    assert!(rolls.next().is_none());
    Ok(())
}

#[test]
fn explicit_enemy_coordinates_are_preserved() -> Result<()> {
    let mut spawns = spawns(2);
    spawns[0].position = Some([-1250, 2000]);
    spawns[1].position = Some([500, -200]);
    assert_eq!(
        enemy_positions(&spawns, &[RandomRow; 2], || panic!("explicit position"))?,
        [[-1250., 2000.], [500., -200.]]
    );
    Ok(())
}

#[test]
fn malformed_enemy_placement_fails_before_random_draws() {
    for (count, choices) in [(0, 0), (2, 1)] {
        assert!(
            enemy_positions(&spawns(count), &vec![RandomRow; choices], || panic!(
                "invalid roster"
            ))
            .is_err()
        );
    }
    let mut mixed = spawns(2);
    mixed[1].position = Some([0, 0]);
    assert!(enemy_positions(&mixed, &[RandomRow; 2], || panic!("mixed placement")).is_err());
}

#[test]
fn combined_rosters_spread_without_overlapping_or_leaving_the_arena() -> Result<()> {
    for count in [9, 18, 32, 64] {
        let positions =
            enemy_positions(&spawns(count), &vec![Front; count], || panic!("fixed row"))?;
        assert_eq!(positions.len(), count);
        for (i, p) in positions.iter().enumerate() {
            assert!(p[0] > 0. && p[0].hypot(p[1]) < 850.);
            assert!(!positions[..i].contains(p));
        }
    }
    Ok(())
}
