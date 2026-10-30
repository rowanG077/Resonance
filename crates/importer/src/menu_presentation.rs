//! Native UI text tables directly into the editable menu presentation file.
use crate::text;
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

const STATUS: u32 = 0x80199398;
const CONDITIONS: u32 = 0x801995a4;
const INVENTORY: u32 = 0x8019d650;
const EQUIPMENT: u32 = 0x8019d370;
const TECHNIQUE: u32 = 0x801aaf34;
const PARTY: u32 = 0x801aaa70;

fn add(
    exe: &[u8],
    labels: &mut BTreeMap<String, String>,
    table: u32,
    names: &[(&str, u32)],
) -> Result<()> {
    for &(name, index) in names {
        labels.insert(name.into(), text::pointer(exe, table + index * 4)?);
    }
    Ok(())
}

fn convert(exe: &[u8]) -> Result<Value> {
    let mut labels = BTreeMap::new();
    add(
        exe,
        &mut labels,
        STATUS,
        &[
            ("status", 0),
            ("next", 1),
            ("strength", 2),
            ("defense", 3),
            ("slash", 4),
            ("accuracy", 5),
            ("attack", 6),
            ("thrust", 7),
            ("evasion", 8),
            ("intelligence", 9),
            ("luck", 10),
            ("weapon", 11),
            ("body", 12),
            ("head", 13),
            ("arm", 14),
            ("accessory_1", 15),
            ("accessory_2", 16),
            ("element_attack", 17),
            ("element_defense", 18),
            ("weak", 19),
            ("absorb", 20),
            ("invalid", 21),
            ("reduce", 22),
            ("growth", 23),
            ("growth_hp", 24),
            ("growth_tp", 25),
            ("growth_strength", 26),
            ("growth_defense", 27),
            ("growth_intelligence", 28),
            ("growth_evasion", 29),
            ("growth_accuracy", 30),
        ],
    )?;
    add(
        exe,
        &mut labels,
        INVENTORY,
        &[
            ("discard", 1),
            ("transformed", 2),
            ("discarded", 3),
            ("item_slash", 4),
            ("item_thrust", 5),
            ("item_defense", 6),
            ("item_accuracy", 7),
            ("item_evasion", 8),
            ("item_intelligence", 9),
            ("item_luck", 10),
            ("item_attack", 11),
            ("confirm_discard", 26),
            ("select_item", 29),
            ("select_target", 31),
            ("equip_target", 33),
            ("transform_full", 34),
            ("transform_empty", 35),
            ("holy_aura", 36),
            ("dark_aura", 37),
            ("collectors_book", 73),
        ],
    )?;
    add(
        exe,
        &mut labels,
        EQUIPMENT,
        &[
            ("optimal", 7),
            ("remove", 8),
            ("change_order", 9),
            ("optimal_selection", 10),
            ("optimal_slash", 11),
            ("optimal_thrust", 12),
            ("alphabetical", 21),
            ("parameter", 22),
        ],
    )?;
    labels.insert("stat_arrow".into(), text::string(exe, 0x8035cee4)?);
    labels.insert("preview_loading".into(), text::string(exe, 0x801aa9f8)?);
    add(
        exe,
        &mut labels,
        TECHNIQUE,
        &[
            ("tech_usage", 2),
            ("tech_remove", 3),
            ("tech_auto", 4),
            ("tech_execute", 5),
            ("tech_forget", 6),
            ("tech_unison", 7),
            ("tech_control", 8),
            ("tech_manual", 9),
            ("tech_semi_auto", 10),
            ("tech_auto_mode", 11),
            ("tech_select", 12),
            ("tech_shortcut", 13),
            ("tech_unison_title", 14),
            ("tech_strength", 15),
            ("tech_slash", 16),
            ("tech_thrust", 17),
            ("tech_defense", 18),
            ("tech_luck", 19),
            ("tech_accuracy", 20),
            ("tech_evasion", 21),
            ("tech_intelligence", 22),
            ("tech_attack", 23),
            ("tech_target", 24),
            ("tech_target_all", 25),
            ("tech_cannot_forget", 26),
            ("tech_related", 27),
            ("tech_forget_warning", 28),
            ("tech_forget_confirm", 29),
            ("unison_title", 32),
        ],
    )?;
    labels.insert("unison_player".into(), text::string(exe, 0x8035d5e0)?);
    add(
        exe,
        &mut labels,
        PARTY,
        &[
            ("party_slash", 5),
            ("party_thrust", 6),
            ("party_attack", 7),
            ("party_defense", 8),
            ("party_luck", 9),
            ("party_accuracy", 10),
            ("party_evasion", 11),
            ("party_swap_target", 12),
            ("party_leader", 13),
            ("party_swap", 14),
        ],
    )?;
    ensure!(labels.len() == 101, "incomplete menu presentation labels");

    let categories = |table: u32, count: u32| -> Result<Vec<String>> {
        (0..count)
            .map(|index| text::pointer(exe, table + index * 4))
            .collect()
    };
    let prompt = text::pointer(exe, INVENTORY + 32 * 4)?;
    let bottle_count = text::pointer(exe, INVENTORY + 30 * 4)?;
    let items = json!({
        "item_categories": categories(0x801ab2e0, 48)?,
        "inventory_categories": categories(0x801ab22c + 9 * 4, 9)?,
        "item_group_prompt": text::decode(&prompt, 9)?,
        "item_bottle_count": text::decode(&bottle_count, 8)?,
    });
    let mut conditions = BTreeMap::new();
    for (name, index) in [
        ("poison", 5),
        ("severe_poison", 6),
        ("paralysis", 7),
        ("petrified", 8),
        ("curse", 9),
        ("attack_up", 12),
        ("attack_down", 13),
        ("defense_up", 14),
        ("defense_down", 15),
        ("accuracy_up", 16),
        ("accuracy_down", 17),
        ("magic_attack_up", 18),
        ("magic_attack_down", 19),
        ("magic_defense_up", 20),
        ("knockout", 31),
    ] {
        conditions.insert(name, text::pointer(exe, CONDITIONS + index * 4)?);
    }
    let status = json!({
        "conditions": conditions,
        "technical_type": text::string(exe, 0x8035d46c)?,
        "strike_type": text::string(exe, 0x8035d474)?,
    });
    Ok(json!({"labels": labels, "items": items, "status": status}))
}

pub fn cook_menu_presentation(extracted: &Path, library: &Path) -> Result<()> {
    let output = library.join("game/menu/presentation.json");
    ensure!(!output.exists(), "menu presentation output already exists");
    let document = convert(&fs::read(extracted.join("disc1/sys/main.dol"))?)?;
    crate::output::json(&output, &document)?;
    Ok(())
}
