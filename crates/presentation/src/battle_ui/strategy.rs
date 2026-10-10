//! The command owns Strategy state; Candidate lends the same field menu page.
use super::*;
use resonance_game::menu::strategy::Page;

impl Artwork {
    pub fn render_strategy(
        &mut self,
        page: Page<'_>,
        tick: u32,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        self.windows.render_battle_strategy(
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
