use super::*;

#[test]
fn movement_null_declarations_use_explicit_cab_words_only() {
    let mut bytes = vec![0; 0x70];
    bytes[0x68..0x6c].copy_from_slice(&0x300_u32.to_be_bytes());
    bytes[0x24..0x28].copy_from_slice(&0x100_u32.to_be_bytes());
    assert_eq!(
        absent_movement_motions(&bytes).unwrap(),
        [EnemyMovementMotion::Run].into()
    );
    bytes[0x6c..0x70].copy_from_slice(&0x200_u32.to_be_bytes());
    assert!(absent_movement_motions(&bytes).unwrap().is_empty());
    bytes[0x24..0x28].fill(0);
    assert_eq!(
        absent_movement_motions(&bytes).unwrap(),
        [EnemyMovementMotion::Walk].into()
    );
    assert!(absent_movement_motions(&bytes[..0x6f]).is_err());
}

#[test]
#[ignore = "requires the original extracted disc; no asset encoding"]
fn original_lightning_rosters_preserve_their_null_movement_slots() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
    let archive = fs::read(extracted.join("files/BTL/BTLenemy.dat")).unwrap();
    let table = word(&usual, 0x2c).unwrap() as usize;
    for monster in [51, 72, 172] {
        let start = word(&usual, table + usize::from(monster) * 4).unwrap() as usize;
        let end = word(&usual, table + (usize::from(monster) + 1) * 4).unwrap() as usize;
        let bytes = compression::decode(&archive[start..end]).unwrap();
        let enemy = enemy_actions(&bytes, monster).unwrap();
        assert_eq!(
            enemy.absent_movement_motions,
            if monster == 51 {
                [EnemyMovementMotion::Run, EnemyMovementMotion::Stop].into()
            } else {
                [
                    EnemyMovementMotion::Walk,
                    EnemyMovementMotion::Run,
                    EnemyMovementMotion::Stop,
                ]
                .into()
            }
        );
        if monster == 172 {
            assert!(matches!(
                enemy.actions[4].approach,
                Some(EnemyApproach::Custom {
                    clip: 1,
                    rate: 2.,
                    speed: 7.
                })
            ));
        }
    }
}
