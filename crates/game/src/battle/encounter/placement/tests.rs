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
fn automatic_enemy_rows_stagger_members_in_formation_order() -> Result<()> {
    let choices = [Back, Front, Back, Middle];
    let enemies = enemy_positions(&spawns(4), &choices, || panic!("fixed row"))?;
    assert!(enemies[0][0] > enemies[3][0] && enemies[3][0] > enemies[1][0]);
    assert!(enemies[0][0] < enemies[2][0]);
    assert!(enemies[0][1] < 0. && enemies[2][1] == -enemies[0][1]);
    assert_eq!(enemies[1][1], 0.);
    assert_eq!(enemies[3][1], 0.);
    let crowded = enemy_positions(&spawns(8), &[Back; 8], || panic!("fixed row"))?;
    const ARENA_RADIUS: f32 = 850.;
    const PUMPKIN_TREE_DIAMETER: f32 = 130.;
    assert!(crowded.iter().all(|[x, z]| x.hypot(*z) < ARENA_RADIUS));
    assert!(
        crowded
            .windows(2)
            .all(|pair| pair[1][1] - pair[0][1] > PUMPKIN_TREE_DIAMETER)
    );
    // Random row selection changes depth, while every isolated member stays centered.
    let mut rolls = [2, 0].into_iter();
    let random = enemy_positions(&spawns(3), &[RandomRow, Middle, RandomRow], || {
        rolls.next().unwrap()
    })?;
    assert!(random[0][0] > random[1][0] && random[1][0] > random[2][0]);
    assert!(random.iter().all(|position| position[1] == 0.));
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
    for (count, choices) in [(0, 0), (2, 1), (9, 9)] {
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
