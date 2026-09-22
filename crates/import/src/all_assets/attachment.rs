//! Physical attachment UV and trail settings.
use crate::read::u16 as half;
use anyhow::{Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum AttachmentUv {
    Disabled,
    Frames,
    ScrollV,
    Reserved,
    Sequence,
    ScrollU,
    Inactive(i8),
}

pub(super) fn decode(bytes: &[u8]) -> Result<Value> {
    ensure!(bytes.len() == 64, "invalid attachment settings size");
    // The two scroll accumulators share storage with the frame sequence.
    // Preserve every slot, including values beyond the active sequence length.
    let sequence_length = usize::from(bytes[11]);
    ensure!(
        !(0..2).any(|channel| bytes[channel] != 0 && bytes[4 + channel] == 4)
            || (1..=36).contains(&sequence_length),
        "active attachment sequence must contain 1..=36 frames"
    );
    let channels = (0..2)
        .map(|index| -> Result<_> {
            let mode = match bytes[4 + index] {
                0 => AttachmentUv::Disabled,
                1 => AttachmentUv::Frames,
                2 => AttachmentUv::ScrollV,
                3 => AttachmentUv::Reserved,
                4 => AttachmentUv::Sequence,
                5 => AttachmentUv::ScrollU,
                other => AttachmentUv::Inactive(other as i8),
            };
            Ok(json!({
                "count": bytes[index], "texture": bytes[2 + index] as i8,
                "mode": mode, "frames_or_step": bytes[6 + index] as i8,
                "period": bytes[8 + index] as i8,
                "initial_scroll": half(bytes, 28 + index * 2)? as i16,
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({
        "uv_channels": channels, "frame_order": &bytes[28..],
        "sequence_length": sequence_length,
        "unused_storage": [{"offset": 12, "bytes": &bytes[12..13]}],
        "effect_interval": bytes[10],
        "trail": {
            "texture": bytes[13] as i8, "palette": bytes[14],
            "flags": bytes[15], "color": &bytes[16..20],
            "uv": [half(bytes,20)? as i16, half(bytes,22)? as i16,
                half(bytes,24)? as i16, half(bytes,26)? as i16]
        }
    }))
}
