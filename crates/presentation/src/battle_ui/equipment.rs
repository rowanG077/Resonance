//! Battle Equipment uses the field menu's prepared page artwork.
use super::*;
use resonance_game::menu::equipment::Page;

impl Artwork {
    pub fn render_equipment(
        &mut self,
        page: Page<'_>,
        tick: u32,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        self.windows.render_battle_equipment(
            page,
            &self.font,
            &self.dialogue,
            &self.settings,
            tick,
            commands,
            meshes,
        )
    }
}
