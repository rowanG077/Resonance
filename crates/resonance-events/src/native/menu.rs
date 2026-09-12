use super::*;

impl NativeHost<'_> {
    pub(super) fn request_menu(
        &mut self,
        _: NativeCall,
        args: &[i32],
        _: &mut Memory,
    ) -> Result<NativeResult, String> {
        let target = crate::menu::Target::try_from(args[0])?;
        require(self.world.menu_request.is_none(), "nested menu request")?;
        require(self.world.party.is_some(), "script menu requires a party")?;
        let data = self
            .resources
            .menu_data
            .as_ref()
            .ok_or("menu data is not cooked")?;
        if let crate::menu::Target::Shop(id) = target {
            require(
                data.world_map.shops.get(usize::from(id)).is_some(),
                "shop is not cooked",
            )?;
        }
        let operation = self.world.operations.begin()?;
        *self.wait = Some(Wait::Menu(operation.clone()));
        self.world.menu_request = Some(crate::menu::Request { target, operation });
        Ok(NativeResult::Suspend)
    }
}
