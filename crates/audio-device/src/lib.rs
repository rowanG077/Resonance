//! Thin CPAL output. Stream callbacks only consume the playback ring.
use anyhow::{Context, Result, ensure};
use cpal::{
    FromSample, SampleFormat, SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use resonance_playback::{Callback, Clock, Output};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Configuration {
    pub device: String,
    pub rate: u32,
    pub channels: u16,
    pub requested_frames: Option<u32>,
    pub format: SampleFormat,
}
pub struct Device {
    device: cpal::Device,
    config: cpal::StreamConfig,
    pub description: Configuration,
}
impl Device {
    pub fn default_output() -> Result<Self> {
        Self::with_period(512)
    }
    pub fn with_period(period: u32) -> Result<Self> {
        ensure!(
            (64..=8192).contains(&period),
            "invalid audio callback period"
        );
        let device = cpal::default_host()
            .default_output_device()
            .context("no audio output device")?;
        let default = device.default_output_config()?;
        let ranges: Vec<_> = device.supported_output_configs()?.collect();
        let rate = if ranges.iter().any(|r| {
            r.channels() == 2 && r.min_sample_rate() <= 48000 && r.max_sample_rate() >= 48000
        }) {
            48000
        } else {
            default.sample_rate()
        };
        let range = ranges
            .into_iter()
            .filter(|r| {
                r.channels() == 2
                    && matches!(
                        r.sample_format(),
                        SampleFormat::F32 | SampleFormat::I16 | SampleFormat::I32
                    )
                    && r.min_sample_rate() <= rate
                    && r.max_sample_rate() >= rate
            })
            .max_by_key(|r| r.sample_format() == SampleFormat::F32);
        let supported = range.map_or(default.clone(), |r| r.with_sample_rate(rate));
        let requested = match supported.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } => Some(period.clamp(*min, *max)),
            cpal::SupportedBufferSize::Unknown => None,
        };
        let mut config = supported.config();
        config.buffer_size = requested.map_or(cpal::BufferSize::Default, cpal::BufferSize::Fixed);
        ensure!(
            config.channels > 0 && (8000..=192000).contains(&config.sample_rate),
            "unsupported device configuration"
        );
        let description = Configuration {
            device: device.id()?.to_string(),
            rate: config.sample_rate,
            channels: config.channels,
            requested_frames: requested,
            format: supported.sample_format(),
        };
        Ok(Self {
            device,
            config,
            description,
        })
    }
    pub fn output(&self) -> Arc<Output> {
        Output::new(
            Arc::new(Clock::new(self.description.rate)),
            self.description.requested_frames.unwrap_or(2048) as usize,
        )
    }
    pub fn start(&self, output: Arc<Output>, silent: bool) -> Result<Stream> {
        ensure!(
            output.ready(),
            "audio device must be primed before starting"
        );
        let stream = match self.description.format {
            SampleFormat::F32 => self.build::<f32>(output, silent),
            SampleFormat::F64 => self.build::<f64>(output, silent),
            SampleFormat::I16 => self.build::<i16>(output, silent),
            SampleFormat::I32 => self.build::<i32>(output, silent),
            SampleFormat::I64 => self.build::<i64>(output, silent),
            SampleFormat::U16 => self.build::<u16>(output, silent),
            SampleFormat::U32 => self.build::<u32>(output, silent),
            SampleFormat::U64 => self.build::<u64>(output, silent),
            SampleFormat::I8 => self.build::<i8>(output, silent),
            SampleFormat::U8 => self.build::<u8>(output, silent),
            format => anyhow::bail!("unsupported audio device sample format {format:?}"),
        }?;
        stream.play()?;
        Ok(Stream {
            _stream: stream,
            silent,
        })
    }
    fn build<T: SizedSample + FromSample<f32>>(
        &self,
        output: Arc<Output>,
        silent: bool,
    ) -> Result<cpal::Stream> {
        let errors = output.clone();
        let mut callback = Callback::new(output, silent);
        let channels = usize::from(self.config.channels);
        Ok(self.device.build_output_stream(
            self.config,
            move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
                let timestamp = info.timestamp();
                let delay = timestamp.playback.duration_since(timestamp.callback);
                callback.render(data, channels, delay, T::from_sample);
            },
            move |error| stream_error(&errors, error.kind()),
            None,
        )?)
    }
}

fn stream_error(output: &Output, kind: cpal::ErrorKind) {
    match kind {
        // CPAL keeps playing after a scheduling refusal or automatic reroute.
        cpal::ErrorKind::RealtimeDenied | cpal::ErrorKind::DeviceChanged => {}
        cpal::ErrorKind::Xrun => output.device_error(true, false),
        cpal::ErrorKind::DeviceNotAvailable | cpal::ErrorKind::StreamInvalidated => {
            output.device_error(false, true);
        }
        _ => output.device_error(false, false),
    }
}

pub struct Stream {
    _stream: cpal::Stream,
    pub silent: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output() -> Arc<Output> {
        Output::new(Arc::new(Clock::new(48000)), 512)
    }

    #[test]
    fn scheduling_and_reroute_notifications_leave_playback_healthy() {
        let output = output();
        for kind in [
            cpal::ErrorKind::RealtimeDenied,
            cpal::ErrorKind::DeviceChanged,
        ] {
            stream_error(&output, kind);
        }
        output.check().unwrap();
        assert_eq!(output.diagnostics().device_errors, 0);
        assert_eq!(output.diagnostics().device_lost, 0);
    }

    #[test]
    fn xruns_are_counted_without_interrupting_playback() {
        let output = output();
        stream_error(&output, cpal::ErrorKind::Xrun);
        output.check().unwrap();
        assert_eq!(output.diagnostics().backend_underruns, 1);
        assert_eq!(output.diagnostics().device_lost, 0);
    }

    #[test]
    fn lost_and_invalidated_devices_request_recovery() {
        for kind in [
            cpal::ErrorKind::DeviceNotAvailable,
            cpal::ErrorKind::StreamInvalidated,
        ] {
            let output = output();
            stream_error(&output, kind);
            assert!(output.check().is_err());
            assert_eq!(output.diagnostics().device_lost, 1);
        }
    }

    #[test]
    fn other_backend_errors_remain_fatal() {
        let output = output();
        stream_error(&output, cpal::ErrorKind::BackendError);
        assert!(output.check().is_err());
        assert_eq!(output.diagnostics().device_lost, 0);
    }
}
