//! Source declarations and constants used to assemble the player menu.
use crate::{
    all_assets::{options_ui, save_menu, shop_ui, technique_ui, ui_style},
    dol,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Bank {
    Frames,
    Plain,
    Alternate,
    Patterns,
    Cursor,
    Symbols,
    WorldMaps,
    Portraits,
    Technique,
    Strategy,
    Elements,
    Numbers,
    Items,
    ItemTabs,
    Buttons,
    Recipes,
    Conditions,
}

const BANKS: [(Bank, u32, usize); 17] = [
    (Bank::Frames, 0x80234260, 0x2c20),
    (Bank::Plain, 0x80231720, 0x240),
    (Bank::Alternate, 0x80236e80, 0x2500),
    (Bank::Patterns, 0x80231960, 0x2900),
    (Bank::Cursor, 0x80249500, 0x280),
    (Bank::Symbols, 0x80249780, 0x16a0),
    (Bank::WorldMaps, 0x8024e2a0, 0x1b0e0),
    (Bank::Portraits, 0x8023d3e0, 0x49a0),
    (Bank::Technique, 0x8024ba00, 0x28a0),
    (Bank::Strategy, 0x80241d80, 0x15c0),
    (Bank::Elements, 0x8024ae20, 0xbe0),
    (Bank::Numbers, 0x8026a1c0, 0xcc0),
    (Bank::Items, 0x80243340, 0x4a00),
    (Bank::ItemTabs, 0x80247d40, 0x1a40),
    (Bank::Buttons, 0x80239380, 0x4060),
    (Bank::Recipes, 0x80212060, 0x3860),
    (Bank::Conditions, 0x80269380, 0xe40),
];

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct TextureBank {
    pub address: u32,
    pub length: usize,
}

#[derive(Serialize, Deserialize)]
pub(super) struct Recipe {
    pub(super) banks: BTreeMap<Bank, TextureBank>,
    pub(super) portraits: String,
    pub(super) fill: [u8; 4],
    pub(super) popup_fill: [u8; 4],
    pub(super) shade: [[u8; 4]; 2],
    pub(super) palette: Vec<[u8; 4]>,
    pub(super) labels: BTreeMap<String, String>,
    pub(super) cursor_motion: [[f32; 2]; 2],
    pub(super) number_colors: [[[u8; 4]; 2]; 18],
    pub(super) bar_colors: [[[u8; 4]; 4]; 3],
    pub(super) names: [String; 9],
    pub(super) equipped: char,
}

pub(super) struct Source {
    banks: BTreeMap<Bank, TextureBank>,
    portraits: String,
    labels: BTreeMap<String, String>,
}

impl Source {
    pub(super) fn read(executable: &[u8]) -> Result<Self> {
        Ok(Self {
            banks: BANKS
                .into_iter()
                .map(|(bank, address, length)| {
                    crate::tpl::parse_tpl(dol::slice(executable, address, length)?)?;
                    Ok((bank, TextureBank { address, length }))
                })
                .collect::<Result<_>>()?,
            portraits: crate::all_assets::roles::status_portraits_declaration(executable)?,
            labels: [
                ("go_in", 0x8035af14),
                ("talk", 0x8035af1c),
                ("shop", 0x8035af24),
                ("examine", 0x8035af2c),
                ("go_out", 0x8035af8c),
            ]
            .into_iter()
            .map(|(key, address)| Ok((key.into(), text(executable, address)?)))
            .collect::<Result<_>>()?,
        })
    }
}

pub(super) fn assemble(
    source: &Source,
    characters: &crate::character_data::Catalogue,
    ui: &technique_ui::Catalogue,
    options: &options_ui::Catalogue,
    save: &save_menu::Catalogue,
    shop: &shop_ui::Catalogue,
    style: &ui_style::Catalogue,
) -> Result<Recipe> {
    let colors = &options.defaults.colors;
    let cursor = &style.cursor;
    let cursor_step = cursor.phase_scale.finite()? / cursor.phase_divisor.finite()?;
    let equipped = style.text(style.symbols.equipped.text);
    ensure!(equipped.len() == 1, "equipped marker is not a single glyph");
    let mut recipe = Recipe {
        banks: source.banks.clone(),
        portraits: source.portraits.clone(),
        fill: colors.menu,
        popup_fill: colors.popup,
        shade: [colors.shade_top, colors.shade_bottom],
        palette: style.palette.to_vec(),
        labels: source.labels.clone(),
        cursor_motion: [
            [cursor.amplitudes[0].finite()?, cursor_step * 2.],
            [cursor.amplitudes[1].finite()?, cursor_step],
        ],
        number_colors: style.number_colors,
        bar_colors: style.bar_colors,
        names: characters
            .definitions
            .iter()
            .take(9)
            .map(|row| row.name.clone())
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| anyhow::anyhow!("incomplete menu character names"))?,
        equipped: equipped.as_bytes()[0] as char,
    };
    use technique_ui::Destination;
    for route in &ui.routes {
        let key = match route.destination {
            Destination::Tech => "tech",
            Destination::Unison => "unison",
            Destination::Strategy => "strategy",
            Destination::Status => "status",
            Destination::Synopsis => "synopsis",
            Destination::Items => "items",
            Destination::ExSkill => "ex_skill",
            Destination::Equip => "equip",
            Destination::Cooking => "cooking",
            Destination::System => "system",
        };
        recipe
            .labels
            .insert(key.into(), ui.text(route.label).into());
    }
    for (key, reference) in [
        ("time", ui.party.time),
        ("encounter", ui.party.encounters),
        ("combo", ui.party.combo),
        ("next", ui.party.next),
        ("gald", ui.party.gald),
    ] {
        recipe.labels.insert(key.into(), ui.text(reference).into());
    }
    use save_menu::{CommonLabel, Prompt};
    for (key, label) in [
        ("save", CommonLabel::Save),
        ("load", CommonLabel::Load),
        ("empty", CommonLabel::NoData),
        ("play_time", CommonLabel::PlayTime),
        ("encounters", CommonLabel::Encounters),
        ("max_combo", CommonLabel::MaxCombo),
        ("yes", CommonLabel::Yes),
        ("no", CommonLabel::No),
    ] {
        recipe.labels.insert(key.into(), save.common(label).into());
    }
    for (prompt, texts) in save.prompts() {
        let name = match prompt {
            Prompt::ConfirmSave => "save",
            Prompt::ConfirmLoad => "load",
            Prompt::ConfirmOverwrite => "overwrite",
            _ => continue,
        };
        for (slot, text) in ["a", "b"].into_iter().zip(texts) {
            recipe
                .labels
                .insert(format!("confirm_{name}_{slot}"), text.into());
        }
    }
    recipe.labels.insert(
        "customize".into(),
        options.required(options.headings[0])?.into(),
    );
    use shop_ui::Label;
    for (key, label) in [
        ("shop_buy", Label::Buy),
        ("shop_sell", Label::Sell),
        ("shop_equip", Label::Equip),
        ("shop_exit", Label::Exit),
        ("shop_status", Label::Status),
        ("shop_empty", Label::Empty),
        ("shop_confirm", Label::Confirm),
        ("shop_yes", Label::Yes),
        ("shop_no", Label::No),
        ("shop_total", Label::Total),
        ("shop_gald", Label::Gald),
        ("shop_select", Label::SelectItem),
        ("shop_add", Label::Add),
        ("shop_reduce", Label::Reduce),
        ("shop_ok", Label::Ok),
        ("shop_info", Label::Info),
        ("shop_slash", Label::Slash),
        ("shop_thrust", Label::Thrust),
        ("shop_defense", Label::Defense),
        ("shop_accuracy", Label::Accuracy),
        ("shop_evasion", Label::Evasion),
        ("shop_intelligence", Label::Intelligence),
        ("shop_luck", Label::Luck),
        ("shop_attack", Label::Attack),
        ("shop_cannot_equip", Label::CannotEquip),
    ] {
        recipe.labels.insert(key.into(), shop.label(label)?.into());
    }
    Ok(recipe)
}

fn text(executable: &[u8], address: u32) -> Result<String> {
    let bytes = dol::slice(executable, address, 128)?;
    let end = bytes
        .iter()
        .position(|&b| b == 0)
        .context("unterminated menu label")?;
    Ok(std::str::from_utf8(&bytes[..end])?.into())
}
