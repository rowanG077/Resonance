//! Streaming writer for our fixed RGB FFV1 + stereo PCM16 FLAC Matroska profile.
//! The EBML subset follows https://www.matroska.org/technical/elements.html.
use anyhow::{Result, ensure};
use codec_ffv1::{Encoder, Frame, Plane};
use flacenc::{
    bitsink::ByteSink,
    component::{BitRepr, StreamInfo},
    error::{Verified, Verify},
    source::{Fill, FrameBuf},
};
use std::{io::Write, time::Duration};

pub const AUDIO_BLOCK: usize = 1024;

// Definite-length EBML elements; only the outer streaming Segment has unknown
// size. Each packet gets a Cluster, so block timecodes never overflow i16.
fn element(id: u32, bytes: &[u8]) -> Vec<u8> {
    let id_bytes = id.to_be_bytes();
    let start = id_bytes.iter().position(|&b| b != 0).unwrap();
    let mut output = id_bytes[start..].to_vec();
    let size = bytes.len() as u64;
    let count = (1..=8).find(|n| size < (1u64 << (7 * n)) - 1).unwrap();
    let encoded = (size | (1u64 << (7 * count))).to_be_bytes();
    output.extend_from_slice(&encoded[8 - count..]);
    output.extend_from_slice(bytes);
    output
}
fn uint(id: u32, value: u64) -> Vec<u8> {
    let bytes = value.to_be_bytes();
    element(
        id,
        &bytes[bytes.iter().position(|&b| b != 0).unwrap_or(7)..],
    )
}
fn master(id: u32, children: &[Vec<u8>]) -> Vec<u8> {
    element(id, &children.concat())
}

pub struct MovieWriter<W: Write> {
    writer: W,
    video: Encoder,
    width: u32,
    height: u32,
    audio_info: StreamInfo,
    audio_config: Verified<flacenc::config::Encoder>,
    audio_frame: FrameBuf,
    audio_blocks: usize,
    audio_frames: u64,
    audio_ended: bool,
    sample_rate: u32,
    last_timestamp: Option<u64>,
    last_video: Option<Duration>,
}
impl<W: Write> MovieWriter<W> {
    pub fn new(
        mut writer: W,
        width: u32,
        height: u32,
        frame_micros: u32,
        sample_rate: u32,
    ) -> Result<Self> {
        ensure!(
            (2..=1920).contains(&width)
                && (2..=1080).contains(&height)
                && frame_micros > 0
                && (8000..=96000).contains(&sample_rate),
            "invalid movie encoding format"
        );
        // Fixed v3 RGB8 range-coded profile: 2x2 slices, two quantization
        // tables, CRC enabled. Kept with the independent interoperability
        // fixture because the encoder's Format builder exposes only one table.
        const CONFIG: &[u8] = &[
            86, 6, 30, 47, 253, 184, 70, 215, 112, 151, 48, 1, 134, 1, 238, 225, 28, 230, 189, 69,
            77, 181, 56, 39, 116, 216, 46, 16, 125, 117, 197, 170, 227, 61, 96, 44, 216, 134, 131,
            254, 91, 71, 177, 15, 197, 94, 241, 123, 255, 129, 120, 181, 125, 193, 33, 34, 33, 190,
            25, 198, 81, 217, 49, 15, 8, 182, 127, 122, 178, 201, 220, 202, 89, 158, 199, 221, 253,
            227, 51, 7, 199, 135, 221, 238, 116, 135, 226, 101, 196, 251, 252, 212, 192, 28, 205,
            37, 228, 104, 74, 215, 14, 223, 26, 82, 53, 190, 86, 103, 254, 48, 111, 190, 150, 197,
            96, 2, 56, 76, 46, 23, 194, 51, 77, 61, 99, 160, 121, 209, 48, 10, 80, 72, 157, 186,
            180, 109, 223, 32, 76, 227, 101, 167, 162, 33, 157, 231, 153, 50, 27, 220, 158, 133,
            106, 215, 210, 209, 98, 190, 141, 127, 219, 68, 202, 6, 125, 251, 19, 122, 245, 248,
            36, 133, 190, 228, 37, 224, 120, 119, 33, 99, 242, 67, 122, 92, 90, 217, 187, 202, 246,
            66,
        ];
        let video = Encoder::from_configuration_record(CONFIG)?;
        let mut audio_info = StreamInfo::new(sample_rate as usize, 2, 16)?;
        audio_info.set_block_sizes(AUDIO_BLOCK, AUDIO_BLOCK)?;
        let mut sink = ByteSink::new();
        audio_info.write(&mut sink)?;
        let mut private = b"fLaC\x80\x00\x00\x22".to_vec();
        private.extend_from_slice(sink.as_slice());
        writer.write_all(&master(
            0x1a45dfa3,
            &[
                uint(0x4286, 1),
                uint(0x42f7, 1),
                uint(0x42f2, 4),
                uint(0x42f3, 8),
                element(0x4282, b"matroska"),
                uint(0x4287, 4),
                uint(0x4285, 2),
            ],
        ))?;
        writer.write_all(&[
            0x18, 0x53, 0x80, 0x67, 0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        ])?;
        writer.write_all(&master(
            0x1549a966,
            &[
                uint(0x2ad7b1, 1000),
                element(0x4d80, b"resonance"),
                element(0x5741, b"resonance"),
            ],
        ))?;
        writer.write_all(&master(
            0x1654ae6b,
            &[
                master(
                    0xae,
                    &[
                        uint(0xd7, 1),
                        uint(0x73c5, 1),
                        uint(0x83, 1),
                        uint(0x9c, 0),
                        element(0x86, b"V_FFV1"),
                        element(0x63a2, &video.configuration_record().unwrap()),
                        uint(0x23e383, u64::from(frame_micros) * 1000),
                        master(
                            0xe0,
                            &[
                                uint(0xb0, u64::from(width)),
                                uint(0xba, u64::from(height)),
                                master(0x55b0, &[uint(0x55b1, 0), uint(0x55b9, 2)]),
                            ],
                        ),
                    ],
                ),
                master(
                    0xae,
                    &[
                        uint(0xd7, 2),
                        uint(0x73c5, 2),
                        uint(0x83, 2),
                        uint(0x9c, 0),
                        element(0x86, b"A_FLAC"),
                        element(0x63a2, &private),
                        master(
                            0xe1,
                            &[
                                element(0xb5, &f64::from(sample_rate).to_be_bytes()),
                                uint(0x9f, 2),
                                uint(0x6264, 16),
                            ],
                        ),
                    ],
                ),
            ],
        ))?;
        Ok(Self {
            writer,
            video,
            width,
            height,
            audio_info,
            audio_config: flacenc::config::Encoder::default()
                .into_verified()
                .map_err(|(_, error)| anyhow::anyhow!("{error}"))?,
            audio_frame: FrameBuf::with_size(2, AUDIO_BLOCK)?,
            audio_blocks: 0,
            audio_frames: 0,
            audio_ended: false,
            sample_rate,
            last_timestamp: None,
            last_video: None,
        })
    }
    fn packet(&mut self, track: u8, timestamp: Duration, data: &[u8]) -> Result<()> {
        let ticks = u64::try_from(timestamp.as_micros())?;
        ensure!(
            self.last_timestamp.is_none_or(|last| ticks >= last),
            "movie packets must be interleaved in timestamp order"
        );
        let mut block = vec![0x80 | track, 0, 0, 0x80];
        block.extend_from_slice(data);
        self.writer.write_all(&master(
            0x1f43b675,
            &[uint(0xe7, ticks), element(0xa3, &block)],
        ))?;
        self.last_timestamp = Some(ticks);
        Ok(())
    }
    pub fn video(&mut self, rgb: &[u8], timestamp: Duration) -> Result<()> {
        ensure!(
            rgb.len() == self.width as usize * self.height as usize * 3,
            "invalid RGB frame size"
        );
        ensure!(
            self.last_video.is_none_or(|last| timestamp > last),
            "video timestamps must increase"
        );
        // codec_ffv1 uses planar G/B/R. Every emitted frame resets entropy state.
        let frame = Frame {
            keyframe: true,
            width: self.width,
            height: self.height,
            sample_aspect_ratio: (0, 0),
            planes: [1, 2, 0]
                .map(|c| Plane {
                    width: self.width,
                    height: self.height,
                    data: rgb.chunks_exact(3).map(|p| u16::from(p[c])).collect(),
                })
                .into(),
        };
        let packet = self
            .video
            .encode_frame(&frame, true, self.width, self.height)?;
        self.packet(1, timestamp, &packet)?;
        self.last_video = Some(timestamp);
        Ok(())
    }
    /// Full blocks except for the final block; samples are interleaved L/R.
    pub fn audio(&mut self, samples: &[i16]) -> Result<()> {
        ensure!(
            !self.audio_ended
                && !samples.is_empty()
                && samples.len().is_multiple_of(2)
                && samples.len() <= AUDIO_BLOCK * 2,
            "invalid FLAC input block"
        );
        let frames = samples.len() / 2;
        self.audio_frame
            .fill_interleaved(&samples.iter().map(|&s| i32::from(s)).collect::<Vec<_>>())
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let frame = flacenc::encode_fixed_size_frame(
            &self.audio_config,
            &self.audio_frame,
            self.audio_blocks,
            &self.audio_info,
        )
        .map_err(|error| anyhow::anyhow!("{error}"))?;
        let mut sink = ByteSink::new();
        frame.write(&mut sink)?;
        self.packet(
            2,
            Duration::from_micros(self.audio_frames * 1_000_000 / u64::from(self.sample_rate)),
            sink.as_slice(),
        )?;
        self.audio_blocks += 1;
        self.audio_frames += frames as u64;
        self.audio_ended = frames < AUDIO_BLOCK;
        Ok(())
    }
    pub fn finish(mut self) -> Result<W> {
        self.writer.flush()?;
        Ok(self.writer)
    }
}
