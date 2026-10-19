//! Preparation resolves source bank identities and rejects invalid gameplay data.
use super::*;
use resonance_content::battle_projectile::Table;

#[test]
fn preparation_binds_gameplay_and_distinct_effect_banks() -> Result<()> {
    let files = files();
    let mut table: Table = files.json("test-projectiles.json")?;
    let row = &mut table.records[0];
    row.birth_effect.bank = 2;
    row.contact.clashes = true;
    row.contact.recoil_direction = resonance_battle::RecoilDirection::Travel;
    let mut hit = fixture_hit();
    hit.reaction.direction = resonance_battle::RecoilDirection::TowardContact;
    hit.reaction.hitstun = 40;
    hit.reaction.recoil.delay = 2;
    hit.reaction.stagger = 5;
    hit.reaction.hits_down = true;
    let local = fixture_effect(&files, vec![])?;
    let common = EffectResource {
        resource: 9,
        ..local.clone()
    };
    let mut resources = ActionResources::default();
    let definition = resources.projectile(&files, row, hit, &[(0, &common), (2, &local)])?;
    assert_eq!(definition.birth.unwrap().resource, local.resource);
    let contact = definition.contact.as_ref().unwrap();
    assert!(contact.clashes);
    assert_eq!(definition.effects.clash.unwrap().resource, common.resource);
    assert_eq!(resources.effects[&common.resource].members, [11]);
    assert_eq!(resources.effects[&local.resource].members, [28]);
    let reaction = contact.hit.reaction;
    let mut expected_reaction = hit.reaction;
    expected_reaction.direction = resonance_battle::RecoilDirection::Travel;
    assert_eq!(reaction, expected_reaction);

    let error = ActionResources::default()
        .projectile(&files, row, hit, &[])
        .err()
        .context("projectile accepted missing required artwork")?;
    assert!(
        format!("{error:#}").contains("effect 28 has no prepared bank 2"),
        "{error:#}"
    );
    Ok(())
}

#[test]
fn preparation_rejects_unsupported_gameplay() -> Result<()> {
    let files = files();
    let table: Table = files.json("test-projectiles.json")?;
    let hit = fixture_hit();
    let bank = fixture_effect(&files, vec![])?;
    for edit in [
        |row: &mut resonance_content::battle_projectile::Projectile| {
            row.unsupported_reason = Some("unsupported projectile".into());
        },
        |row: &mut resonance_content::battle_projectile::Projectile| {
            row.motion.velocity_jitter[0] = f32::NAN
        },
    ] {
        let mut row = table.records[0].clone();
        edit(&mut row);
        assert!(
            ActionResources::default()
                .projectile(&files, &row, hit, &[(1, &bank)])
                .is_err()
        );
    }
    let mut hit = hit;
    hit.reaction.recoil.impulse[0] = f32::NAN;
    assert!(
        ActionResources::default()
            .projectile(&files, &table.records[0], hit, &[(1, &bank)])
            .is_err()
    );
    Ok(())
}
