//! Battle Tech uses the field menu artwork and the command's retained page.
use super::*;
use resonance_game::menu::techniques::Page;

impl Artwork {
    pub fn render_tech(
        &mut self,
        page: Page<'_>,
        tick: u32,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        self.windows.render_battle_tech(
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
