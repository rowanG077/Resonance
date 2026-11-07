//! Shared entrance recipes with native Steam audio identities resolved at cook time.
use super::*;
use resonance_content::battle_enemy::EntranceCue;

// The native battle-entry camera dispatch. AI policy selectors with a null
// entry use the ordinary encounter opening rather than custom choreography.
const CALLBACKS: [u32; 28] = [
    0, 0, 0x4016f0, 0x4062e0, 0, 0, 0x4062e0, 0, 0x4062e0, 0, 0x402900, 0, 0x402fb0, 0, 0,
    0x403900, 0, 0, 0, 0x4062e0, 0x4062e0, 0x404b30, 0x4053c0, 0x4062e0, 0x4062e0, 0, 0x4065a0, 0,
];

pub(super) fn has_callback(selector: u8) -> Result<bool> {
    Ok(*CALLBACKS
        .get(usize::from(selector))
        .context("unknown enemy entrance selector")?
        != 0)
}

pub(super) fn verify(image: &pe::Image<'_>) -> Result<()> {
    for (address, bytes) in [
        // A null entry skips custom intro setup and dispatch at both consumers.
        (0x45caae, &[0x83, 0x3c, 0x85, 0x90, 0xd1, 0x8e, 0, 0][..]),
        (0x45cdcc, &[0x39, 0x1c, 0xbd, 0x90, 0xd1, 0x8e, 0]),
        (0x45cdf3, &[0x8b, 0x14, 0x8d, 0x90, 0xd1, 0x8e, 0]),
        // Sword Dancer: feedback starts at 280 and the camera finishes at 360.
        (0x401633, &[0x3d, 0x18, 1, 0, 0][..]),
        (0x401810, &[0xb9, 0x68, 1, 0, 0]),
        // Celsius: descent poses 44/45, roar 0x578, finish at 570.
        (0x402841, &[0x6a, 0x2c]),
        (0x40286e, &[0x3d, 0x18, 1, 0, 0]),
        (0x402875, &[0x3d, 0xea, 1, 0, 0]),
        (0x402898, &[0x6a, 0x2d]),
        (0x4028d0, &[0xc7, 0x86, 0x94, 0x3a, 1, 0, 0x78, 5, 0, 0]),
        (0x402a0f, &[0xb9, 0x3a, 2, 0, 0]),
        // Efreet's authored fall uses poses 36/37, voice 0x576 and sound 0x142.
        (0x402c7f, &[0x6a, 0x24]),
        (0x402d83, &[0x6a, 0x25]),
        (0x402e25, &[0xc7, 0x86, 0x94, 0x3a, 1, 0, 0x76, 5, 0, 0]),
        (0x402db2, &[0xbf, 0x42, 1, 0, 0]),
        (0x4030b7, &[0xb9, 0x2c, 1, 0, 0]),
        // Final Mithos camera callback ends at 210 and requests the dissolve.
        (0x8ed1c0, &[0xb0, 0x2f, 0x40, 0]),
        (0x8ed1f8, &[0xa0, 0x65, 0x40, 0]),
        (0x40667f, &[0xba, 0xd2, 0, 0, 0]),
        // Kratos and Yuan introductions store their three stream identities.
        (0x404a2c, &[0xc7, 0x83, 0x94, 0x3a, 1, 0, 0x5a, 5, 0, 0]),
        (0x404a6e, &[0xc7, 0x80, 0x94, 0x3a, 1, 0, 0x5b, 5, 0, 0]),
        (0x404b07, &[0xc7, 0x83, 0x94, 0x3a, 1, 0, 0x5c, 5, 0, 0]),
        (0x405275, &[0xc7, 0x83, 0x94, 0x3a, 1, 0, 0x29, 5, 0, 0]),
        (0x4052ec, &[0xc7, 0x80, 0x94, 0x3a, 1, 0, 0x2a, 5, 0, 0]),
        (0x405385, &[0xc7, 0x83, 0x94, 0x3a, 1, 0, 0x2b, 5, 0, 0]),
        (0x8ed1e4, &[0x30, 0x4b, 0x40, 0]),
        (0x8ed1e8, &[0xc0, 0x53, 0x40, 0]),
        (0x404e40, &[0xba, 0xcc, 2, 0, 0]),
        (0x405722, &[0xba, 0x94, 2, 0, 0]),
        // The duel's positional sound wrapper forwards selector 0x13a to PHD.
        (0x40359c, &[0xc7, 0x80, 0x94, 0x3a, 1, 0, 0x33, 5, 0, 0]),
        (0x4035ea, &[0xc7, 0x80, 0x94, 0x3a, 1, 0, 0x34, 5, 0, 0]),
        (0x4037dd, &[0xba, 0x3a, 1, 0, 0]),
        (0x4037e2, &[0xe8, 0x99, 0xa9, 0x17, 0]),
        (0x57e1cd, &[0xe8, 0x0e, 0x29, 0xff, 0xff]),
        (0x570bea, &[0xe8, 0x11, 0xfd, 0, 0]),
        (0x403b0b, &[0xba, 0xae, 1, 0, 0]),
    ] {
        ensure!(
            image.slice(address, bytes.len())? == bytes,
            "Steam enemy entrance consumer changed at {address:#x}"
        );
    }
    for (index, callback) in CALLBACKS.iter().enumerate() {
        ensure!(
            pe::word(image.slice(0x8ed190 + index as u32 * 4, 4)?, 0)? == *callback,
            "Steam entrance dispatch changed at selector {index}"
        );
    }
    for (address, expected) in [
        (0x883358, 1950.),
        (0x8833f8, 1500.),
        (0x8829a8, 20.),
        (0x882b50, f64::from(0.1f32)),
    ] {
        ensure!(
            f64::from_le_bytes(image.slice(address, 8)?.try_into()?) == expected,
            "Steam final entrance camera changed at {address:#x}"
        );
    }
    Ok(())
}

pub(super) fn read(
    selector: u8,
    profile: &Profile,
    fallback: serde_json::Value,
) -> Result<Option<Opening>> {
    if !matches!(selector, 2 | 10 | 15 | 21 | 22 | 26) {
        return Ok(None);
    }
    // These recipes have no package-relative operands. Their camera,
    // movement and feedback already use the shared GC entrance contract.
    let value = resonance_importer::decode_enemy_entrance(selector, &[], &[], fallback, profile)?;
    let mut opening: Opening = serde_json::from_value(value)?;
    if selector == 10 {
        let Opening::Entrance(entrance) = &mut opening else {
            anyhow::bail!("Celsius entrance changed kind")
        };
        let sound = battle_voices::sound(0x578)?.context("missing Celsius entrance sound")?;
        let mut voices = 0;
        for (_, cue) in &mut entrance.cues {
            if let EntranceCue::Voice(voice) = cue {
                *voice = sound;
                voices += 1;
            }
        }
        ensure!(
            voices == 1,
            "Celsius entrance has an unexpected voice count"
        );
    }
    if matches!(selector, 15 | 21 | 22) {
        let Opening::Entrance(entrance) = &mut opening else {
            anyhow::bail!("conversational entrance changed kind")
        };
        let native: &[u32] = match selector {
            15 => &[0x533, 0x534],
            21 => &[0x55a, 0x55b, 0x55c],
            _ => &[0x529, 0x52a, 0x52b],
        };
        let mut voices = native.iter().copied();
        for (_, cue) in &mut entrance.cues {
            if let EntranceCue::Voice(voice) | EntranceCue::PartyVoice(voice) = cue {
                *voice =
                    battle_voices::sound(voices.next().context("unexpected introduction voice")?)?
                        .context("missing native introduction stream")?;
            }
            if selector == 15
                && let EntranceCue::Sound(sound) = cue
            {
                *sound =
                    resonance_content::battle_voice::Sound::Cue(crate::audio_effects::id(0x13a)?);
            }
        }
        ensure!(voices.next().is_none(), "introduction voice count changed");
    }
    Ok(Some(opening))
}

pub(super) fn falling(package: &[u8], metadata: &[u8], profile: &Profile) -> Result<Opening> {
    let commands = super::super::section(package, 14)?;
    let mut cursor = usize::from(pe::half(metadata, 0xf6)?) * 2;
    let mut canonical = vec![0; 0x1e8];
    canonical[..4].copy_from_slice(b"em8\0");
    canonical[14..16].copy_from_slice(&0x1e8u16.to_be_bytes());
    loop {
        let time = pe::half(commands, cursor)?;
        canonical.extend(time.to_be_bytes());
        if time == u16::MAX {
            break;
        }
        ensure!(
            pe::half(commands, cursor + 2)? == 1,
            "unsupported Steam entrance launch command"
        );
        canonical.extend(1u16.to_be_bytes());
        canonical.extend(pe::half(commands, cursor + 4)?.to_be_bytes());
        cursor += 6;
    }
    // The launch stream above starts at zero in the canonical command pool.
    let metadata = vec![0; 496];
    let value = resonance_importer::decode_enemy_entrance(
        12,
        &canonical,
        &metadata,
        serde_json::Value::Null,
        profile,
    )?;
    let mut opening: Opening = serde_json::from_value(value)?;
    let Opening::Entrance(entrance) = &mut opening else {
        anyhow::bail!("Efreet entrance changed kind")
    };
    for (_, cue) in &mut entrance.cues {
        match cue {
            EntranceCue::Voice(voice) => {
                *voice = battle_voices::sound(0x576)?.context("missing Efreet entrance voice")?
            }
            EntranceCue::Sound(sound) => {
                *sound =
                    resonance_content::battle_voice::Sound::Cue(crate::audio_effects::id(0x142)?)
            }
            _ => {}
        }
    }
    Ok(opening)
}
