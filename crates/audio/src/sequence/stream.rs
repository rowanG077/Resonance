//! Owned, resumable score rendering. There are no worker threads or channels.
use super::{BusFrame, ClockStart, LiveControls, kernel::Kernel};
use crate::package::Loaded;
use anyhow::{Result, ensure};
use std::sync::Arc;

self_cell::self_cell!(
    struct Owned {
        owner: Arc<Loaded>,
        #[not_covariant]
        dependent: Kernel,
    }
);

pub struct Stream {
    state: Option<Owned>,
}
impl Stream {
    pub fn new(loaded: Arc<Loaded>, looping: bool) -> Result<Self> {
        Self::with_clock(loaded, looping, ClockStart::Running)
    }
    pub fn cold(loaded: Arc<Loaded>, looping: bool) -> Result<Self> {
        Self::with_clock(loaded, looping, ClockStart::Cold)
    }
    fn with_clock(loaded: Arc<Loaded>, looping: bool, clock: ClockStart) -> Result<Self> {
        Ok(Self {
            state: Some(Owned::try_new(loaded, |loaded| {
                Kernel::new(
                    &loaded.resources,
                    &loaded.score,
                    &loaded.tables,
                    None,
                    looping,
                    clock,
                )
            })?),
        })
    }
    pub fn block(&mut self, controls: LiveControls) -> Result<Option<Vec<BusFrame>>> {
        self.block_envelope([controls; 5])
    }
    /// Compatibility convenience for offline callers. Live mixing uses the
    /// caller-owned buffer in `render_block`.
    pub fn block_envelope(&mut self, controls: [LiveControls; 5]) -> Result<Option<Vec<BusFrame>>> {
        let mut block = [[[0; 2]; 3]; 160];
        let len = self.render_block(controls, &mut block)?;
        Ok((len > 0).then(|| block[..len].to_vec()))
    }
    pub fn render_block(
        &mut self,
        controls: [LiveControls; 5],
        output: &mut [BusFrame; 160],
    ) -> Result<usize> {
        ensure!(
            controls.iter().all(|c| c.volume.is_finite()
                && (0.0..=1.0).contains(&c.volume)
                && c.pan.is_none_or(|p| p < 128)),
            "invalid stream controls"
        );
        let Some(state) = &mut self.state else {
            return Ok(0);
        };
        state.with_dependent_mut(|_, kernel| {
            let block = kernel.next_block(|frame| {
                let input = controls[(frame % 160 / 32) as usize];
                LiveControls {
                    release: input.release && frame.is_multiple_of(160),
                    ..input
                }
            })?;
            let Some(block) = block else {
                return Ok(0);
            };
            output[..block.len()].copy_from_slice(block);
            Ok(block.len())
        })
    }
    pub fn stop(&mut self) -> Result<()> {
        self.state.take();
        Ok(())
    }
}
