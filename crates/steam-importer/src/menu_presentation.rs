//! Localized captions for the existing GC menu pages.
use crate::{
    output, pe,
    source::Source,
    text::{Bank, Menu},
};
use anyhow::{Result, ensure};
use serde_json::json;
use std::{collections::BTreeMap, path::Path};

fn caption(source: &Menu<'_>, directory: u32, index: u32) -> Result<String> {
    let mut text = source.caption(source.directory(directory)?, index)?;
    // A few translations omit captions required by the shared menu pages.
    if text.is_empty() {
        text = source.caption(source.word(directory + 4)?, index)?;
    }
    ensure!(
        !text.is_empty(),
        "missing menu caption {directory:#x}:{index}"
    );
    Ok(text)
}

pub(crate) fn cook(source: &Source, output: &Path) -> Result<()> {
    let bytes = source.require("TOS_TEXT.TXB")?;
    let source = Menu {
        image: pe::Image::parse(&source.executable)?,
        bank: Bank::parse(&bytes)?,
        slot: source.language.slot(),
    };
    let mut labels = BTreeMap::new();
    for (key, directory, index) in [
        ("status", 0x8b87e8, 19),
        ("next", 0x8b87e8, 3),
        ("strength", 0x8b87e8, 4),
        ("defense", 0x8b87e8, 5),
        ("slash", 0x8b87e8, 6),
        ("accuracy", 0x8b87e8, 7),
        ("attack", 0x8b87e8, 8),
        ("thrust", 0x8b87e8, 9),
        ("evasion", 0x8b87e8, 10),
        ("intelligence", 0x8b87e8, 11),
        ("luck", 0x8b87e8, 12),
        ("weapon", 0x8b87b8, 0),
        ("body", 0x8b87b8, 1),
        ("head", 0x8b87b8, 2),
        ("arm", 0x8b87b8, 3),
        ("accessory_1", 0x8b87b8, 4),
        ("accessory_2", 0x8b87b8, 5),
        ("element_attack", 0x8b87e8, 13),
        ("element_defense", 0x8b87e8, 14),
        ("weak", 0x8b87e8, 15),
        ("absorb", 0x8b87e8, 16),
        ("invalid", 0x8b87e8, 17),
        ("reduce", 0x8b87e8, 18),
        ("growth", 0x8b87e8, 20),
        ("growth_hp", 0x8b87d0, 0),
        ("growth_tp", 0x8b87d0, 1),
        ("growth_strength", 0x8b87d0, 2),
        ("growth_defense", 0x8b87d0, 3),
        ("growth_intelligence", 0x8b87d0, 4),
        ("growth_evasion", 0x8b87d0, 5),
        ("growth_accuracy", 0x8b87d0, 6),
    ] {
        labels.insert(key.to_owned(), caption(&source, directory, index)?);
    }
    for (directory, entries) in [
        (
            0x8b943c,
            &[
                ("collectors_book", 27),
                ("discard", 29),
                ("transformed", 30),
                ("discarded", 31),
                ("confirm_discard", 32),
                ("select_item", 35),
                ("select_target", 37),
                ("equip_target", 39),
                ("transform_full", 40),
                ("transform_empty", 41),
                ("holy_aura", 42),
                ("dark_aura", 43),
            ][..],
        ),
        (
            0x8bfd2c,
            &[
                ("item_slash", 0),
                ("item_thrust", 1),
                ("item_defense", 2),
                ("item_accuracy", 3),
                ("item_evasion", 4),
                ("item_intelligence", 5),
                ("item_luck", 6),
            ][..],
        ),
        (
            0x8bfd5c,
            &[
                ("optimal", 1),
                ("remove", 2),
                ("change_order", 3),
                ("optimal_selection", 4),
                ("optimal_slash", 5),
                ("optimal_thrust", 6),
                ("item_attack", 7),
                ("stat_arrow", 8),
                ("party_swap", 3),
            ][..],
        ),
        (0x8bfd44, &[("alphabetical", 0), ("parameter", 1)][..]),
        (
            0x8b8890,
            &[
                ("tech_usage", 3),
                ("tech_remove", 5),
                ("tech_auto", 9),
                ("tech_execute", 7),
                ("tech_forget", 8),
                ("tech_unison", 10),
                ("tech_control", 11),
                ("tech_select", 12),
                ("tech_shortcut", 13),
                ("tech_unison_title", 14),
                ("tech_attack", 15),
                ("tech_target", 16),
                ("tech_cannot_forget", 18),
                ("tech_related", 19),
                ("tech_forget_warning", 20),
                ("tech_forget_confirm", 21),
                ("unison_title", 24),
                ("party_swap_target", 16),
            ][..],
        ),
        (
            0x8b8860,
            &[
                ("tech_manual", 0),
                ("tech_semi_auto", 1),
                ("tech_auto_mode", 2),
            ][..],
        ),
        (
            0x8b8878,
            &[
                ("tech_strength", 0),
                ("tech_slash", 1),
                ("tech_thrust", 2),
                ("tech_defense", 3),
                ("tech_luck", 4),
                ("tech_accuracy", 5),
                ("tech_evasion", 6),
                ("tech_intelligence", 7),
            ][..],
        ),
        (
            0x8b9340,
            &[
                ("party_slash", 2),
                ("party_thrust", 3),
                ("party_attack", 9),
                ("party_defense", 4),
                ("party_luck", 8),
                ("party_accuracy", 5),
                ("party_evasion", 6),
            ][..],
        ),
    ] {
        for &(key, index) in entries {
            labels.insert(key.to_owned(), caption(&source, directory, index)?);
        }
    }
    let techniques = source.directory(0x8b8890)?;
    let player = source.caption(techniques, 25)?;
    ensure!(
        player.matches('1').count() == 1,
        "unrecognized Unison player caption"
    );
    let player = player.replace('1', "%d");
    for index in 2..=4 {
        ensure!(
            source.caption(techniques, 24 + index)? == player.replace("%d", &index.to_string()),
            "inconsistent Unison player captions"
        );
    }
    labels.insert("unison_player".into(), player);
    // These captions belong to shared-engine UI actions absent from the PC menus.
    for (key, label) in [
        ("preview_loading", "Loading..."),
        ("party_leader", "Leader"),
        ("tech_target_all", "All"),
    ] {
        labels.insert(key.into(), label.into());
    }
    ensure!(labels.len() == 101, "incomplete menu presentation labels");
    let inventory = source.directory(0x8b943c)?;
    let categories = |at, count| -> Result<Vec<String>> {
        let table = source.directory(at)?;
        (0..count).map(|i| source.caption(table, i)).collect()
    };
    let prompt = source.caption(inventory, 38)?;
    let bottle = source.caption(inventory, 36)?;
    let items = json!({
        "item_categories": categories(0x8b7188,48)?,
        "inventory_categories": categories(0x8b71d0,9)?,
        "item_group_prompt": resonance_importer::decode_menu_text(&prompt,9)?,
        "item_bottle_count": resonance_importer::decode_menu_text(&bottle,8)?,
    });
    let mut conditions = BTreeMap::new();
    let table = source.directory(0x8b8830)?;
    for (key, index) in [
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
        conditions.insert(key, source.caption(table, index)?);
    }
    let ex = source.directory(0x8cb88c)?;
    let status = json!({"conditions":conditions, "technical_type":source.caption(ex,16)?, "strike_type":source.caption(ex,15)?});
    output::json(
        &output.join("game/menu/presentation.json"),
        &json!({"labels":labels,"items":items,"status":status}),
    )
}
