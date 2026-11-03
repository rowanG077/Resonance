use super::*;
use resonance_content::{
    diagnostics::Diagnostics, library::Library, preparation, session::SessionData,
};
use resonance_events::{
    battle::{DefeatPolicy, Encounter, Setup},
    party::Party,
};

#[test]
#[ignore = "requires current cooked costume props; CPU preparation and unchanged-stat checks"]
fn native_costume_props_change_only_selected_weapon_artwork() -> Result<()> {
    let root = std::path::PathBuf::from(
        std::env::var_os("RESONANCE_TEST_ASSETS").context("set RESONANCE_TEST_ASSETS")?,
    );
    let library = Library::open(&root, &[])?;
    let files = preparation::capture(&library, &[], || false, Diagnostics::new(true))?;
    let data = SessionData::load(&files)?;
    let menus = data.rules.as_ref().context("missing menu rules")?;
    for (character, costume, prop) in [(5, 2, 534u16), (6, 1, 532), (7, 1, 535), (7, 5, 536)] {
        let mut baseline = None;
        for clears in [0, 1] {
            let mut party = Party::new(&data, Default::default()).map_err(anyhow::Error::msg)?;
            party.formation = vec![character];
            party.field_leader = character;
            party.game_clears = clears;
            party.members[usize::from(character - 1)].costume = costume;
            let original = serde_json::to_value(&party)?;
            let inputs = Assets::capture(
                &mut library.snapshot(),
                menus,
                &data,
                &party,
                Setup {
                    route: [0; 5],
                    encounter: Encounter::Formation(1),
                    arena: 13,
                    music: None,
                    defeat: DefeatPolicy::ResumeEvent,
                },
                Diagnostics::new(true),
                &|| false,
            )?;
            let input = &inputs.party_actors[0];
            let profile = &inputs.party_profiles.records[input.character.index()];
            let member = &party.members[input.character.index()];
            let loadout = battle::party::loadout(menus, member, input.character)?;
            let roll = |seed: u64| -> Result<bool> {
                let mut random = resonance_battle::Random::new(!seed);
                entry::initial_playback(
                    &mut random,
                    &inputs.party_profiles.entry,
                    profile,
                    resonance_battle::Side::Party,
                    &input.motions,
                    (i32::from(member.hp), loadout.attributes.max_hp),
                    member.ailments.petrified,
                )?;
                Ok(random.next_u32().is_multiple_of(16))
            };
            let (mut winning, mut losing) = (None, None);
            for seed in 0..1000 {
                if roll(seed)? {
                    winning.get_or_insert(seed);
                } else {
                    losing.get_or_insert(seed);
                }
                if winning.is_some() && losing.is_some() {
                    break;
                }
            }
            let winning = winning.context("no winning cosmetic seed")?;
            let losing = losing.context("no losing cosmetic seed")?;
            for (seed, selected) in if clears == 0 {
                vec![(winning, false)]
            } else {
                vec![(losing, false), (winning, true)]
            } {
                // Exercise the ordinary body/equipment preparation and the
                // same cosmetic selection step without preparing unrelated
                // spell/effect inventories for every action this actor owns.
                let mut random = resonance_battle::Random::new(!seed);
                let initial = entry::initial_playback(
                    &mut random,
                    &inputs.party_profiles.entry,
                    profile,
                    resonance_battle::Side::Party,
                    &input.motions,
                    (i32::from(member.hp), loadout.attributes.max_hp),
                    member.ailments.petrified,
                )?;
                let mut prepared = inputs.prepare_party_member(
                    menus,
                    0,
                    battle::party::loadout(menus, member, input.character)?,
                    battle::party::Setup {
                        position: input.position,
                        heading: 0.,
                        model: Some(model::ModelSetup {
                            resource: ResourceId(100),
                            initial,
                            suppress_root_translation: [false; 3],
                        }),
                    },
                    false,
                    ResourceId(200),
                    None,
                )?;
                prepared.select_costume_prop(
                    member.equipment[0],
                    input.source.weapon_prop,
                    party.game_clears,
                    &mut random,
                )?;
                let model = prepared.model.as_ref().context("missing party body")?;
                let prop_path = resonance_content::battle_model::document::equipment_path(
                    &mut library.snapshot(),
                    u32::from(prop),
                )?
                .context("missing prop")?;
                let owner = prop_path.strip_suffix("equipment.json").unwrap();
                let active_prop = model.weapons.iter().any(|weapon| {
                    prepared
                        .models
                        .iter()
                        .filter(|model| model.resource == weapon.primary_resource())
                        .flat_map(|model| &model.parts)
                        .any(|part| part.scene.mesh.starts_with(owner))
                });
                assert_eq!(
                    active_prop, selected,
                    "costume {character}:{costume}, clears{clears}, seed{seed}"
                );
                let stats = &prepared.actor.equipment;
                if let Some(baseline) = &baseline {
                    assert_eq!(stats, baseline);
                } else {
                    baseline = Some(stats.clone());
                }
                assert_eq!(
                    serde_json::to_value(&party)?,
                    original,
                    "cosmetic selection changed party state"
                );
            }
        }
    }
    Ok(())
}
