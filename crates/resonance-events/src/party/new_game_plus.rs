use super::Party;
use anyhow::{Result, ensure};
use resonance_content::grade::{Benefit, MAX_GRADE, Shop};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct State {
    pub benefits: BTreeSet<Benefit>,
    pub purchases: BTreeSet<Benefit>,
    pub cleared: bool,
}

impl Party {
    pub fn record_clear(&mut self, shop: &Shop) -> Result<u8> {
        let previous = self.game_clears;
        if !self.new_game_plus.cleared {
            let refund = shop.cost(&self.new_game_plus.benefits)?;
            self.grade_hundredths = self.grade_hundredths.saturating_add(refund).min(MAX_GRADE);
            self.game_clears = self.game_clears.saturating_add(1).min(100);
            self.new_game_plus.cleared = true;
        }
        Ok(previous)
    }

    pub fn buy_new_game_plus(&mut self, shop: &Shop, purchases: BTreeSet<Benefit>) -> Result<()> {
        ensure!(
            self.new_game_plus.cleared,
            "Grade Shop requires a completed game"
        );
        let cost = shop.cost(&purchases)?;
        ensure!(cost <= self.grade_hundredths, "not enough Grade");
        self.grade_hundredths -= cost;
        self.new_game_plus.purchases = purchases;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::grade::Purchase;

    #[test]
    fn purchases_are_atomic_and_previous_benefits_are_refunded_once() {
        let shop = Shop {
            options: vec![Purchase {
                benefit: Benefit::Gald,
                price: 10,
                name: "Gald".into(),
                description: "Keep Gald".into(),
                excludes: vec![],
            }],
            labels: Default::default(),
        };
        let mut party = Party::new(&crate::party::tests::data(), Default::default()).unwrap();
        party.grade_hundredths = 250;
        party.new_game_plus.benefits.insert(Benefit::Gald);
        assert_eq!(party.record_clear(&shop).unwrap(), 0);
        party.record_clear(&shop).unwrap();
        assert_eq!((party.game_clears, party.grade_hundredths), (1, 1250));
        assert!(
            party
                .buy_new_game_plus(&shop, [Benefit::Tech].into())
                .is_err()
        );
        assert_eq!(party.grade_hundredths, 1250);
        party
            .buy_new_game_plus(&shop, [Benefit::Gald].into())
            .unwrap();
        assert_eq!(party.grade_hundredths, 250);
        assert!(
            party
                .buy_new_game_plus(&shop, [Benefit::Gald].into())
                .is_err()
        );
        assert_eq!(party.grade_hundredths, 250);
    }
}
