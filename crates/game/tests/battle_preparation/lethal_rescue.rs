use super::prepare_party_fixture;
use super::*;
use resonance_content::{menu_data::MenuData, prepared::Cache, session::SessionData};
use resonance_events::party::Party;

#[test]
#[ignore = "requires current party and equipment publications; CPU only"]
fn equipped_rescue_traits_arm_angel_tear() -> Result<()> {
    let root = common::asset_root();
    let mut cache = Cache::default();
    let files = Files::load(&root, &["fields/map-332.preload.json"], &mut cache, || {
        false
    })?;
    let files = battle::model::load_files(
        &root,
        files,
        &[battle::model::ModelSource::Party(2)],
        &mut cache,
        || false,
    )?;
    let menus: MenuData = files.json("game/menu-data.json")?;
    let mut data: SessionData = files.json("game/session-data.json")?;
    data.rules = Some(Arc::new(menus.clone()));
    let mut party = Party::new(&data, Default::default())?;
    party.formation = vec![2];
    let member = &mut party.members[1];
    member.ex_skills = [20, 21, 22, 9];
    member.compound_ex_skills.clear();
    member.recent_compound_ex_skills.clear();
    member.equipment[3] = 414;
    member.equipment[4] = 457;
    let mut actors = prepare_party_fixture(
        &files,
        &menus,
        &party,
        vec![battle::party::Setup {
            position: [0.; 3],
            heading: 0.,
            model: Some(battle::model::ModelSetup {
                resource: 2,
                initial: resonance_battle::Playback {
                    clip: 0,
                    frame: 0.,
                    rate: 0.,
                    repeat: true,
                },
                suppress_root_translation: [false; 3],
            }),
        }],
    )?;
    let actor = actors.remove(0);
    assert!(
        actor.actor.equipment.recovery.lethal.resurrect
            && actor.actor.equipment.recovery.lethal.angel_tear
            && actor.actor.equipment.recovery.boost
    );
    assert_eq!(
        actor.actor.equipment.recovery.lethal.equipment,
        [
            None,
            Some(resonance_battle::RescueEquipment::Chance),
            Some(resonance_battle::RescueEquipment::Consumable(4)),
        ]
    );
    let prepared = resonance_battle::PreparedBattle::new(
        vec![(actor.actor, Default::default())],
        Default::default(),
        1,
    )?;
    let id = prepared.actor_ids().next().unwrap();
    let active = prepared.finish()?;
    assert!(active.angel_tear_armed(id)?);
    Ok(())
}
