use super::*;
use anyhow::ensure;
use resonance_game::menu::unison::{Focus, Page as UnisonPage, Unison, VISIBLE_TECHNIQUES};

fn loadout_cursor(state: &Unison) -> [f32; 2] {
    [32., 88. + state.character as f32 * 54.]
}
fn main_anchor(state: &Unison) -> Result<[f32; 2]> {
    ensure!(
        state.character < 4 && state.first <= state.row,
        "invalid Unison loadout pose"
    );
    Ok(if state.focus == Focus::Loadout {
        loadout_cursor(state)
    } else {
        [280., 92. + (state.row - state.first) as f32 * 26.]
    })
}
impl Drawing<'_> {
    pub(super) fn unison(
        &mut self,
        page: UnisonPage<'_>,
        _at_save_point: bool,
    ) -> Result<[f32; 2]> {
        use resonance_content::input::StandardAction as A;
        self.opacity = 255;
        self.offset = [0.; 2];
        let title = page
            .data
            .presentation
            .labels
            .get("unison_title")
            .context("Unison title was not prepared")?;
        self.heading(&format!("{title} Loadout"))?;
        self.framed([16., 60., 604., 242.], true)?;
        for (index, &id) in page
            .party
            .formation
            .iter()
            .take(page.party_count())
            .enumerate()
        {
            let member = usize::from(id - 1);
            let y = 72. + index as f32 * 54.;
            if index == page.state.character {
                self.highlight(
                    [
                        28.,
                        y,
                        if page.state.focus == Focus::List {
                            216.
                        } else {
                            580.
                        },
                        44.,
                    ],
                    if page.state.focus == Focus::Loadout {
                        255
                    } else {
                        127
                    },
                );
            }
            self.fitted_text(
                page.character_name(member)
                    .context("Unison character name is missing")?,
                [40., y + 8.],
                [20., 24.],
                204.,
                GOLD,
            )?;
            if page.state.focus == Focus::Loadout {
                let name = page.party.members[member]
                    .unison_arte
                    .map(|id| page.data.technique_text(id).map(|t| t.name.as_str()))
                    .transpose()?
                    .unwrap_or("-");
                self.fitted_text(name, [272., y + 8.], [20., 24.], 328., WHITE)?;
            }
        }
        self.framed([16., 312., 604., 116.], true)?;
        let map = &page.party.settings.preferences.keymap;
        for (action, label, x, right) in [
            (
                A::Confirm,
                if page.state.focus == Focus::List {
                    "Assign"
                } else {
                    "Select arte"
                },
                28.,
                222.,
            ),
            (A::Alternate, "Clear", 254., 420.),
            (A::Cancel, "Back", 474., 608.),
        ] {
            if action != A::Alternate || page.state.focus == Focus::Loadout {
                self.input_hint(map, action, label, [x, 326.], right)?;
            }
        }
        if let Some(selected) = page.selection() {
            let description = &page.data.technique_text(selected.technique)?.description;
            for (line, text) in self
                .wrap_notice(description, 16., 576.)?
                .iter()
                .take(2)
                .enumerate()
            {
                self.text_size(text, [28., 364. + line as f32 * 22.], [16., 20.], WHITE)?;
            }
        } else {
            self.text_size(
                "Select one Unison arte for this character.",
                [28., 364.],
                [16., 20.],
                WHITE,
            )?;
        }
        if page.state.focus == Focus::List {
            self.plane += 2;
            self.shade([260., 64., 612., 302.]);
            self.frame([260., 64., 352., 238.])?;
            let choices = page.techniques();
            self.text_size(
                &format!("{}/{}", page.state.row + 1, choices.len()),
                [276., 66.],
                [12., 16.],
                GOLD,
            )?;
            for (offset, &id) in choices
                .iter()
                .skip(page.state.first)
                .take(VISIBLE_TECHNIQUES)
                .enumerate()
            {
                let y = 84. + offset as f32 * 26.;
                if page.state.first + offset == page.state.row {
                    self.highlight([272., y, 328., 24.], 255);
                }
                self.fitted_text(
                    &page.data.technique_text(id)?.name,
                    [288., y],
                    [18., 22.],
                    312.,
                    WHITE,
                )?;
            }
            if page.state.first > 0 {
                self.scroll_arrow(SCROLL_UP, [580., 58.])?;
            }
            if page.state.first + VISIBLE_TECHNIQUES < choices.len() {
                self.scroll_arrow(SCROLL_DOWN, [580., 286.])?;
            }
        }
        main_anchor(page.state)
    }
}
impl MenuArtwork {
    #[allow(clippy::too_many_arguments)] // One borrowed page and its prepared drawing resources.
    pub(crate) fn render_battle_unison(
        &mut self,
        page: UnisonPage<'_>,
        font: &BitmapFont,
        dialogue: &DialogueArt,
        preferences: &resonance_content::menu_data::CustomizeSettings,
        tick: u32,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        self.set_page(Some(ActivePage::Menu(Page::Unison)));
        let mut draw = self.begin_drawing(font, dialogue, Some(preferences), tick)?;
        draw.opacity = 255;
        // A field caller already owns its background.
        draw.shade(draw.screen);
        draw.plane = 1;
        draw.unison_page(page)?;
        let trail = draw.trail;
        let drawing = draw.batches;
        self.trail = trail;
        self.submit_drawing(drawing, commands, meshes)
    }
}
impl Drawing<'_> {
    fn unison_page(&mut self, page: UnisonPage<'_>) -> Result<()> {
        let anchor = self.unison(page, false)?;
        self.cursor(anchor, 255);
        Ok(())
    }
}
#[cfg(test)]
#[path = "unison_tests.rs"]
mod tests;
