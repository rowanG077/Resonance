use super::*;
use resonance_content::grade::UNITS_PER_GRADE;
use resonance_game::menu::grade_shop::VISIBLE;

impl Drawing<'_> {
    pub(super) fn grade_shop(&mut self, menu: &Menu) -> Result<[f32; 2]> {
        let shop = &menu.resources.as_ref().unwrap().data.grade_shop;
        let state = &menu.grade_shop;
        self.heading(&shop.labels["heading"])?;
        self.frame([16., 64., 604., 232.])?;
        self.frame([16., 302., 604., 122.])?;
        let cost = menu.grade_cost();
        self.text(
            &format!(
                "{}: {}     {}: {}",
                shop.labels["grade"],
                menu.party().grade_hundredths / UNITS_PER_GRADE,
                shop.labels["total_cost"],
                cost / UNITS_PER_GRADE
            ),
            [36., 304.],
            20.,
            GOLD,
        )?;
        let y = 68. + state.row.saturating_sub(state.first) as f32 * 28.;
        self.highlight([32., y, 568., 26.], 255);
        for row in state.first..(state.first + VISIBLE).min(shop.options.len() + 1) {
            let y = 68. + (row - state.first) as f32 * 28.;
            if let Some(purchase) = shop.options.get(row) {
                let selected = state.selected.contains(&purchase.benefit);
                self.text(if selected { "+" } else { " " }, [36., y], 20., GOLD)?;
                self.text(
                    &purchase.name,
                    [60., y],
                    20.,
                    if selected { GOLD } else { WHITE },
                )?;
                self.text(&purchase.price.to_string(), [520., y], 20., WHITE)?;
            } else {
                self.text(&shop.labels["finish"], [60., y], 20., WHITE)?;
            }
        }
        if state.first > 0 {
            self.scroll_arrow(SCROLL_UP, [600., 68.])?;
        }
        if state.first + VISIBLE <= shop.options.len() {
            self.scroll_arrow(SCROLL_DOWN, [600., 276.])?;
        }
        if let Some(yes) = state.confirmation {
            for (row, line) in self
                .wrap_notice(&shop.labels["confirmation"].replace('\n', " "), 20., 568.)?
                .iter()
                .enumerate()
            {
                self.text(line, [36., 338. + row as f32 * 24.], 20., WHITE)?;
            }
            let x = if yes { 200. } else { 360. };
            self.highlight([x - 8., 394., 120., 26.], 255);
            self.text(&shop.labels["yes"], [200., 394.], 20., WHITE)?;
            self.text(&shop.labels["no"], [360., 394.], 20., WHITE)?;
            Ok([x, 402.])
        } else {
            if let Some(purchase) = shop.options.get(state.row) {
                for (row, line) in self
                    .wrap_notice(&purchase.description, 20., 568.)?
                    .iter()
                    .enumerate()
                {
                    self.text(line, [36., 338. + row as f32 * 24.], 20., WHITE)?;
                }
            }
            Ok([32., y + 8.])
        }
    }
}
