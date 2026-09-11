use super::*;

impl Drawing<'_> {
    pub(super) fn figurines(&mut self, menu: &Menu) -> Result<[f32; 2]> {
        let state = &menu.figurines;
        let fade = u32::from(state.view.page_fade);
        self.opacity = 255 - state.view.page_fade;
        self.offset = [0., -((fade * 76 / 256) as f32)];
        self.heading(&menu.resources.as_ref().unwrap().data.figurines.title)?;
        let left = -((fade * 383 / 256) as f32);
        self.offset = [left, 0.];
        self.framed([32., 60., 320., 368.], true);
        let mut anchor = self.catalogue_list(
            menu.figurine_records()
                .iter()
                .map(|r| r.name.as_str())
                .collect(),
            48.,
            state.row,
            state.first,
            state.scroll,
        )?;
        anchor[0] += left;
        self.offset = [0.; 2];
        if menu.busy {
            self.preview_loading(menu)?;
        }
        self.opacity = 255;
        Ok(anchor)
    }
}
