//! Decode authored UV motion for title lighting and battle arenas.
mod arena;
mod settings;
mod title;
#[cfg(test)]
use anyhow::Result;
#[cfg(test)]
pub(crate) use arena::{Layer as ArenaUvLayer, read as arena_records};
#[cfg(test)]
use resonance_content::battle::visual::ArenaUvChannel;
pub(crate) use settings::ArenaSettings;
pub(crate) use title::{bind as bind_title, cook as cook_title};

/// Four fixed arena layers each contain up to four 40-byte UV channels.
#[cfg(test)]
pub(crate) fn arena_layer(bytes: &[u8], layer: usize) -> Result<Vec<ArenaUvChannel>> {
    arena_records(bytes, layer)?.channels()
}

#[cfg(test)]
mod tests {
    use super::*;
    use resonance_content::battle::visual::ArenaUvMode;

    #[test]
    fn arena_rows_preserve_disabled_channels_duplicate_order_and_inactive_parameters() {
        let mut bytes = [0u8; 800];
        let start = 44 + 2 * 168;
        bytes[start + 165] = 3;
        for (index, mode) in [0, 1, 3].into_iter().enumerate() {
            let row = &mut bytes[start + index * 40..start + (index + 1) * 40];
            row[..4].copy_from_slice(&[mode, 3, 2, 4]);
            row[20..22].copy_from_slice(&[255, 254]);
            for (at, value) in [(4, 8f32), (8, -2.), (12, 90.), (24, 3.), (32, 270.)] {
                row[at..at + 4].copy_from_slice(&value.to_be_bytes());
            }
        }
        let channels = arena_layer(&bytes, 2).unwrap();
        assert_eq!(
            channels.iter().map(|c| c.mode).collect::<Vec<_>>(),
            [
                ArenaUvMode::Disabled,
                ArenaUvMode::Frames,
                ArenaUvMode::Oscillate
            ]
        );
        for channel in &channels {
            assert_eq!(channel.texture, 3);
            assert_eq!(channel.speed, [8., -2.]);
            assert_eq!(channel.initial_tick, 255);
            assert_eq!(channel.initial_frame, 254);
            assert_eq!(channel.initial_offset, [3., 0.]);
            assert_eq!(channel.initial_angle, [270., 0.]);
        }
        assert!(arena_layer(&bytes[..start + 167], 2).is_err());
        bytes[start + 165] = 5;
        assert!(arena_layer(&bytes, 2).is_err());
        bytes[start + 165] = 3;
        bytes[start] = 4;
        assert_eq!(
            arena_layer(&bytes, 2).unwrap()[0].mode,
            ArenaUvMode::Disabled
        );
        bytes[start] = 0;
        bytes[start + 43] = 0;
        assert!(arena_layer(&bytes, 2).is_err());
    }
}
