//! The command retains the draw pose; Candidate lends the actual shared page.
use super::*;
use resonance_game::menu::unison::Page;
impl Artwork {
    pub fn render_unison(
        &mut self,
        page: Page<'_>,
        tick: u32,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        self.windows.render_battle_unison(
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
