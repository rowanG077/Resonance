//! Shared field dependencies prepared once before any field is assembled.
use anyhow::Result;
use resonance_content::font::{BitmapFont, TextSpan};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(crate) struct Prepared {
    pub catalogue: crate::resource::Catalogue,
    pub texture_animations: BTreeMap<u32, Vec<resonance_content::field::FieldTextureAnimation>>,
    pub resource_catalogue: String,
    pub font: BitmapFont,
    pub effects: crate::field_effects::Prepared,
    pub toon_ramp: String,
    pub save_point_tutorial: Vec<TextSpan>,
    pub save_point_unlock: Vec<TextSpan>,
    pub save_point_no_gem: Vec<TextSpan>,
    pub files: BTreeSet<String>,
    pub enemy_paths: Vec<String>,
    pub equipment_paths: Vec<String>,
}

#[expect(
    clippy::too_many_arguments,
    reason = "Preparation borrows independently owned content and runtime resources."
)]
pub(crate) fn prepare(
    extracted: &Path,
    output: &Path,
    discs: &[std::path::PathBuf],
    executable: &[u8],
    catalogues: &crate::all_assets::Catalogues,
    battle_sources: &crate::source_assets::Sources,
    usual: &[u8],
    shared_effects: &crate::battle_effect::SharedPalettes,
    skits: &crate::skit::Source,
) -> Result<Prepared> {
    let dialogue = crate::font::prepare(extracted, output)?;
    let font = crate::named::Publisher::new(output, output)?.dialogue(dialogue)?;
    let screens = crate::named::Publisher::new(extracted, output)?.screens(extracted)?;
    let toon_ramp = crate::field_lighting::cook(extracted, output)?;
    let menu = crate::menu::publish(
        extracted,
        output,
        executable,
        catalogues,
        battle_sources,
        usual,
        shared_effects,
    )?;
    let weapons = crate::battle_model::weapon::prepare_source(extracted, battle_sources, output)?;
    let equipment_paths =
        crate::named::Publisher::new(output, output)?.battle_equipment(weapons)?;
    let session = crate::named::Publisher::new(output, output)?.session(
        &crate::session::read(executable, &catalogues.menu)?,
        &crate::session::read_text(executable, &catalogues.menu)?,
        "game",
    )?;
    let skits = skits.prepare(extracted, output)?;
    let skits = crate::named::Publisher::new(output, output)?.skits(skits)?;
    let effects = crate::field_effects::cook(extracted, output)?;
    let catalogue = &catalogues.resources;
    let resource_paths =
        crate::named::Publisher::new(output, output)?.resource_catalogue(discs, catalogue)?;
    let resource_catalogue = resonance_content::resource::PATH.to_owned();
    let save_point_tutorial =
        crate::font::system_text(crate::dol::slice(executable, 0x8017A274, 256)?)?;
    let (save_point_unlock, save_point_no_gem) = memory_circle_messages(
        executable,
        catalogues.menu.items[491]
            .name
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Memory Gem name is missing"))?,
    )?;
    let files = [
        resource_catalogue.clone(),
        "ui/dialogue.json".into(),
        "ui/story-subtitles.json".into(),
        "ui/menu.json".into(),
        "game/menu-data.json".into(),
        resonance_content::menu_data::MANUAL_PATH.into(),
        resonance_content::menu_data::FIGURINES_PATH.into(),
        resonance_content::menu_data::SYNOPSIS_PATH.into(),
        resonance_content::menu_data::CUSTOMIZE_PATH.into(),
        resonance_content::menu_data::RENAME_PATH.into(),
        resonance_content::battle_formation::PATH.into(),
        resonance_content::battle_ui::document::PATH.into(),
        resonance_content::battle_victory::document::GROUPS.into(),
        resonance_content::battle_effect::document::COMMON.into(),
        resonance_content::battle_effect::document::TECHNIQUES.into(),
        resonance_content::battle_effect::TINTS_PATH.into(),
        resonance_content::battle_projectile::PATH.into(),
        resonance_content::battle_recoil::PATH.into(),
        resonance_content::battle_rules::PARTY.into(),
        resonance_content::battle_defense_system::PATH.into(),
        resonance_content::arte::PATH.into(),
        resonance_content::battle_unison::PATH.into(),
        "fonts/dialogue.json".into(),
        font.texture.clone(),
        toon_ramp.clone(),
    ]
    .into_iter()
    .chain([
        resonance_content::battle_scene::document::TECHNIQUES.into(),
        resonance_content::battle_scene::document::SKILLS.into(),
    ])
    .chain(resource_paths)
    .chain(session)
    .chain(menu.paths)
    .chain(menu.enemies.iter().cloned())
    .chain(equipment_paths.iter().cloned())
    .chain(skits)
    .chain(screens)
    .chain(effects.files.iter().cloned())
    .chain(
        resonance_script_content::FILES
            .iter()
            .filter(|(path, _)| path.ends_with(".sym"))
            .map(|(path, _)| format!("scripts/{path}")),
    )
    .collect();
    let mut texture_animations = crate::texture_animation::field::profiles();
    texture_animations.extend(crate::texture_animation::title_profiles(executable)?);
    Ok(Prepared {
        catalogue: catalogue.clone(),
        texture_animations,
        resource_catalogue,
        font,
        effects,
        toon_ramp,
        save_point_tutorial,
        save_point_unlock,
        save_point_no_gem,
        files,
        enemy_paths: menu.enemies,
        equipment_paths,
    })
}

fn memory_circle_messages(executable: &[u8], item: &str) -> Result<(Vec<TextSpan>, Vec<TextSpan>)> {
    // The question starts with a system-window control, outside its visible text.
    let question = crate::font::system_text(crate::dol::slice(executable, 0x8017A31A, 32)?)?;
    let mut missing = crate::dol::slice(executable, 0x8017A3DC + 0x1C, 32)?.to_vec();
    // Restore the initial white palette after the formatted item name.
    for i in 1..missing.len() {
        if missing[i - 1] == 3 && missing[i] == b'8' {
            missing[i] = b'9';
        }
    }
    let mut missing = crate::font::system_text(&missing)?;
    for span in &mut missing {
        span.text = span.text.replace("%s", item);
    }
    Ok((question, missing))
}
