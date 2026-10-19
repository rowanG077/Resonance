//! Owned, resumable score rendering. There are no worker threads or channels.
use super::{BusFrame, LiveControls, kernel::Kernel};
use crate::{BLOCK_FRAMES, CONTROLS_PER_BLOCK, package::Loaded};
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
    output: super::shared::Stream,
    /// Standalone playback owns its clock; live playback uses the mixer's clock.
    clock: Option<super::shared::Synthesizer>,
}
impl Stream {
    pub fn new(loaded: Arc<Loaded>, looping: bool) -> Result<Self> {
        Self::standalone(loaded, looping, false)
    }
    pub(super) fn recorded(loaded: Arc<Loaded>, looping: bool) -> Result<Self> {
        Self::standalone(loaded, looping, true)
    }
    fn standalone(loaded: Arc<Loaded>, looping: bool, record: bool) -> Result<Self> {
        let clock = super::shared::Synthesizer::default();
        let output = clock.start_recorded(loaded, looping, record)?;
        Ok(Self {
            output,
            clock: Some(clock),
        })
    }
    pub(super) fn take_preview(&self) -> super::Preview {
        self.clock
            .as_ref()
            .unwrap()
            .take_preview(&self.output)
            .unwrap()
    }
    pub fn in_synthesizer(
        loaded: Arc<Loaded>,
        looping: bool,
        synth: &super::shared::Synthesizer,
    ) -> Result<Self> {
        Ok(Self {
            output: synth.start(loaded, looping)?,
            clock: None,
        })
    }
    /// Pause musical time and retire held notes on the next mixer block.
    /// Resuming preserves the event cursor, controllers, tempo and shared RNG.
    pub fn pause(&mut self, paused: bool) -> Result<()> {
        self.output.pause(paused)
    }
    pub fn started(&self) -> bool {
        self.output.started()
    }
    /// End of PCM already queued on the mixer's frame clock.
    pub fn submitted_until(&self) -> u64 {
        self.output.submitted_until()
    }
    pub fn shared_control_boundary(&self) -> bool {
        self.output.control_boundary()
    }
    pub fn set_shared_controls(&self, controls: [LiveControls; CONTROLS_PER_BLOCK]) -> Result<()> {
        self.output.controls(controls)
    }
    pub fn shared_frame(&self) -> Option<BusFrame> {
        self.output.frame()
    }
    pub fn block(&mut self, controls: LiveControls) -> Result<Option<Vec<BusFrame>>> {
        self.block_envelope([controls; CONTROLS_PER_BLOCK])
    }
    /// Offline convenience. Live mixing uses the caller-owned buffer in `render_block`.
    pub fn block_envelope(
        &mut self,
        controls: [LiveControls; CONTROLS_PER_BLOCK],
    ) -> Result<Option<Vec<BusFrame>>> {
        let mut block = [[[0; 2]; 3]; BLOCK_FRAMES];
        let len = self.render_block(controls, &mut block)?;
        Ok((len > 0).then(|| block[..len].to_vec()))
    }
    pub fn render_block(
        &mut self,
        controls: [LiveControls; CONTROLS_PER_BLOCK],
        output: &mut [BusFrame; BLOCK_FRAMES],
    ) -> Result<usize> {
        let clock = self
            .clock
            .as_ref()
            .context("shared cues must use the synthesizer frame clock")?;
        self.output.controls(controls)?;
        let mut length = 0;
        for target in output {
            clock.advance()?;
            if let Some(frame) = self.output.frame() {
                *target = frame;
                length += 1;
            } else {
                *target = [[0; 2]; 3];
            }
        }
        Ok(length)
    }
}

pub(super) fn validate_controls(controls: [LiveControls; CONTROLS_PER_BLOCK]) -> Result<()> {
    ensure!(
        controls.iter().all(|c| c.volume.is_finite()
            && (0.0..=1.0).contains(&c.volume)
            && c.pan.is_none_or(|p| p < 128)),
        "invalid stream controls"
    );
    Ok(())
}
