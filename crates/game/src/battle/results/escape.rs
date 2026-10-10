use super::*;

impl Candidate {
    pub(in crate::battle) fn toggle_escape(
        &mut self,
        battle: &mut Battle,
        actor: ActorId,
    ) -> Result<bool> {
        battle.validate_escape_toggle(actor)?;
        self.sync_party(battle)?;
        battle.toggle_escape(actor)
    }

    pub(in crate::battle) fn sync_escape_history(&mut self, battle: &Battle) -> Result<()> {
        let Some(leader) = battle.ledger().ordinary_escape else {
            return Ok(());
        };
        if self.recorded_ordinary_escape {
            return Ok(());
        }
        let character = self
            .setup
            .actors
            .iter()
            .find_map(|&(id, character)| (id == leader).then_some(character))
            .context("ordinary escape leader is not a prepared party member")?;
        ensure!(
            (1..=9).contains(&character),
            "invalid escape leader character"
        );
        self.party.battles.record_ordinary_escape(character);
        self.recorded_ordinary_escape = true;
        Ok(())
    }
}
