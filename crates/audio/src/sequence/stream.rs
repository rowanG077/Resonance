//! Owned, resumable score rendering. There are no worker threads or channels.
use super::{BusFrame, ClockStart, LiveControls, kernel::Kernel};
use crate::package::Loaded;
use anyhow::{Context, Result, ensure};
use std::sync::Arc;

self_cell::self_cell!(
    pub(super) struct Owned {
        owner: Arc<Loaded>,
        #[not_covariant]
        dependent: Kernel,
    }
);

pub struct Stream {
    state: Option<Owned>,
    shared: Option<super::shared::Stream>,
}
impl Stream {
    pub fn new(loaded: Arc<Loaded>, looping: bool) -> Result<Self> {
        Self::with_clock(loaded, looping, ClockStart::Running)
    }
    pub fn cold(loaded: Arc<Loaded>, looping: bool) -> Result<Self> {
        Self::with_clock(loaded, looping, ClockStart::Cold)
    }
    pub fn in_synthesizer(
        loaded: Arc<Loaded>,
        looping: bool,
        synth: &super::shared::Synthesizer,
    ) -> Result<Self> {
        Ok(Self {
            state: None,
            shared: Some(synth.start(loaded, looping)?),
        })
    }
    /// Pause musical time and retire held notes on the next mixer block.
    /// Resuming preserves the event cursor, controllers, tempo and shared RNG.
    pub fn pause(&mut self, paused: bool) -> Result<()> {
        if let Some(shared) = &self.shared {
            shared.pause(paused)
        } else {
            let state = self.state.as_mut().context("score is absent")?;
            ensure!(
                state.borrow_owner().score.origin == crate::data::ScoreOrigin::Sequence,
                "only a sequence can be paused"
            );
            state.with_dependent_mut(|_, kernel| kernel.pause(paused));
            Ok(())
        }
    }
    pub fn is_shared(&self) -> bool {
        self.shared.is_some()
    }
    pub fn started(&self) -> bool {
        self.shared.as_ref().is_none_or(|s| s.started())
    }
    pub fn shared_control_boundary(&self) -> bool {
        self.shared.as_ref().is_some_and(|s| s.control_boundary())
    }
    pub fn set_shared_controls(&self, controls: [LiveControls; 5]) -> Result<()> {
        if let Some(shared) = &self.shared {
            shared.controls(controls)?;
        }
        Ok(())
    }
    pub fn shared_frame(&self) -> Result<Option<BusFrame>> {
        Ok(self
            .shared
            .as_ref()
            .context("stream is not shared")?
            .frame())
    }
    fn with_clock(loaded: Arc<Loaded>, looping: bool, clock: ClockStart) -> Result<Self> {
        ensure!(
            !super::shared::requires_shared(&loaded.resources),
            "cue requires shared synthesizer state; use Stream::in_synthesizer"
        );
        Ok(Self {
            shared: None,
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
        validate_controls(controls)?;
        ensure!(
            self.shared.is_none(),
            "shared cues must use the synthesizer frame clock"
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
        self.shared.take();
        Ok(())
    }
}

pub(super) fn validate_controls(controls: [LiveControls; 5]) -> Result<()> {
    ensure!(
        controls.iter().all(|c| c.volume.is_finite()
            && (0.0..=1.0).contains(&c.volume)
            && c.pan.is_none_or(|p| p < 128)),
        "invalid stream controls"
    );
    Ok(())
}
