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
            match self
                .resources
                .as_ref()
                .unwrap()
                .data
                .label("transform_empty")
            {
                Ok(text) => self.inventory.notice = Some(text.to_owned()),
                Err(error) => self.report_failure("Item description unavailable", error),
            }
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
        2
    }

    pub(super) fn close_transformation(&mut self) {
        let state = &mut self.inventory;
        state.focus = Focus::List;
        state.row = state.transform.original_row;
        state.first = state.transform.original_first;
        state.notice = None;
        state.transform.result = None;
        self.inventory.clamp(self.inventory_items().len());
    }

    fn transformation_message(&self, id: u16) -> anyhow::Result<String> {
        let data = &self.resources.as_ref().unwrap().data;
        let target = data.items[usize::from(id)].transforms_to;
        Ok(data
            .label("transformed")?
            .replace("%s", &data.item_text(target)?.name))
    }

    pub(super) fn preview_transformation(&mut self, id: u16) -> i16 {
        let resources = self.resources.as_ref().unwrap();
        let target = resources.data.items[usize::from(id)].transforms_to;
        let party = self.party();
        if party.items.get(&target).copied().unwrap_or(0)
            >= party.item_limit(&resources.session.items[usize::from(target)])
        {
            match resources.data.label("transform_full") {
                Ok(text) => self.inventory.notice = Some(text.to_owned()),
                Err(error) => self.report_failure("Item description unavailable", error),
            }
            return 4;
        }
        match self.transformation_message(id) {
            Ok(message) => {
                self.inventory.transform.result = Some(id);
                self.inventory.notice = Some(message);
                2
            }
            Err(error) => {
                self.report_failure("Item description unavailable", error);
                4
            }
        }
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
