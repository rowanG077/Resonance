//! A fixed-angle formation view fitted to the party's current bounds.
use super::{Bounds, Camera, FRAMING_MARGIN, MINIMUM_RADIUS, bounds_center};
use crate::{Actor, Battle, BattlePhase, Side};
use anyhow::{Result, ensure};

impl Battle {
    /// Keep every party member visible in one centered row, in formation order.
    pub fn arrange_result_actors(&mut self) -> Result<()> {
        let count = self
            .actors
            .iter()
            .filter(|actor| actor.side == Side::Party)
            .count();
        ensure!(
            self.phase() == BattlePhase::Results && count != 0,
            "invalid result formation"
        );
        let spacing = self
            .actors
            .iter()
            .filter(|actor| actor.side == Side::Party)
            .map(|actor| {
                let bounds = Bounds::actor(actor);
                (bounds.maximum[0] - bounds.minimum[0]).max(bounds.maximum[2] - bounds.minimum[2])
            })
            .fold(0., f32::max)
            + FRAMING_MARGIN;
        for (slot, actor) in self
            .actors
            .iter_mut()
            .filter(|actor| actor.side == Side::Party)
            .enumerate()
        {
            actor.position = [(slot as f32 - (count - 1) as f32 / 2.) * spacing, 0., 0.];
            actor.heading = 0.;
        }
        Ok(())
    }
}

impl Camera {
    pub(crate) fn frame_results(&mut self, actors: &[Actor]) {
        let subjects: Vec<_> = actors
            .iter()
            .filter(|actor| actor.side == Side::Party)
            .map(Bounds::actor)
            .collect();
        self.pose.focus = bounds_center(&subjects);
        self.pose.pitch = 0.;
        self.pose.yaw = 90.;
        self.pose.radius = self.fitted_radius(&subjects).max(MINIMUM_RADIUS);
        self.pose.place_eye();
    }
}
