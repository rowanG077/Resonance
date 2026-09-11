use super::*;

#[derive(Default, serde::Serialize)]
pub struct Transform {
    pub original_row: usize,
    pub original_first: usize,
    pub result: Option<u16>,
}

impl Menu {
    pub(super) fn open_transformation(&mut self, bottle: u16) -> i16 {
        self.inventory.focus = Focus::Transform(bottle);
        if self.inventory_items().is_empty() {
            self.inventory.focus = Focus::List;
            self.inventory.notice =
                Some(self.resources.as_ref().unwrap().data.labels["transform_empty"].clone());
            return 4;
        }
        let state = &mut self.inventory;
        state.transform = Transform {
            original_row: state.row,
            original_first: state.first,
            result: None,
        };
        state.row = 0;
        state.first = 0;
        state.target_closing = false;
        2
    }

    pub(super) fn close_transformation(&mut self) {
        let state = &mut self.inventory;
        state.target_closing = true;
        state.row = state.transform.original_row;
        state.first = state.transform.original_first;
        self.inventory.clamp(self.inventory_items().len());
        if self.inventory.transform.result.is_some() {
            // The result stays visible over the restored row during the closing slide.
            let id = self
                .inventory_items()
                .get(self.inventory.row)
                .copied()
                .unwrap_or(0);
            self.inventory.notice = Some(self.transformation_message(id));
        }
    }

    fn transformation_message(&self, id: u16) -> String {
        let data = &self.resources.as_ref().unwrap().data;
        let target = data.items[usize::from(id)].transforms_to;
        data.labels["transformed"].replace("%s", &data.items[usize::from(target)].name)
    }

    pub(super) fn preview_transformation(&mut self, id: u16) -> i16 {
        let resources = self.resources.as_ref().unwrap();
        let target = resources.data.items[usize::from(id)].transforms_to;
        let party = self.party();
        if party.items.get(&target).copied().unwrap_or(0)
            >= resources.session.items[usize::from(target)].stack_limit
        {
            self.inventory.notice = Some(resources.data.labels["transform_full"].clone());
            return 4;
        }
        self.inventory.transform.result = Some(id);
        self.inventory.notice = Some(self.transformation_message(id));
        2
    }

    pub(super) fn finish_transformation(&mut self, id: u16) -> Option<i16> {
        let Focus::Transform(bottle) = self.inventory.focus else {
            unreachable!("transformation result outside its picker")
        };
        let resources = self.resources.as_ref().unwrap();
        let party = &mut self.checkpoint.as_mut().unwrap().progress.party;
        let result = party
            .transform_item(&resources.session, &resources.data, bottle, id)
            .map(|changed| changed.then_some(2));
        if matches!(result, Ok(Some(_))) {
            if !party.items.contains_key(&bottle) || self.inventory_items().is_empty() {
                self.close_transformation();
            } else {
                self.inventory.transform.result = None;
                self.inventory.notice = None;
            }
        }
        self.item_result(result)
    }
}
