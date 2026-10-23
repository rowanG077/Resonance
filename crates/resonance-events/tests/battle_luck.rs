//! Equipment and EX stat domains shared by menus and battle; no devices.
use resonance_content::{
    menu_data::{ExStat, MenuData},
    session::SessionData,
};
use resonance_events::party::{Member, Party};

fn cooked<T: serde::de::DeserializeOwned>(name: &str) -> T {
    let root = std::env::var_os("RESONANCE_COOKED")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../local/all-assets")
        });
    serde_json::from_slice(&std::fs::read(root.join("game").join(name)).unwrap()).unwrap()
}

fn fixture(character: usize) -> (MenuData, Member) {
    let menus = cooked("menu-data.json");
    let session: SessionData = cooked("session-data.json");
    let mut party = Party::new(&session, Default::default()).unwrap();
    let mut member = party.members.remove(character);
    member.equipment = [0; 6];
    member.ex_skills = [0; 4];
    member.compound_ex_skills.clear();
    member.recent_compound_ex_skills.clear();
    (menus, member)
}

#[test]
#[ignore = "requires locally cooked GQSEAF menu and session data; CPU only"]
fn equipment_luck_is_nonnegative_and_matches_menu_stats() {
    let (menus, mut member) = fixture(7);
    assert_eq!(menus.items[263].equipment_stats[6], -10);
    assert_eq!(menus.items[420].equipment_stats[6], 30);
    member.luck = 1;
    for (equipment, expected) in [
        ([263, 0, 0, 0, 0, 0], 0),
        ([0, 0, 0, 420, 0, 0], 31),
        ([263, 0, 0, 420, 0, 0], 30),
        ([263, 0, 0, 420, 420, 0], 60),
    ] {
        member.equipment = equipment;
        let saved = serde_json::to_value(&member).unwrap();
        let field = member.stats_for(&menus, 7);
        assert_eq!(field.luck, expected);
        assert_eq!(member.stats_for(&menus, 7).luck, expected);
        assert_eq!(member.stats_for(&menus, 7), field);
        assert_eq!(serde_json::to_value(&member).unwrap(), saved);
    }
}

#[test]
#[ignore = "requires locally cooked GQSEAF menu and session data; CPU only"]
fn authored_lucky_ex_uses_base_luck_and_rounds_before_adding_to_equipment() {
    let (menus, mut member) = fixture(1);
    let luck_rows: Vec<_> = menus
        .ex_skills
        .skills
        .iter()
        .flat_map(|(&id, skill)| {
            skill
                .stat_bonuses
                .iter()
                .filter(|bonus| bonus.stat == ExStat::Luck)
                .map(move |bonus| (id, bonus.percent))
        })
        .collect();
    assert_eq!(luck_rows, [(19, 10)]);
    assert!(
        menus
            .ex_skills
            .characters
            .iter()
            .all(|character| character.compounds.iter().all(|recipe| recipe.skill != 19))
    );
    member.ex_skills = [0, 19, 0, 0];
    member.equipment[3] = 420;
    // EX luck uses the base value, so Rabbit's Foot does not increase
    // that contribution. Products 400 and 500 straddle its rounding threshold.
    for (base, expected) in [(4, 34), (5, 36), (14, 45), (15, 47)] {
        member.luck = base;
        assert_eq!(member.stats_for(&menus, 1).luck, expected);
    }
    member.equipment[4] = 420;
    member.luck = 5;
    assert_eq!(member.stats_for(&menus, 1).luck, 66);
    member.compound_ex_skills = (0..24).collect();
    assert_eq!(member.stats_for(&menus, 1).luck, 66);
}

#[test]
#[ignore = "requires locally cooked menu and session data; CPU only"]
fn equipment_luck_is_monotonic_and_caps_without_narrowing() {
    let (mut menus, mut member) = fixture(1);
    menus.items[263].equipment_stats[6] = 200;
    member.luck = 255;
    let mut previous = member.stats_for(&menus, 1).luck;
    for slot in 0..6 {
        member.equipment[slot] = 263;
        let luck = member.stats_for(&menus, 1).luck;
        assert!(luck >= previous && luck <= 999);
        assert_eq!(luck, member.stats_for(&menus, 1).luck);
        previous = luck;
    }
    assert_eq!(previous, 999);
    member.ex_skills = [19, 0, 0, 0];
    assert_eq!(member.stats_for(&menus, 1).luck, 999);
    member.ex_skills = [0; 4];
    menus.items[263].equipment_stats[6] = i16::MAX;
    assert_eq!(member.stats_for(&menus, 1).luck, 999);
    menus.items[263].equipment_stats[6] = i16::MIN;
    assert_eq!(member.stats_for(&menus, 1).luck, 0);
    member.equipment = [0; 6];
    menus.items[0].equipment_stats[6] = i16::MAX;
    assert_eq!(member.stats_for(&menus, 1).luck, u16::from(member.luck));
}

#[test]
#[ignore = "requires locally cooked menu and session data; CPU only"]
fn high_base_stats_and_signed_bonuses_stay_capped_without_changing_the_save() {
    let (mut menus, mut member) = fixture(1);
    member.base_stats = [u16::MAX; 7];
    member.luck = u8::MAX;
    let derived = |member: &Member, menus: &MenuData| {
        let saved = serde_json::to_value(member).unwrap();
        let stats = member.stats_for(menus, 1);
        assert_eq!(serde_json::to_value(member).unwrap(), saved);
        assert_eq!([stats.hp, stats.tp], [9999, 999]);
        [
            stats.strength,
            stats.slash,
            stats.thrust,
            stats.defense,
            stats.intelligence,
            stats.accuracy,
            stats.evasion,
            stats.luck,
        ]
    };
    for (equipment, bonus, expected) in [
        (0, 0, [3000, 3000, 3000, 3000, 999, 999, 999, 255]),
        (263, -50, [3000, 2950, 2950, 2950, 949, 949, 949, 205]),
        (263, i16::MAX, [3000, 3000, 3000, 3000, 999, 999, 999, 999]),
        (263, i16::MIN, [3000, 0, 0, 0, 0, 0, 0, 0]),
    ] {
        member.equipment[0] = equipment;
        menus.items[263].equipment_stats = [bonus; 7];
        assert_eq!(derived(&member, &menus), expected);
    }
    let bonus = &mut menus.ex_skills.skills.get_mut(&19).unwrap().stat_bonuses[0];
    bonus.stat = ExStat::Strength;
    bonus.percent = u8::MAX;
    member.ex_skills[0] = 19;
    assert_eq!(derived(&member, &menus), [3000, 3000, 3000, 0, 0, 0, 0, 0]);
}

#[test]
#[ignore = "requires locally cooked menu and session data; CPU only"]
fn critical_equipment_bonus_is_monotonic_and_caps_at_a_probability() {
    let (mut menus, mut member) = fixture(1);
    menus.items[263].properties.critical_chance_bonus = 30;
    for count in 0..=6 {
        member.equipment = [0; 6];
        member.equipment[..count].fill(263);
        assert_eq!(
            member.equipment_traits(&menus).critical_chance_bonus,
            (count as u16 * 30).min(100)
        );
    }
    menus.items[263].properties.critical_chance_bonus = u8::MAX;
    assert_eq!(member.equipment_traits(&menus).critical_chance_bonus, 100);
    member.equipment = [0; 6];
    menus.items[0].properties.critical_chance_bonus = u8::MAX;
    assert_eq!(member.equipment_traits(&menus).critical_chance_bonus, 0);
}
