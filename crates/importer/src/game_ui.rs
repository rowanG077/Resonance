//! The game tables and interface artwork, in their final library locations.
use anyhow::{Context, Result};
use std::path::Path;

pub fn cook_game_ui(source: &Path, library: &Path) -> Result<Vec<u32>> {
    // The world-map menu names field destinations from this mapping.
    let fields = crate::cook_field_directory(source, library).context("field names")?;

    crate::cook_manual(source, library).context("training manual")?;
    crate::cook_rename(source, library).context("name editor")?;
    crate::cook_options(source, library).context("customize")?;
    crate::cook_titles(source, library).context("character titles")?;
    crate::cook_ex_skills(source, library).context("EX skills")?;
    crate::cook_strategy(source, library).context("strategy")?;
    crate::cook_cooking(source, library).context("cooking")?;
    crate::cook_items(source, library).context("items")?;
    crate::cook_techniques(source, library).context("techniques")?;
    crate::cook_characters(source, library).context("characters")?;
    crate::cook_menu_catalogues(source, library).context("Grade Shop and crafting")?;
    crate::cook_menu_figurines(source, library).context("figurine catalogue")?;
    crate::cook_menu_synopsis(source, library).context("synopsis")?;
    crate::cook_menu_presentation(source, library).context("menu presentation")?;
    crate::cook_menu_world_map(source, library).context("world-map menu")?;
    crate::cook_menu_monster_book(source, library).context("monster book")?;

    crate::cook_dialogue_font(source, library).context("dialogue font")?;
    crate::cook_dialogue_art(source, library).context("dialogue artwork")?;
    crate::cook_battle_font(source, library).context("battle font")?;
    crate::cook_menu_art(source, library).context("menu artwork")?;
    crate::cook_battle_ui_portraits(source, library).context("battle portraits")?;
    crate::cook_battle_ui_hud(source, library).context("battle interface")?;
    crate::cook_boot(source, library).context("boot artwork")?;
    crate::cook_ending(source, library).context("ending artwork")?;
    crate::cook_title_screen(source, library).context("title screen")?;
    crate::cook_ui_controls(source, library).context("controller help")?;
    crate::cook_input_prompts(library).context("input button atlas")?;
    crate::cook_ui_emotions(source, library).context("emotions")?;
    crate::cook_ui_location_titles(source, library, None).context("location titles")?;
    crate::cook_credits_screen(source, library).context("credits")?;
    crate::cook_game_over_screen(source, library).context("game over")?;
    Ok(fields)
}
