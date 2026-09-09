//! Bounded single-voice macro rendering, with separate dry and auxiliary buses.
//! Source mode 2 advances the decoded PCM directly. Other source modes and
//! unsupported commands fail explicitly while the remaining renderer is ported.
use crate::{
    bank::{Bank, ObjectKind},
    control::{self, Combine, Term},
    envelope::{Envelope, Parameters},
    mix::{self, Tables},
    read,
};
use anyhow::{Context, Result, bail, ensure};

pub const SYNTHESIS_RATE: u32 = 32000;
pub const PLAYBACK_RATE: u32 = 32028;

pub struct VoiceBuses {
    /// Direct, auxiliary A, auxiliary B; each bus is interleaved stereo PCM16.
    pub buses: [Vec<i16>; 3],
    pub macro_id: u16,
    pub samples: Vec<u16>,
    /// Linear-envelope PCM and five-ms controls for runtime category fades.
    pub pcm: Vec<i16>,
    pub controls: Vec<resonance_audio::cue::Control>,
}

#[derive(Clone, Copy)]
struct VolumeRamp {
    end: i32,
    step: i32,
}

pub fn render_voice_buses(
    bank: &Bank<'_>,
    sound_id: u16,
    tables: &Tables,
    max_frames: u32,
) -> Result<VoiceBuses> {
    ensure!(
        (1..=SYNTHESIS_RATE * 10).contains(&max_frames),
        "voice render limit must be 1 sample to 10 seconds"
    );
    tables.validate()?;
    let sound = bank.sound(sound_id)?;
    ensure!(
        sound.object < 0x4000,
        "keymap/layer dispatch is not implemented yet"
    );
    let program = bank.object(ObjectKind::Macro, sound.object)?;
    ensure!(program.len().is_multiple_of(8), "unaligned sound macro");
    let mut result = VoiceBuses {
        buses: Default::default(),
        macro_id: sound.object,
        samples: Vec::new(),
        pcm: Vec::new(),
        controls: Vec::new(),
    };
    let mut pc = 0;
    let mut frame = 0u32;
    let mut wait_until = 0;
    let mut wait_sample_end = false;
    let mut source_mode = 0;
    let mut sample = Vec::new();
    let mut sample_at = 0;
    let mut volume = u32::from(sound.volume) << 16;
    let initial_volume = volume;
    let mut ramp: Option<VolumeRamp> = None;
    let mut post = [0; 2];
    let mut steps = 0;
    let mut gains: Option<[[mix::GainRamp; 2]; 3]> = None;
    let mut envelope_parameters = Parameters::default();
    let mut envelope = Envelope::new(envelope_parameters);
    let volume_control = control::evaluate(&[
        Term {
            value: 127 << 7,
            signed: false,
            scale: 65536,
            combine: Combine::Set,
        },
        Term {
            value: 127 << 7,
            signed: false,
            scale: 65536,
            combine: Combine::Multiply,
        },
    ])?;
    loop {
        while frame >= wait_until || (wait_sample_end && sample_at >= sample.len()) {
            steps += 1;
            ensure!(steps <= 65536, "sound macro instruction budget exhausted");
            let instruction = read::slice(program, pc, 8)
                .with_context(|| format!("macro {} at step {}", sound.object, pc / 8))?;
            let a = read::u32(instruction, 0)?;
            let b = read::u32(instruction, 4)?;
            pc += 8;
            match a as u8 {
                0 => return Ok(result),
                0x31 | 0x38 => {} // Voice-allocation age does not change this isolated voice.
                0x5a => {
                    source_mode = (a >> 8) as u8;
                    ensure!(
                        source_mode == 2,
                        "source mode {source_mode} is not implemented yet"
                    );
                }
                0x0c => {
                    ensure!(a >> 24 == 0, "DLS ADSR is not implemented yet");
                    envelope_parameters = crate::parameters::ordinary(
                        bank.object(ObjectKind::Table, (a >> 8) as u16)?,
                    )?;
                    envelope = Envelope::new(envelope_parameters);
                }
                0x19 => {
                    // Source mode 2 consumes PCM directly, without pitch conversion.
                    // SetNote also embeds a wait; this path accepts only zero duration.
                    ensure!(
                        source_mode == 2 && b >> 16 == 0,
                        "pitched sources and SetNote waits are not implemented yet"
                    );
                }
                0x0d => {
                    let curve = ((a >> 24) | ((b & 255) << 8)) as u16;
                    ensure!(curve == u16::MAX, "volume curves are not implemented yet");
                    let start = if (b >> 8) as u8 != 0 {
                        initial_volume
                    } else {
                        volume
                    };
                    volume = (start * ((a >> 8) & 255) / 127 + (a & 0x00ff0000)).min(127 << 16);
                }
                0x4c => {
                    // An isolated voice starts with zeroed control variables.
                    ensure!(
                        (b >> 8) as u8 == 1 && (b & 255) == 0,
                        "only variable Set selectors are implemented yet"
                    );
                    let coarse = (a >> 16) as i16;
                    let fine = (b >> 16) as i8;
                    let scale = i32::from(coarse) * 65536 / 100 + i32::from(fine) * 65536 / 10000;
                    ensure!(
                        scale == 0,
                        "nonzero variable selectors need shared variable state"
                    );
                    post[1] = control::evaluate(&[Term {
                        value: 0,
                        signed: true,
                        scale,
                        combine: Combine::Set,
                    }])?;
                }
                0x10 => {
                    ensure!(
                        source_mode == 2,
                        "sound must select a supported source mode before starting a sample"
                    );
                    ensure!(
                        (a >> 24) == 0 && b == 0,
                        "sample offsets are not implemented yet"
                    );
                    let id = (a >> 8) as u16;
                    let decoded = bank.sample(id)?;
                    ensure!(
                        decoded.rate == SYNTHESIS_RATE as u16 && decoded.loop_length == 0,
                        "only non-looping 32 kHz samples are supported yet"
                    );
                    result.samples.push(id);
                    sample = decoded.pcm;
                    sample_at = 0;
                    envelope = Envelope::new(envelope_parameters);
                }
                0x07 => {
                    ensure!(
                        (a >> 16) as u8 == 0 && (b & 255) == 0,
                        "random/absolute waits are not implemented yet"
                    );
                    let duration = b >> 16;
                    wait_until = frame
                        .checked_add(duration * 32)
                        .context("sound wait overflow")?;
                    wait_sample_end = (a >> 24) != 0;
                    ensure!(
                        duration != 65535 || wait_sample_end,
                        "unbounded sound wait without an end event"
                    );
                    if duration == 65535 {
                        wait_until = u32::MAX;
                    }
                }
                0x0f => {
                    let curve = ((a >> 24) | ((b & 255) << 8)) as u16;
                    ensure!(
                        curve == u16::MAX && (b >> 8) as u8 == 1,
                        "only linear millisecond volume envelopes are supported yet"
                    );
                    let end = (((volume * ((a >> 8) & 255)) >> 7) + (a & 0x00ff0000)).min(127 << 16)
                        as i32;
                    ramp = Some(VolumeRamp {
                        end,
                        step: (end - volume as i32) / (b >> 16).max(1) as i32,
                    });
                }
                0x11 => {
                    sample.clear();
                    sample_at = 0;
                }
                opcode => bail!(
                    "macro {} step {}: unsupported opcode {opcode:#04x}",
                    sound.object,
                    pc / 8 - 1
                ),
            }
            if matches!(a as u8, 0x07)
                && frame < wait_until
                && !(wait_sample_end && sample_at >= sample.len())
            {
                break;
            }
        }
        ensure!(frame < max_frames, "sound exceeded its render limit");
        if frame.is_multiple_of(160) {
            if let Some(active) = ramp {
                let next = volume as i32 + active.step * 5;
                volume = if active.step < 0 {
                    next.max(active.end)
                } else {
                    next.min(active.end)
                } as u32;
                if volume as i32 == active.end {
                    ramp = None;
                }
            }
            let targets = tables.gains(volume, volume_control, sound.pan, post);
            result.controls.push(resonance_audio::cue::Control {
                volume,
                controller: volume_control,
                pan: sound.pan,
                post,
            });
            if let Some(gains) = &mut gains {
                for (bus, target) in gains.iter_mut().zip(targets) {
                    for (channel, target) in bus.iter_mut().zip(target) {
                        channel.set_target(target);
                    }
                }
            } else {
                gains = Some(targets.map(|bus| bus.map(mix::GainRamp::new)));
            }
        }
        let pcm = sample.get(sample_at).copied().unwrap_or(0);
        sample_at += usize::from(sample_at < sample.len());
        let envelope_gain = envelope.next_gain();
        result
            .pcm
            .push(((i32::from(pcm) * i32::from(envelope_gain)) >> 15) as i16);
        for (bus, gains) in result
            .buses
            .iter_mut()
            .zip(gains.as_mut().expect("first block initializes gains"))
        {
            for gain in gains {
                bus.push(mix::apply(pcm, envelope_gain, gain.next_gain()));
            }
        }
        frame += 1;
    }
}
