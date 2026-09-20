//! Constructor metadata for all native field particle kinds.
//! These records describe initialization and callback selection, not callback execution.
use super::catalogue::{Catalogue, ImageBinding};
use crate::dol;
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Constructors {
    pub fresh_slot: FreshSlot,
    pub defaults: Defaults,
    pub palette: Vec<[u8; 4]>,
    pub callback_table: Vec<Option<Controller>>,
    pub kinds: BTreeMap<i32, Constructor>,
    /// Other kind IDs preserve fields not written by the common constructor prefix.
    pub fallback: Constructor,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct FreshSlot {
    pub blend: Blend,
    pub pass: RenderPass,
    pub billboard: bool,
    pub sequence: Option<u32>,
    pub rotation_z: f32,
    pub angular_velocity: [f32; 3],
    pub callback: Option<Controller>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Defaults {
    pub pool_capacity: u16,
    pub reuse_first_slot_on_exhaustion: bool,
    pub lifetime: i16,
    pub initial_image: ImageBinding,
    pub initial_uv_origin: [u8; 2],
    pub initial_uv_extent: [u8; 2],
    pub alpha_from_fade: bool,
}

/// Constructor writes. Optional fields preserve their previous value when absent,
/// including when allocation exhaustion reuses an occupied slot.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Constructor {
    /// Relative catalogue offset; None preserves the slot's previous sequence.
    pub sequence: Option<u32>,
    pub blend: Option<Blend>,
    pub pass: Option<RenderPass>,
    pub billboard: Option<bool>,
    /// Native size parameters, including negative values left for callers to replace.
    pub size: [f32; 2],
    /// RGB overrides preserve the alpha selected by the color argument from the palette.
    pub palette_rgb_override: Option<[u8; 3]>,
    pub rotation_z: Initializer,
    pub angular_velocity: [Initializer; 3],
    pub callback: CallbackSelection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Blend {
    Alpha,
    Additive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RenderPass {
    Ordinary,
    Refraction,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Initializer {
    Keep,
    Constant(f32),
    FrameCounterMask(u32),
    RandomMask(u32),
    FrameCounterParity {
        even: f32,
        odd: f32,
    },
    /// Convert the signed i32 third argument to f32 only when nonzero; zero keeps the field.
    NonzeroArgument,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Controller {
    FlutterAddVerticalVelocity,
    FlutterSubtractVerticalVelocity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CallbackSelection {
    Unchanged,
    Fixed(Controller),
    /// Zero keeps the callback. Otherwise `(argument as u32) << 2` wraps to the
    /// callback table's byte offset, equivalent to masking its index to 30 bits.
    NonzeroArgument,
}

// Recovered from the complete kind switch in fn_800830BC. These offsets identify
// declarative sequences, not a claim that the selected runtime controller is supported.
const SEQUENCES: [u32; 97] = [
    0x028, 0x000, 0x028, 0x000, 0x0A0, 0x0B8, 0x0B8, 0x0C4, 0x0C4, 0x358, 0x0D0, 0x0DC, 0x0E8,
    0x0F4, 0x100, 0x10C, 0x124, 0x130, 0x118, 0x118, 0x140, 0x14C, 0x158, 0x164, 0x170, 0x170,
    0x170, 0x364, 0x364, 0x370, 0x370, 0x358, 0x37C, 0x388, 0x394, 0x3A0, 0x3AC, 0x3B8, 0x3C4,
    0x3D0, 0x0B8, 0x0B8, 0x1DC, 0x214, 0x220, 0x1B8, 0x1C4, 0x1D0, 0x22C, 0x238, 0x034, 0x040,
    0x04C, 0x058, 0x064, 0x070, 0x07C, 0x088, 0x0AC, 0x0AC, 0x17C, 0x17C, 0x188, 0x188, 0x194,
    0x194, 0x1A0, 0x1A0, 0x214, 0x244, 0x1AC, 0x1F0, 0x1FC, 0x208, 0x28C, 0x250, 0x25C, 0x268,
    0x274, 0x094, 0x298, 0x2A4, 0x2B0, 0x2BC, 0x2C8, 0x2D4, 0x2E0, 0x2EC, 0x2F8, 0x304, 0x310,
    0x31C, 0x328, 0x334, 0x340, 0x34C, 0x280,
];

impl Constructors {
    pub fn read(executable: &[u8], catalogue: &Catalogue) -> Result<Self> {
        let constants = dol::slice(executable, 0x8035_C1B0, 20)?;
        let mut values = [0.; 5];
        for (value, bytes) in values.iter_mut().zip(constants.chunks_exact(4)) {
            *value = f32::from_be_bytes(bytes.try_into()?);
            ensure!(value.is_finite(), "nonfinite particle constructor constant");
        }
        let palette = dol::slice(executable, 0x8020_A240, 0x1B8)?
            .chunks_exact(4)
            .map(|bytes| bytes.try_into().map_err(Into::into))
            .collect::<Result<Vec<[u8; 4]>>>()?;
        let callback_table = dol::slice(executable, 0x8020_A80C, 17 * 4)?
            .chunks_exact(4)
            .map(|bytes| {
                Ok(match u32::from_be_bytes(bytes.try_into()?) {
                    0 => None,
                    0x8008_6CE4 => Some(Controller::FlutterAddVerticalVelocity),
                    0x8008_6FC4 => Some(Controller::FlutterSubtractVerticalVelocity),
                    address => bail!("unrecovered particle callback {address:#x}"),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let (kinds, fallback) = bindings(catalogue, values)?;
        Ok(Self {
            fresh_slot: FreshSlot {
                blend: Blend::Alpha,
                pass: RenderPass::Ordinary,
                billboard: true,
                sequence: None,
                rotation_z: 0.,
                angular_velocity: [0.; 3],
                callback: None,
            },
            defaults: Defaults {
                pool_capacity: 0x800,
                reuse_first_slot_on_exhaustion: true,
                lifetime: 20,
                initial_image: ImageBinding::Shared(0),
                initial_uv_origin: [0, 64],
                initial_uv_extent: [64, 64],
                alpha_from_fade: true,
            },
            palette,
            callback_table,
            kinds,
            fallback,
        })
    }
}

fn bindings(
    catalogue: &Catalogue,
    [size, rotation, narrow_width, tall_height, short_height]: [f32; 5],
) -> Result<(BTreeMap<i32, Constructor>, Constructor)> {
    let fallback = Constructor {
        sequence: None,
        blend: None,
        pass: None,
        billboard: None,
        size: [size; 2],
        palette_rgb_override: None,
        rotation_z: Initializer::Keep,
        angular_velocity: [Initializer::Keep; 3],
        callback: CallbackSelection::Unchanged,
    };
    let mut kinds = BTreeMap::new();
    for (kind, offset) in SEQUENCES.into_iter().enumerate() {
        catalogue.at_offset(offset)?;
        let mut entry = fallback.clone();
        entry.sequence = Some(offset);
        if !matches!(kind, 0 | 1 | 9 | 25 | 27..=39 | 50..=57 | 79) {
            entry.blend = Some(Blend::Additive);
        } else if kind == 25 {
            entry.blend = Some(Blend::Alpha);
        }
        if matches!(kind, 9 | 27..=31) {
            entry.pass = Some(RenderPass::Refraction);
        }
        if matches!(
            kind,
            6 | 24..=26 | 28 | 30 | 31 | 41 | 43 | 59 | 61 | 63 | 65 | 67
        ) {
            entry.billboard = Some(false);
        }
        match kind {
            23 => entry.size = [narrow_width, tall_height],
            24..=26 => entry.size = [narrow_width, short_height],
            _ => {}
        }
        if matches!(kind, 2 | 3) {
            entry.palette_rgb_override = Some([255, 10, 10]);
        }
        entry.rotation_z = match kind {
            1 | 3 | 8 => Initializer::FrameCounterMask(0x7F),
            7 => Initializer::Constant(rotation),
            15..=17 => Initializer::RandomMask(0x7F),
            _ => Initializer::Keep,
        };
        if matches!(kind, 0..=3) {
            entry.angular_velocity[2] = Initializer::FrameCounterParity { even: -3., odd: 3. };
        } else if matches!(kind, 8 | 9 | 11..=17 | 19..=23 | 31) {
            entry.angular_velocity[2] = Initializer::NonzeroArgument;
        } else if kind == 6 {
            entry.angular_velocity[0] = Initializer::NonzeroArgument;
        }
        entry.callback = match kind {
            24 | 25 | 43 => CallbackSelection::Fixed(Controller::FlutterSubtractVerticalVelocity),
            26 => CallbackSelection::Fixed(Controller::FlutterAddVerticalVelocity),
            32..=39 => CallbackSelection::NonzeroArgument,
            _ => CallbackSelection::Unchanged,
        };
        kinds.insert(kind as i32, entry);
    }
    Ok((kinds, fallback))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field_effects::catalogue::{EndAction, Frame, Sequence, Terminator};
    use std::collections::BTreeSet;

    fn catalogue() -> Catalogue {
        Catalogue {
            entries: SEQUENCES
                .into_iter()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .map(|offset| Sequence {
                    offset,
                    dimensions: [64, 64],
                    image: ImageBinding::Shared(0),
                    frames: vec![Frame {
                        origin: [0, 0],
                        duration: 60,
                    }],
                    terminator: Terminator {
                        unused: [0, 0],
                        action: EndAction::Loop,
                    },
                })
                .collect(),
        }
    }

    #[test]
    fn shared_sequences_preserve_distinct_constructor_behavior() -> Result<()> {
        let (kinds, fallback) = bindings(&catalogue(), [64., 45., -1., 6., 3.])?;
        assert_eq!(kinds.len(), 97);
        assert_eq!(kinds[&0].sequence, kinds[&2].sequence);
        assert_eq!(kinds[&0].blend, None);
        assert_eq!(kinds[&2].blend, Some(Blend::Additive));
        assert_eq!(kinds[&2].palette_rgb_override, Some([255, 10, 10]));
        assert_eq!(kinds[&6].angular_velocity[0], Initializer::NonzeroArgument);
        assert_eq!(kinds[&8].angular_velocity[2], Initializer::NonzeroArgument);
        assert_eq!(kinds[&6].billboard, Some(false));
        assert_eq!(kinds[&23].size, [-1., 6.]);
        assert_eq!(kinds[&25].size, [-1., 3.]);
        assert_eq!(kinds[&24].callback, kinds[&25].callback);
        assert_ne!(kinds[&25].callback, kinds[&26].callback);
        assert_eq!(kinds[&25].blend, Some(Blend::Alpha));
        assert_eq!(kinds[&24].blend, Some(Blend::Additive));
        assert_eq!(kinds[&28].pass, Some(RenderPass::Refraction));
        assert_eq!(kinds[&32].callback, CallbackSelection::NonzeroArgument);
        assert!(fallback.sequence.is_none());
        assert_eq!(fallback.blend, None);
        assert_eq!(fallback.pass, None);
        assert_eq!(fallback.billboard, None);
        assert_eq!(fallback.rotation_z, Initializer::Keep);
        assert_eq!(fallback.angular_velocity, [Initializer::Keep; 3]);
        assert_eq!(fallback.callback, CallbackSelection::Unchanged);
        let json = serde_json::to_vec(&kinds)?;
        let restored: BTreeMap<i32, Constructor> = serde_json::from_slice(&json)?;
        assert_eq!(restored[&15].rotation_z, Initializer::RandomMask(0x7F));
        let mut missing = catalogue();
        missing.entries.retain(|sequence| sequence.offset != 0x170);
        assert!(bindings(&missing, [64., 45., -1., 6., 3.]).is_err());
        Ok(())
    }

    #[test]
    #[ignore = "requires both original extracted discs; read-only, no cooking or playback"]
    fn original_constructors_bind_all_sequences_on_both_discs() -> Result<()> {
        let local = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
        for disc in [1, 2] {
            let executable = std::fs::read(local.join(format!("disc{disc}/sys/main.dol")))?;
            let catalogue = Catalogue::read(&executable)?;
            let constructors = Constructors::read(&executable, &catalogue)?;
            assert_eq!(constructors.kinds.len(), 97);
            assert_eq!(constructors.palette.len(), 110);
            assert_eq!(
                constructors
                    .palette
                    .iter()
                    .flatten()
                    .copied()
                    .collect::<Vec<_>>(),
                dol::slice(&executable, 0x8020_A240, 0x1B8)?
            );
            let Initializer::Constant(rotation) = constructors.kinds[&7].rotation_z else {
                panic!("fixed particle rotation became dynamic");
            };
            let constant_bytes = [
                constructors.fallback.size[0],
                rotation,
                constructors.kinds[&23].size[0],
                constructors.kinds[&23].size[1],
                constructors.kinds[&25].size[1],
            ]
            .into_iter()
            .flat_map(f32::to_be_bytes)
            .collect::<Vec<_>>();
            assert_eq!(constant_bytes, dol::slice(&executable, 0x8035_C1B0, 20)?);
            assert_eq!(constructors.callback_table.len(), 17);
            assert_eq!(
                constructors.callback_table[0],
                Some(Controller::FlutterAddVerticalVelocity)
            );
            assert!(constructors.callback_table[1..].iter().all(Option::is_none));
            let callback_bytes = constructors
                .callback_table
                .iter()
                .flat_map(|callback| {
                    match callback {
                        None => 0_u32,
                        Some(Controller::FlutterAddVerticalVelocity) => 0x8008_6CE4,
                        Some(Controller::FlutterSubtractVerticalVelocity) => 0x8008_6FC4,
                    }
                    .to_be_bytes()
                })
                .collect::<Vec<_>>();
            assert_eq!(
                callback_bytes,
                dol::slice(&executable, 0x8020_A80C, 17 * 4)?
            );
            assert_eq!(
                constructors
                    .kinds
                    .values()
                    .filter_map(|kind| kind.sequence)
                    .collect::<BTreeSet<_>>(),
                catalogue
                    .entries
                    .iter()
                    .map(|sequence| sequence.offset)
                    .collect::<BTreeSet<_>>()
            );
            let restored: Constructors =
                serde_json::from_slice(&serde_json::to_vec(&constructors)?)?;
            assert_eq!(restored.kinds.len(), 97);
            assert_eq!(restored.kinds[&7].rotation_z, Initializer::Constant(45.));
            assert_eq!(restored.kinds[&23].size, [-1., 6.]);
        }
        Ok(())
    }
}
