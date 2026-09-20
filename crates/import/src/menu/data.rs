//! Parse original databases into validated, editable JSON records.
//! Resolve pointers and decode packed fields here; runtime schemas use their meaning.
use super::*;
use crate::all_assets::{
    cooking_ui, ex_skills as ex_catalogue, figurine_catalogue as figurine_records, inventory_ui,
    monster_catalogue as monster_records, options_ui, rename_ui, save_menu, shop_ui, status_ui,
    strategy_ui, synopsis as synopsis_catalogue, technique_ui, ui_style,
};
use crate::dol;
use resonance_content::menu_data::{
    Item, ItemAttention, ItemUse, ItemView, MenuData, Technique, TechniqueUse, Title,
};
mod cooking;
mod costume;
mod customize;
mod ex_skills;
mod manual;
mod physical;
pub(crate) use physical::{BoneRule, FigurineModel, FigurineRow};
pub(crate) fn figurine_catalogue(output: &Path, disc: u8) -> Result<physical::Figurines> {
    #[derive(serde::Deserialize)]
    struct Tables {
        figurines: physical::Figurines,
    }
    let tables: Tables = crate::cooked::Source::open(output, disc, "sys/main.dol")?
        .document("embedded/menu/tables.json")?;
    ensure!(
        tables.figurines.records.len() == 328,
        "incomplete cooked figurine catalogue"
    );
    Ok(tables.figurines)
}
pub(crate) fn monster_catalogue(output: &Path, disc: u8) -> Result<physical::Monsters> {
    #[derive(serde::Deserialize)]
    struct Tables {
        monsters: physical::Monsters,
    }
    let tables: Tables = crate::cooked::Source::open(output, disc, "sys/main.dol")?
        .document("embedded/menu/tables.json")?;
    ensure!(
        tables.monsters.records.len() == resonance_content::monster::MONSTER_COUNT,
        "incomplete cooked monster catalogue"
    );
    Ok(tables.monsters)
}
mod rename;
mod status;
mod strategy;
mod synopsis;
pub(super) mod text;
pub(super) mod world_map;

pub(super) fn cook(output: &Path, disc: u8) -> Result<()> {
    let mut tables: serde_json::Value = crate::cooked::Source::open(output, disc, "sys/main.dol")?
        .document("embedded/menu/tables.json")?;
    tables["figurines"] = serde_json::to_value(crate::figurines::book(output, disc)?)?;
    tables["monsters"] = serde_json::to_value(crate::monsters::book(output, disc)?)?;
    // World-map source records include additional authored rows; the runtime
    // schema selects its already-decoded locations, shops and field mappings.
    let data: MenuData = serde_json::from_value(tables)?;
    data.validate()?;
    write_atomic(
        &output.join("game/menu-data.json"),
        &serde_json::to_vec_pretty(&data)?,
    )
}

/// Physical source tables do not require prepared model previews or runtime admission.
pub(super) fn cook_source(executable: &[u8], output: &Path) -> Result<()> {
    write_atomic(
        &output.join("embedded/menu/tables.json"),
        &serde_json::to_vec_pretty(&read(executable)?)?,
    )
}

pub(super) fn items(executable: &[u8]) -> Result<Vec<Item>> {
    crate::item::read(executable)?
        .into_iter()
        .enumerate()
        .map(|(id, row)| {
            Ok(Item {
                attention: match id {
                    1 | 2 | 8 => Some(ItemAttention::LowHp),
                    3 | 4 | 9 => Some(ItemAttention::LowTp),
                    5..=7 => Some(ItemAttention::LowVitals),
                    10 => Some(ItemAttention::Ailment),
                    11 => Some(ItemAttention::Knockout),
                    _ => None,
                },
                field_usable: row.usage_flags & 1 != 0,
                view: match id {
                    68 => Some(ItemView::TetheallaMap),
                    69 => Some(ItemView::SylvarantMap),
                    70 => Some(ItemView::CollectorsBook),
                    71 => Some(ItemView::MonsterList),
                    72 => Some(ItemView::FigurineBook),
                    73 => Some(ItemView::TrainingManual),
                    _ => None,
                },
                properties: status::properties(&row)?,
                price: row.price.try_into().context("negative item price")?,
                transforms_to: row.transforms_to,
                field_use: if row.usage_flags & 1 == 0 {
                    None
                } else {
                    match id {
                        1..=9 => {
                            let (hp, tp, party) = [
                                (30, 0, false),
                                (60, 0, false),
                                (0, 30, false),
                                (0, 60, false),
                                (30, 30, false),
                                (60, 60, false),
                                (100, 100, false),
                                (30, 0, true),
                                (0, 30, true),
                            ][id - 1];
                            Some(ItemUse::Recover { hp, tp, party })
                        }
                        10 | 12 => Some(ItemUse::Cure),
                        11 => Some(ItemUse::Revive),
                        22 => Some(ItemUse::Transform),
                        23 | 24 => Some(ItemUse::EncounterRate {
                            rate: (id - 22) as u8,
                        }),
                        25..=36 => {
                            let index = (id - 25) % 6;
                            let strong = id >= 31;
                            Some(ItemUse::Herb {
                                stat: [1, 0, 2, 3, 6, 5][index],
                                amount: if index < 2 {
                                    if strong { 10 } else { 5 }
                                } else if strong {
                                    30
                                } else {
                                    10
                                },
                                percent: index < 2,
                            })
                        }
                        _ => None,
                    }
                },
                name: row.name.unwrap_or_default(),
                description: row.description.unwrap_or_default(),
                details: row.details.unwrap_or_default(),
                category: row.category,
                equipment_stats: [
                    row.slash,
                    row.thrust,
                    row.defense,
                    i16::from(row.intelligence),
                    i16::from(row.accuracy),
                    i16::from(row.evasion),
                    i16::from(row.luck),
                ],
            })
        })
        .collect()
}

fn read(executable: &[u8]) -> Result<serde_json::Value> {
    let ui = inventory_ui::read(executable)?;
    let technique_ui = technique_ui::read(executable)?;
    let status_ui = status_ui::read(executable)?;
    let strategy_ui = strategy_ui::read(executable)?;
    let cooking_ui = cooking_ui::read(executable)?;
    let options_ui = options_ui::read(executable)?;
    let synopsis_catalogue = synopsis_catalogue::read(executable)?;
    let font = crate::font_directory::Directory::read(executable)?;
    let items = items(executable)?;
    let techniques: Vec<Technique> = crate::arte::read(executable)?
        .definitions
        .into_iter()
        .enumerate()
        .map(|(id, row)| {
            let (hp, party) = match id {
                98 | 221 => (30, false),
                99 | 117 => (45, true),
                100 => (60, false),
                118 => (30, true),
                119 => (100, false),
                120 => (60, true),
                122 => (70, true),
                192 => (25, false),
                193 => (35, false),
                194 => (45, false),
                _ => (0, false),
            };
            Ok(Technique {
                name: row.name.unwrap_or_default(),
                description: row.description.unwrap_or_default(),
                tp: row.tp_cost,
                tp_percent: row.native_id == 34,
                unison_usable: row.flags & 0x100 != 0,
                rank: row.menu_category,
                element: row.element,
                route: row.learning_route,
                level: row.required_level,
                prerequisite: row
                    .learning_parent
                    .try_into()
                    .context("negative menu prerequisite")?,
                alternatives: row.mutually_exclusive.map(|id| id as u16),
                field_use: if hp != 0 {
                    Some(TechniqueUse::Recover { hp, party })
                } else {
                    match id {
                        101 | 102 => Some(TechniqueUse::Cure { party: id == 102 }),
                        121 => Some(TechniqueUse::Revive),
                        _ => None,
                    }
                },
            })
        })
        .collect::<Result<_>>()?;
    let titles = titles(executable)?;
    let full_names: Vec<String> = status_ui
        .full_name_formats
        .iter()
        .map(|&reference| {
            status_ui
                .required_text(reference)
                .map(|s| s.replace("%s", "{name}"))
        })
        .collect::<Result<_>>()?;
    let status_labels = &status_ui.labels;
    let mut labels: std::collections::BTreeMap<String, String> = [
        ("status", status_labels.title),
        ("next", status_labels.next),
        ("strength", status_labels.strength),
        ("defense", status_labels.defense),
        ("slash", status_labels.slash),
        ("accuracy", status_labels.accuracy),
        ("attack", status_labels.attack),
        ("thrust", status_labels.thrust),
        ("evasion", status_labels.evasion),
        ("intelligence", status_labels.intelligence),
        ("luck", status_labels.luck),
        ("weapon", status_labels.weapon),
        ("body", status_labels.body),
        ("head", status_labels.head),
        ("arm", status_labels.arm),
        ("accessory_1", status_labels.accessory_1),
        ("accessory_2", status_labels.accessory_2),
        ("element_attack", status_labels.element_attack),
        ("element_defense", status_labels.element_defense),
        ("weak", status_labels.weak),
        ("absorb", status_labels.absorb),
        ("invalid", status_labels.invalid),
        ("reduce", status_labels.reduce),
        ("growth", status_labels.growth),
        ("growth_hp", status_labels.growth_hp),
        ("growth_tp", status_labels.growth_tp),
        ("growth_strength", status_labels.growth_strength),
        ("growth_defense", status_labels.growth_defense),
        ("growth_intelligence", status_labels.growth_intelligence),
        ("growth_evasion", status_labels.growth_evasion),
        ("growth_accuracy", status_labels.growth_accuracy),
    ]
    .into_iter()
    .map(|(key, reference)| Ok((key.into(), status_ui.required_text(reference)?.to_owned())))
    .collect::<Result<_>>()?;
    let inventory = &ui.inventory;
    let equipment = &ui.equipment;
    for (key, reference) in [
        ("item_slash", inventory.comparison.slash),
        ("item_thrust", inventory.comparison.thrust),
        ("item_defense", inventory.comparison.defense),
        ("item_accuracy", inventory.comparison.accuracy),
        ("item_evasion", inventory.comparison.evasion),
        ("item_intelligence", inventory.comparison.intelligence),
        ("item_luck", inventory.comparison.luck),
        ("item_attack", inventory.comparison.attack),
        ("discard", inventory.discard),
        ("transformed", inventory.transformed),
        ("discarded", inventory.discarded),
        ("confirm_discard", inventory.actions.confirm_discard),
        ("select_item", inventory.actions.select_item),
        ("select_target", inventory.actions.select_target),
        ("equip_target", inventory.actions.equip_target),
        ("transform_full", inventory.actions.transform_full),
        ("transform_empty", inventory.actions.transform_empty),
        ("collectors_book", inventory.books.collectors_book),
        ("holy_aura", inventory.actions.holy_aura),
        ("dark_aura", inventory.actions.dark_aura),
        ("optimal", equipment.optimal),
        ("remove", equipment.remove),
        ("change_order", equipment.change_order),
        ("optimal_selection", equipment.optimal_selection),
        ("optimal_slash", equipment.attack_preference[0]),
        ("optimal_thrust", equipment.attack_preference[1]),
        ("alphabetical", equipment.ordering[0]),
        ("parameter", equipment.ordering[1]),
        ("stat_arrow", ui.formats.stat_arrow),
    ] {
        labels.insert(key.into(), ui.text(reference).to_owned());
    }
    labels.insert(
        "preview_loading".into(),
        ui.text(ui.preview_loading.text).to_owned(),
    );
    let tech = &technique_ui.technique;
    let party = &technique_ui.party;
    for (key, reference) in [
        ("tech_usage", tech.usage),
        ("tech_remove", tech.remove),
        ("tech_auto", tech.auto),
        ("tech_execute", tech.execute),
        ("tech_forget", tech.forget),
        ("tech_unison", tech.unison_settings),
        ("tech_control", tech.control_type),
        ("tech_manual", tech.manual),
        ("tech_semi_auto", tech.semi_auto),
        ("tech_auto_mode", tech.auto_mode),
        ("tech_select", tech.select),
        ("tech_shortcut", tech.shortcut),
        ("tech_unison_title", tech.unison_setting),
        ("tech_strength", tech.attributes.strength),
        ("tech_slash", tech.attributes.slash),
        ("tech_thrust", tech.attributes.thrust),
        ("tech_defense", tech.attributes.defense),
        ("tech_luck", tech.attributes.luck),
        ("tech_accuracy", tech.attributes.accuracy),
        ("tech_evasion", tech.attributes.evasion),
        ("tech_intelligence", tech.attributes.intelligence),
        ("tech_attack", tech.attributes.attack),
        ("tech_target", tech.target),
        ("tech_target_all", tech.target_all),
        ("tech_cannot_forget", tech.cannot_forget),
        ("tech_related", tech.related),
        ("tech_forget_warning", tech.forget_warning),
        ("tech_forget_confirm", tech.forget_confirm),
        ("unison_title", tech.unison_title),
        ("unison_player", technique_ui.unison_formats.player),
        ("party_slash", party.slash),
        ("party_thrust", party.thrust),
        ("party_attack", party.attack),
        ("party_defense", party.defense),
        ("party_luck", party.luck),
        ("party_accuracy", party.accuracy),
        ("party_evasion", party.evasion),
        ("party_swap_target", party.exchange_target),
        ("party_leader", party.display_change),
        ("party_swap", party.exchange),
    ] {
        labels.insert(key.into(), technique_ui.text(reference).to_owned());
    }
    let categories = |references: &[Option<_>]| -> Result<Vec<String>> {
        references
            .iter()
            .map(|&reference| ui.required_text(reference).map(str::to_owned))
            .collect()
    };
    for (key, reference) in [
        ("strategy_title", strategy_ui.labels.title),
        ("strategy_orders", strategy_ui.labels.orders),
        ("strategy_rename", strategy_ui.labels.rename),
        ("strategy_default", strategy_ui.labels.default),
    ] {
        labels.insert(key.into(), strategy_ui.required_text(reference)?.to_owned());
    }
    let world = crate::all_assets::world_map::read(executable)?;
    Ok(serde_json::json!({
        "version": MenuData::VERSION,
        "artwork": super::recipe::read(executable, &technique_ui, &options_ui, &save_menu::read(executable)?, &shop_ui::read(executable)?, &ui_style::read(executable)?)?,
        "item_group_prompt": text::decode(ui.text(inventory.actions.use_hint), 9)?,
        "item_bottle_count": text::decode(ui.text(inventory.actions.remaining_format), 8)?,
        "ex_skills": ex_skills::cook(&ex_catalogue::read(executable)?)?,
        "figurines": physical::figurines(&figurine_records::read(executable)?)?,
        "monsters": physical::monsters(&monster_records::read(executable)?, &ui)?,
        "manual": manual::cook(&synopsis_catalogue)?,
        "world_map": world_map::cook_source(
            &world,
            &crate::field_catalogue::read(executable)?,
            &ui,
        )?,
        "status": status::cook(&status_ui, &items)?,
        "strategy": strategy::cook(&strategy_ui)?,
        "synopsis": synopsis::cook(&font.metrics, &synopsis_catalogue, &world)?,
        "cooking": cooking::cook(&cooking_ui)?,
        "customize": customize::cook(&options_ui)?,
        "techniques": techniques,
        "items": items,
        "titles": titles,
        "full_names": full_names,
        "rename": rename::cook(&rename_ui::read(executable)?, &crate::character_data::read(executable)?)?,
        "labels": labels,
        "item_categories": categories(&ui.item_categories)?,
        "inventory_categories": categories(&ui.inventory_categories)?,
    }))
}

/// Shared title records supply menu text and battle costume requirements.
pub(crate) fn titles(executable: &[u8]) -> Result<Vec<Vec<Title>>> {
    let catalogue = crate::all_assets::title_catalogue::read(executable)?;
    let mut titles = (1..=catalogue.character_starts.len())
        .map(|character| {
            catalogue
                .for_character(character as u8)?
                .iter()
                .map(|row| {
                    Ok(Title {
                        name: catalogue.required_text(row.name)?.to_owned(),
                        description: catalogue.required_text(row.description)?.to_owned(),
                        growth: row.growth,
                        costume: None,
                    })
                })
                .collect::<Result<Vec<_>>>()
        })
        .collect::<Result<Vec<_>>>()?;
    costume::cook(&catalogue.costumes, &mut titles)?;
    Ok(titles)
}

#[test]
#[ignore = "requires both extracted executables and cook-all"]
fn original_menu_tables_match_shared_publications() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local");
    for disc in [1, 2] {
        let executable = fs::read(local.join(format!("extracted/disc{disc}/sys/main.dol")))?;
        let published: serde_json::Value =
            crate::cooked::Source::open(&local.join("all-assets"), disc, "sys/main.dol")?
                .document("embedded/menu/tables.json")?;
        ensure!(
            read(&executable)? == published,
            "disc {disc} menu tables differ from the original definitions"
        );
    }
    Ok(())
}
