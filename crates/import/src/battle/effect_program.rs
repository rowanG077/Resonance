//! Convert effect programs into typed timelines and palette-resolved KTX2 atlases.
pub(super) mod actor;
pub(super) mod all;
pub(super) mod events;
pub(super) mod modifiers;
#[cfg(test)]
mod source;
pub(super) use all::SkillArchive;
#[cfg(test)]
mod air_thrust_tests;
#[cfg(test)]
mod contact_tests;
#[cfg(test)]
#[path = "effect_program/earth_field_tests.rs"]
mod earth_field_tests;
#[cfg(test)]
#[path = "effect_program/earth_small_tests.rs"]
mod earth_small_tests;
#[cfg(test)]
#[path = "effect_program/enemy156_tests.rs"]
mod enemy156_tests;
#[cfg(test)]
mod flame_lance_tests;
#[cfg(test)]
#[path = "effect_program/freeze_lancer_tests.rs"]
mod freeze_lancer_tests;
#[cfg(test)]
#[path = "effect_program/ice_tornado_tests.rs"]
mod ice_tornado_tests;
#[cfg(test)]
#[path = "effect_program/icicle_tests.rs"]
mod icicle_tests;
#[cfg(test)]
#[path = "effect_program/lightning_tests.rs"]
mod lightning_tests;
#[cfg(test)]
#[path = "effect_program/thunder_arrow_tests.rs"]
mod thunder_arrow_tests;
mod uv;
#[cfg(test)]
#[path = "effect_program/water_tests.rs"]
mod water_tests;
#[cfg(test)]
#[path = "effect_program/wind_field_tests.rs"]
mod wind_field_tests;

use super::actions::{Rel, member};
#[cfg(test)]
use crate::read::f32 as float;
use crate::{
    compression,
    read::{u16 as half, u32 as word},
    tpl,
};
use anyhow::{Context, Result, bail, ensure};
use resonance_content::{
    battle::{
        effect_program::*,
        effects::{EffectBank, EffectId},
    },
    font::UiTexture,
    menu_data::Element,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

#[cfg(test)]
#[path = "effect_program/screen_texture_tests.rs"]
mod screen_texture_tests;

pub(super) const ACTOR_BYTES: usize = 352;
const RECORD_LIMIT: usize = 512;

#[cfg(test)]
#[path = "effect_program/pow_followup_tests.rs"]
mod pow_followup_tests;

#[cfg(test)]
mod palette_tests;
#[cfg(test)]
#[path = "effect_program/particle_motion_tests.rs"]
mod particle_motion_tests;

fn actor_tail_record(record: &actor::Record, flags: u32) -> Result<Option<ScreenTexture>> {
    let p = &record.prefix;
    let actor::Body::Particle { geometry, context } = &record.body else {
        bail!("controller declaration cannot be lowered as a particle");
    };
    let geometry = geometry.as_ref();
    let context_zero = context.stored_origin.iter().all(|value| value.bits() == 0)
        && context.stored_uv_binding == 0
        && context.stored_heading.bits() == 0
        && context.stored_program_binding == 0;
    if p.resource_slot == 10 {
        ensure!(
            p.kind == 4
                && p.blend == 0
                && flags & 0x602020 == 0
                && p.uv_track == -1
                && p.additional_copies == 0,
            "unsupported screen-textured effect recipe"
        );
        let actor::GeometryOperands::Parameters {
            radius_step,
            texture_offset_words,
            storage_e0,
            stored_elevation,
            ..
        } = geometry
        else {
            bail!("screen-textured effect has incompatible geometry operands");
        };
        ensure!(
            radius_step.bits() & 0xffff == 0
                && storage_e0.iter().all(|&b| b == 0)
                && stored_elevation.bits() == 0
                && context_zero,
            "screen-textured effect contains unsupported animation or runtime state"
        );
        // Draw packets truncate both words before interpreting the bytes as signed offsets.
        Ok(Some(ScreenTexture {
            offset: texture_offset_words.map(|word| word as i8),
        }))
    } else if p.kind == 3 {
        let actor::GeometryOperands::Model {
            stored_animation_change,
            storage_d7,
            animation_state,
            stored_model_binding,
            elevation,
            ..
        } = geometry
        else {
            bail!("model effect has incompatible geometry operands");
        };
        ensure!(
            *stored_animation_change == 0
                && *storage_d7 == 0
                && animation_state.iter().all(|&b| b == 0)
                && *stored_model_binding == 0
                && context_zero,
            "model effect contains unsupported animation or runtime state"
        );
        ensure!(
            f32::from_bits(elevation.bits()).is_finite(),
            "non-finite model elevation"
        );
        Ok(None)
    } else {
        let geometry_zero = match geometry {
            actor::GeometryOperands::Parameters {
                radius_step,
                texture_offset_words,
                storage_e0,
                stored_elevation,
                ..
            } => {
                (matches!(p.kind, 8 | 10) || radius_step.bits() & 0xffff == 0)
                    && *texture_offset_words == [0; 2]
                    && storage_e0.iter().all(|&b| b == 0)
                    && stored_elevation.bits() == 0
            }
            actor::GeometryOperands::VertexQuad {
                storage_110,
                stored_elevation,
                ..
            } => storage_110.iter().all(|&b| b == 0) && stored_elevation.bits() == 0,
            actor::GeometryOperands::Model { .. } => false,
        };
        ensure!(
            geometry_zero && context_zero,
            "effect recipe contains unsupported animation or runtime state"
        );
        Ok(None)
    }
}

#[cfg(test)]
fn actor_tail(row: &[u8], flags: u32) -> Result<Option<ScreenTexture>> {
    actor_tail_record(&actor::Record::read(row)?, flags)
}

#[test]
fn screen_texture_offsets_preserve_signed_packet_bytes_and_reject_other_tail_data() {
    let mut row = [0; ACTOR_BYTES];
    row[0] = 4;
    row[2] = 10;
    row[0x32] = 255;
    let flags = 0x4000040;
    for (words, expected) in [([4u32, 4], [4, 4]), ([0x12345680, 255], [-128, -1])] {
        row[0xd8..0xdc].copy_from_slice(&words[0].to_be_bytes());
        row[0xdc..0xe0].copy_from_slice(&words[1].to_be_bytes());
        assert_eq!(actor_tail(&row, flags).unwrap().unwrap().offset, expected);
    }
    row[0xe0] = 1;
    assert!(actor_tail(&row, flags).is_err());
    row[0xe0] = 0;
    row[2] = 0;
    assert!(actor_tail(&row, flags).is_err());
    row[2] = 10;
    row[0] = 3;
    assert!(actor_tail(&row, flags).is_err());
}

/// Spell packages share a directory but retain distinct identities, including aliases.
pub(super) struct MagicArchive {
    bytes: Vec<u8>,
    directory: super::archive_directories::Directory,
    zero_aliases: BTreeSet<u16>,
}

impl MagicArchive {
    pub fn read(extracted: &Path) -> Result<Self> {
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel"))?;
        Self::with_sources(extracted, &rel, &super::all::Sources::read(extracted)?)
    }

    fn with_sources(extracted: &Path, rel: &Rel, sources: &super::all::Sources) -> Result<Self> {
        let executable = fs::read(extracted.join("sys/main.dol"))?;
        let mut zero_aliases = BTreeSet::from([1]);
        for row in crate::arte::read(&executable)?.definitions {
            if row.native_id >= 200 && row.flags & 0x10000001 != 0 {
                zero_aliases.insert((row.native_id - 200) as u16);
            }
        }
        let path = sources.archive(super::all::Archive::Magic);
        let bytes = fs::read(extracted.join("files").join(path))?;
        let directory = super::archive_directories::read(
            rel,
            super::embedded::Layout::RETAIL.archives,
            super::all::Archive::Magic,
            path,
            bytes.len() as u64,
        )?;
        Ok(Self {
            bytes,
            directory,
            zero_aliases,
        })
    }

    pub fn package(&self, id: u16) -> Result<&[u8]> {
        let range = self.directory.range(id, self.zero_aliases.contains(&id))?;
        self.bytes
            .get(range)
            .with_context(|| format!("magic package {id} exceeds archive"))
    }
}

#[test]
fn magic_packages_keep_alias_identity_and_bound_each_resource() {
    let mut archive = MagicArchive {
        bytes: vec![0; 600],
        directory: super::archive_directories::Directory::from_offsets(
            super::all::Archive::Magic,
            "renamed-spells.dat",
            &[0, 0, 300, 0, 300, 600, 0, 0],
            1,
            600,
        )
        .unwrap(),
        zero_aliases: BTreeSet::from([1, 3, 6]),
    };
    assert_eq!(archive.package(1).unwrap().len(), 300);
    assert_eq!(archive.package(3).unwrap().len(), 300);
    assert_eq!(archive.package(2).unwrap().len(), 300);
    assert_eq!(archive.package(4).unwrap().len(), 300);
    assert!(archive.package(0).is_err());
    assert!(archive.package(5).is_err());
    assert!(archive.package(6).is_err());
    archive.zero_aliases.remove(&3);
    assert!(archive.package(3).is_err());
    let mut package = archive.package(2).unwrap().to_vec();
    package[4..8].copy_from_slice(&276u32.to_be_bytes());
    package[8..12].copy_from_slice(&288u32.to_be_bytes());
    assert_eq!(magic_member(&package, 4).unwrap().unwrap().len(), 12);
    assert_eq!(magic_member(&package, 8).unwrap().unwrap().len(), 12);
    assert!(magic_member(&package, 12).unwrap().is_none());
    // The typed publication retains null slots and aliases independently of member data.
    package[12..16].copy_from_slice(&276u32.to_be_bytes());
    let resources = super::all::NativeResources::read(&package).unwrap();
    let published: super::all::NativeResources =
        serde_json::from_slice(&serde_json::to_vec(&resources).unwrap()).unwrap();
    assert_eq!(published.models[0], published.effects);
    assert_eq!(published.member(12).unwrap(), Some(276..288));
    assert_eq!(published.member(16).unwrap(), None);
    package[12..16].copy_from_slice(&600u32.to_be_bytes());
    assert!(magic_member(&package, 4).is_err());
    assert!(magic_member(&package, 0).is_err());
    assert_ne!(
        texture_bank(EffectBank::Magic(38), 6).unwrap(),
        texture_bank(EffectBank::Magic(57), 6).unwrap()
    );
    assert!(texture_bank(EffectBank::Common, 6).is_err());
    assert_eq!(
        texture_bank(EffectBank::Magic(38), 1).unwrap(),
        texture_bank(EffectBank::Common, 1).unwrap()
    );
}

/// Offset fields describe separately owned resources; padding never supplies absent data.
pub(super) fn magic_member(package: &[u8], field: usize) -> Result<Option<&[u8]>> {
    Ok(super::all::NativeResources::read(package)?
        .member(field)?
        .map(|range| &package[range]))
}

#[cfg(test)]
fn test_cooker() -> Cooker {
    Cooker {
        pending_images: Vec::new(),
        textures: BTreeMap::new(),
        element_palettes: [0; 8],
        element_colors: [[0; 3]; 8],
        material_indices: BTreeMap::new(),
        result: BattleEffectPrograms {
            programs: Vec::new(),
            actors: Vec::new(),
            materials: Vec::new(),
        },
    }
}

#[test]
fn radial_ellipsoid_and_ribbon_decode_their_distinct_fields() {
    let mut cooker = test_cooker();
    let id = EffectId {
        bank: EffectBank::Common,
        id: 0,
    };
    let mut row = [0; ACTOR_BYTES];
    row[2] = 255;
    row[0x32] = 255;
    row[0x12] = 4;
    row[0] = 9;
    row[0x14..0x18].copy_from_slice(&0x2000050u32.to_be_bytes());
    let radial = cooker.actor(&row, id, &[], false).unwrap();
    assert!(matches!(radial.orientation, Orientation::CameraRelative));
    assert!(matches!(
        radial.geometry,
        Geometry::RadialQuads {
            segments: 4,
            plane: RadialQuadPlane::Tangent,
            advance_u: true
        }
    ));
    row[0] = 13;
    row[0x14..0x18].copy_from_slice(&0x1000050u32.to_be_bytes());
    let sphere = cooker.actor(&row, id, &[], false).unwrap();
    assert!(matches!(
        sphere.geometry,
        Geometry::Ellipsoid {
            longitude_panels: 8
        }
    ));
    assert!(matches!(sphere.orientation, Orientation::World));
    row[0] = 18;
    row[0x13] = 6;
    row[0x33] = 254;
    row[0xc0..0xc4].copy_from_slice(&104f32.to_be_bytes());
    row[0x14..0x18].copy_from_slice(&0x2000020u32.to_be_bytes());
    let ribbon = cooker.actor(&row, id, &[], false).unwrap();
    assert!(matches!(ribbon.orientation, Orientation::World));
    assert!(matches!(
        ribbon.geometry,
        Geometry::JitterRibbon {
            segments: 4,
            phase: 6,
            phase_period: -2,
            jitter_span: 104.,
            plane: RibbonPlane::Depth
        }
    ));
    assert_eq!(ribbon.dimension_velocity[1], 0.);
    let bytes = [
        0, 12, 0, 0x12, 0, 2, 0, 0, 0, 10, 0, 0x13, 0, 12, 0, 0, 255, 255,
    ];
    let mut stream = vec![0; 2];
    stream.extend(bytes);
    let changes = modifiers(&stream, 2).unwrap();
    assert!(matches!(
        changes[0],
        Modifier::Byte {
            field: ByteField::GeometryCount,
            operation: Arithmetic::Add,
            value: IntegerValue::Constant(2)
        }
    ));
    assert!(matches!(
        changes[1],
        Modifier::Byte {
            field: ByteField::NoisePhase,
            operation: Arithmetic::Set,
            value: IntegerValue::Constant(12)
        }
    ));
    ribbon.validate_modifiers(&changes).unwrap();
    assert!(sphere.validate_modifiers(&changes).is_err());
    let legacy: Modifier = serde_json::from_str(r#"{"kind":"set_byte","field":{"kind":"uv_columns"},"value":{"kind":"constant","value":4}}"#).unwrap();
    assert!(matches!(
        legacy,
        Modifier::Byte {
            field: ByteField::GeometryCount,
            operation: Arithmetic::Set,
            value: IntegerValue::Constant(4)
        }
    ));
    for (segments, phase, span, copies) in [
        (0, 0, 104., 0),
        (4, 255, 104., 0),
        (4, 0, 1., 0),
        (4, 0, 104., 1),
    ] {
        row[0x12] = segments;
        row[0x13] = phase;
        row[0xc0..0xc4].copy_from_slice(&f32::to_be_bytes(span));
        row[0x8e] = copies;
        assert!(cooker.actor(&row, id, &[], false).is_err());
    }
    row[0] = 22;
    assert!(cooker.actor(&row, id, &[], false).is_err());
}

#[test]
fn direct_particles_preserve_depth_flags_and_stay_inside_the_actor_table() {
    let mut cooker = test_cooker();
    let mut row = [0; ACTOR_BYTES];
    row[0] = 7; // Billboard ring, as used by the stun controller.
    row[2] = 255; // No atlas access is needed to exercise material flags.
    row[0x12] = 3;
    row[0x32] = 255;
    for (flags, test, write, layer, during_pause) in [
        (0x280148u32, false, false, Some(OwnerLayer::Before), true),
        (0x4082148, false, false, Some(OwnerLayer::After), true),
        (0x280048, false, false, Some(OwnerLayer::Before), false),
        (0x440, true, false, None, false),
        (0x080440, false, false, None, false),
        (0x800440, true, true, None, false),
        (0x880440, false, true, None, false),
    ] {
        row[0x14..0x18].copy_from_slice(&flags.to_be_bytes());
        let actor = cooker
            .actor(
                &row,
                EffectId {
                    bank: EffectBank::Common,
                    id: 19,
                },
                &[],
                false,
            )
            .unwrap();
        assert_eq!((actor.depth_test, actor.depth_write), (test, write));
        assert_eq!(actor.owner_layer, layer);
        assert_eq!(actor.during_pause, during_pause);
        assert_eq!(actor.follow_emitter, flags & 0x400 != 0);
        assert!(matches!(actor.orientation, Orientation::Billboard));
    }
    let mut bank = vec![0; 20 + 2 * ACTOR_BYTES];
    bank[..4].copy_from_slice(b"ef1\0");
    bank[8..10].copy_from_slice(&20u16.to_be_bytes());
    bank[12..14].copy_from_slice(&(20 + ACTOR_BYTES as u16).to_be_bytes());
    bank[20..20 + ACTOR_BYTES].copy_from_slice(&row);
    let id = EffectId {
        bank: EffectBank::Common,
        id: 0,
    };
    cooker.recover_actor(&bank, id, false).unwrap();
    assert!(cooker.result.programs.is_empty());
    assert!(cooker.result.actor(id).unwrap().depth_write);
    assert!(
        cooker
            .recover_actor(&bank, EffectId { id: 1, ..id }, false)
            .is_err()
    );
    for kind in [4, 10, 11] {
        row[0] = kind;
        row[0x90] = 1; // Inert marker is separate from neighboring emission flags.
        row[0x91] = 0;
        assert!(cooker.actor(&row, id, &[], false).unwrap().ground.is_none());
        row[0x91] = 4;
        assert!(matches!(cooker.actor(&row, id, &[], false).unwrap().ground,
            Some(GroundResponse::EmitOnce { effect, clamp: false }) if effect == id));
        for offset in [0x91, 0x96, 0x97] {
            row[0x91..0x98].fill(0);
            row[offset] = 8;
            assert!(cooker.actor(&row, id, &[], false).is_err());
        }
        row[0x91..0x98].fill(0);
        row[0x92..0x96].fill(255); // No consumer while the enabling flags are clear.
        assert!(
            cooker
                .actor(&row, id, &[], false)
                .unwrap()
                .periodic
                .is_none()
        );
        row[0x91] = 8;
        let periodic = cooker
            .actor(&row, id, &[], false)
            .unwrap()
            .periodic
            .unwrap();
        assert_eq!((periodic.effect.id, periodic.period.get()), (255, 255));
        row[0x93] = 0;
        assert!(cooker.actor(&row, id, &[], false).is_err());
        row[0x91..0x98].fill(0);
        row[0x90] = 2;
        assert!(cooker.actor(&row, id, &[], false).is_err());
    }
    row[0] = 4;
    row[0x90..0x98].fill(0);
    row[0x14..0x18].copy_from_slice(&0x10000000u32.to_be_bytes());
    assert!(cooker.actor(&row, id, &[], false).unwrap().cull_back);
    row[0] = 8;
    row[0x13] = 1;
    row[0x14..0x18].copy_from_slice(&0x04400108u32.to_be_bytes());
    let trail = cooker.actor(&row, id, &[], true).unwrap();
    assert!(matches!(trail.geometry, Geometry::BillboardTrail { .. }));
    assert_eq!(trail.element_variants.len(), Element::ALL.len());
    row[0] = 6;
    row[0x12] = 2;
    row[0x13] = 3;
    row[0x14..0x18].copy_from_slice(&0x00401010u32.to_be_bytes());
    let hemisphere = cooker.actor(&row, id, &[], false).unwrap();
    assert!(matches!(
        hemisphere.geometry,
        Geometry::Hemisphere {
            repeat_uv: true,
            uv_columns: 2,
            uv_rows: 3
        }
    ));
    assert!(hemisphere.ground_relative);
    assert_eq!(hemisphere.element_variants.len(), Element::ALL.len());
    row[0x8e] = 1;
    assert!(cooker.actor(&row, id, &[], false).is_err());
    row[0] = 5;
    row[0x8e] = 0;
    for flags in [0x84000000u32, 0x84200000] {
        row[0x14..0x18].copy_from_slice(&flags.to_be_bytes());
        assert!(
            cooker
                .actor(&row, id, &[], false)
                .unwrap_err()
                .to_string()
                .contains("camera-space effect draw list")
        );
    }
}

#[test]
fn retained_commands_resolve_late_material_dependencies_and_bound_slots() {
    let id = EffectId {
        bank: EffectBank::Common,
        id: 0,
    };
    let mut bytes = vec![0; 402];
    bytes[..5].copy_from_slice(b"ef1\0\x01");
    for (at, value) in [(8, 20u16), (10, 382), (12, 372), (16, 400), (18, 402)] {
        bytes[at..at + 2].copy_from_slice(&value.to_be_bytes());
    }
    let row = &mut bytes[20..372];
    row[0] = 4;
    row[2] = 255;
    row[0x32] = 255;
    row[0x14..0x18].copy_from_slice(&0x40000u32.to_be_bytes());
    // Slot zero is born without elemental materials; a later command requests them.
    bytes[372..382].copy_from_slice(&[0, 21, 0, 0x14, 0, 0x40, 0, 0, 255, 255]);
    bytes[382..400].copy_from_slice(&[0, 0, 0, 0, 0, 0, 0, 1, 253, 0, 1, 0x74, 0, 2, 254, 0, 0, 0]);
    let mut cooker = test_cooker();
    cooker.program(&bytes, id).unwrap();
    cooker.result.validate().unwrap();
    let actor = cooker.result.actor(id).unwrap();
    assert!(actor.retained);
    assert_eq!(actor.element_variants.len(), Element::ALL.len());
    assert!(matches!(
        cooker.result.program(id).unwrap().emissions[1].command,
        EffectCommand::ModifyRetained { slot: 0, .. }
    ));
    bytes[391] = RETAINED_EFFECT_SLOTS as u8;
    assert!(test_cooker().program(&bytes, id).is_err());
    bytes[391..394].fill(0);
    assert!(test_cooker().program(&bytes, id).is_err());
}

#[test]
#[ignore = "requires privately extracted GameCube records; no texture cooking"]
fn original_fang_startup_closure_keeps_ring_marker_without_inventing_children() {
    let usual = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/extracted/disc1/files/BTL/BTLusual.dat"),
    )
    .unwrap();
    let bank = member(&usual, 3).unwrap();
    let mut cooker = test_cooker();
    // A cache hit exercises the complete unchanged recipes and modifier streams
    // without invoking a texture encoder. This test establishes semantic closure.
    for index in [8, 9, 10] {
        let row = actor_source(
            bank,
            EffectId {
                bank: EffectBank::Techniques,
                id: index,
            },
        )
        .unwrap();
        let flags = word(row, 0x14).unwrap();
        let material = cooker.result.materials.len() as u16;
        cooker.result.materials.push(EffectMaterial {
            texture: UiTexture {
                path: format!("battle/effects/fang-{index}.ktx2"),
                width: 256,
                height: 256,
            },
            rgb_scale: 2.,
        });
        cooker.material_indices.insert(
            MaterialKey {
                texture: texture_bank(EffectBank::Techniques, row[2]).unwrap(),
                color: row[3],
                alpha: (flags & 0x4000000 != 0).then_some(row[4]),
                stride: row[6],
            },
            material,
        );
    }
    for id in [9, 10, 11, 12] {
        cooker
            .program(
                bank,
                EffectId {
                    bank: EffectBank::Techniques,
                    id,
                },
            )
            .unwrap();
    }
    cooker.result.validate().unwrap();
    let shell = cooker
        .result
        .actor(EffectId {
            bank: EffectBank::Techniques,
            id: 8,
        })
        .unwrap();
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(actor_source(bank, shell.id).unwrap())
        ),
        "66d1d0d35007683aa464ee7eb3b11135d132aa3b8de13442badbfcc9323ff0f2"
    );
    assert_eq!((shell.blend, shell.material), (Blend::Additive, Some(0)));
    assert_eq!(
        shell.blend_variants,
        BTreeSet::from([Blend::Alpha, Blend::Additive])
    );
    let palette = shell.palette.as_ref().unwrap();
    assert_eq!(palette.index, 9);
    assert_eq!(palette.materials, BTreeMap::from([(9, 0), (26, 1)]));
    assert_eq!(cooker.result.materials.len(), 3);
    for offset in [0xc4a0, 0xc4cc] {
        assert_eq!(
            &bank[offset..offset + 16],
            &[0, 10, 0, 1, 0, 0, 0, 0, 0, 10, 0, 3, 0, 26, 0, 0]
        );
        let changes = modifiers(bank, offset).unwrap();
        assert!(matches!(
            changes[0],
            Modifier::Byte {
                field: ByteField::Blend,
                operation: Arithmetic::Set,
                value: IntegerValue::Constant(0)
            }
        ));
        assert!(matches!(
            changes[1],
            Modifier::Byte {
                field: ByteField::Palette,
                operation: Arithmetic::Set,
                value: IntegerValue::Constant(26)
            }
        ));
        shell.validate_modifiers(&changes).unwrap();
    }
    assert_eq!(
        cooker
            .result
            .programs
            .iter()
            .map(|p| (p.id.id, p.end_tick))
            .collect::<Vec<_>>(),
        vec![(9, 70), (10, 70), (11, 70), (12, 0)]
    );
    for (id, tick, changed) in [(9, 20, false), (10, 25, false), (11, 20, true)] {
        let program = cooker
            .result
            .program(EffectId {
                bank: EffectBank::Techniques,
                id,
            })
            .unwrap();
        let births = program
            .emissions
            .iter()
            .filter(|e| matches!(e.command, EffectCommand::Particle { actor, .. } if actor.id == 8))
            .collect::<Vec<_>>();
        assert_eq!(births.len(), 2);
        for (birth, count, interval) in [(births[0], 6, 3), (births[1], 2, 8)] {
            assert_eq!(birth.tick, tick);
            assert_eq!(
                (birth.repeat.unwrap().count, birth.repeat.unwrap().interval),
                (count, interval)
            );
            let EffectCommand::Particle { modifiers, .. } = &birth.command else {
                unreachable!()
            };
            assert_eq!(
                modifiers.iter().any(|m| matches!(
                    m,
                    Modifier::Byte {
                        field: ByteField::Blend,
                        ..
                    }
                )),
                changed
            );
        }
    }
    let id = EffectId {
        bank: EffectBank::Techniques,
        id: 9,
    };
    let row = actor_source(bank, id).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(row)),
        "4fffe63d17f94baa5e8a6dfed6029c2a0ad8c113f29bbc32dedbfaabe1102096"
    );
    assert_eq!(&row[0x90..0x98], &[1, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(word(row, 0x14).unwrap(), 0x04001002);
    let ring = cooker.result.actor(id).unwrap();
    assert!(matches!(ring.geometry, Geometry::Ring { segments: 16, .. }));
    assert_eq!(ring.lifetime, Some(10));
    assert_eq!(ring.position, [0., 0., 16.]);
    assert_eq!(ring.dimensions, [80., 32., 128.]);
    assert_eq!(ring.dimension_velocity, [2., 4., 0.]);
    assert!(
        cooker
            .result
            .actors
            .iter()
            .all(|actor| actor.ground.is_none())
    );
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct MaterialKey {
    texture: TextureBank,
    color: u8,
    alpha: Option<u8>,
    stride: u8,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TextureBank {
    Fixed(u8),
    Enemy(u8),
    Magic(u16),
    Skill(u16),
    Arena(u16),
}

fn texture_bank(bank: EffectBank, slot: u8) -> Result<TextureBank> {
    match (bank, slot) {
        (_, 0 | 1 | 10) => Ok(TextureBank::Fixed(slot)),
        (EffectBank::Enemy(monster), 2) => Ok(TextureBank::Enemy(monster)),
        (EffectBank::Magic(package), 6) => Ok(TextureBank::Magic(package)),
        (EffectBank::Skill(package), 6) => Ok(TextureBank::Skill(package)),
        (EffectBank::Arena(arena), 9) => Ok(TextureBank::Arena(arena)),
        _ => bail!("unsupported texture slot {slot} in effect bank {bank:?}"),
    }
}

fn fixed_textures(usual: &[u8]) -> Result<BTreeMap<TextureBank, Vec<u8>>> {
    let textures = member(usual, 4)?;
    Ok(BTreeMap::from([
        (
            TextureBank::Fixed(0),
            compression::decode(member(textures, 1)?)?,
        ),
        (TextureBank::Fixed(1), member(textures, 4)?.to_vec()),
        (TextureBank::Fixed(10), member(textures, 0)?.to_vec()),
    ]))
}
struct Cooker {
    pending_images: Vec<(u16, Vec<u8>)>,
    textures: BTreeMap<TextureBank, Vec<u8>>,
    element_palettes: [u8; 8],
    element_colors: [[i16; 3]; 8],
    material_indices: BTreeMap<MaterialKey, u16>,
    result: BattleEffectPrograms,
}

#[derive(Debug, Clone, Copy)]
enum EffectRequest {
    Program(EffectId),
    Actor(EffectId),
}

/// Keep every failed root's context; partial recipes are never published on failure.
fn preflight(
    requests: impl IntoIterator<Item = EffectRequest>,
    mut parse: impl FnMut(EffectRequest) -> Result<()>,
) -> Result<()> {
    let errors = requests
        .into_iter()
        .filter_map(|request| {
            parse(request)
                .err()
                .map(|error| format!("- {request:?}: {error:#}"))
        })
        .collect::<Vec<_>>();
    ensure!(
        errors.is_empty(),
        "effect preflight failed for {} requested roots:\n{}",
        errors.len(),
        errors.join("\n")
    );
    Ok(())
}

pub(crate) fn cook(
    extracted: &Path,
    output: &Path,
    impacts: &BTreeSet<EffectId>,
    actors: &BTreeSet<EffectId>,
) -> Result<BattleEffectPrograms> {
    let sources = crate::battle::all::Sources::cooked(output, crate::disc_number(extracted)?)?;
    let usual = fs::read(extracted.join("files").join(&sources.usual))?;
    let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel"))?;
    let palettes = rel.at((4, 0x2174))?;
    let colors = rel
        .at((4, 0x2180))?
        .get(..36)
        .context("missing effect element colors")?;
    let mut cook = Cooker {
        pending_images: Vec::new(),
        textures: fixed_textures(&usual)?,
        element_palettes: palettes
            .get(1..9)
            .context("missing effect element palettes")?
            .try_into()?,
        element_colors: std::array::from_fn(|i| {
            std::array::from_fn(|channel| i16::from(colors[(i + 1) * 4 + channel]))
        }),
        material_indices: BTreeMap::new(),
        result: BattleEffectPrograms {
            programs: Vec::new(),
            actors: Vec::new(),
            materials: Vec::new(),
        },
    };
    let mut required = impacts.clone();
    required.extend([3, 5, 6, 7, 9, 11, 20, 42].map(|id| EffectId {
        bank: EffectBank::Common,
        id,
    }));
    required.extend([3, 4, 7, 23, 24, 25, 26, 29, 30].map(|id| EffectId {
        bank: EffectBank::Techniques,
        id,
    }));
    required.insert(EffectId {
        bank: EffectBank::Enemy(49),
        id: 1,
    });
    let table = word(&usual, 0x2c)? as usize;
    let archive = fs::read(extracted.join("files").join(&sources.enemy))?;
    let mut enemies = BTreeMap::new();
    let extra = required
        .union(actors)
        .any(|id| matches!(id.bank, EffectBank::Skill(_) | EffectBank::Arena(_)))
        .then(|| all::ExtraArchives::read(extracted, &rel))
        .transpose()?;
    let mut extra_packages = BTreeMap::new();
    let magic = required
        .union(actors)
        .any(|id| matches!(id.bank, EffectBank::Magic(_)))
        .then(|| MagicArchive::read(extracted))
        .transpose()?;
    let requests = required.union(actors).flat_map(|&id| {
        [
            required.contains(&id).then_some(EffectRequest::Program(id)),
            actors.contains(&id).then_some(EffectRequest::Actor(id)),
        ]
        .into_iter()
        .flatten()
    });
    preflight(requests, |request| {
        let (EffectRequest::Program(id) | EffectRequest::Actor(id)) = request;
        let bytes = match id.bank {
            EffectBank::Common => member(&usual, 2)?,
            EffectBank::Techniques => member(&usual, 3)?,
            EffectBank::Enemy(monster) => {
                if let std::collections::btree_map::Entry::Vacant(entry) = enemies.entry(monster) {
                    let start = word(&usual, table + usize::from(monster) * 4)? as usize;
                    let end = word(&usual, table + (usize::from(monster) + 1) * 4)? as usize;
                    entry.insert(compression::decode(
                        archive.get(start..end).context("missing enemy package")?,
                    )?);
                }
                let enemy = &enemies[&monster];
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    cook.textures.entry(TextureBank::Enemy(monster))
                {
                    let offset = word(enemy, 0x1d0)? as usize;
                    if offset != 0 {
                        entry.insert(
                            enemy
                                .get(offset..)
                                .context("enemy effect atlas exceeds package")?
                                .to_vec(),
                        );
                    }
                }
                let start = word(enemy, 0x1cc)? as usize;
                let end = word(enemy, 0x1d0)? as usize;
                ensure!(
                    start != 0 && (end == 0 || end > start),
                    "invalid enemy {monster} effect bank range"
                );
                enemy
                    .get(start..if end == 0 { enemy.len() } else { end })
                    .with_context(|| format!("missing enemy {monster} effect bank"))?
            }
            EffectBank::Magic(package) => {
                let source = magic
                    .as_ref()
                    .context("missing magic archive")?
                    .package(package)?;
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    cook.textures.entry(TextureBank::Magic(package))
                    && let Some(texture) = magic_member(source, 8)?
                {
                    entry.insert(texture.to_vec());
                }
                magic_member(source, 4)?
                    .with_context(|| format!("missing magic {package} effect bank"))?
            }
            bank @ (EffectBank::Skill(_) | EffectBank::Arena(_)) => {
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    extra_packages.entry(bank)
                {
                    let (bytes, texture) = extra
                        .as_ref()
                        .context("missing effect resource archives")?
                        .bank(bank)?;
                    if let Some(texture) = texture {
                        let key = match bank {
                            EffectBank::Skill(id) => TextureBank::Skill(id),
                            EffectBank::Arena(id) => TextureBank::Arena(id),
                            _ => unreachable!(),
                        };
                        cook.textures.insert(key, texture);
                    }
                    entry.insert(bytes);
                }
                &extra_packages[&bank]
            }
        };
        match request {
            EffectRequest::Program(_) => cook.program(bytes, id),
            EffectRequest::Actor(_) => {
                cook.recover_actor_record(
                    bytes,
                    &actor::Record::read(actor_source(bytes, id)?)?,
                    id,
                    false,
                )?;
                cook.child_effects(bytes, id.bank)
            }
        }
    })?;
    cook.finish(output)
}

fn actor_source(bytes: &[u8], actor: EffectId) -> Result<&[u8]> {
    ensure!(
        bytes.len() >= 20 && bytes.starts_with(b"ef1\0"),
        "invalid battle effect bank"
    );
    let start = usize::from(half(bytes, 8)?);
    let end = usize::from(half(bytes, 12)?);
    ensure!(
        start >= 20
            && end >= start
            && end <= bytes.len()
            && (end - start).is_multiple_of(ACTOR_BYTES),
        "invalid effect actor region"
    );
    let at = start + usize::from(actor.id) * ACTOR_BYTES;
    ensure!(
        at + ACTOR_BYTES <= end,
        "effect actor exceeds declared region"
    );
    Ok(&bytes[at..at + ACTOR_BYTES])
}

fn controller_record(record: &actor::Record) -> Result<Option<EffectController>> {
    let controller = authored_controller(record)?;
    if let Some(controller) = &controller {
        controller.validate()?;
    }
    Ok(controller)
}

fn authored_controller(record: &actor::Record) -> Result<Option<EffectController>> {
    let p = &record.prefix;
    let duration = p.lifetime as u16;
    let controller = match p.kind {
        23 => {
            let actor::Body::Camera {
                distance,
                elevation,
                ..
            } = &record.body
            else {
                bail!("camera controller has incompatible operands");
            };
            EffectController::Camera {
                duration,
                distance: f32::from_bits(distance.bits()),
                elevation: f32::from_bits(elevation.bits()),
            }
        }
        22 => EffectController::Shake {
            duration,
            amplitude: p.flags_or_shake_amplitude,
        },
        24 => EffectController::StageColor {
            color: p.colors[0].map(|value| value as u8),
            duration,
        },
        25 => {
            let actor::Body::Caption { text_storage } = &record.body else {
                bail!("caption controller has incompatible operands");
            };
            let end = text_storage
                .iter()
                .position(|&b| b == 0)
                .context("unterminated effect caption")?;
            let (text, _, invalid) = encoding_rs::SHIFT_JIS.decode(&text_storage[..end]);
            ensure!(!invalid, "invalid effect caption encoding");
            EffectController::Caption {
                text: text.into_owned(),
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(controller))
}

#[cfg(test)]
fn controller(row: &[u8]) -> Result<Option<EffectController>> {
    controller_record(&actor::Record::read(row)?)
}

fn program_timeline(bytes: &[u8], id: u8) -> Result<&[u8]> {
    ensure!(
        bytes.len() >= 20 && bytes.starts_with(b"ef1\0"),
        "invalid battle effect bank"
    );
    ensure!(id < bytes[4], "effect program exceeds declared count");
    let table = usize::from(half(bytes, 16)?);
    let uv_table = usize::from(half(bytes, 18)?);
    let end = uv_table + usize::from(bytes[5]) * 2;
    ensure!(
        table >= 20 && uv_table == table + usize::from(bytes[4]) * 2 && end <= bytes.len(),
        "invalid effect program table extent"
    );
    let relative = half(bytes, table + usize::from(id) * 2)? as i16;
    let start = usize::try_from(i32::from(half(bytes, 10)?) + i32::from(relative))
        .context("negative effect program address")?;
    ensure!(
        start >= 20 && start + 6 <= end,
        "effect program address outside bank"
    );
    Ok(&bytes[start..end])
}

#[test]
fn timeline_offsets_are_signed_and_cannot_escape_the_declared_bank() {
    let mut bytes = [0; 72]; // Padding after the logical bank must not supply a missing record.
    bytes[..5].copy_from_slice(b"ef1\0\x02");
    bytes[10..12].copy_from_slice(&40u16.to_be_bytes());
    bytes[16..20].copy_from_slice(&[0, 60, 0, 64]);
    bytes[24..30].copy_from_slice(&[0, 8, 254, 0, 0, 0]);
    bytes[60..62].copy_from_slice(&(-16i16).to_be_bytes());
    assert_eq!(
        &program_timeline(&bytes, 0).unwrap()[..6],
        &[0, 8, 254, 0, 0, 0]
    );
    assert!(program_timeline(&bytes, 2).is_err());
    for relative in [-41i16, 24] {
        bytes[62..64].copy_from_slice(&relative.to_be_bytes());
        assert!(program_timeline(&bytes, 1).is_err());
    }
    assert!(program_timeline(&bytes[..63], 0).is_err());
}

impl Cooker {
    fn finish(self, output: &Path) -> Result<BattleEffectPrograms> {
        self.result.validate()?;
        fs::create_dir_all(output.join("intermediate/battle/effects"))?;
        fs::create_dir_all(output.join("battle/effects"))?;
        for (index, rgba) in self.pending_images {
            let texture = &self.result.materials[usize::from(index)].texture;
            let png = crate::temporary_path(
                &output
                    .join("intermediate")
                    .join(&texture.path)
                    .with_extension("png"),
            );
            image::save_buffer(
                &png,
                &rgba,
                texture.width,
                texture.height,
                image::ColorType::Rgba8,
            )?;
            crate::texture::cook_png(&png, &output.join(&texture.path))?;
            fs::remove_file(png)?;
        }
        Ok(self.result)
    }

    fn child_effects(&mut self, bytes: &[u8], bank: EffectBank) -> Result<()> {
        let children = self
            .result
            .actors
            .iter()
            .filter(|actor| actor.id.bank == bank)
            .flat_map(EffectActor::child_effects)
            .collect::<BTreeSet<_>>();
        for child in children {
            // Programs are inserted before following children, so cycles close once.
            self.program(bytes, child)
                .with_context(|| format!("particle child effect {child:?}"))?;
        }
        Ok(())
    }

    fn program(&mut self, bytes: &[u8], id: EffectId) -> Result<()> {
        use events::Command as C;
        use resonance_content::battle::effect_inventory::EffectAttachment;
        if self.result.program(id).is_some() {
            return Ok(());
        }
        let timeline = program_timeline(bytes, id.id)?;
        let mut rows = events::Timeline::new(timeline);
        let mut dispatch_tick = 0;
        let mut emissions = Vec::new();
        let mut errors = Vec::new();
        for _ in 0..RECORD_LIMIT {
            let event = rows.next()?;
            ensure!(event.tick >= 0, "negative effect timeline time");
            // Commands are sequential: a lower timestamp cannot run before the
            // preceding command. Keep that order and cook their actual dispatch time.
            dispatch_tick = dispatch_tick.max(event.tick as u16);
            let tick = dispatch_tick;
            if event.repeat.is_none() && matches!(event.command, C::End { .. }) {
                ensure!(errors.is_empty(), "{}", errors.join("\n"));
                let mut program = EffectProgram {
                    id,
                    end_tick: tick,
                    emissions,
                };
                bind_retained_birth_animations(&mut program, &self.result)?;
                if program.emissions.iter().any(|e| {
                    matches!(
                        &e.command,
                        EffectCommand::ModifyRetained { modifiers, .. }
                            if modifiers.iter().any(|m| matches!(m, Modifier::Flag { field: EffectFlag::UseElementVariant, enabled: true }))
                    )
                }) {
                    let actors = program
                        .retained_actors(&self.result)
                        .map(|a| a.id)
                        .collect::<BTreeSet<_>>();
                    for actor in actors {
                        self.recover_actor_record(
                            bytes,
                            &actor::Record::read(actor_source(bytes, actor)?)?,
                            actor,
                            true,
                        )?;
                    }
                }
                self.retained_material_changes(bytes, &program)?;
                self.result.programs.push(program);
                self.child_effects(bytes, id.bank)?;
                return Ok(());
            }
            let repeat = event.repeat.map(|repeat| Repeat {
                count: repeat.count,
                interval: repeat.interval as u16,
            });
            if let Some(repeat) = repeat {
                ensure!(repeat.count > 0, "empty effect repeat");
            }
            let command = match event.command {
                C::Emit { attachment, .. } => self
                    .particle_command_with_scratch(
                        bytes,
                        id.bank,
                        event.command,
                        fresh_integer_scratch(&emissions)
                            && errors.is_empty()
                            && repeat.is_none()
                            && !matches!(attachment, EffectAttachment::BoneGroup(_)),
                        if errors.is_empty()
                            && repeat.is_none()
                            && !matches!(attachment, EffectAttachment::BoneGroup(_))
                        {
                            integer_birth_ranges(&emissions, tick)?
                        } else {
                            [None; 4]
                        },
                    )
                    .with_context(|| format!("effect emission at tick {tick}")),
                C::Sound { sound, priority } => Ok(EffectCommand::Sound {
                    sound: u16::from(sound),
                    priority: priority as u8,
                }),
                C::ModifyRetained { slot, modifier } => retained_command(bytes, slot, modifier)
                    .with_context(|| format!("retained modification at tick {tick}")),
                C::Repeat { .. } => Err(anyhow::anyhow!("unsupported effect timeline command 255")),
                C::End { .. } => Err(anyhow::anyhow!("unsupported effect timeline command 254")),
            };
            // Each command has a bounded record; inspect later emissions too,
            // but never publish a program containing failed commands.
            match command {
                Ok(command) => emissions.push(EffectEmission {
                    tick,
                    repeat,
                    command,
                }),
                Err(error) => errors.push(format!("{error:#}")),
            }
        }
        bail!("effect timeline exceeds record limit")
    }
    #[cfg(test)]
    fn particle_command(
        &mut self,
        bytes: &[u8],
        bank: EffectBank,
        row: &[u8],
    ) -> Result<EffectCommand> {
        self.particle_command_with_scratch(
            bytes,
            bank,
            events::Record::read(row)?.command,
            false,
            [None; 4],
        )
    }
    fn particle_command_with_scratch(
        &mut self,
        bytes: &[u8],
        bank: EffectBank,
        command: events::Command,
        fresh_integers: bool,
        ranges: [Option<[i16; 2]>; 4],
    ) -> Result<EffectCommand> {
        use resonance_content::battle::effect_inventory::EffectAttachment;
        let events::Command::Emit {
            actor,
            attachment,
            modifier,
        } = command
        else {
            bail!("expected effect particle emission");
        };
        let actor = EffectId { bank, id: actor };
        let source = actor::Record::read(actor_source(bytes, actor)?)?;
        if let Some(controller) = controller_record(&source)? {
            return Ok(EffectCommand::Controller { actor, controller });
        }
        let offset = usize::from(modifier);
        let mut modifiers = decode_modifiers(
            bytes,
            offset,
            !matches!(attachment, EffectAttachment::BoneGroup(_)),
        )
        .with_context(|| format!("effect actor {actor:?}, modifier {offset:#x}"))?;
        if fresh_integers
            && let actor::Body::Particle { geometry, .. } = &source.body
            && let actor::GeometryOperands::Model {
                index, animation, ..
            } = geometry.as_ref()
            && model_indices(*index, *animation, &modifiers).is_err()
        {
            // Initialization clears scratch once. Claim that state only when it closes
            // the actual ordered resource dependency; never replace the native writes.
            let mut proven = Vec::with_capacity(modifiers.len() + 1);
            proven.push(Modifier::RequireFreshIntegers);
            proven.extend_from_slice(&modifiers);
            if model_indices(*index, *animation, &proven).is_ok() {
                modifiers = proven;
            }
        }
        for (index, range) in ranges.into_iter().enumerate() {
            let Some([min, max]) = range else {
                continue;
            };
            let index = index as u8;
            let reads = modifiers.iter().any(|m| match *m {
                Modifier::Byte {
                    value: IntegerValue::Temporary(i),
                    ..
                }
                | Modifier::Lifetime {
                    value: IntegerValue::Temporary(i),
                    ..
                } => i == index,
                Modifier::Integer {
                    field,
                    operation,
                    value,
                } => {
                    matches!(value, IntegerValue::Temporary(i) if i == index)
                        || matches!(field, IntegerField::Temporary(i) if i == index)
                            && !matches!(operation, Arithmetic::Set)
                }
                _ => false,
            });
            if reads {
                modifiers.insert(0, Modifier::RequireIntegerRange { index, min, max });
            }
        }
        let variants_required = modifiers.iter().any(|m| {
            matches!(
                m,
                Modifier::Flag {
                    field: EffectFlag::UseElementVariant,
                    enabled: true
                }
            )
        });
        self.recover_actor_record(bytes, &source, actor, variants_required)?;
        self.palette_changes(&source.prefix, actor, &modifiers, None)?;
        let group_choices = if matches!(attachment, EffectAttachment::BoneGroup(_))
            && modifiers.iter().any(Modifier::writes_material_selection)
        {
            Some(
                group_materials(self.result.actor(actor).unwrap(), &modifiers)
                    .with_context(|| format!("group material choices for {actor:?}"))?,
            )
        } else {
            None
        };
        if let Some(choices) = &group_choices
            && !choices.palettes.is_empty()
        {
            self.palette_changes(&source.prefix, actor, &modifiers, Some(&choices.palettes))?;
        }
        let actor_state = self
            .result
            .actors
            .iter_mut()
            .find(|a| a.id == actor)
            .unwrap();
        if modifiers.iter().any(|m| {
            matches!(
                m,
                Modifier::Byte {
                    field: ByteField::ModelIndex,
                    ..
                }
            )
        }) && let Geometry::Model {
            model: model @ ModelRef::Enemy { .. },
            animation: Some(0),
            ..
        } = &mut actor_state.geometry
        {
            let ModelRef::Enemy { monster, index } = *model else {
                unreachable!()
            };
            // Animation binds before birth modifiers; the later pointer refresh changes only geometry.
            *model = ModelRef::EnemyAnimated {
                monster,
                index,
                animation_model: index,
            };
        }
        if modifiers.iter().any(|m| {
            matches!(
                m,
                Modifier::Byte {
                    field: ByteField::Blend,
                    ..
                }
            )
        }) {
            actor_state.blend_variants.extend(match group_choices {
                Some(choices) => choices.blends,
                None => blend_choices(actor_state.blend, &modifiers)?,
            });
        }
        let actor_state = self
            .result
            .actor(actor)
            .context("missing recovered effect actor")?;
        actor_state
            .validate_modifiers(&modifiers)
            .with_context(|| format!("effect actor {actor:?}, modifier {offset:#x}"))?;
        if matches!(attachment, EffectAttachment::BoneGroup(_))
            && let Geometry::Model {
                model,
                presentation,
                ..
            } = actor_state.geometry
        {
            group_model_choices(model.index(), presentation.animation_selector, &modifiers)
                .with_context(|| format!("group model choices for {actor:?}"))?;
        }
        let attachment = match attachment {
            EffectAttachment::Emitter => Attachment::Emitter,
            EffectAttachment::Bone(bone) => Attachment::Bone(bone),
            EffectAttachment::BoneGroup(group) => Attachment::BoneGroup(group),
        };
        Ok(EffectCommand::Particle {
            actor,
            attachment,
            modifiers,
        })
    }

    fn palette_changes(
        &mut self,
        prefix: &actor::Prefix,
        id: EffectId,
        modifiers: &[Modifier],
        choices: Option<&BTreeSet<u16>>,
    ) -> Result<()> {
        if choices.is_none()
            && !modifiers.iter().any(|m| {
                m.writes_palette_selection()
                    || matches!(
                        m,
                        Modifier::Byte {
                            field: ByteField::Palette,
                            ..
                        }
                    )
            })
        {
            return Ok(());
        }
        let actor = self
            .result
            .actor(id)
            .context("missing palette effect actor")?;
        if matches!(actor.geometry, Geometry::Model { .. }) {
            return Ok(());
        }
        ensure!(
            actor.screen_texture.is_none()
                && actor.element_variants.is_empty()
                && actor
                    .uv_animation
                    .as_ref()
                    .is_none_or(|uv| uv.frames.iter().all(|f| f.update.material().is_none())),
            "palette modifier conflicts with another palette consumer"
        );
        let mut palette = actor.palette.clone().unwrap_or(Palette {
            index: prefix.palettes[0],
            alpha: None,
            materials: BTreeMap::from([(
                u16::from(prefix.palettes[0]),
                actor.material.context("palette effect has no texture")?,
            )]),
        });
        if palette.alpha.is_none() && modifiers.iter().any(Modifier::writes_palette_selection) {
            palette.alpha = Some(prefix.palettes[1]);
            palette.materials = palette
                .materials
                .into_iter()
                .map(|(color, material)| {
                    (
                        u16::from_be_bytes([color as u8, prefix.palettes[1]]),
                        material,
                    )
                })
                .collect();
        }
        let key = MaterialKey {
            texture: texture_bank(id.bank, prefix.resource_slot)?,
            color: prefix.palettes[0],
            alpha: (prefix.flags_or_shake_amplitude & 0x4000000 != 0).then_some(prefix.palettes[1]),
            stride: prefix.palette_stride,
        };
        let indices = match choices {
            Some(choices) => choices.clone(),
            None => palette_indices(prefix.palettes[0], palette.alpha, modifiers)?,
        };
        for index in indices {
            let [color, alpha] = if palette.alpha.is_some() {
                index.to_be_bytes()
            } else {
                [index as u8, prefix.palettes[1]]
            };
            palette.materials.insert(
                index,
                self.material(MaterialKey {
                    color,
                    alpha: key.alpha.map(|_| alpha),
                    ..key
                })
                .with_context(|| format!("palette {index:#x} for effect actor {id:?}"))?,
            );
        }
        self.result
            .actors
            .iter_mut()
            .find(|a| a.id == id)
            .unwrap()
            .palette = Some(palette);
        Ok(())
    }

    fn retained_material_changes(&mut self, bytes: &[u8], program: &EffectProgram) -> Result<()> {
        if !program.emissions.iter().any(|emission| matches!(&emission.command,
            EffectCommand::ModifyRetained { modifiers, .. } if modifiers.iter().any(Modifier::writes_material_selection))) {
            return Ok(());
        }
        let actors = program
            .retained_actors(&self.result)
            .map(|actor| actor.id)
            .collect::<BTreeSet<_>>();
        let prefixes = actors
            .into_iter()
            .map(|id| Ok((id, actor::Record::read(actor_source(bytes, id)?)?.prefix)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        for emission in &program.emissions {
            if emission.repeat.is_some() && emission.tick >= program.end_tick {
                continue;
            }
            if let EffectCommand::ModifyRetained { modifiers, .. } = &emission.command {
                for (id, prefix) in &prefixes {
                    self.palette_changes(prefix, *id, modifiers, None)?;
                }
            }
        }
        for (id, choices) in retained_materials(program, &self.result)? {
            if !choices.palettes.is_empty() {
                self.palette_changes(&prefixes[&id], id, &[], Some(&choices.palettes))?;
            }
            self.result
                .actors
                .iter_mut()
                .find(|actor| actor.id == id)
                .unwrap()
                .blend_variants
                .extend(choices.blends);
        }
        Ok(())
    }

    fn recover_actor_record(
        &mut self,
        bytes: &[u8],
        record: &actor::Record,
        actor: EffectId,
        variants_required: bool,
    ) -> Result<()> {
        let existing = self.result.actors.iter().position(|a| a.id == actor);
        ensure!(
            !variants_required || existing.is_none_or(|i| self.result.actors[i].palette.is_none()),
            "element and modifier palettes cannot be combined"
        );
        if existing
            .is_none_or(|i| variants_required && self.result.actors[i].element_variants.is_empty())
        {
            let parsed = self
                .actor_record(record, actor, bytes, variants_required)
                .with_context(|| format!("effect actor {actor:?}"))?;
            if let Some(i) = existing {
                let variants = std::mem::take(&mut self.result.actors[i].blend_variants);
                self.result.actors[i] = parsed;
                self.result.actors[i].blend_variants = variants;
            } else {
                self.result.actors.push(parsed);
            }
        }
        Ok(())
    }
    #[cfg(test)]
    fn recover_actor(
        &mut self,
        bytes: &[u8],
        actor: EffectId,
        variants_required: bool,
    ) -> Result<()> {
        self.recover_actor_record(
            bytes,
            &actor::Record::read(actor_source(bytes, actor)?)?,
            actor,
            variants_required,
        )
    }
    #[cfg(test)]
    fn actor(
        &mut self,
        row: &[u8],
        id: EffectId,
        bank: &[u8],
        variants_required: bool,
    ) -> Result<EffectActor> {
        self.actor_record(&actor::Record::read(row)?, id, bank, variants_required)
    }

    fn actor_record(
        &mut self,
        record: &actor::Record,
        id: EffectId,
        bank: &[u8],
        variants_required: bool,
    ) -> Result<EffectActor> {
        let p = &record.prefix;
        ensure!(p.kind != 2, "effect kind 2 has a null native draw dispatch");
        let flags = p.flags_or_shake_amplitude;
        let uv_track = p.uv_track as u8;
        let actor::Body::Particle {
            geometry: operands, ..
        } = &record.body
        else {
            bail!("controller declaration cannot be lowered as a particle");
        };
        let operands = operands.as_ref();
        let (dimensions, dimension_velocity, dimension_acceleration) = match operands {
            actor::GeometryOperands::Parameters { dimensions, .. }
            | actor::GeometryOperands::Model { dimensions, .. } => (
                dimensions.value,
                dimensions.velocity,
                dimensions.acceleration,
            ),
            // The common dimension slots overlap the first three vertex positions.
            actor::GeometryOperands::VertexQuad { vertices, .. } => {
                (vertices[0], vertices[1], vertices[2])
            }
        };
        ensure!(
            flags & 0x80000000 == 0,
            "unsupported camera-space effect draw list; no active scene dispatch is recovered"
        );
        // Every accepted bit has a named field or geometry/material interpretation below.
        const KNOWN_FLAGS: u32 = 1
            | 2
            | 4
            | 8
            | 0x10
            | 0x20
            | 0x40
            | 0x80
            | 0x100
            | 0x200
            | 0x400
            | 0x800
            | 0x1000
            | 0x2000
            | 0x20000
            | 0x40000
            | 0x80000
            | 0x100000
            | 0x200000
            | 0x400000
            | 0x800000
            | 0x1000000
            | 0x2000000
            | 0x4000000
            | 0x8000000
            | 0x10000000
            | 0x20000000
            | 0x40000000;
        ensure!(
            flags & !KNOWN_FLAGS == 0,
            "unsupported effect actor flags {flags:#x}"
        );
        let space = if flags & 0x20000000 != 0 {
            ensure!(
                p.kind != 17 && flags & 0x40000000 == 0,
                "unsupported live owner-joint position consumer"
            );
            // fn_1_403F4 refreshes +0x148 through fn_1_5B984 before the
            // ordinary 0x400 emitter branch. Only the joint's origin is copied.
            EffectSpace::OwnerBonePosition { bone: p.bone }
        } else if flags & 0x40000000 == 0 {
            EffectSpace::World
        } else {
            match p.kind {
                4 if flags & 0x2000040 == 0
                    && p.resource_slot != 10
                    && p.texture_frame_or_joint_slot == 0 =>
                {
                    EffectSpace::OwnerBone { bone: p.bone }
                }
                // Billboard quads use camera orientation and emitter translation; no bone lookup.
                5 if flags & 0x40 != 0 => EffectSpace::World,
                _ => bail!("unsupported owner-bone flag consumer"),
            }
        };
        ensure!(
            flags & 0x80 == 0
                || (p.kind == 3 && flags & 0x400 != 0)
                || (p.kind == 15
                    && flags & 0x400 != 0
                    && flags & (0x40 | 0x2000000 | 0x60000000) == 0)
                || (p.kind == 4 && flags & (0x400 | 0x40 | 0x2000000 | 0x60000000) == 0),
            "unsupported motion-facing or fixed-orientation effect"
        );
        ensure!(
            // Model draw has no consumer for this authoring flag; only quads copy to EF bones.
            flags & 0x200 == 0 || matches!(p.kind, 3 | 5),
            "bone-group flag is unsupported for this geometry"
        );
        ensure!(
            flags & 0x202000 != 0x202000,
            "conflicting effect owner layers"
        );
        ensure!(p.blend <= 2, "unsupported effect blend flags");
        ensure!(
            flags & 0x20000 == 0 || p.kind == 3,
            "joint matrix binding is only defined for model effects"
        );
        ensure!(
            p.retained_slot == 0 && p.storage_07 == 0,
            "unsupported effect actor allocation state"
        );
        ensure!(
            // Rings leave +0x84 unused; trails and spirals decode it as their angle step.
            // A ring's bone selector is inert without an enabled bone flag; its neighbor stays reserved.
            // Models never consume recipe +0x80. The model draw's object +0x80
            // is recipe +0x58 (Euler angles), because allocation copies at +0x28.
            p.storage_7c[..4] == [0; 4]
                && (p.kind == 3 || p.storage_7c[4..] == [0; 4])
                && (matches!(p.kind, 7 | 8 | 10) || p.angle_step.bits() == 0)
                && p.storage_8a == [0; 2]
                && (matches!(p.kind, 3 | 4)
                    || flags & 0x40000200 != 0
                    || (flags & 0x20000000 != 0 && p.texture_frame_or_joint_slot == 0)
                    || (p.bone == 0 && p.texture_frame_or_joint_slot == 0))
                && (!(p.kind == 4 || (p.kind == 5 && flags & 0x200 != 0))
                    || p.texture_frame_or_joint_slot == 0),
            "unsupported effect actor motion state"
        );
        ensure!(
            flags & 0x1000 == 0 || matches!(p.kind, 3..=13 | 15 | 18 | 19),
            "unsupported ground-relative effect geometry"
        );
        ensure!(
            // Ring, Spiral and Disc do not read this reserved authoring byte.
            (p.storage_90 == 0 || (matches!(p.kind, 4 | 10 | 11) && p.storage_90 == 1))
                && p.secondary.flags & !15 == 0
                && p.storage_96 == [0; 2],
            "unsupported effect secondary emission"
        );
        let screen_texture = if p.kind == 17 {
            // The initializer clears its history and returns before UV/model setup.
            None
        } else {
            actor_tail_record(record, flags)?
        };
        let orientation = if p.kind == 4 && flags & 0x80 != 0 {
            Orientation::FixedWorld
        } else if p.kind == 15 && flags & 0x80 != 0 {
            Orientation::FollowMotion
        } else if p.kind == 13 {
            Orientation::World
        } else if p.kind == 9 && flags & 0x2000000 != 0 {
            Orientation::CameraRelative
        } else if flags & 0x40 != 0 {
            Orientation::Billboard
        } else if p.kind != 18 && flags & 0x2000000 != 0 {
            Orientation::CameraRelative
        } else {
            Orientation::World
        };
        ensure!(
            p.kind != 3 || flags & 0x20 == 0 || (p.resource_slot == 6 && flags & 4 == 0),
            "unsupported effect model animation ownership"
        );
        let geometry = match p.kind {
            0 | 14 | 16 => Geometry::NoDraw {
                motion: if p.kind == 0 {
                    NoDrawMotion::Local
                } else {
                    NoDrawMotion::Origin
                },
            },
            17 => Geometry::NoDraw {
                motion: NoDrawMotion::PointHistory {
                    capacity: p.geometry_count,
                },
            },
            19 => Geometry::TriangleBand,

            3 => {
                let actor::GeometryOperands::Model {
                    index,
                    animation,
                    elevation,
                    ..
                } = operands
                else {
                    bail!("model effect has incompatible geometry operands");
                };
                Geometry::Model {
                    model: match (p.resource_slot, *index, id.bank) {
                        (0, 0, _) => ModelRef::ColetteWeapon,
                        (0, index, _) => ModelRef::Common { index },
                        (2, index, EffectBank::Enemy(monster)) => {
                            ModelRef::Enemy { monster, index }
                        }
                        (6, index, EffectBank::Magic(package)) => {
                            ModelRef::Magic { package, index }
                        }
                        (6, index, EffectBank::Skill(package)) => {
                            ModelRef::Skill { package, index }
                        }
                        _ => bail!("unsupported effect model binding"),
                    },
                    animation: (flags & 4 != 0).then_some(*animation),
                    loop_animation: p.secondary.flags & 2 != 0,
                    presentation: ModelPresentation {
                        animation_selector: *animation,
                        reverse_at: (p.secondary.flags & 1 != 0)
                            .then_some(p.secondary.animation_change_age),
                        orientation: if flags & 0x80 != 0 {
                            ModelOrientation::FollowMotion
                        } else {
                            ModelOrientation::Authored
                        },
                        external_animation: flags & 0x20 != 0,
                        elevation: f32::from_bits(elevation.bits()),
                        texture_rows: p.bone,
                        texture_frame: p.texture_frame_or_joint_slot,
                        palettes: p.palettes,
                        attachment: (flags & 0x20000 != 0).then_some(RetainedJoint {
                            slot: p.texture_frame_or_joint_slot,
                            joint: p.bone,
                        }),
                    },
                }
            }
            4 => Geometry::Ring {
                segments: if flags & 0x100000 != 0 {
                    32
                } else if flags & 0x1000000 != 0 {
                    8
                } else {
                    16
                },
                flared: flags & 0x800 != 0,
                lines: flags & 0x20 != 0,
                repeat_uv: flags & 0x10 != 0,
                uv_columns: p.geometry_count,
            },
            5 if flags & 0x200 != 0 => Geometry::BoneGroupQuad { group: p.bone },
            5 => Geometry::Quad,
            9 => Geometry::RadialQuads {
                segments: p.geometry_count,
                plane: if flags & 0x20 != 0 {
                    RadialQuadPlane::Flat
                } else {
                    RadialQuadPlane::Tangent
                },
                advance_u: flags & 0x10 != 0,
            },
            13 => Geometry::Ellipsoid {
                longitude_panels: if flags & 0x1000000 != 0 { 8 } else { 16 },
            },
            18 => Geometry::JitterRibbon {
                segments: p.geometry_count,
                phase: p.geometry_phase,
                phase_period: p.copy_axis_or_phase_period as i8,
                jitter_span: f32::from_bits(dimension_velocity[1].bits()),
                plane: if flags & 0x20 != 0 {
                    RibbonPlane::Depth
                } else {
                    RibbonPlane::Flat
                },
            },
            6 => {
                ensure!(
                    p.additional_copies == 0,
                    "hemisphere effect requests unsupported copies"
                );
                Geometry::Hemisphere {
                    repeat_uv: flags & 0x10 != 0,
                    uv_columns: p.geometry_count,
                    uv_rows: p.geometry_phase,
                }
            }
            7 => Geometry::BillboardRing {
                segments: p.geometry_count,
            },
            8 => Geometry::BillboardTrail {
                segments: p.geometry_count,
                steps_per_segment: p.geometry_phase,
                segment_offset: actor_vector(p.acceleration_change_or_segment_offset),
                angle_step: f32::from_bits(p.angle_step.bits()),
                size_step: actor_vector(dimension_acceleration),
            },
            10 => {
                let actor::GeometryOperands::Parameters { radius_step, .. } = operands else {
                    bail!("spiral effect has incompatible geometry operands");
                };
                Geometry::Spiral {
                    segments: p.geometry_count,
                    uv_rows: p.geometry_phase,
                    segment_offset: actor_vector(p.acceleration_change_or_segment_offset),
                    angle_step: f32::from_bits(p.angle_step.bits()),
                    radius_step: f32::from_bits(radius_step.bits()),
                    repeat_uv: flags & 0x10 != 0,
                }
            }
            11 => Geometry::Disc {
                segments: if flags & 0x1000000 != 0 { 8 } else { 16 },
            },
            12 => Geometry::CurvedShell {
                segments: if flags & 0x100000 != 0 { 16 } else { 8 },
                elliptical: flags & 0x20 != 0,
                repeat_uv: flags & 0x10 != 0,
                uv_columns: p.geometry_count,
            },
            15 => {
                let actor::GeometryOperands::VertexQuad {
                    vertices,
                    velocities,
                    ..
                } = operands
                else {
                    bail!("vertex quad has incompatible geometry operands");
                };
                Geometry::VertexQuad {
                    vertices: vertices.map(actor_vector),
                    velocities: velocities.map(actor_vector),
                    copy_axis: p.copy_axis_or_phase_period,
                    rotate_copies_locally: flags & 0x20 != 0,
                }
            }
            kind => bail!("unsupported effect geometry {kind}"),
        };
        geometry.validate_dynamic()?;
        if matches!(p.kind, 9 | 13 | 18 | 19) {
            ensure!(
                p.additional_copies == 0,
                "procedural effect requests unsupported copies"
            );
        }
        let material_key = if matches!(geometry, Geometry::Model { .. } | Geometry::NoDraw { .. })
            || p.resource_slot == 255
        {
            None
        } else {
            Some(MaterialKey {
                texture: if screen_texture.is_some() {
                    TextureBank::Fixed(0)
                } else {
                    texture_bank(id.bank, p.resource_slot)?
                },
                color: p.palettes[0],
                // The final screen pass calls every draw with mode 2, regardless of
                // the ordinary dual-texture flag (484D8 -> 47FA4).
                alpha: (screen_texture.is_some() || flags & 0x4000000 != 0)
                    .then_some(p.palettes[1]),
                stride: p.palette_stride,
            })
        };
        let material = material_key.map(|key| self.material(key)).transpose()?;
        ensure!(
            p.kind == 17 || !matches!(geometry, Geometry::NoDraw { .. }) || uv_track == 255,
            "nondrawing effect has unsupported texture animation"
        );
        let uv_animation = (p.kind != 17 && uv_track != 255)
            .then(|| {
                uv_animation(
                    bank,
                    uv_track,
                    matches!(geometry, Geometry::Model { .. }),
                    |color| {
                        self.material(MaterialKey {
                            color,
                            ..material_key.context("palette animation requires an atlas")?
                        })
                    },
                )
            })
            .transpose()?;
        let mut element_variants = Vec::new();
        let use_element_variant = flags & 0x400000 != 0;
        if use_element_variant || variants_required {
            ensure!(
                matches!(p.kind, 4..=11 | 15 | 19),
                "unsupported elemental effect geometry"
            );
            for (i, element) in Element::ALL.into_iter().enumerate() {
                let material = material_key
                    .map(|key| {
                        self.material(MaterialKey {
                            color: self.element_palettes[i],
                            ..key
                        })
                    })
                    .transpose()?;
                element_variants.push(ElementVariant {
                    element,
                    material,
                    outer_color: self.element_colors[i],
                });
            }
        }
        let first = p.colors[0];
        Ok(EffectActor {
            id,
            geometry,
            material,
            palette: None,
            blend_variants: BTreeSet::new(),
            screen_texture,
            element_variants,
            use_element_variant,
            blend: match p.blend {
                0 => Blend::Alpha,
                1 => Blend::Additive,
                _ => Blend::Subtractive,
            },
            uv: p.uv,
            uv_animation,
            depth_test: flags & 0x80000 == 0,
            depth_write: flags & 0x800000 != 0,
            cull_back: flags & 0x10000000 != 0,
            owner_layer: if flags & 0x200000 != 0 {
                Some(OwnerLayer::Before)
            } else if flags & 0x2000 != 0 {
                Some(OwnerLayer::After)
            } else {
                None
            },
            during_pause: flags & 0x100 != 0,
            retained: flags & 0x40000 != 0,
            lifetime: nonzero(p.lifetime as u16),
            orientation,
            space,
            follow_emitter: flags & 0x400 != 0,
            bottom_anchored: flags & 2 != 0,
            ground_relative: flags & 0x1000 != 0,
            ground: (p.kind != 17)
                .then(|| ground_response(&p.secondary, id.bank, flags))
                .flatten(),
            periodic: if p.kind != 17 && p.secondary.flags & 8 != 0 {
                Some(PeriodicEmission {
                    effect: EffectId {
                        bank: id.bank,
                        id: p.secondary.periodic_program,
                    },
                    period: std::num::NonZeroU8::new(p.secondary.period)
                        .context("periodic particle emission has zero period")?,
                })
            } else {
                None
            },
            position: actor_vector(p.position),
            velocity: actor_vector(p.velocity),
            acceleration: actor_vector(p.acceleration),
            acceleration_change: if matches!(p.kind, 8 | 10) {
                [0.; 3]
            } else {
                actor_vector(p.acceleration_change_or_segment_offset)
            },
            angles: actor_vector(p.angles),
            angular_velocity: if p.kind == 17 {
                // The last word becomes the integer history count at initialization.
                [
                    p.angular_velocity[0].finite()?,
                    p.angular_velocity[1].finite()?,
                    0.,
                ]
            } else {
                actor_vector(p.angular_velocity)
            },
            local_offset: actor_vector(p.local_offset),
            local_velocity: actor_vector(p.local_velocity),
            dimensions: actor_vector(dimensions),
            dimension_velocity: if p.kind == 17 {
                [0.; 3]
            } else if p.kind == 18 {
                // The middle component is the ribbon's spatial jitter divisor.
                [
                    f32::from_bits(dimension_velocity[0].bits()),
                    0.,
                    f32::from_bits(dimension_velocity[2].bits()),
                ]
            } else {
                actor_vector(dimension_velocity)
            },
            dimension_acceleration: if matches!(p.kind, 8 | 17) {
                [0.; 3]
            } else {
                actor_vector(dimension_acceleration)
            },
            dimension_acceleration_until: nonzero(p.dimension_acceleration_until as u16),
            colors: [first, if flags & 8 != 0 { p.colors[1] } else { first }],
            color_gradient: flags & 8 != 0,
            brighten: p.brighten,
            darken: p.darken,
            brighten_until: p.brighten_until,
            darken_from: p.darken_from,
            copies: p
                .additional_copies
                .checked_add(1)
                .context("effect copy count overflow")?,
            copy_rotation: p.copy_rotation,
        })
    }
    fn material(&mut self, key: MaterialKey) -> Result<u16> {
        if let Some(&i) = self.material_indices.get(&key) {
            return Ok(i);
        }
        let bytes = self
            .textures
            .get(&key.texture)
            .context("effect requires an absent texture bank")?;
        let textures = tpl::parse_tpl(bytes)?;
        ensure!(
            (1..=2).contains(&textures.len()) && (key.alpha.is_none() || textures.len() == 2),
            "effect atlas is missing its requested color or alpha image"
        );
        let decode = |image: usize, palette: u8| -> Result<Vec<u8>> {
            let texture = &textures[image];
            let stride = if key.stride != 0 {
                usize::from(key.stride)
            } else if texture.format == 9 {
                256
            } else {
                16
            };
            Ok(tpl::decode_palette_window(
                bytes,
                texture,
                usize::from(palette) * stride,
            )?)
        };
        let mut rgba = decode(0, key.color)?;
        if let Some(palette) = key.alpha {
            ensure!(
                textures[0].width == textures[1].width && textures[0].height == textures[1].height,
                "effect alpha atlas dimensions differ"
            );
            let alpha = decode(1, palette)?;
            for (pixel, mask) in rgba.chunks_exact_mut(4).zip(alpha.chunks_exact(4)) {
                pixel[3] = mask[3];
            }
        }
        let width = u32::from(textures[0].width);
        let height = u32::from(textures[0].height);
        let mut digest = Sha256::new();
        digest.update(width.to_le_bytes());
        digest.update(height.to_le_bytes());
        digest.update(&rgba);
        let digest = format!("{:x}", digest.finalize());
        let path = format!("battle/effects/{digest}.ktx2");
        if let Some(existing) = self
            .result
            .materials
            .iter()
            .position(|m| m.texture.path == path)
        {
            let existing = u16::try_from(existing)?;
            self.material_indices.insert(key, existing);
            return Ok(existing);
        }
        let index = u16::try_from(self.result.materials.len())?;
        self.pending_images.push((index, rgba));
        self.result.materials.push(EffectMaterial {
            texture: UiTexture {
                path,
                width,
                height,
            },
            rgb_scale: 2.,
        });
        self.material_indices.insert(key, index);
        Ok(index)
    }
}
fn ground_response(
    secondary: &actor::Secondary,
    bank: EffectBank,
    flags: u32,
) -> Option<GroundResponse> {
    let effect = (secondary.flags & 4 != 0).then_some(EffectId {
        bank,
        id: secondary.ground_program,
    });
    if flags & 1 != 0 {
        Some(GroundResponse::Bounce { effect })
    } else if let Some(effect) = effect {
        Some(GroundResponse::EmitOnce {
            effect,
            clamp: flags & 0x8000000 != 0,
        })
    } else {
        (flags & 0x8000000 != 0).then_some(GroundResponse::Clamp)
    }
}

fn actor_vector(vector: [crate::read::FloatOperand; 3]) -> [f32; 3] {
    vector.map(|value| f32::from_bits(value.bits()))
}

fn nonzero(value: u16) -> Option<u16> {
    (value != 0).then_some(value)
}
fn modifiers(bytes: &[u8], at: usize) -> Result<Vec<Modifier>> {
    decode_modifiers(bytes, at, false)
}

fn decode_modifiers(bytes: &[u8], mut at: usize, single_birth: bool) -> Result<Vec<Modifier>> {
    use modifiers::{Operation as Op, Record};
    use resonance_content::battle::effect_inventory::{
        EffectArithmetic as A, EffectIntegerWidth as W,
    };
    if at == 0 {
        return Ok(Vec::new());
    }
    let arithmetic = |operation| match operation {
        A::Set => Arithmetic::Set,
        A::Add => Arithmetic::Add,
        A::Subtract => Arithmetic::Subtract,
        A::Multiply => Arithmetic::Multiply,
        A::Divide => Arithmetic::Divide,
    };
    let floating = |value: modifiers::Floating| -> Result<FloatValue> {
        Ok(match value.selector {
            selector @ 0x7ff8..=0x7ffb => FloatValue::Temporary((selector - 0x7ff8) as u8),
            0x7ffc..=0x7fff => bail!("invalid float temporary selector"),
            _ => FloatValue::Constant(value.literal.finite()?),
        })
    };
    let compact_float = |value: i16| -> Result<FloatValue> {
        Ok(match value as u16 {
            selector @ 0x7ff8..=0x7ffb => FloatValue::Temporary((selector - 0x7ff8) as u8),
            0x7ffc..=0x7fff => bail!("integer temporary used as a vector scalar"),
            _ => FloatValue::Constant(f32::from(value) * 0.1),
        })
    };
    let axis = |value| -> Result<VectorAxis> {
        Ok(match value {
            0 => VectorAxis::X,
            1 => VectorAxis::Y,
            2 => VectorAxis::Z,
            _ => bail!("invalid effect vector axis {value}"),
        })
    };
    let mut out = Vec::new();
    for _ in 0..RECORD_LIMIT {
        let (record, next) = modifiers::read(bytes, at)?;
        let Record::Instruction { instruction } = record else {
            return Ok(out);
        };
        let destination = instruction.destination as u16;
        let instruction = match instruction.operation {
            Op::SetVector { value } => {
                for (axis, value) in value.into_iter().enumerate() {
                    out.push(Modifier::Float {
                        field: float_field(
                            destination
                                .checked_add(axis as u16 * 4)
                                .context("effect vector field overflow")?,
                        )?,
                        operation: Arithmetic::Set,
                        value: FloatValue::Constant(value.finite()?),
                    });
                }
                at = next;
                continue;
            }
            Op::SetColor { color } => {
                ensure!(
                    !(0x7ff8..=0x7fff).contains(&destination) || destination == 0x7ffc,
                    "unsupported effect color temporary span {destination:#x}"
                );
                for (channel, value) in color.into_iter().enumerate() {
                    let field = if destination == 0x7ffc {
                        IntegerField::Temporary(channel as u8)
                    } else {
                        integer_field(
                            destination
                                .checked_add(channel as u16 * 2)
                                .context("effect color field overflow")?,
                        )?
                    };
                    out.push(Modifier::Integer {
                        field,
                        operation: Arithmetic::Set,
                        // The block copy never interprets channel values as selectors.
                        value: IntegerValue::Constant(value),
                    });
                }
                at = next;
                continue;
            }
            Op::RandomFloat { literal, selector } => {
                let range = if selector == 0 {
                    FloatRandomRange::Remainder(literal as u16)
                } else {
                    // A float selector leaves the caller's integer scratch unchanged.
                    // Only a proven fresh, single birth can specialize that value to zero.
                    ensure!(
                        single_birth && out.is_empty() && (0x7ff8..=0x7ffb).contains(&selector),
                        "unsupported random float selector context"
                    );
                    FloatRandomRange::Signed
                };
                Modifier::RandomFloat {
                    field: float_field(destination)?,
                    range,
                    scale: 0.1,
                }
            }
            Op::EmitterAxisVector {
                axis: component,
                value,
            } => {
                let field = vector_field(destination)?;
                ensure!(
                    matches!(field, VectorField::Velocity | VectorField::Acceleration),
                    "unsupported heading-relative vector destination {destination:#x}"
                );
                Modifier::SetEmitterAxisVector {
                    field,
                    axis: axis(i16::from(component))?,
                    value: floating(value)?,
                }
            }
            Op::RotateVector {
                axis: component,
                angle,
            } => Modifier::RotateVector {
                field: vector_field(destination)?,
                axis: axis(component)?,
                angle: compact_float(angle)?,
            },
            Op::PolarVector { radius, angle } => Modifier::PolarVector {
                field: vector_field(destination)?,
                radius: compact_float(radius)?,
                angle: compact_float(angle)?,
            },
            Op::ModelAnimation {
                model,
                clip,
                blend_ticks,
                rate_percent,
                flags,
                storage,
            } => {
                ensure!(blend_ticks >= 0, "negative effect model blend duration");
                ensure!(
                    destination == 0x5c
                        && (0..10).contains(&model)
                        && (0..4).contains(&clip)
                        && flags & !8 == 0
                        && storage == 0,
                    "unsupported model animation command"
                );
                Modifier::PlayModelAnimation {
                    animation: ModelAnimation {
                        model: model as u8,
                        clip: clip as u8,
                        blend_ticks: blend_ticks as u16,
                        rate: if rate_percent == 0 {
                            0.5
                        } else {
                            f32::from(rate_percent) * 0.01
                        },
                        hold: flags & 8 != 0,
                    },
                }
            }
            Op::AnimationPosition { value } => Modifier::AnimationPosition {
                value: floating(value)?,
            },
            Op::AnimationRate { value } => Modifier::AnimationRate {
                value: floating(value)?,
            },
            Op::SetFlags { flags } | Op::ClearFlags { flags } => {
                // These instructions always address the flag word, ignoring destination.
                let enabled = matches!(instruction.operation, Op::SetFlags { .. });
                const FIELDS: [(u32, EffectFlag); 9] = [
                    (2, EffectFlag::BottomAnchored),
                    (8, EffectFlag::ColorGradient),
                    (0x20, EffectFlag::RibbonDepth),
                    (0x400, EffectFlag::FollowEmitter),
                    (0x1000, EffectFlag::GroundRelative),
                    (0x80000, EffectFlag::DepthTest),
                    (0x400000, EffectFlag::UseElementVariant),
                    (0x800000, EffectFlag::DepthWrite),
                    (0x10000000, EffectFlag::CullBack),
                ];
                let supported = FIELDS.iter().fold(0, |mask, (bit, _)| mask | bit);
                ensure!(
                    flags & !supported == 0,
                    "unsupported effect flag modifier {flags:#x}"
                );
                for (bit, field) in FIELDS {
                    if flags & bit != 0 {
                        out.push(Modifier::Flag {
                            field,
                            enabled: enabled ^ (field == EffectFlag::DepthTest),
                        });
                    }
                }
                at = next;
                continue;
            }
            Op::Integer {
                width: W::Byte,
                operation,
                value,
                ..
            } => Modifier::Byte {
                field: match destination {
                    1 => ByteField::Blend,
                    3 => ByteField::Palette,
                    0x28..=0x2b => ByteField::Brighten((destination - 0x28) as u8),
                    0x2c..=0x2f => ByteField::Darken((destination - 0x2c) as u8),
                    0x30 => ByteField::BrightenUntil,
                    0x31 => ByteField::DarkenFrom,
                    0x12 => ByteField::GeometryCount,
                    0x13 => ByteField::NoisePhase,
                    0xd4 => ByteField::ModelIndex,
                    0x8d => ByteField::ModelTextureFrame,
                    0x8c => ByteField::BoneSelector,
                    _ => bail!("unsupported effect byte field {destination:#x}"),
                },
                operation: arithmetic(operation),
                value: integer_value(value as u16)?,
            },
            Op::Integer {
                width: W::Halfword,
                operation,
                value,
                ..
            } => {
                let operation = arithmetic(operation);
                let value = integer_value(value as u16)?;
                if destination == 0x10 {
                    Modifier::Lifetime { operation, value }
                } else {
                    Modifier::Integer {
                        field: integer_field(destination)?,
                        operation,
                        value,
                    }
                }
            }
            Op::RandomInteger { modulus, .. } => Modifier::RandomInteger {
                field: integer_field(destination)?,
                modulus: modulus as u16,
            },
            Op::Float { operation, value } => Modifier::Float {
                field: float_field(destination)?,
                operation: arithmetic(operation),
                value: floating(value)?,
            },
            Op::RandomPolarVector {
                angles,
                radius,
                radius_jitter,
                angle_jitter,
            } => {
                let [x, y, z] = angles.map(|value| value.finite());
                Modifier::RandomPolarVector {
                    field: vector_field(destination)?,
                    angles: [x?, y?, z?],
                    radius: radius.finite()?,
                    radius_jitter,
                    angle_jitter,
                }
            }
            other => bail!("unsupported required effect modifier {other:?} at {at:#x}"),
        };
        out.push(instruction);
        at = next;
    }
    bail!("effect modifiers exceed record limit")
}

/// Bind a new model before it can be presented. Ordinary births advance the retained
/// cursor, so its first eight slots are unambiguous even when earlier actors expire.
fn bind_retained_birth_animations(
    program: &mut EffectProgram,
    content: &BattleEffectPrograms,
) -> Result<()> {
    let mut births = Vec::new();
    let mut folds = Vec::new();
    for (index, emission) in program.emissions.iter().enumerate() {
        match &emission.command {
            EffectCommand::Particle {
                actor, attachment, ..
            } if content.actor(*actor).is_some_and(|a| a.retained) => {
                if emission.repeat.is_some()
                    || !matches!(attachment, Attachment::Emitter)
                    || births.len() == RETAINED_EFFECT_SLOTS
                {
                    break;
                }
                births.push(index);
            }
            EffectCommand::ModifyRetained { slot, modifiers } if emission.repeat.is_none() => {
                let [modifier @ Modifier::PlayModelAnimation { .. }] = modifiers.as_slice() else {
                    continue;
                };
                let Some(&birth) = births.get(usize::from(*slot)) else {
                    continue;
                };
                let EffectCommand::Particle {
                    actor, modifiers, ..
                } = &program.emissions[birth].command
                else {
                    unreachable!()
                };
                if !modifiers.is_empty() || program.emissions[birth].tick != emission.tick {
                    continue;
                }
                // Constructors only capture these children; retained joint poses are sampled
                // after the command group. No sound, modifier, repeat or other work may cross.
                if !program.emissions[birth + 1..index].iter().all(|e| e.tick == emission.tick && e.repeat.is_none()
                    && matches!(&e.command, EffectCommand::Particle {actor, attachment: Attachment::Emitter, modifiers}
                        if modifiers.is_empty() && content.actor(*actor).is_some_and(|a| !a.retained))) {
                    continue;
                }
                let actor = content
                    .actor(*actor)
                    .context("missing retained model birth")?;
                if !matches!(
                    actor.geometry,
                    Geometry::Model {
                        animation: None,
                        presentation: ModelPresentation {
                            external_animation: true,
                            ..
                        },
                        ..
                    }
                ) {
                    continue;
                }
                actor.validate_modifiers(&[*modifier])?;
                folds.push((birth, index, *modifier));
            }
            _ => {}
        }
    }
    let mut removed = BTreeSet::new();
    for (birth, index, modifier) in folds {
        let EffectCommand::Particle { modifiers, .. } = &mut program.emissions[birth].command
        else {
            unreachable!()
        };
        modifiers.push(modifier);
        removed.insert(index);
    }
    let mut index = 0;
    program.emissions.retain(|_| {
        let keep = !removed.contains(&index);
        index += 1;
        keep
    });
    Ok(())
}

fn retained_command(bytes: &[u8], slot: u8, modifier: u16) -> Result<EffectCommand> {
    ensure!(
        usize::from(slot) < RETAINED_EFFECT_SLOTS,
        "invalid retained effect slot {slot}"
    );
    let offset = usize::from(modifier);
    ensure!(offset != 0, "retained effect command has no modifier block");
    Ok(EffectCommand::ModifyRetained {
        slot,
        modifiers: modifiers(bytes, offset)
            .with_context(|| format!("retained slot {slot}, modifier {offset:#x}"))?,
    })
}

#[test]
fn healing_ring_columns_and_staff_spiral_direction_decode_as_geometry_fields() {
    let mut bytes = vec![0; 8];
    bytes.extend([0, 10, 0, 0x12, 0, 4, 0, 0]);
    bytes.extend([0, 14, 0, 0x84, 0xbf, 0x80, 0, 0, 0, 0, 0, 0]);
    bytes.extend([0, 21, 0, 0x14, 0x10, 0, 0, 0]);
    bytes.extend([255, 255]);
    let parsed = modifiers(&bytes, 8).unwrap();
    assert!(matches!(
        parsed[0],
        Modifier::Byte {
            field: ByteField::GeometryCount,
            operation: Arithmetic::Set,
            value: IntegerValue::Constant(4)
        }
    ));
    assert!(matches!(
        parsed[1],
        Modifier::Float {
            field: FloatField::GeometryAngleStep,
            operation: Arithmetic::Multiply,
            value: FloatValue::Constant(-1.)
        }
    ));
    assert!(matches!(
        parsed[2],
        Modifier::Flag {
            field: EffectFlag::CullBack,
            enabled: true
        }
    ));
}

#[test]
fn vector_and_color_copies_preserve_literals_and_following_instructions() {
    let mut bytes = vec![0; 8];
    bytes.extend([0, 1, 0, 0x98]);
    for value in [-48f32, 0., 72.] {
        bytes.extend(value.to_be_bytes());
    }
    bytes.extend([0, 10, 0, 0x30, 0, 8, 0, 0, 255, 255]);
    let parsed = modifiers(&bytes, 8).unwrap();
    assert_eq!(parsed.len(), 4);
    for (axis, expected) in [-48., 0., 72.].into_iter().enumerate() {
        let Modifier::Float {
            field: FloatField::LocalOffset(actual_axis),
            operation: Arithmetic::Set,
            value: FloatValue::Constant(value),
        } = parsed[axis]
        else {
            panic!("vector component was not a literal float assignment")
        };
        assert_eq!(usize::from(actual_axis), axis);
        assert_eq!(value, expected);
    }
    assert!(matches!(
        parsed[3],
        Modifier::Byte {
            field: ByteField::BrightenUntil,
            ..
        }
    ));
    assert!(modifiers(&bytes[..20], 8).is_err());

    for destination in [0x18u16, 0x20, 0x7ffc] {
        let mut bytes = vec![0; 8];
        for value in [18, destination, u16::MAX, 0x7ffc, 128, 96] {
            bytes.extend(value.to_be_bytes());
        }
        bytes.extend([0, 10, 0, 0x30, 0, 8, 0, 0, 255, 255]);
        let parsed = modifiers(&bytes, 8).unwrap();
        assert_eq!(parsed.len(), 5);
        for (channel, expected) in [-1, 0x7ffc, 128, 96].into_iter().enumerate() {
            let Modifier::Integer {
                field,
                operation: Arithmetic::Set,
                value: IntegerValue::Constant(value),
            } = parsed[channel]
            else {
                panic!("color component was not a literal integer assignment")
            };
            assert_eq!(value, expected);
            match field {
                IntegerField::Color {
                    end,
                    channel: actual,
                } => {
                    assert_eq!(u16::from(end), (destination - 0x18) / 8);
                    assert_eq!(usize::from(actual), channel);
                }
                IntegerField::Temporary(actual) => {
                    assert_eq!(destination, 0x7ffc);
                    assert_eq!(usize::from(actual), channel);
                }
                _ => panic!("incorrect color destination"),
            }
        }
        assert!(matches!(
            parsed[4],
            Modifier::Byte {
                field: ByteField::BrightenUntil,
                ..
            }
        ));
        assert!(modifiers(&bytes[..19], 8).is_err());
        for invalid in [0x7ff8u16, 0x7ffb, 0x7ffd, 0xfffe] {
            bytes[10..12].copy_from_slice(&invalid.to_be_bytes());
            assert!(modifiers(&bytes, 8).is_err());
        }
    }
}

fn integer_value(v: u16) -> Result<IntegerValue> {
    Ok(if (0x7ffc..=0x7fff).contains(&v) {
        IntegerValue::Temporary((v - 0x7ffc) as u8)
    } else {
        ensure!(
            !(0x7ff8..=0x7ffb).contains(&v),
            "float temporary used as integer"
        );
        IntegerValue::Constant(v as i16)
    })
}
fn integer_field(v: u16) -> Result<IntegerField> {
    Ok(match v {
        3 => IntegerField::PaletteSelection,
        0xd4 => IntegerField::ModelSelection,
        0x8c => IntegerField::ModelTextureSelection,
        8 => IntegerField::U,
        10 => IntegerField::V,
        12 => IntegerField::Width,
        14 => IntegerField::Height,
        0x18..=0x26 if v.is_multiple_of(2) => IntegerField::Color {
            end: ((v - 0x18) / 8) as u8,
            channel: ((v - 0x18) % 8 / 2) as u8,
        },
        0x7ff8..=0x7ffb => IntegerField::FloatTemporaryHigh((v - 0x7ff8) as u8),
        0x7ffc..=0x7fff => IntegerField::Temporary((v - 0x7ffc) as u8),
        _ => bail!("unsupported effect integer field {v:#x}"),
    })
}
fn vector_field(v: u16) -> Result<VectorField> {
    Ok(match v {
        0x34 => VectorField::Position,
        0x40 => VectorField::Velocity,
        0x4c => VectorField::Acceleration,
        0x58 => VectorField::Angle,
        0x64 => VectorField::AngularVelocity,
        0x98 => VectorField::LocalOffset,
        0xa4 => VectorField::LocalVelocity,
        0xb0 => VectorField::Dimension,
        0xbc => VectorField::DimensionVelocity,
        0xc8 => VectorField::DimensionAcceleration,
        0x7ff8..=0x7ff9 => VectorField::Temporary((v - 0x7ff8) as u8),
        _ => bail!("unsupported effect vector field {v:#x}"),
    })
}

fn float_field(v: u16) -> Result<FloatField> {
    let axis = |base| ((v - base) / 4) as u8;
    ensure!(
        v.is_multiple_of(4) || (0x7ff8..=0x7ffb).contains(&v),
        "unaligned effect float field"
    );
    Ok(match v {
        0x34..=0x3c => FloatField::Position(axis(0x34)),
        0x40..=0x48 => FloatField::Velocity(axis(0x40)),
        0x4c..=0x54 => FloatField::Acceleration(axis(0x4c)),
        0x58..=0x60 => FloatField::Angle(axis(0x58)),
        0x64..=0x6c => FloatField::AngularVelocity(axis(0x64)),
        0x70..=0x78 => FloatField::GeometrySegmentOffset(axis(0x70)),
        0x84 => FloatField::GeometryAngleStep,
        0x98..=0xa0 => FloatField::LocalOffset(axis(0x98)),
        0xa4..=0xac => FloatField::LocalVelocity(axis(0xa4)),
        0xb0..=0xb8 => FloatField::Dimension(axis(0xb0)),
        0xbc..=0xc4 => FloatField::DimensionVelocity(axis(0xbc)),
        0xc8..=0xd0 => FloatField::DimensionAcceleration(axis(0xc8)),
        0x7ff8..=0x7ffb => FloatField::Temporary((v - 0x7ff8) as u8),
        _ => bail!("unsupported effect float field {v:#x}"),
    })
}

fn uv_animation(
    bank: &[u8],
    index: u8,
    model: bool,
    mut palette: impl FnMut(u8) -> Result<u16>,
) -> Result<UvAnimation> {
    let mut frames = Vec::new();
    for command in uv::commands(bank, u16::from(index) * 10)? {
        let command = match command {
            // A first high-bit row initializes scrolling directly; it is not
            // entered through the ordinary next-keyframe payload copy.
            uv::Command::End { values } if frames.is_empty() => uv::Command::Scroll {
                duration: UvEnd::INTERVAL,
                values,
            },
            command => command,
        };
        match command {
            uv::Command::End { values } => {
                let end = if values[0] == -32000 {
                    if model {
                        UvEnd::ModelPalette {
                            origin: [values[0], values[1]],
                            step: [values[2], values[3]],
                        }
                    } else {
                        UvEnd::Palette {
                            material: palette(values[1] as u8)?,
                            origin: [values[0], values[1]],
                            step: [values[2], values[3]],
                        }
                    }
                } else {
                    UvEnd::Rect { rect: values }
                };
                frames.push(UvFrame {
                    duration: UvEnd::INTERVAL,
                    update: UvUpdate::End { end },
                });
                return Ok(UvAnimation {
                    frames,
                    loop_to: None,
                    model_scroll: None,
                });
            }
            uv::Command::Loop { target } => {
                return Ok(UvAnimation {
                    frames,
                    loop_to: Some(target),
                    model_scroll: None,
                });
            }
            uv::Command::Frame { duration, update } => {
                let update = match update {
                    uv::Update::Palette { index } => {
                        if model {
                            UvUpdate::ModelPalette { index }
                        } else {
                            UvUpdate::Palette {
                                material: palette(index)?,
                            }
                        }
                    }
                    uv::Update::Rect { rect } => UvUpdate::Rect { rect },
                };
                frames.push(UvFrame { duration, update });
            }
            uv::Command::Scroll { values, .. } if model && frames.is_empty() => {
                let scroll = ModelUvScroll {
                    step: [values[0], values[1]],
                    period: [values[2], values[3]],
                };
                return Ok(UvAnimation {
                    frames,
                    loop_to: None,
                    model_scroll: Some(scroll),
                });
            }
            uv::Command::Scroll { duration, values } if !model && frames.is_empty() => {
                return Ok(UvAnimation {
                    frames: vec![UvFrame {
                        duration,
                        update: UvUpdate::Scroll {
                            origin: [values[0], values[1]],
                            step: [values[2], values[3]],
                        },
                    }],
                    loop_to: None,
                    model_scroll: None,
                });
            }
            _ => bail!("unsupported effect UV scrolling command"),
        }
    }
    bail!("unterminated effect UV animation")
}

#[test]
fn terminal_uv_rows_preserve_entry_palette_and_later_scroll_operands() -> Result<()> {
    let mut bank = vec![0; 40];
    bank[14..16].copy_from_slice(&20u16.to_be_bytes());
    bank[20] = 3;
    bank[30] = 254;
    for (axis, value) in [-32000i16, 0x1234, -2, 3].into_iter().enumerate() {
        bank[32 + axis * 2..34 + axis * 2].copy_from_slice(&value.to_be_bytes());
    }
    let animation = uv_animation(&bank, 0, false, |index| {
        assert_eq!(index, 0x34);
        Ok(7)
    })?;
    let animation: UvAnimation = serde_json::from_value(serde_json::to_value(animation)?)?;
    assert_eq!(animation.frames.len(), 2);
    assert_eq!(animation.frames[1].duration, UvEnd::INTERVAL);
    assert_eq!(animation.frames[1].update.material(), Some(7));
    assert_eq!(
        animation.frames[1].update.scroll(),
        Some(([-32000, 0x1234], [-2, 3]))
    );
    let model = uv_animation(&bank, 0, true, |_| unreachable!())?;
    let model: UvAnimation = serde_json::from_value(serde_json::to_value(model)?)?;
    assert!(matches!(
        model.frames[1].update,
        UvUpdate::End {
            end: UvEnd::ModelPalette {
                origin: [-32000, 0x1234],
                step: [-2, 3]
            }
        }
    ));
    assert_eq!(model.frames[1].update.material(), None);
    assert_eq!(
        model.frames[1].update.scroll(),
        Some(([-32000, 0x1234], [-2, 3]))
    );
    // A directly selected high-bit row does not apply an entry palette update.
    let initial = uv_animation(&bank, 1, false, |_| unreachable!())?;
    assert!(matches!(initial.frames[0].update, UvUpdate::Scroll { .. }));
    assert_eq!(initial.frames[0].duration, UvEnd::INTERVAL);
    assert!(initial.frames[0].update.material().is_none());
    let initial = uv_animation(&bank, 1, true, |_| unreachable!())?;
    assert_eq!(initial.model_scroll.unwrap().step, [-32000, 0x1234]);
    assert_eq!(initial.model_scroll.unwrap().period, [-2, 3]);
    bank[20] = 2;
    for (axis, value) in [3i16, -2, 9, 7].into_iter().enumerate() {
        bank[32 + axis * 2..34 + axis * 2].copy_from_slice(&value.to_be_bytes());
    }
    let model = uv_animation(&bank, 0, true, |_| unreachable!())?;
    let model: UvAnimation = serde_json::from_value(serde_json::to_value(model)?)?;
    assert!(model.model_scroll.is_none()); // Rendering still tests the original low-bit root.
    assert_eq!(model.frames[0].duration, 2);
    assert!(matches!(
        model.frames[1].update,
        UvUpdate::End {
            end: UvEnd::Rect {
                rect: [3, -2, 9, 7]
            }
        }
    ));
    assert_eq!(model.frames[1].duration, UvEnd::INTERVAL);
    Ok(())
}

#[test]
#[ignore = "requires original battle UV records and native code; no texture cooking"]
fn original_common33_terminal_uv_retains_zero_rectangle_on_both_discs() -> Result<()> {
    for disc in ["disc1", "disc2"] {
        let files = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/extracted")
            .join(disc)
            .join("files");
        let usual = fs::read(files.join("BTL/BTLusual.dat"))?;
        let bank = member(&usual, 2)?;
        let actor = actor_source(
            bank,
            EffectId {
                bank: EffectBank::Common,
                id: 46,
            },
        )?;
        assert_eq!((actor[0], half(actor, 16)?, actor[50]), (15, 16, 5));
        let births = source::program(bank, 33)?
            .into_iter()
            .filter(|event| event.repeat.is_none())
            .filter_map(|event| match event.command {
                source::Command::Emit {
                    actor: 46,
                    modifier,
                    ..
                } => Some((event.tick, nonzero(modifier))),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(births, [(0, None), (4, Some(32100)), (8, Some(32128))]);
        let animation = uv_animation(bank, actor[50], false, |_| unreachable!())?;
        assert_eq!(
            animation
                .frames
                .iter()
                .map(|frame| frame.duration)
                .collect::<Vec<_>>(),
            [3, 3, 3, 3, UvEnd::INTERVAL]
        );
        for (index, frame) in animation.frames[..4].iter().enumerate() {
            assert!(
                matches!(frame.update, UvUpdate::Rect { rect } if rect == [index as i16 * 32, 144, 32, 32])
            );
        }
        assert!(matches!(
            animation.frames[4].update,
            UvUpdate::End {
                end: UvEnd::Rect { rect: [0, 0, 0, 0] }
            }
        ));
        let start = usize::from(half(bank, 14)?) + 90;
        assert_eq!(&bank[start..start + 10], &[254, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let rel = fs::read(files.join("US_r_Top2Btl.rel"))?;
        let code = (word(&rel, word(&rel, 16)? as usize + 8)? & !3) as usize;
        for (at, instruction) in [
            (0x40b40, 0x54a00631),
            (0x40c44, 0x7c000774),
            (0x40c80, 0x2c00ffff),
            (0x40ca4, 0xa8030002),
            (0x40cbc, 0xb01f0008),
            (0x40cd4, 0xb01f000e),
        ] {
            assert_eq!(word(&rel, code + at)?, instruction);
        }
    }
    Ok(())
}

#[test]
fn uv_tracks_resolve_palette_frames_and_distinguish_model_scroll() {
    let mut bank = vec![0; 50];
    bank[14..16].copy_from_slice(&20u16.to_be_bytes());
    for (at, palette) in [(20, 27u16), (30, 28)] {
        bank[at] = 2;
        bank[at + 2..at + 4].copy_from_slice(&(-32000i16).to_be_bytes());
        bank[at + 4..at + 6].copy_from_slice(&palette.to_be_bytes());
    }
    bank[40] = 255;
    let animation = uv_animation(&bank, 0, false, |palette| Ok(u16::from(palette) - 20)).unwrap();
    assert_eq!(animation.loop_to, Some(0));
    assert!(matches!(
        animation.frames[0].update,
        UvUpdate::Palette { material: 7 }
    ));
    assert!(matches!(
        animation.frames[1].update,
        UvUpdate::Palette { material: 8 }
    ));
    let model = uv_animation(&bank, 0, true, |_| unreachable!()).unwrap();
    assert_eq!(model.loop_to, Some(0));
    assert!(matches!(
        model.frames[0].update,
        UvUpdate::ModelPalette { index: 27 }
    ));
    assert!(matches!(
        model.frames[1].update,
        UvUpdate::ModelPalette { index: 28 }
    ));
    assert!(
        model
            .frames
            .iter()
            .all(|frame| frame.update.material().is_none())
    );
    bank[20] = 135; // A model ignores the low seven interval bits.
    for (axis, value) in [4i16, 0, 250, 250].into_iter().enumerate() {
        bank[22 + axis * 2..24 + axis * 2].copy_from_slice(&value.to_be_bytes());
    }
    let animation = uv_animation(&bank, 0, true, |_| unreachable!()).unwrap();
    assert!(animation.frames.is_empty());
    let scroll = animation.model_scroll.unwrap();
    assert_eq!(scroll.step, [4, 0]);
    assert_eq!(scroll.period, [250, 250]);
    let sprite = uv_animation(&bank, 0, false, |_| unreachable!()).unwrap();
    assert_eq!(sprite.frames[0].duration, 7);
    assert!(matches!(
        sprite.frames[0].update,
        UvUpdate::Scroll {
            origin: [4, 0],
            step: [250, 250]
        }
    ));
}

#[test]
fn controllers_decode_before_particle_modifiers_and_preserve_source_text() {
    let mut bytes = vec![0; 20 + ACTOR_BYTES];
    bytes[..4].copy_from_slice(b"ef1\0");
    bytes[8..10].copy_from_slice(&20u16.to_be_bytes());
    bytes[12..14].copy_from_slice(&((20 + ACTOR_BYTES) as u16).to_be_bytes());
    let row = &mut bytes[20..];
    row[0] = 24;
    row[0x10..0x12].copy_from_slice(&60u16.to_be_bytes());
    for (at, value) in [(0x18, 0x118u16), (0x1a, 0xff18), (0x1c, 24), (0x1e, 64)] {
        row[at..at + 2].copy_from_slice(&value.to_be_bytes());
    }
    // Attachment and modifier bytes are not consulted by controller dispatch.
    let emission = [0, 0, 0, 255, 255, 255];
    let EffectCommand::Controller {
        actor,
        controller: decoded,
    } = test_cooker()
        .particle_command(&bytes, EffectBank::Enemy(197), &emission)
        .unwrap()
    else {
        panic!("stage tint decoded as a particle")
    };
    assert_eq!(
        actor,
        EffectId {
            bank: EffectBank::Enemy(197),
            id: 0
        }
    );
    assert_eq!(
        decoded,
        EffectController::StageColor {
            color: [24, 24, 24, 64],
            duration: 60
        }
    );
    for duration in [0u16, u16::MAX] {
        bytes[20 + 0x10..20 + 0x12].copy_from_slice(&duration.to_be_bytes());
        assert!(controller(&bytes[20..]).is_err());
    }
    let row = &mut bytes[20..];
    row[0] = 25;
    // Enemy30/5 and Enemy35/5 contain this Japanese string in the US recipes.
    let text = [
        0x82, 0xbd, 0x82, 0xb7, 0x82, 0xaf, 0x82, 0xc4, 0x82, 0xa5, 0x81, 0x5b, 0x82, 0xf1, 0x81,
        0x40, 0x83, 0x8b, 0x83, 0x70, 0x81, 0x5b, 0x83, 0x93,
    ];
    row[0xb0..0xb0 + text.len()].copy_from_slice(&text);
    assert_eq!(
        controller(row).unwrap(),
        Some(EffectController::Caption {
            text: "たすけてぇーん　ルパーン".into(),
        })
    );
    row[0xb0..].fill(0x81);
    assert!(controller(row).is_err());
    row[0xb1] = 0;
    assert!(controller(row).is_err());
}

#[test]
fn shake_decodes_integer_amplitude_before_unused_particle_data() {
    let mut bytes = vec![0xff; 20 + ACTOR_BYTES];
    bytes[..4].copy_from_slice(b"ef1\0");
    bytes[8..10].copy_from_slice(&20u16.to_be_bytes());
    bytes[12..14].copy_from_slice(&((20 + ACTOR_BYTES) as u16).to_be_bytes());
    bytes[20] = 22;
    // This bit pattern means a large integer amplitude, never IEEE 1.0.
    bytes[20 + 0x14..20 + 0x18].copy_from_slice(&0x3f80_0000u32.to_be_bytes());
    for duration in [0u16, 12, 514, i16::MAX as u16] {
        bytes[20 + 0x10..20 + 0x12].copy_from_slice(&duration.to_be_bytes());
        let EffectCommand::Controller { controller, .. } = test_cooker()
            .particle_command(&bytes, EffectBank::Techniques, &[0, 0, 0, 255, 255, 255])
            .unwrap()
        else {
            panic!("shake allocated a particle")
        };
        assert_eq!(
            controller,
            EffectController::Shake {
                duration,
                amplitude: 0x3f80_0000
            }
        );
    }
    bytes[20 + 0x10..20 + 0x12].copy_from_slice(&u16::MAX.to_be_bytes());
    assert!(controller(&bytes[20..]).is_err());
}

#[test]
fn camera_controller_decodes_radius_and_elevation_without_particle_requirements() {
    let mut row = [255; ACTOR_BYTES];
    row[0] = 23;
    row[0x10..0x12].copy_from_slice(&24u16.to_be_bytes());
    row[0xb0..0xb4].copy_from_slice(&12f32.to_be_bytes());
    row[0xb8..0xbc].copy_from_slice(&3000f32.to_be_bytes());
    assert_eq!(
        controller(&row).unwrap(),
        Some(EffectController::Camera {
            duration: 24,
            distance: 3000.,
            elevation: 12.,
        })
    );
    row[0xb8..0xbc].copy_from_slice(&f32::NAN.to_be_bytes());
    assert!(controller(&row).is_err());
    row[0xb8..0xbc].copy_from_slice(&3000f32.to_be_bytes());
    row[0x10..0x12].copy_from_slice(&u16::MAX.to_be_bytes());
    assert!(controller(&row).is_err());
}

#[test]
fn nondrawing_motion_actor_keeps_its_lifetime_and_ground_response() {
    let mut row = [0; ACTOR_BYTES];
    row[0] = 14;
    row[1] = 1;
    row[2] = 255;
    row[0x32] = 255;
    row[0x10..0x12].copy_from_slice(&24u16.to_be_bytes());
    row[0x14..0x18].copy_from_slice(&0x29u32.to_be_bytes());
    row[0x50..0x54].copy_from_slice(&(-2.5f32).to_be_bytes());
    row[0xb0..0xb4].copy_from_slice(&128f32.to_be_bytes());
    row[0xb4..0xb8].copy_from_slice(&3f32.to_be_bytes());
    let id = EffectId {
        bank: EffectBank::Common,
        id: 10,
    };
    let actor = test_cooker().actor(&row, id, &[], false).unwrap();
    assert!(matches!(
        actor.geometry,
        Geometry::NoDraw {
            motion: NoDrawMotion::Origin
        }
    ));
    assert_eq!(actor.lifetime, Some(24));
    assert_eq!(actor.acceleration, [0., -2.5, 0.]);
    assert!(actor.material.is_none());
    assert!(matches!(
        actor.ground,
        Some(GroundResponse::Bounce { effect: None })
    ));
    row[0] = 16;
    assert_eq!(
        test_cooker().actor(&row, id, &[], false).unwrap().geometry,
        actor.geometry
    );
    row[0] = 17;
    row[0x12] = 8;
    row[0x32] = 0; // The history initializer never reads this UV track.
    row[0x91..0x96].copy_from_slice(&[12, 253, 0, 0, 254]);
    row[0xbc..0x11c].fill(255); // Cleared history slots, not float motion data.
    let history = test_cooker().actor(&row, id, &[], false).unwrap();
    assert_eq!(
        history.geometry,
        Geometry::NoDraw {
            motion: NoDrawMotion::PointHistory { capacity: 8 }
        }
    );
    assert!(history.uv_animation.is_none() && history.material.is_none());
    assert!(history.ground.is_none() && history.periodic.is_none());
    assert_eq!(history.dimension_velocity, [0.; 3]);
    row[0] = 2;
    assert!(
        test_cooker()
            .actor(&row, id, &[], false)
            .unwrap_err()
            .to_string()
            .contains("null native draw dispatch")
    );
    row[0] = 14;
    row[0x12] = 0;
    row[0x32] = 255;
    row[0x91..0x96].fill(0);
    row[0xbc..0x11c].fill(0);
    row[0x17] = 0x28;
    row[0x91] = 4;
    row[0x95] = 3;
    let actor = test_cooker().actor(&row, id, &[], false).unwrap();
    assert!(
        matches!(actor.ground, Some(GroundResponse::EmitOnce { effect, clamp: false }) if effect == EffectId { id: 3, ..id })
    );
    // One parent closes both secondary roots, including aliased child timelines.
    row[0x14..0x18].copy_from_slice(&0x08000000u32.to_be_bytes());
    row[0x91..0x96].copy_from_slice(&[12, 2, 3, 0, 1]);
    let events = 20 + 2 * ACTOR_BYTES;
    let table = events + 24;
    let mut bank = vec![0; table + 6];
    bank[..5].copy_from_slice(b"ef1\0\x03");
    for (at, value) in [
        (8, 20),
        (10, events),
        (12, events),
        (16, table),
        (18, table + 6),
    ] {
        bank[at..at + 2].copy_from_slice(&(value as u16).to_be_bytes());
    }
    bank[20..20 + ACTOR_BYTES].copy_from_slice(&row);
    row[0x91..0x96].fill(0);
    row[0x14..0x18].fill(0);
    bank[20 + ACTOR_BYTES..events].copy_from_slice(&row);
    bank[events..table].copy_from_slice(&[
        0, 0, 0, 0, 0, 0, 0, 1, 254, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 254, 0, 0, 0,
    ]);
    bank[table..].copy_from_slice(&[0, 0, 0, 12, 0, 12]);
    let mut cooker = test_cooker();
    let parent = EffectId { id: 0, ..id };
    cooker.program(&bank, parent).unwrap();
    cooker.result.validate().unwrap();
    let actor = cooker.result.actor(parent).unwrap();
    assert!(matches!(
        actor.ground,
        Some(GroundResponse::EmitOnce { clamp: true, .. })
    ));
    assert_eq!(actor.periodic.unwrap().period.get(), 3);
    assert_eq!(
        actor
            .child_effects()
            .map(|effect| effect.id)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    assert_eq!(cooker.result.programs.len(), 3);
    assert_eq!(cooker.result.actors.len(), 2);
    assert!(cooker.result.materials.is_empty());
}

#[test]
fn zero_type_emissions_keep_both_enemy_banks_and_their_indefinite_actor() {
    // Enemy 124/125 program 0 emits actor 0 at birth and ends at tick 10.
    // Its sole nonzero actor byte is the absent-UV-track sentinel, not a lifetime.
    let mut bank = vec![0; 1106];
    bank[..5].copy_from_slice(b"ef1\0\x01");
    for (at, value) in [(8, 32u16), (10, 1092), (12, 384), (16, 1104), (18, 1106)] {
        bank[at..at + 2].copy_from_slice(&value.to_be_bytes());
    }
    bank[32 + 0x32] = 255;
    bank[1092..1104].copy_from_slice(&[0, 0, 0, 0, 0, 0, 0, 10, 254, 0, 0, 0]);
    let mut cooker = test_cooker();
    for monster in [124, 125] {
        let id = EffectId {
            bank: EffectBank::Enemy(monster),
            id: 0,
        };
        cooker.program(&bank, id).unwrap();
        let actor = cooker.result.actor(id).unwrap();
        assert!(matches!(
            actor.geometry,
            Geometry::NoDraw {
                motion: NoDrawMotion::Local
            }
        ));
        assert_eq!(actor.lifetime, None);
        assert_eq!(actor.colors, [[0; 4]; 2]);
        assert!(actor.material.is_none());
        let program = cooker.result.program(id).unwrap();
        assert_eq!(program.end_tick, 10);
        assert_eq!(program.emissions.len(), 1);
        assert!(matches!(program.emissions[0].command,
            EffectCommand::Particle { actor, attachment: Attachment::Emitter, .. } if actor==id));
    }
    cooker.result.validate().unwrap();
    assert_eq!(
        (cooker.result.programs.len(), cooker.result.actors.len()),
        (2, 2)
    );
    assert!(cooker.result.materials.is_empty());
    let legacy: Geometry = serde_json::from_str(r#"{"kind":"no_draw"}"#).unwrap();
    assert!(matches!(
        legacy,
        Geometry::NoDraw {
            motion: NoDrawMotion::Origin
        }
    ));
    bank[32 + 0x32] = 0;
    assert!(
        test_cooker()
            .program(
                &bank,
                EffectId {
                    bank: EffectBank::Enemy(124),
                    id: 0
                }
            )
            .is_err()
    );
}

#[test]
fn triangular_band_decodes_the_common_recipe_and_element_materials() {
    let mut row = [0; ACTOR_BYTES];
    row[..32].copy_from_slice(&[
        19, 1, 0, 18, 1, 0, 16, 0, 1, 128, 1, 128, 0, 64, 0, 64, 0, 120, 0, 0, 4, 64, 16, 0, 0,
        128, 0, 128, 0, 128, 0, 0,
    ]);
    row[0x2b] = 16;
    row[0x2f] = 16;
    row[0x30..0x33].copy_from_slice(&[16, 104, 255]);
    for (at, value) in [(0x38, 1f32), (0x58, -90.), (0xb4, 96.), (0xb8, 304.)] {
        row[at..at + 4].copy_from_slice(&value.to_be_bytes());
    }
    let mut cooker = test_cooker();
    cooker.element_palettes = [1, 2, 3, 4, 5, 6, 7, 8];
    for color in [18, 1, 2, 3, 4, 5, 6, 7, 8] {
        cooker.material_indices.insert(
            MaterialKey {
                texture: TextureBank::Fixed(0),
                color,
                alpha: Some(1),
                stride: 16,
            },
            u16::from(color),
        );
    }
    let id = EffectId {
        bank: EffectBank::Common,
        id: 55,
    };
    let actor = cooker.actor(&row, id, &[], false).unwrap();
    assert!(matches!(actor.geometry, Geometry::TriangleBand));
    assert!(matches!(actor.blend, Blend::Additive));
    assert_eq!(actor.material, Some(18));
    assert_eq!(actor.dimensions, [0., 96., 304.]);
    assert_eq!(actor.angles, [-90., 0., 0.]);
    assert_eq!(actor.uv, [384, 384, 64, 64]);
    assert_eq!(actor.lifetime, Some(120));
    assert!(actor.ground_relative && actor.depth_test && !actor.depth_write);
    assert!(actor.use_element_variant);
    assert_eq!(
        actor
            .element_variants
            .iter()
            .map(|v| v.material)
            .collect::<Vec<_>>(),
        (1..=8).map(Some).collect::<Vec<_>>()
    );
    assert_eq!((actor.brighten_until, actor.darken_from), (16, 104));
    row[0x8e] = 1;
    assert!(cooker.actor(&row, id, &[], false).is_err());
    row[0x8e] = 0;
    row[0xe0] = 1;
    assert!(cooker.actor(&row, id, &[], false).is_err());
}

#[test]
fn polar_modifier_reads_the_full_instruction_before_following_scalar_writes() {
    // Magic 101, modifier 3200: angle, polar velocity, vertical jitter, vertical bias.
    let mut bytes = vec![0; 8];
    bytes.extend([
        0, 11, 0, 0x60, 7, 8, 0, 0, 0, 9, 0, 0x40, 0x42, 0xb4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x41,
        0, 0, 0, 0, 30, 0x0e, 0x10, 0, 11, 0, 0x44, 0, 40, 0, 0, 0, 8, 0, 0x44, 0x41, 0x40, 0, 0,
        0, 0, 0, 0, 255, 255,
    ]);
    let parsed = modifiers(&bytes, 8).unwrap();
    assert_eq!(parsed.len(), 4);
    assert!(matches!(
        parsed[1],
        Modifier::RandomPolarVector {
            field: VectorField::Velocity,
            angles: [90., 0., 0.],
            radius: 8.,
            radius_jitter: 30,
            angle_jitter: 3600,
        }
    ));
    assert!(matches!(
        parsed[2],
        Modifier::RandomFloat {
            field: FloatField::Velocity(1),
            range: FloatRandomRange::Remainder(40),
            ..
        }
    ));
    assert!(matches!(
        parsed[3],
        Modifier::Float {
            field: FloatField::Velocity(1),
            operation: Arithmetic::Add,
            value: FloatValue::Constant(12.),
        }
    ));
    assert!(modifiers(&bytes[..39], 8).is_err());
    bytes[18..20].copy_from_slice(&0x44u16.to_be_bytes());
    assert!(modifiers(&bytes, 8).is_err());
}

#[test]
fn ground_children_close_same_bank_dependencies_and_cycles_without_fabricating_programs() {
    let id = EffectId {
        bank: EffectBank::Magic(101),
        id: 0,
    };
    let actor_end = 20 + 2 * ACTOR_BYTES;
    let table = actor_end + 24;
    let mut bank = vec![0; table + 4];
    bank[..5].copy_from_slice(b"ef1\0\x02");
    for (at, value) in [
        (8, 20),
        (10, actor_end),
        (12, actor_end),
        (16, table),
        (18, table + 4),
    ] {
        bank[at..at + 2].copy_from_slice(&(value as u16).to_be_bytes());
    }
    for i in 0..2 {
        let row = &mut bank[20 + i * ACTOR_BYTES..20 + (i + 1) * ACTOR_BYTES];
        row[0] = if i == 0 { 14 } else { 11 }; // Nondrawing parents still own child dependencies.
        row[2] = 255;
        row[0x32] = 255;
        row[0x17] = 1;
        row[0x91] = 4;
        row[0x95] = (1 - i) as u8;
        bank[actor_end + i * 12..actor_end + (i + 1) * 12]
            .copy_from_slice(&[0, 0, i as u8, 0, 0, 0, 0, 0, 254, 0, 0, 0]);
        bank[table + i * 2..table + i * 2 + 2].copy_from_slice(&((i * 12) as u16).to_be_bytes());
    }
    let mut cooker = test_cooker();
    cooker.program(&bank, id).unwrap();
    cooker.result.validate().unwrap();
    assert_eq!(cooker.result.programs.len(), 2);
    assert_eq!(cooker.result.actors.len(), 2);
    assert!(matches!(cooker.result.actor(id).unwrap().ground,
        Some(GroundResponse::Bounce { effect: Some(child) }) if child == EffectId { id: 1, ..id }));
    bank[20 + 0x95] = 2;
    assert!(test_cooker().program(&bank, id).is_err());
    cooker.result.programs.pop();
    assert!(cooker.result.validate().is_err());
}

#[test]
fn hammer_model_recipe_retains_height_texture_rows_and_native_model_replacement() {
    let mut row = [0u8; ACTOR_BYTES];
    // Complete nonzero bytes of original Techniques actor13, including its model-only tail.
    for (at, value) in [
        (0, 3),
        (0x16, 4),
        (0x17, 2),
        (0x19, 64),
        (0x1b, 64),
        (0x1d, 64),
        (0x1f, 255),
        (0x2f, 16),
        (0x31, 64),
        (0x32, 255),
        (0x64, 64),
        (0x65, 160),
        (0x8c, 2),
        (0xb0, 63),
        (0xb1, 128),
        (0xb4, 63),
        (0xb5, 128),
        (0xb8, 63),
        (0xb9, 128),
        (0xd4, 1),
        (0x144, 66),
        (0x145, 32),
    ] {
        row[at] = value;
    }
    assert_eq!(
        format!("{:x}", Sha256::digest(row)),
        "d64c347be85fb70e2a3c5b48d63aec2fdc1171e159e105e3b1e104d48d4b5254"
    );
    let id = EffectId {
        bank: EffectBank::Techniques,
        id: 13,
    };
    let actor = test_cooker().actor(&row, id, &[], false).unwrap();
    assert_eq!(
        actor.geometry,
        Geometry::Model {
            model: ModelRef::Common { index: 1 },
            animation: None,
            loop_animation: false,
            presentation: ModelPresentation {
                animation_selector: 0,
                reverse_at: None,
                elevation: 40.,
                texture_rows: 2,
                texture_frame: 0,
                palettes: [0; 2],
                attachment: None,
                external_animation: false,
                orientation: ModelOrientation::Authored,
            }
        }
    );
    assert!(actor.bottom_anchored && actor.follow_emitter);
    assert_eq!(actor.angular_velocity, [5., 0., 0.]);
    let mut stream = vec![0, 0];
    stream.extend([
        0, 11, 0, 88, 14, 16, 0, 0, 0, 10, 0, 212, 0, 2, 0, 0, 255, 255, 0, 0,
    ]);
    let model = modifiers(&stream, 2).unwrap();
    actor.validate_modifiers(&model).unwrap();
    assert!(matches!(
        model[1],
        Modifier::Byte {
            field: ByteField::ModelIndex,
            operation: Arithmetic::Set,
            value: IntegerValue::Constant(2)
        }
    ));
    stream[12..14].copy_from_slice(&0x8du16.to_be_bytes());
    stream[14..16].copy_from_slice(&1u16.to_be_bytes());
    let texture = modifiers(&stream, 2).unwrap();
    actor.validate_modifiers(&texture).unwrap();
    assert!(matches!(
        texture[1],
        Modifier::Byte {
            field: ByteField::ModelTextureFrame,
            operation: Arithmetic::Set,
            value: IntegerValue::Constant(1)
        }
    ));
    // Opcode3 writes the complete signed +8C halfword, carrying from frame to rows.
    stream[10..12].copy_from_slice(&3u16.to_be_bytes());
    stream[12..14].copy_from_slice(&0x8cu16.to_be_bytes());
    stream[14..16].copy_from_slice(&0x01ffu16.to_be_bytes());
    let packed = modifiers(&stream, 2).unwrap();
    actor.validate_modifiers(&packed).unwrap();
    assert!(matches!(
        packed[1],
        Modifier::Integer {
            field: IntegerField::ModelTextureSelection,
            operation: Arithmetic::Add,
            value: IntegerValue::Constant(0x01ff),
        }
    ));
    let mut authored_palettes = row;
    authored_palettes[3..5].copy_from_slice(&[255, 128]);
    authored_palettes[0x8c..0x8e].copy_from_slice(&[0, 255]);
    let mut cooker = test_cooker();
    let inert = cooker.actor(&authored_palettes, id, &[], false).unwrap();
    assert!(
        matches!(inert.geometry, Geometry::Model { presentation, .. }
        if presentation.palettes == [255, 128] && presentation.texture_frame == 255)
    );
    assert!(inert.material.is_none());
    assert!(cooker.result.materials.is_empty());
    cooker.result.actors.push(inert);
    let palette_write = Modifier::Integer {
        field: IntegerField::PaletteSelection,
        operation: Arithmetic::Add,
        value: IntegerValue::Constant(1),
    };
    let prefix = actor::Record::read(&authored_palettes).unwrap().prefix;
    cooker
        .palette_changes(&prefix, id, &[palette_write], None)
        .unwrap();
    assert!(cooker.result.materials.is_empty());
    cooker
        .result
        .actor(id)
        .unwrap()
        .validate_modifiers(&[palette_write])
        .unwrap();
    for (at, value) in [(0x8b, 1), (0x140, 1), (0x148, 1)] {
        let mut invalid = row;
        invalid[at] = value;
        assert!(test_cooker().actor(&invalid, id, &[], false).is_err());
    }
    // Source-derived secondary cases, separate from the unchanged original row above.
    for age in [0, 255] {
        let mut secondary = row;
        secondary[0x94] = age;
        for flags in [0, 1, 3] {
            secondary[0x91] = flags;
            let geometry = test_cooker()
                .actor(&secondary, id, &[], false)
                .unwrap()
                .geometry;
            assert!(
                matches!(geometry, Geometry::Model { loop_animation, presentation, .. }
                if loop_animation == (flags & 2 != 0)
                    && presentation.reverse_at == (flags & 1 != 0).then_some(age))
            );
        }
    }
    row[0x144..0x148].copy_from_slice(&f32::NAN.to_be_bytes());
    assert!(test_cooker().actor(&row, id, &[], false).is_err());
}

#[test]
fn enemy_group_quad_keeps_original_recipe_and_its_local_atlas_binding() {
    let mut row = [0; ACTOR_BYTES];
    for (at, byte) in [
        (0x0, 5),
        (0x1, 1),
        (0x2, 2),
        (0x9, 160),
        (0xd, 16),
        (0xf, 32),
        (0x11, 16),
        (0x16, 2),
        (0x17, 64),
        (0x19, 192),
        (0x1b, 64),
        (0x1d, 64),
        (0x2b, 32),
        (0x2f, 16),
        (0x30, 4),
        (0x31, 8),
        (0x32, 255),
        (0x6c, 63),
        (0x6d, 192),
        (0x8e, 1),
        (0x8f, 90),
        (0xb0, 66),
        (0xb1, 192),
        (0xb4, 64),
        (0xb5, 128),
    ] {
        row[at] = byte;
    }
    assert_eq!(
        format!("{:x}", Sha256::digest(row)),
        "fd02f501ae06b03b11a21ec42aa38c311a5a4ee9f27930945cdb2d741030607f"
    );
    let id = EffectId {
        bank: EffectBank::Enemy(62),
        id: 0,
    };
    let mut cooker = test_cooker();
    cooker.material_indices.insert(
        MaterialKey {
            texture: TextureBank::Enemy(62),
            color: 0,
            alpha: None,
            stride: 0,
        },
        0,
    );
    let actor = cooker.actor(&row, id, &[], false).unwrap();
    assert!(matches!(
        actor.geometry,
        Geometry::BoneGroupQuad { group: 0 }
    ));
    assert_eq!((actor.material, actor.lifetime), (Some(0), Some(16)));
    assert_eq!((actor.copies, actor.copy_rotation), (2, 90));
    assert_eq!(actor.dimensions, [96., 4., 0.]);
    assert_eq!(actor.angular_velocity, [0., 0., 1.5]);
    assert_eq!(actor.uv, [160, 0, 16, 32]);
    assert_eq!(actor.colors[0], [192, 64, 64, 0]);
    assert_eq!((actor.brighten_until, actor.darken_from), (4, 8));
    assert_eq!((actor.brighten[3], actor.darken[3]), (32, 16));
    assert!(matches!(
        texture_bank(EffectBank::Enemy(46), 2).unwrap(),
        TextureBank::Enemy(46)
    ));
    assert!(texture_bank(EffectBank::Common, 2).is_err());
    for (at, byte) in [(0x8c, 16), (0x8d, 1)] {
        let mut invalid = row;
        invalid[at] = byte;
        assert!(cooker.actor(&invalid, id, &[], false).is_err());
    }
    // Enemy 117 modifier 736 selects a group through integer temporary 0, then advances by 3.
    let mut bytes = vec![0; 8];
    bytes.extend([
        0, 10, 0, 140, 127, 252, 0, 0, 0, 3, 127, 252, 0, 3, 0, 0, 255, 255,
    ]);
    let modifiers = modifiers(&bytes, 8).unwrap();
    assert!(matches!(
        modifiers[0],
        Modifier::Byte {
            field: ByteField::BoneSelector,
            operation: Arithmetic::Set,
            value: IntegerValue::Temporary(0)
        }
    ));
    actor.validate_modifiers(&modifiers).unwrap();

    // The native update reads the same byte as an owner joint for ordinary quads.
    // Combine this original quad/modifier fixture with Volt's stable binding flag.
    let mut attached = row;
    let flags = (word(&row, 0x14).unwrap() & !0x200) | 0x20000000;
    attached[0x14..0x18].copy_from_slice(&flags.to_be_bytes());
    attached[0x8c] = 16;
    let actor = cooker.actor(&attached, id, &[], false).unwrap();
    assert_eq!(actor.geometry, Geometry::Quad);
    assert_eq!(actor.space, EffectSpace::OwnerBonePosition { bone: 16 });
    actor.validate_modifiers(&modifiers).unwrap();
    attached[0x8d] = 1;
    assert!(cooker.actor(&attached, id, &[], false).is_err());
    attached[0x8d] = 0;
    attached[0] = 17;
    assert!(
        cooker
            .actor(&attached, id, &[], false)
            .unwrap_err()
            .to_string()
            .contains("live owner-joint")
    );
}

#[test]
fn summon_joint_recipe_preserves_original_selectors() {
    // Full original Magic86 actor5.
    let mut row = [0u8; ACTOR_BYTES];
    for (at, value) in [
        (0, 3),
        (2, 6),
        (17, 63),
        (21, 2),
        (25, 64),
        (27, 64),
        (29, 64),
        (43, 32),
        (47, 32),
        (48, 8),
        (49, 55),
        (50, 255),
        (56, 66),
        (57, 72),
        (88, 194),
        (89, 180),
        (176, 63),
        (177, 128),
        (180, 63),
        (181, 128),
        (184, 63),
        (185, 128),
        (212, 4),
    ] {
        row[at] = value;
    }
    assert_eq!(
        format!("{:x}", Sha256::digest(row)),
        "ad465063a8f3d8d13ceb3f7920c02f305643174b4d8cde4248539b9e61ec0327"
    );
    let id = EffectId {
        bank: EffectBank::Magic(86),
        id: 5,
    };
    let actor = test_cooker().actor(&row, id, &[], false).unwrap();
    assert_eq!(
        actor.geometry,
        Geometry::Model {
            model: ModelRef::Magic {
                package: 86,
                index: 4
            },
            animation: None,
            loop_animation: false,
            presentation: ModelPresentation {
                attachment: Some(RetainedJoint { slot: 0, joint: 0 }),
                ..Default::default()
            }
        }
    );
    assert_eq!(actor.lifetime, Some(63));
    assert_eq!(actor.position, [0., 50., 0.]);
    assert_eq!(actor.angles, [-90., 0., 0.]);
    assert_eq!(actor.dimensions, [1.; 3]);
    assert_eq!(actor.colors[0], [64, 64, 64, 0]);
    assert_eq!((actor.brighten_until, actor.darken_from), (8, 55));
    // The seven real child selectors include slot2/joint0, which is not a texture frame.
    for (package, child, model, slot, joint) in [
        (86, 5, 4, 0, 0),
        (86, 8, 6, 2, 0),
        (90, 5, 1, 0, 0),
        (90, 6, 2, 0, 1),
        (92, 5, 1, 0, 0),
        (93, 7, 3, 0, 2),
        (93, 8, 3, 0, 3),
    ] {
        row[0xd4] = model;
        row[0x8c] = joint;
        row[0x8d] = slot;
        let actor = test_cooker()
            .actor(
                &row,
                EffectId {
                    bank: EffectBank::Magic(package),
                    id: child,
                },
                &[],
                false,
            )
            .unwrap();
        let Geometry::Model { presentation, .. } = actor.geometry else {
            panic!()
        };
        assert_eq!(presentation.attachment, Some(RetainedJoint { slot, joint }));
        actor
            .validate_modifiers(&[Modifier::Byte {
                field: ByteField::ModelTextureFrame,
                operation: Arithmetic::Set,
                value: IntegerValue::Constant(0),
            }])
            .unwrap();
    }
    row[0x8d] = 8;
    assert!(test_cooker().actor(&row, id, &[], false).is_err());
    row[0x8d] = 0;
    row[0x14..0x18].copy_from_slice(&0x20000u32.to_be_bytes());
    row[0] = 5;
    assert!(test_cooker().actor(&row, id, &[], false).is_err());
}

#[test]
fn original_summon_parent_and_clip_commands_retain_animation_ownership() {
    let mut row = [0u8; ACTOR_BYTES];
    for (at, value) in [
        (0, 3),
        (2, 6),
        (17, 180),
        (21, 4),
        (23, 32),
        (25, 64),
        (27, 64),
        (29, 64),
        (43, 32),
        (47, 32),
        (48, 8),
        (49, 172),
        (50, 255),
        (56, 67),
        (57, 22),
        (88, 194),
        (89, 180),
        (176, 63),
        (177, 128),
        (180, 63),
        (181, 128),
        (184, 63),
        (185, 128),
    ] {
        row[at] = value;
    }
    assert_eq!(
        format!("{:x}", Sha256::digest(row)),
        "d9cc0b326d048ea245b9ced6731a49c262491f5ecb90680a4d03eb42074d6d7c"
    );
    let actor = test_cooker()
        .actor(
            &row,
            EffectId {
                bank: EffectBank::Magic(90),
                id: 4,
            },
            &[],
            false,
        )
        .unwrap();
    assert!(actor.retained);
    assert!(matches!(
        actor.geometry,
        Geometry::Model {
            presentation: ModelPresentation {
                external_animation: true,
                ..
            },
            ..
        }
    ));
    // Original Magic90 bank streams8832/8852 and Magic92 stream6036; no altered parent recipes.
    for (clip, blend, flags) in [(0, 0, 0), (1, 4, 8), (1, 10, 8)] {
        let mut bytes = vec![0, 0];
        for word in [22u16, 92, 0, clip, blend, 0, flags, 0, 65535, 0] {
            bytes.extend(word.to_be_bytes());
        }
        let decoded = modifiers(&bytes, 2).unwrap();
        actor.validate_modifiers(&decoded).unwrap();
        assert!(
            matches!(decoded[0],Modifier::PlayModelAnimation {animation: ModelAnimation {model:0,clip:c,blend_ticks:b,rate:0.5,hold}} if c==clip as u8 && b==blend && hold==(flags==8))
        );
        bytes[6..8].copy_from_slice(&1u16.to_be_bytes());
        assert!(
            actor
                .validate_modifiers(&modifiers(&bytes, 2).unwrap())
                .is_err()
        );
        bytes[14..16].copy_from_slice(&2u16.to_be_bytes());
        assert!(modifiers(&bytes, 2).is_err());
    }
    // Opcodes26/27 ignore their destination and retain float-temporary semantics.
    let mut bytes = vec![0, 0];
    for (opcode, value, selector) in [(26u16, 3.25f32, 0u16), (27, -8., 0x7ff9)] {
        bytes.extend(opcode.to_be_bytes());
        bytes.extend(0x7fffu16.to_be_bytes());
        bytes.extend(value.to_be_bytes());
        bytes.extend(selector.to_be_bytes());
        bytes.extend(0xbeefu16.to_be_bytes());
    }
    bytes.extend(u16::MAX.to_be_bytes());
    let decoded = modifiers(&bytes, 2).unwrap();
    actor.validate_modifiers(&decoded).unwrap();
    assert!(matches!(
        decoded.as_slice(),
        [
            Modifier::AnimationPosition {
                value: FloatValue::Constant(3.25)
            },
            Modifier::AnimationRate {
                value: FloatValue::Temporary(1)
            }
        ]
    ));
    bytes[10..12].copy_from_slice(&0x7ffcu16.to_be_bytes());
    assert!(modifiers(&bytes, 2).is_err());
}

#[test]
fn seal_program_prepares_its_authored_palette_additions_once() {
    let mut bank = vec![0; 62208];
    bank[..20].copy_from_slice(&[
        101, 102, 49, 0, 138, 5, 0, 0, 0, 32, 217, 204, 195, 96, 241, 6, 241, 206, 242, 226,
    ]);
    let row = &mut bank[49664..50016];
    for (at, byte) in [
        (0x0, 8),
        (0x1, 1),
        (0x2, 1),
        (0x3, 32),
        (0x4, 1),
        (0x6, 32),
        (0x9, 193),
        (0xb, 193),
        (0xd, 62),
        (0xf, 62),
        (0x11, 24),
        (0x12, 12),
        (0x13, 1),
        (0x14, 4),
        (0x17, 8),
        (0x19, 128),
        (0x1b, 128),
        (0x1d, 128),
        (0x21, 128),
        (0x23, 128),
        (0x25, 128),
        (0x27, 255),
        (0x2f, 32),
        (0x31, 16),
        (0x32, 255),
        (0x58, 194),
        (0x59, 180),
        (0x6c, 193),
        (0x6d, 64),
        (0x84, 64),
        (0x85, 128),
        (0xb0, 66),
        (0xb4, 66),
        (0xb8, 66),
        (0xb9, 128),
        (0xc4, 64),
        (0xc5, 192),
        (0xc8, 64),
        (0xcc, 64),
        (0xd4, 65),
        (0xd5, 32),
    ] {
        row[at] = byte;
    }
    assert_eq!(
        format!("{:x}", Sha256::digest(&*row)),
        "29c7c714807b69d8b80afa13d333ac5813e513a703e2bf4b52539f18fcb909b9"
    );
    for (at, x, z) in [(55564, 45f32, 0f32), (55600, 0., 90.), (55636, -45., 90.)] {
        bank[at..at + 8].copy_from_slice(&[0, 12, 0, 3, 0, 1, 0, 0]);
        for (delta, field, value) in [(8, 0x58, x), (20, 0x60, z)] {
            bank[at + delta..at + delta + 4].copy_from_slice(&[0, 8, 0, field]);
            bank[at + delta + 4..at + delta + 8].copy_from_slice(&value.to_be_bytes());
        }
        bank[at + 32..at + 34].fill(255);
    }
    bank[61570..61594].copy_from_slice(&[
        0, 0, 141, 0, 217, 12, 0, 4, 141, 0, 217, 48, 0, 8, 141, 0, 217, 84, 0, 8, 254, 0, 0, 0,
    ]);
    bank[62170..62172].copy_from_slice(&[22, 182]);
    let mut cooker = test_cooker();
    for color in [32, 33] {
        cooker.material_indices.insert(
            MaterialKey {
                texture: TextureBank::Fixed(1),
                color,
                alpha: Some(1),
                stride: 32,
            },
            u16::from(color - 32),
        );
        cooker.result.materials.push(EffectMaterial {
            texture: UiTexture {
                path: format!("battle/effects/seal-{color}.ktx2"),
                width: 256,
                height: 256,
            },
            rgb_scale: 2.,
        });
    }
    let program = EffectId {
        bank: EffectBank::Techniques,
        id: 134,
    };
    let id = EffectId { id: 141, ..program };
    let mut row = actor_source(&bank, id).unwrap().to_vec();
    assert_eq!(float(&row, 0xd4).unwrap(), 10.);
    let original = cooker.actor(&row, id, &bank, false).unwrap();
    // Trail draw/init/update never consume this fourth word after the size-step vector.
    row[0xd4..0xd8].copy_from_slice(&12.345f32.to_be_bytes());
    let changed = cooker.actor(&row, id, &bank, false).unwrap();
    assert_eq!(
        serde_json::to_value(original).unwrap(),
        serde_json::to_value(changed).unwrap()
    );
    row[0xd8] = 1;
    assert!(cooker.actor(&row, id, &bank, false).is_err());
    cooker.program(&bank, program).unwrap();
    cooker.result.validate().unwrap();
    let actor = cooker
        .result
        .actor(EffectId { id: 141, ..program })
        .unwrap();
    let palette = actor.palette.as_ref().unwrap();
    assert_eq!((palette.index, actor.material), (32, Some(0)));
    assert_eq!(palette.materials, BTreeMap::from([(32, 0), (33, 1)]));
    assert_eq!(cooker.material_indices.len(), 2);
    let program = cooker.result.program(program).unwrap();
    assert_eq!(program.end_tick, 8);
    assert_eq!(
        program.emissions.iter().map(|e| e.tick).collect::<Vec<_>>(),
        vec![0, 4, 8]
    );
    for emission in &program.emissions {
        let EffectCommand::Particle { modifiers, .. } = &emission.command else {
            panic!("seal particle")
        };
        assert!(matches!(
            modifiers[0],
            Modifier::Byte {
                field: ByteField::Palette,
                operation: Arithmetic::Add,
                value: IntegerValue::Constant(1)
            }
        ));
    }
    // Reuse the original Seal +1 stream in a synthetic retained timeline. Three
    // actual writes reach35; the repeat at the End tick must not request36.
    let mut retained_bank = bank.clone();
    let flags = word(&retained_bank, 49664 + 0x14).unwrap() | 0x40000;
    retained_bank[49664 + 0x14..49664 + 0x18].copy_from_slice(&flags.to_be_bytes());
    retained_bank[61570..61600].copy_from_slice(&[
        0, 0, 141, 0, 0, 0, 0, 1, 253, 0, 217, 12, 0, 2, 255, 3, 0, 1, 0, 2, 253, 0, 217, 12, 0, 4,
        254, 0, 0, 0,
    ]);
    let mut retained = test_cooker();
    retained.material_indices = cooker.material_indices.clone();
    retained.result.materials = cooker.result.materials.clone();
    for color in [34, 35] {
        retained.material_indices.insert(
            MaterialKey {
                texture: TextureBank::Fixed(1),
                color,
                alpha: Some(1),
                stride: 32,
            },
            u16::from(color - 32),
        );
        retained
            .result
            .materials
            .push(retained.result.materials[0].clone());
    }
    retained.program(&retained_bank, program.id).unwrap();
    retained.result.validate().unwrap();
    let palette = retained.result.actor(id).unwrap().palette.as_ref().unwrap();
    assert_eq!(
        palette.materials.keys().copied().collect::<Vec<_>>(),
        vec![32, 33, 34, 35]
    );
    assert_eq!(retained.material_indices.len(), 4);
    retained
        .result
        .actors
        .iter_mut()
        .find(|actor| actor.id == id)
        .unwrap()
        .palette
        .as_mut()
        .unwrap()
        .materials
        .remove(&35);
    assert!(
        retained
            .result
            .validate()
            .unwrap_err()
            .to_string()
            .contains("unprepared effect palette")
    );
    // A synthetic group uses Seal's declaration and angle writes, replacing its
    // palette addition with division. A later retained +1 must cover every birth.
    let mut grouped_bank = retained_bank.clone();
    grouped_bank[55564..55572].copy_from_slice(&[0, 25, 0, 3, 0, 2, 0, 0]);
    grouped_bank[61570..61588].copy_from_slice(&[
        0, 0, 141, 250, 217, 12, 0, 1, 253, 1, 217, 48, 0, 2, 254, 0, 0, 0,
    ]);
    let expected = [0, 1, 2, 3, 4, 5, 8, 9, 16, 17, 32, 33];
    let mut grouped = test_cooker();
    for (material, color) in expected.into_iter().enumerate() {
        grouped.material_indices.insert(
            MaterialKey {
                texture: TextureBank::Fixed(1),
                color,
                alpha: Some(1),
                stride: 32,
            },
            material as u16,
        );
        grouped
            .result
            .materials
            .push(cooker.result.materials[0].clone());
    }
    grouped.program(&grouped_bank, program.id).unwrap();
    grouped.result.validate().unwrap();
    let palette = grouped.result.actor(id).unwrap().palette.as_ref().unwrap();
    assert_eq!(
        palette.materials.keys().copied().collect::<Vec<_>>(),
        expected.map(u16::from)
    );
    let mut missing = grouped.result.clone();
    missing
        .actors
        .iter_mut()
        .find(|actor| actor.id == id)
        .unwrap()
        .palette
        .as_mut()
        .unwrap()
        .materials
        .remove(&9);
    assert!(
        missing
            .validate()
            .unwrap_err()
            .to_string()
            .contains("unprepared effect palette")
    );
    // The original +1 group needs the complete byte-selector cycle; a small rig
    // is not inferred from the fixture's attachment selector or cached palettes.
    grouped_bank[55564..55572].copy_from_slice(&bank[55564..55572]);
    let mut unavailable = test_cooker();
    unavailable.material_indices = grouped.material_indices;
    unavailable.result.materials = grouped.result.materials;
    let error = unavailable.program(&grouped_bank, program.id).unwrap_err();
    assert!(format!("{error:#}").contains("effect requires an absent texture bank"));
    let actor = cooker
        .result
        .actors
        .iter_mut()
        .find(|a| a.id.id == 141)
        .unwrap();
    let add = |value| Modifier::Byte {
        field: ByteField::Palette,
        operation: Arithmetic::Add,
        value,
    };
    assert!(
        actor
            .validate_modifiers(&[add(IntegerValue::Constant(2))])
            .is_err()
    );
    assert!(
        actor
            .validate_modifiers(&[add(IntegerValue::Temporary(0))])
            .is_err()
    );
    actor
        .palette
        .as_mut()
        .unwrap()
        .materials
        .insert(33, u16::MAX);
    assert!(cooker.result.validate().is_err());
}

#[test]
#[ignore = "requires the privately extracted US disc; parses complete records without encoding textures"]
fn original_thrust_and_enemy_effects_decode_modifiers_and_empty_programs() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let archive = MagicArchive::read(&extracted).unwrap();
    for (package, actor_count, program_count, digest) in [
        (
            102,
            2,
            2,
            "e274f20d0dc1a9091ac186a761341b227a0ee8cf50eec11f8708c6019e6245e1",
        ),
        (
            103,
            4,
            2,
            "0053f7d634a4d01147ad54abd3a6636d78c8f164cbca1eec9e4eee15e0542d6e",
        ),
        (
            104,
            4,
            3,
            "66f451304e3718f0f2b050aeca202a7c61cdfe9b1a9b089bb36d05ea8721a01a",
        ),
    ] {
        let bytes = magic_member(archive.package(package).unwrap(), 4)
            .unwrap()
            .unwrap();
        let end = usize::from(half(bytes, 18).unwrap());
        assert_eq!(format!("{:x}", Sha256::digest(&bytes[..end])), digest);
        assert_eq!((bytes[4], bytes[5]), (program_count, 0));
        let bank = EffectBank::Magic(package);
        let mut cooker = test_cooker();
        // Bypass only texture encoding; original actor bytes and full timelines stay intact.
        cooker.result.materials.push(EffectMaterial {
            texture: UiTexture {
                path: "test.ktx2".into(),
                width: 512,
                height: 128,
            },
            rgb_scale: 2.,
        });
        for id in 0..actor_count {
            let row = actor_source(bytes, EffectId { bank, id }).unwrap();
            cooker.material_indices.insert(
                MaterialKey {
                    texture: TextureBank::Magic(package),
                    color: row[3],
                    alpha: (word(row, 20).unwrap() & 0x4000000 != 0).then_some(row[4]),
                    stride: row[6],
                },
                0,
            );
        }
        for id in 0..program_count {
            cooker.program(bytes, EffectId { bank, id }).unwrap();
        }
        cooker.result.validate().unwrap();
        assert_eq!(cooker.result.actors.len(), usize::from(actor_count));
        assert_eq!(cooker.result.programs.len(), usize::from(program_count));
        let empty = cooker.result.program(EffectId { bank, id: 0 }).unwrap();
        assert_eq!(empty.end_tick, 0);
        assert!(empty.emissions.is_empty());
        if package == 104 {
            let modifiers = modifiers(bytes, 0x5bc).unwrap();
            assert!(matches!(
                modifiers[3],
                Modifier::Float {
                    field: FloatField::GeometrySegmentOffset(1),
                    operation: Arithmetic::Add,
                    value: FloatValue::Temporary(0),
                }
            ));
            let mut actor = cooker
                .result
                .actor(EffectId { bank, id: 2 })
                .unwrap()
                .clone();
            actor.validate_modifiers(&modifiers).unwrap();
            actor.geometry = Geometry::Quad;
            assert!(actor.validate_modifiers(&modifiers).is_err());
        }
    }

    let sources = crate::battle::all::Sources::read(&extracted).unwrap();
    let files = extracted.join("files");
    let usual = fs::read(files.join(&sources.usual)).unwrap();
    let enemy =
        crate::battle::archive_directories::enemy_package(&files.join(&sources.enemy), &usual, 2)
            .unwrap();
    let bytes = crate::battle::enemy_inventory::offset_section(
        &enemy,
        word(&enemy, 0x1cc).unwrap() as usize,
    )
    .unwrap();
    let bank = EffectBank::Enemy(2);
    let mut cooker = test_cooker();
    cooker.result.materials.push(EffectMaterial {
        texture: UiTexture {
            path: "test.ktx2".into(),
            width: 256,
            height: 256,
        },
        rgb_scale: 2.,
    });
    cooker.material_indices.insert(
        MaterialKey {
            texture: TextureBank::Enemy(2),
            color: 0,
            alpha: Some(0),
            stride: 0,
        },
        0,
    );
    let id = EffectId { bank, id: 3 };
    cooker.program(bytes, id).unwrap();
    cooker.result.validate().unwrap();
    let colored = cooker
        .result
        .program(id)
        .unwrap()
        .emissions
        .iter()
        .filter_map(|emission| {
            let EffectCommand::Particle {
                actor, modifiers, ..
            } = &emission.command
            else {
                return None;
            };
            if actor.id != 5 || modifiers.is_empty() {
                return None;
            }
            for (channel, expected) in [64, 128, 64, 96].into_iter().enumerate() {
                assert!(matches!(modifiers[channel], Modifier::Integer {
                field: IntegerField::Color { end: 0, channel: actual },
                operation: Arithmetic::Set,
                value: IntegerValue::Constant(value),
            } if usize::from(actual) == channel && value == expected));
            }
            Some(emission.tick)
        })
        .collect::<Vec<_>>();
    assert_eq!(colored, [39, 42, 45]);
}

#[test]
fn model_group_authoring_flag_keeps_one_model_and_preserves_invalid_resource_rejection() {
    for (monster, index, hash, bytes) in [
        (
            72,
            1,
            "adf48b2040733cefd1ee32f1a385d158dc7e8f629eb33bdb7c50b400040f7b86",
            &[
                (0x0, 3),
                (0x2, 255),
                (0x11, 32),
                (0x16, 2),
                (0x17, 64),
                (0x19, 80),
                (0x1b, 64),
                (0x1d, 96),
                (0x1f, 128),
                (0x32, 255),
                (0x38, 66),
                (0x39, 72),
                (0xb0, 67),
                (0xb1, 72),
                (0xb4, 67),
                (0xb5, 72),
            ][..],
        ),
        (
            76,
            2,
            "1fdee5237360ec7a43bd00edbe50e68c40c3d09bb5fd5cf0eac406527492cc35",
            &[
                (0x0, 3),
                (0x2, 2),
                (0x11, 60),
                (0x16, 6),
                (0x19, 64),
                (0x1b, 64),
                (0x1d, 64),
                (0x2b, 16),
                (0x2f, 16),
                (0x30, 16),
                (0x31, 44),
                (0x32, 255),
                (0x64, 192),
                (0x65, 57),
                (0x66, 153),
                (0x67, 154),
                (0x68, 192),
                (0x69, 57),
                (0x6a, 153),
                (0x6b, 154),
                (0x6c, 64),
                (0x6d, 57),
                (0x6e, 153),
                (0x6f, 154),
                (0x8c, 1),
                (0xb0, 63),
                (0xb1, 128),
                (0xb4, 63),
                (0xb5, 128),
                (0xb8, 63),
                (0xb9, 128),
            ][..],
        ),
    ] {
        let mut row = [0; ACTOR_BYTES];
        for &(at, value) in bytes {
            row[at] = value;
        }
        assert_eq!(format!("{:x}", Sha256::digest(row)), hash);
        let id = EffectId {
            bank: EffectBank::Enemy(monster),
            id: index,
        };
        let flags = word(&row, 0x14).unwrap();
        let mut cooker = test_cooker();
        if monster == 72 {
            assert_eq!((row[2], row[0xd4]), (255, 0));
            assert!(
                cooker
                    .actor(&row, id, &[], false)
                    .unwrap_err()
                    .to_string()
                    .contains("unsupported effect model binding")
            );
            continue;
        }
        let actor = cooker.actor(&row, id, &[], false).unwrap();
        assert!(matches!(
            actor.geometry,
            Geometry::Model {
                model: ModelRef::Enemy {
                    monster: 76,
                    index: 0
                },
                animation: None,
                presentation: ModelPresentation {
                    texture_rows: 1,
                    texture_frame: 0,
                    attachment: None,
                    ..
                },
                ..
            }
        ));
        assert_eq!(actor.lifetime, Some(60));
        assert_eq!(actor.angular_velocity, [-2.9, -2.9, 2.9]);
        assert_eq!(actor.dimensions, [1.; 3]);
        assert_eq!(actor.colors[0], [64, 64, 64, 0]);
        assert!(actor.follow_emitter);
        assert_eq!(actor.copies, 1);
        row[0x14..0x18].copy_from_slice(&(flags & !0x200).to_be_bytes());
        let without_reserved_flag = cooker.actor(&row, id, &[], false).unwrap();
        assert_eq!(
            serde_json::to_value(actor).unwrap(),
            serde_json::to_value(without_reserved_flag).unwrap()
        );
        // On models these bytes select texture rows, including when the unused flag is set.
        row[0x8c..0x8e].copy_from_slice(&[2, 1]);
        row[0x14..0x18].copy_from_slice(&flags.to_be_bytes());
        assert!(matches!(
            cooker.actor(&row, id, &[], false).unwrap().geometry,
            Geometry::Model {
                presentation: ModelPresentation {
                    texture_rows: 2,
                    texture_frame: 1,
                    ..
                },
                ..
            }
        ));
        row[0] = 4;
        row[0x14..0x18].copy_from_slice(&flags.to_be_bytes());
        assert!(cooker.actor(&row, id, &[], false).is_err());
    }
}

#[test]
fn preflight_reports_independent_roots_and_never_writes_queued_textures_on_failure() {
    let mut atlas = vec![0; 192];
    for (offset, value) in [
        (0, 0x0020af30_u32),
        (4, 1),
        (8, 12),
        (12, 32),
        (16, 72),
        (32, 0x00080008),
        (36, 8),
        (40, 96), // One CI4 8x8 tile.
        (72, 0x00200000),
        (76, 1),
        (80, 128), // Two identical 16-color palettes.
    ] {
        atlas[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    let mut cooker = test_cooker();
    cooker.textures.insert(TextureBank::Fixed(0), atlas);
    let key = MaterialKey {
        texture: TextureBank::Fixed(0),
        color: 0,
        alpha: None,
        stride: 16,
    };
    assert_eq!(cooker.material(key).unwrap(), 0);
    assert_eq!(cooker.material(MaterialKey { color: 1, ..key }).unwrap(), 0);
    assert_eq!(cooker.pending_images.len(), 1); // Decoded pixels remain deduplicated in memory.
    assert_eq!(cooker.pending_images[0].1.len(), 8 * 8 * 4);

    let actor_end = 20 + 2 * ACTOR_BYTES;
    let table = actor_end + 36;
    let mut bank = vec![0; table + 4];
    bank[..5].copy_from_slice(b"ef1\0\x02");
    for (offset, value) in [
        (8, 20),
        (10, actor_end),
        (12, actor_end),
        (16, table),
        (18, table + 4),
    ] {
        bank[offset..offset + 2].copy_from_slice(&(value as u16).to_be_bytes());
    }
    bank[20] = 2; // Unsupported shape in root0.
    bank[20 + ACTOR_BYTES] = 5;
    bank[20 + ACTOR_BYTES + 0x14] = 0x80; // Independent unsupported flags in root1.
    for id in 0..2 {
        bank[actor_end + id * 18..actor_end + (id + 1) * 18].copy_from_slice(&[
            0,
            0,
            id as u8,
            0,
            0,
            0,
            0,
            4,
            (1 - id) as u8,
            0,
            0,
            0,
            0,
            4,
            254,
            0,
            0,
            0,
        ]);
        bank[table + id * 2..table + id * 2 + 2].copy_from_slice(&((id * 18) as u16).to_be_bytes());
    }
    let output = std::env::temp_dir().join(format!(
        "resonance-effect-preflight-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let result = preflight(
        (0..2).map(|id| {
            EffectRequest::Program(EffectId {
                bank: EffectBank::Magic(112),
                id,
            })
        }),
        |request| {
            let EffectRequest::Program(id) = request else {
                unreachable!()
            };
            cooker.program(&bank, id)
        },
    );
    assert!(cooker.result.programs.is_empty());
    let error = result.and_then(|_| cooker.finish(&output)).unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("2 requested roots"), "{message}");
    for tick in [0, 4] {
        assert_eq!(
            message
                .matches(&format!("effect emission at tick {tick}"))
                .count(),
            2,
            "{message}"
        );
    }
    assert!(
        message.contains("Magic(112), id: 0") && message.contains("null native draw dispatch"),
        "{message}"
    );
    assert!(
        message.contains("Magic(112), id: 1")
            && message.contains("unsupported camera-space effect draw list"),
        "{message}"
    );
    assert!(!output.exists(), "failed preflight created effect output");
}

#[test]
#[ignore = "requires privately extracted GameCube records; no texture cooking"]
fn original_seal_closure_decodes_count_palette_and_all_neighboring_modifiers() {
    let usual = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local/extracted/disc1/files/BTL/BTLusual.dat"),
    )
    .unwrap();
    let bank = member(&usual, 3).unwrap();
    let mut cooker = test_cooker();
    for index in [94, 103, 104, 105, 141] {
        let id = EffectId {
            bank: EffectBank::Techniques,
            id: index,
        };
        let row = actor_source(bank, id).unwrap();
        for color in std::iter::once(row[3]).chain((index == 141).then_some(33)) {
            let key = MaterialKey {
                texture: texture_bank(id.bank, row[2]).unwrap(),
                color,
                alpha: (word(row, 0x14).unwrap() & 0x4000000 != 0).then_some(row[4]),
                stride: row[6],
            };
            if !cooker.material_indices.contains_key(&key) {
                let material = cooker.result.materials.len() as u16;
                cooker.material_indices.insert(key, material);
                cooker.result.materials.push(EffectMaterial {
                    texture: UiTexture {
                        path: format!("battle/effects/seal-closure-{material}.ktx2"),
                        width: 256,
                        height: 256,
                    },
                    rgb_scale: 2.,
                });
            }
        }
    }
    for id in [49, 59, 61, 134, 135] {
        cooker
            .program(
                bank,
                EffectId {
                    bank: EffectBank::Techniques,
                    id,
                },
            )
            .unwrap();
    }
    cooker.result.validate().unwrap();
    assert_eq!(cooker.result.materials.len(), 5);
    let id = EffectId {
        bank: EffectBank::Techniques,
        id: 104,
    };
    let row = actor_source(bank, id).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(row)),
        "c12a98c4efa63d39dfdfeac71edb25723f1dd267969610663e3233a3935820f8"
    );
    assert_eq!((row[0], row[0x12], half(row, 0x10).unwrap()), (7, 10, 50));
    assert_eq!(
        &bank[0xcfd4..0xcfe4],
        &[0, 11, 0, 0x5c, 7, 8, 0, 0, 0, 10, 0, 0x12, 0, 4, 0, 0]
    );
    let changes = modifiers(bank, 0xcfd4).unwrap();
    assert_eq!(changes.len(), 13);
    assert!(matches!(
        changes[1],
        Modifier::Byte {
            field: ByteField::GeometryCount,
            operation: Arithmetic::Set,
            value: IntegerValue::Constant(4)
        }
    ));
    let actor = cooker.result.actor(id).unwrap();
    actor.validate_modifiers(&changes).unwrap();
    assert!(matches!(
        actor.geometry,
        Geometry::BillboardRing { segments: 10 }
    ));
    let program = cooker.result.program(EffectId { id: 59, ..id }).unwrap();
    assert_eq!(program.end_tick, 16);
    assert_eq!(
        program.emissions.iter().map(|e| e.tick).collect::<Vec<_>>(),
        vec![0, 0, 5, 8]
    );
    assert_eq!(
        program
            .emissions
            .iter()
            .map(|e| e.repeat.map(|r| (r.count, r.interval)))
            .collect::<Vec<_>>(),
        vec![None, Some((3, 3)), None, Some((2, 8))]
    );
    assert_eq!(
        cooker
            .result
            .programs
            .iter()
            .map(|p| (p.id.id, p.end_tick))
            .collect::<Vec<_>>(),
        vec![(49, 4), (59, 16), (61, 38), (134, 8), (135, 8)]
    );
}

#[test]
#[ignore = "requires the original extracted disc; parses only source records"]
fn original_magic115_sprite_scroll_retains_interval_origin_and_signed_steps() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let archive = MagicArchive::read(&extracted).unwrap();
    let bank = magic_member(archive.package(115).unwrap(), 4)
        .unwrap()
        .unwrap();
    for (id, index, u, base, step) in [(6, 2, 65, 64, 4), (7, 4, 129, 128, 8)] {
        let row = actor_source(
            bank,
            EffectId {
                bank: EffectBank::Magic(115),
                id,
            },
        )
        .unwrap();
        assert_eq!((row[0], row[0x32]), (6, index));
        assert_eq!(
            [8, 10, 12, 14].map(|at| half(row, at).unwrap()),
            [u, 129, 62, 62]
        );
        let animation = uv_animation(bank, row[0x32], false, |_| unreachable!()).unwrap();
        assert_eq!((animation.frames.len(), animation.loop_to), (1, None));
        assert_eq!(animation.frames[0].duration, 1);
        let UvUpdate::Scroll {
            origin,
            step: delta,
        } = animation.frames[0].update
        else {
            panic!("missing authored scroll")
        };
        assert_eq!((origin, delta), ([base, 128], [0, step]));
    }
    let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
    for (at, word_value) in [
        (0x40b60, 0x7c040000),
        (0x40b7c, 0xb01d027c),
        (0x40b9c, 0x4180000c),
        (0x40ba4, 0xb01d027c),
        (0x40bdc, 0xb01f000a),
        (0x40e14, 0x881e1183),
        (0x40e1c, 0x40820010),
    ] {
        assert_eq!(
            word(rel.at((1, at)).unwrap(), 0).unwrap(),
            word_value,
            "source scroll at {at:x}"
        );
    }
}

#[test]
fn random_float_selector_requires_a_known_zero_birth_scratch() {
    let mut bytes = vec![0; 8];
    bytes.extend([0, 11, 0, 0x5c, 0, 0, 0x7f, 0xf9, 255, 255]);
    let changes = decode_modifiers(&bytes, 8, true).unwrap();
    assert!(matches!(
        changes.as_slice(),
        [Modifier::RandomFloat {
            field: FloatField::Angle(1),
            range: FloatRandomRange::Signed,
            scale: 0.1,
        }]
    ));
    assert!(modifiers(&bytes, 8).is_err()); // Retained calls have no known caller scratch.
    let mut preceded = vec![0; 8];
    preceded.extend([0, 0, 0x7f, 0xfc, 0, 3, 0, 0]);
    preceded.extend(&bytes[8..]);
    assert!(decode_modifiers(&preceded, 8, true).is_err());
    bytes[15] = 0xfc; // Integer selectors still require their own typed consumer.
    assert!(decode_modifiers(&bytes, 8, true).is_err());
    bytes[14..16].fill(0);
    assert!(matches!(
        decode_modifiers(&bytes, 8, true).unwrap()[0],
        Modifier::RandomFloat {
            range: FloatRandomRange::Remainder(0),
            ..
        }
    ));
    let mut cooker = test_cooker();
    let mut row = [0; ACTOR_BYTES];
    row[0] = 7;
    row[2] = 255;
    row[0x12] = 4;
    row[0x32] = 255;
    let actor = cooker
        .actor(
            &row,
            EffectId {
                bank: EffectBank::Common,
                id: 0,
            },
            &[],
            false,
        )
        .unwrap();
    assert!(
        actor
            .validate_modifiers(&decode_modifiers(&bytes, 8, true).unwrap())
            .is_err()
    );
}

#[test]
#[ignore = "requires privately extracted GameCube records; no texture encoding or output"]
fn original_random_selector_closure_keeps_all_three_programs_and_materials() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
    let textures = member(&usual, 4).unwrap();
    let archive = MagicArchive::read(&extracted).unwrap();
    let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
    // Original instructions: zero caller scratch, float-selector load, integer-scratch move,
    // then quotient * modulus and subtraction. No float-to-integer conversion occurs.
    for (offset, instruction) in [
        (0x41b54, 0x3b800000),
        (0x3f88c, 0x7ffe1c2e),
        (0x3f8a0, 0x7f9de378),
        (0x3f8d8, 0x7c0639d6),
        (0x3f8dc, 0x7c001850),
    ] {
        assert_eq!(word(rel.at((1, offset)).unwrap(), 0).unwrap(), instruction);
    }
    let mut cooker = test_cooker();
    cooker.textures.insert(
        TextureBank::Fixed(0),
        compression::decode(member(textures, 1).unwrap()).unwrap(),
    );
    cooker
        .textures
        .insert(TextureBank::Fixed(1), member(textures, 4).unwrap().to_vec());
    cooker.element_palettes = rel.at((4, 0x2174)).unwrap()[1..9].try_into().unwrap();
    let colors = rel.at((4, 0x2180)).unwrap();
    cooker.element_colors =
        std::array::from_fn(|i| std::array::from_fn(|c| i16::from(colors[(i + 1) * 4 + c])));
    for (bank, id, actor, offset, tick, interval, digest) in [
        (
            EffectBank::Common,
            36,
            58,
            0x7ed8,
            40,
            3,
            "16de183a4fd8adc470717dea8f8c7ab6e8032b693642d3847ea25260a5c3594f",
        ),
        (
            EffectBank::Magic(90),
            1,
            3,
            0x22a8,
            20,
            2,
            "d7071d312fbd421ff41fd22822277efe179541fe872e873dafc8fd1eb65caa32",
        ),
        (
            EffectBank::Magic(92),
            1,
            3,
            0x17a8,
            20,
            2,
            "3dcc0985fad4010de425fa904191850ea4996d6173362c628ab578890d76f461",
        ),
    ] {
        let bytes = if let EffectBank::Magic(package) = bank {
            let source = archive.package(package).unwrap();
            cooker.textures.insert(
                TextureBank::Magic(package),
                magic_member(source, 8).unwrap().unwrap().to_vec(),
            );
            magic_member(source, 4).unwrap().unwrap()
        } else {
            member(&usual, 2).unwrap()
        };
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(&bytes[..usize::from(half(bytes, 18).unwrap())])
            ),
            digest
        );
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes[offset..offset + 82])),
            "f0cc171d9f3dce154b8b240f5f4a09fff6849257a64bd02a43509261d1e13e1a"
        );
        cooker.program(bytes, EffectId { bank, id }).unwrap();
        let program = cooker.result.program(EffectId { bank, id }).unwrap();
        let emission = program
            .emissions
            .iter()
            .find(|e| {
                matches!(&e.command,
            EffectCommand::Particle { actor: a, .. } if a.id == actor)
            })
            .unwrap();
        assert_eq!(emission.tick, tick);
        assert!(matches!(emission.repeat, Some(Repeat { count:35, interval:i }) if i == interval));
        let EffectCommand::Particle {
            attachment: Attachment::Emitter,
            modifiers,
            ..
        } = &emission.command
        else {
            panic!("random selector lost ordinary birth ownership")
        };
        assert_eq!(modifiers.len(), 8);
        assert!(matches!(
            modifiers[0],
            Modifier::RandomFloat {
                field: FloatField::Angle(1),
                range: FloatRandomRange::Signed,
                scale: 0.1
            }
        ));
        assert_eq!(
            cooker
                .result
                .actor(EffectId { bank, id: actor })
                .unwrap()
                .lifetime,
            Some(48)
        );
    }
    let ring = cooker
        .result
        .actor(EffectId {
            bank: EffectBank::Common,
            id: 58,
        })
        .unwrap();
    assert!(matches!(
        ring.geometry,
        Geometry::BillboardRing { segments: 4 }
    ));
    assert!(ring.use_element_variant);
    assert_eq!(ring.element_variants.len(), Element::ALL.len());
    let base = MaterialKey {
        texture: TextureBank::Fixed(1),
        color: 3,
        alpha: Some(1),
        stride: 32,
    };
    assert_eq!(ring.material, cooker.material_indices.get(&base).copied());
    for (i, variant) in ring.element_variants.iter().enumerate() {
        assert_eq!(variant.element, Element::ALL[i]);
        assert_eq!(variant.outer_color, cooker.element_colors[i]);
        assert_eq!(
            variant.material,
            cooker
                .material_indices
                .get(&MaterialKey {
                    color: cooker.element_palettes[i],
                    ..base
                })
                .copied()
        );
    }
    cooker.result.validate().unwrap();
    assert!(!cooker.result.materials.is_empty());
}

#[cfg(test)]
mod explosion_tests;

#[cfg(test)]
mod eruption_tests;

#[test]
fn thunder_blade_lifetime_and_darken_modifiers_decode_with_strict_destinations() {
    // Magic19 program 1, tick 88, actor 8, modifier 0x12e0.
    // fn_1_3F0C0: opcode 3 adds a signed halfword; opcode 25 divides signed bytes.
    let mut bytes = vec![0; 8];
    bytes.extend([
        0, 14, 0, 0xc4, 0x3f, 0, 0, 0, 0, 0, 0, 0, 0, 14, 0, 0xc0, 0x3f, 0, 0, 0, 0, 0, 0, 0, 0, 3,
        0, 0x10, 0, 30, 0, 0, 0, 25, 0, 0x2c, 0, 2, 0, 0, 0, 25, 0, 0x2d, 0, 2, 0, 0, 0, 25, 0,
        0x2e, 0, 2, 0, 0, 0, 25, 0, 0x2f, 0, 2, 0, 0, 255, 255,
    ]);
    let decoded = modifiers(&bytes, 8).unwrap();
    assert_eq!(decoded.len(), 7);
    assert!(matches!(
        decoded[2],
        Modifier::Lifetime {
            operation: Arithmetic::Add,
            value: IntegerValue::Constant(30)
        }
    ));
    for (channel, modifier) in decoded[3..].iter().enumerate() {
        assert!(matches!(modifier, Modifier::Byte {
            field: ByteField::Darken(actual), operation: Arithmetic::Divide,
            value: IntegerValue::Constant(2)
        } if usize::from(*actual) == channel));
    }
    for opcode in [0u16, 3, 4, 5, 6] {
        for index in 0..4 {
            let mut dynamic = vec![0; 8];
            for half in [opcode, 0x10, 0x7ffc + index, 0, 65535] {
                dynamic.extend(half.to_be_bytes());
            }
            assert!(
                matches!(modifiers(&dynamic, 8).unwrap()[0], Modifier::Lifetime {
                value: IntegerValue::Temporary(actual), ..
            } if u16::from(actual) == index)
            );
        }
    }
    for opcode in [0u16, 2, 3, 4, 5, 6] {
        let mut palette = vec![0; 8];
        for half in [opcode, 3, 4, 0, 65535] {
            palette.extend(half.to_be_bytes());
        }
        assert!(matches!(
            modifiers(&palette, 8).unwrap()[0],
            Modifier::Integer {
                field: IntegerField::PaletteSelection,
                ..
            } | Modifier::RandomInteger {
                field: IntegerField::PaletteSelection,
                ..
            }
        ));
        for index in 0..4 {
            let mut bytes = vec![0; 8];
            for half in [opcode, 0x7ff8 + index, 2, 0, 65535] {
                bytes.extend(half.to_be_bytes());
            }
            assert!(matches!(modifiers(&bytes, 8).unwrap()[0],
                Modifier::Integer { field: IntegerField::FloatTemporaryHigh(actual), .. }
                | Modifier::RandomInteger { field: IntegerField::FloatTemporaryHigh(actual), .. }
                if u16::from(actual) == index));
        }
    }
    for (opcode, destination, value) in [
        (0u16, 0x10u16, 0x7ff8u16),
        (2, 0x10, 4),
        (3, 0x11, 30),
        (2, 0x7ff7, 2),
        (12, 0x32, 1),
    ] {
        let mut invalid = vec![0; 8];
        for half in [opcode, destination, value, 0, 65535] {
            invalid.extend(half.to_be_bytes());
        }
        assert!(modifiers(&invalid, 8).is_err());
    }
    assert!(modifiers(&bytes[..bytes.len() - 3], 8).is_err());
}

#[test]
#[ignore = "requires the original extracted disc; parses Orb effect closures without encoding"]
fn original_orb_effect_closures_keep_randomized_lifetimes() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let archive = MagicArchive::read(&extracted).unwrap();
    for (package, offset, birth_tick) in [
        (51, 0xeec, 60),
        (78, 0xedc, 60),
        (109, 0xeec, 60),
        (116, 0x104c, 90),
        (118, 0x15cc, 60),
    ] {
        let source = archive.package(package).unwrap();
        let bytes = magic_member(source, 4).unwrap().unwrap();
        let mut cooker = test_cooker();
        cooker.textures.insert(
            TextureBank::Magic(package),
            magic_member(source, 8).unwrap().unwrap().to_vec(),
        );
        let bank = EffectBank::Magic(package);
        cooker.program(bytes, EffectId { bank, id: 1 }).unwrap();
        if matches!(package, 109 | 116) {
            use sha2::{Digest, Sha256};
            let (end, count, digest) = if package == 109 {
                (
                    120,
                    9,
                    "f58e608de999a2584846bf0767ceba917f4c7ae4ff203da152677141d897fdc3",
                )
            } else {
                (
                    150,
                    10,
                    "a67e1533e13126cf38ee323277b7c77c52c0dce01613bc43d6096211c1a8f7f8",
                )
            };
            assert_eq!(format!("{:x}", Sha256::digest(bytes)), digest);
            assert_eq!(cooker.result.programs.len(), 1);
            assert_eq!(cooker.result.programs[0].end_tick, end);
            assert_eq!(cooker.result.actors.len(), count);
            assert!(
                cooker.result.programs[0]
                    .emissions
                    .iter()
                    .all(|e| !matches!(e.command, EffectCommand::Sound { .. }))
            );
        }
        if package == 118 {
            use sha2::{Digest, Sha256};
            let usual = std::fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
            let textures = member(&usual, 4).unwrap();
            cooker.textures.insert(
                TextureBank::Fixed(0),
                compression::decode(member(textures, 1).unwrap()).unwrap(),
            );
            cooker
                .textures
                .insert(TextureBank::Fixed(1), member(textures, 4).unwrap().to_vec());
            cooker.program(bytes, EffectId { bank, id: 2 }).unwrap();
            assert_eq!(
                format!("{:x}", Sha256::digest(bytes)),
                "df5327e8d74a9fd0e0445f386fff5e9190042d831b6bf3020fc757c25c647f81"
            );
            assert_eq!(
                cooker
                    .result
                    .programs
                    .iter()
                    .map(|p| (p.id.id, p.end_tick))
                    .collect::<Vec<_>>(),
                [(1, 120), (2, 18)]
            );
            assert_eq!(
                cooker
                    .result
                    .actors
                    .iter()
                    .map(|a| a.id.id)
                    .collect::<BTreeSet<_>>(),
                BTreeSet::from([0, 1, 2, 3, 4, 5, 6, 8, 9, 10, 11, 12, 13, 14])
            );
        }
        cooker.result.validate().unwrap();
        let actor = cooker.result.actor(EffectId { bank, id: 8 }).unwrap();
        assert_eq!((actor.lifetime, actor.darken_from), (Some(50), 18));
        let changes = modifiers(bytes, offset).unwrap();
        assert!(changes.windows(5).any(|sequence| matches!(
            sequence,
            [
                Modifier::RandomInteger {
                    field: IntegerField::Temporary(0),
                    modulus: 15
                },
                Modifier::Lifetime {
                    operation: Arithmetic::Add,
                    value: IntegerValue::Temporary(0)
                },
                Modifier::Byte {
                    field: ByteField::DarkenFrom,
                    operation: Arithmetic::Add,
                    value: IntegerValue::Temporary(0)
                },
                Modifier::RandomInteger {
                    field: IntegerField::FloatTemporaryHigh(0),
                    modulus: 2
                },
                Modifier::Byte {
                    field: ByteField::GeometryCount,
                    operation: Arithmetic::Add,
                    value: IntegerValue::Temporary(0)
                }
            ]
        )));
        assert!(cooker.result.program(EffectId { bank, id: 1 }).unwrap().emissions.iter()
            .any(|emission| emission.tick == birth_tick && matches!(&emission.command,
                EffectCommand::Particle { actor, modifiers, .. }
                if actor.id == 8 && modifiers.iter().any(|modifier| matches!(modifier,
                    Modifier::Lifetime { operation: Arithmetic::Add, value: IntegerValue::Temporary(0) })) )));
    }
}

#[test]
#[ignore = "requires original US disc; parses complete rain/splash closure without encoding"]
fn original_acid_rain_closes_ground_splashes_without_projectiles_or_unused_actor() {
    let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
    let archive = MagicArchive::read(&extracted).unwrap();
    let source = archive.package(65).unwrap();
    let bytes = magic_member(source, 4).unwrap().unwrap();
    let mut cooker = test_cooker();
    cooker.textures.insert(
        TextureBank::Magic(65),
        magic_member(source, 8).unwrap().unwrap().to_vec(),
    );
    let bank = EffectBank::Magic(65);
    cooker.program(bytes, EffectId { bank, id: 1 }).unwrap();
    cooker.result.validate().unwrap();
    assert_eq!(
        cooker
            .result
            .programs
            .iter()
            .map(|program| program.id.id)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([1, 2])
    );
    assert_eq!(
        cooker
            .result
            .actors
            .iter()
            .map(|actor| actor.id.id)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([0, 2])
    );
    let rain = cooker.result.actor(EffectId { bank, id: 2 }).unwrap();
    assert_eq!((rain.lifetime, rain.copies), (Some(12), 2));
    assert_eq!(
        rain.ground.and_then(GroundResponse::effect),
        Some(EffectId { bank, id: 2 })
    );
    let splash = cooker.result.actor(EffectId { bank, id: 0 }).unwrap();
    assert!(splash.ground_relative);
    assert_eq!(splash.lifetime, Some(24));
    assert!(magic_member(source, 252).unwrap().is_none());
    assert!(
        cooker
            .result
            .programs
            .iter()
            .flat_map(|program| &program.emissions)
            .all(|emission| !matches!(emission.command, EffectCommand::Sound { .. }))
    );
}

#[cfg(test)]
#[path = "effect_program/nurse_tests.rs"]
mod nurse_tests;

#[cfg(test)]
#[path = "effect_program/medusa_tests.rs"]
mod medusa_tests;

#[cfg(test)]
fn volt_owner_joint_row(actor: u8) -> [u8; ACTOR_BYTES] {
    // Complete original Enemy198 actor records, including all otherwise-zero bytes.
    let entries: &[(usize, u8)] = match actor {
        0 => &[
            (0, 3),
            (1, 1),
            (2, 2),
            (17, 40),
            (20, 32),
            (22, 4),
            (25, 64),
            (27, 64),
            (29, 64),
            (43, 8),
            (47, 4),
            (48, 8),
            (49, 24),
            (50, 255),
            (100, 64),
            (101, 192),
            (104, 65),
            (105, 32),
            (108, 65),
            (109, 144),
            (140, 16),
            (176, 63),
            (177, 230),
            (178, 102),
            (179, 102),
            (180, 63),
            (181, 230),
            (182, 102),
            (183, 102),
            (184, 63),
            (185, 230),
            (186, 102),
            (187, 102),
            (188, 59),
            (189, 163),
            (190, 215),
            (191, 10),
            (192, 59),
            (193, 163),
            (194, 215),
            (195, 10),
            (196, 59),
            (197, 163),
            (198, 215),
            (199, 10),
        ],
        1 => &[
            (0, 4),
            (1, 1),
            (2, 2),
            (3, 1),
            (4, 1),
            (6, 32),
            (9, 128),
            (11, 144),
            (13, 32),
            (15, 48),
            (17, 119),
            (18, 2),
            (20, 36),
            (22, 4),
            (23, 16),
            (25, 96),
            (27, 32),
            (29, 128),
            (31, 160),
            (100, 64),
            (101, 64),
            (104, 64),
            (105, 192),
            (140, 16),
            (180, 66),
            (181, 224),
            (184, 67),
        ],
        _ => panic!("not a Volt owner-joint actor"),
    };
    let mut row = [0; ACTOR_BYTES];
    for &(at, value) in entries {
        row[at] = value;
    }
    row
}

#[test]
fn volt_owner_joint_recipes_preserve_position_binding_and_all_other_actor_fields() {
    let mut cooker = test_cooker();
    cooker.material_indices.insert(
        MaterialKey {
            texture: TextureBank::Enemy(198),
            color: 1,
            alpha: Some(1),
            stride: 32,
        },
        0,
    );
    for (index, digest, lifetime, flags) in [
        (
            0,
            "fc542ebb1be8276fd54d4d38d2287eb187eca2ea379de07dd0e85b79d0ded503",
            40,
            0x20000400u32,
        ),
        (
            1,
            "b29db6024bbcc5d81eb17be4e7011ed5b8b0e9ec1a7936f60727c980ee329e0b",
            119,
            0x24000410u32,
        ),
    ] {
        let source = volt_owner_joint_row(index);
        assert_eq!(format!("{:x}", Sha256::digest(source)), digest);
        let id = EffectId {
            bank: EffectBank::Enemy(198),
            id: index,
        };
        let mut row = source;
        // Actor1 has UV stream zero; supply its complete real stream below in the
        // extracted-record test. This isolated recipe test disables only that lookup.
        row[0x32] = 255;
        let actor = cooker.actor(&row, id, &[], false).unwrap();
        assert_eq!(actor.space, EffectSpace::OwnerBonePosition { bone: 16 });
        assert!(actor.follow_emitter);
        assert_eq!(actor.lifetime, Some(lifetime));
        assert_eq!(actor.blend, Blend::Additive);
        assert_eq!(actor.position, [0.; 3]);
        assert_eq!(actor.velocity, [0.; 3]);
        if index == 0 {
            assert!(matches!(
                actor.geometry,
                Geometry::Model {
                    model: ModelRef::Enemy {
                        monster: 198,
                        index: 0
                    },
                    animation: None,
                    presentation: ModelPresentation {
                        texture_rows: 16,
                        texture_frame: 0,
                        attachment: None,
                        ..
                    },
                    ..
                }
            ));
            assert_eq!(actor.dimensions, [1.8; 3]);
            assert_eq!(actor.dimension_velocity, [0.005; 3]);
            assert_eq!(actor.angular_velocity, [6., 10., 18.]);
            assert_eq!(actor.colors[0], [64, 64, 64, 0]);
            assert_eq!((actor.brighten_until, actor.darken_from), (8, 24));
        } else {
            assert_eq!(
                actor.geometry,
                Geometry::Ring {
                    segments: 16,
                    flared: false,
                    lines: false,
                    repeat_uv: true,
                    uv_columns: 2,
                }
            );
            assert_eq!(actor.material, Some(0));
            assert_eq!(actor.dimensions, [0., 112., 128.]);
            assert_eq!(actor.angular_velocity, [3., 6., 0.]);
            assert_eq!(actor.colors[0], [96, 32, 128, 160]);
            assert_eq!(actor.uv, [128, 144, 32, 48]);
        }
        for selector in [0, 16, 255] {
            row[0x8c] = selector;
            assert_eq!(
                cooker.actor(&row, id, &[], false).unwrap().space,
                EffectSpace::OwnerBonePosition { bone: selector }
            );
        }
        row = source;
        row[0x32] = 255;
        for other_flags in [0x80000000, 0x40000000, 0x8000] {
            row[0x14..0x18].copy_from_slice(&(flags | other_flags).to_be_bytes());
            assert!(cooker.actor(&row, id, &[], false).is_err());
        }
        row[0x14..0x18].copy_from_slice(&flags.to_be_bytes());
        row[0x7c] = 1;
        assert!(cooker.actor(&row, id, &[], false).is_err());
    }
}

#[test]
#[ignore = "requires privately extracted GameCube records; no texture encoding or output"]
fn original_volt_owner_joint_aura_keeps_complete_actor_uv_and_repeat_program() {
    let extracted =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/files/BTL");
    let usual = fs::read(extracted.join("BTLusual.dat")).unwrap();
    let archive = fs::read(extracted.join("BTLenemy.dat")).unwrap();
    let table = word(&usual, 0x2c).unwrap() as usize;
    let start = word(&usual, table + 198 * 4).unwrap() as usize;
    let end = word(&usual, table + 199 * 4).unwrap() as usize;
    let source = compression::decode(&archive[start..end]).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&source)),
        "4a2c27e50216296a9b4e66b3cbba097cccd673dc5cdc9824f8b98d7f062bfa44"
    );
    let start = word(&source, 0x1cc).unwrap() as usize;
    let end = word(&source, 0x1d0).unwrap() as usize;
    let bytes = &source[start..end];
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "1226a57a04475851c0d66764e7a01f3f069d41d58d11e84923d1b2a13d87a2d5"
    );
    let bank = EffectBank::Enemy(198);
    for id in 0..=1 {
        assert_eq!(
            actor_source(bytes, EffectId { bank, id }).unwrap(),
            volt_owner_joint_row(id)
        );
    }
    let mut cooker = test_cooker();
    cooker.result.materials.push(EffectMaterial {
        texture: UiTexture {
            path: "battle/effects/volt-test.ktx2".into(),
            width: 256,
            height: 256,
        },
        rgb_scale: 2.,
    });
    cooker.material_indices.insert(
        MaterialKey {
            texture: TextureBank::Enemy(198),
            color: 1,
            alpha: Some(1),
            stride: 32,
        },
        0,
    );
    let id = EffectId { bank, id: 1 };
    cooker.program(bytes, id).unwrap();
    cooker.result.validate().unwrap();
    let program = cooker.result.program(id).unwrap();
    assert_eq!(program.end_tick, 120);
    assert_eq!(program.emissions.len(), 3);
    assert!(matches!(
        program.emissions[0].repeat,
        Some(Repeat {
            count: 6,
            interval: 20
        })
    ));
    assert_eq!(cooker.result.actors.len(), 2);
    for actor in &cooker.result.actors {
        assert_eq!(actor.space, EffectSpace::OwnerBonePosition { bone: 16 });
    }
    assert!(
        cooker
            .result
            .actor(EffectId { bank, id: 1 })
            .unwrap()
            .uv_animation
            .is_some()
    );
    assert!(cooker.pending_images.is_empty());
}

#[test]
#[ignore = "requires privately extracted original enemy records; parses without cooking"]
fn original_amphitra_random_models_keep_template_animation_and_complete_resources() {
    let extracted =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1/files/BTL");
    let usual = fs::read(extracted.join("BTLusual.dat")).unwrap();
    let archive = fs::read(extracted.join("BTLenemy.dat")).unwrap();
    let table = word(&usual, 0x2c).unwrap() as usize;
    let start = word(&usual, table + 183 * 4).unwrap() as usize;
    let end = word(&usual, table + 184 * 4).unwrap() as usize;
    let source = compression::decode(&archive[start..end]).unwrap();
    let bytes =
        &source[word(&source, 0x1cc).unwrap() as usize..word(&source, 0x1d0).unwrap() as usize];
    assert_eq!(
        format!("{:x}", Sha256::digest(bytes)),
        "5333f221a805a6e8a549a8a11e3d9431ec8df17f06acfac9ac812c56276336ef"
    );
    assert_eq!(
        &bytes[0x700..0x712],
        &[
            0, 2, 127, 252, 0, 3, 0, 0, 0, 10, 0, 212, 127, 252, 0, 0, 255, 255
        ]
    );
    let mut cooker = test_cooker();
    let bank = EffectBank::Enemy(183);
    cooker.program(bytes, EffectId { bank, id: 3 }).unwrap();
    cooker.result.validate().unwrap();
    let bindings = cooker.result.models_for(EffectId { bank, id: 0 });
    assert_eq!(
        bindings,
        (0..3)
            .map(|index| ModelRef::EnemyAnimated {
                monster: 183,
                index,
                animation_model: 2
            })
            .collect()
    );
    let animation = &source[word(&source, 0x1b8).unwrap() as usize..];
    for binding in bindings {
        let index = usize::from(binding.index());
        let model = &source[word(&source, 0x180 + index * 4).unwrap() as usize..];
        crate::model_preview::preflight(crate::model_preview::Layer {
            model,
            outline: None,
            animation: Some(animation),
            attached_to: None,
            additive: false,
        })
        .unwrap();
        let skeleton = crate::battle::pose::skeleton(model).unwrap();
        let motion = crate::battle::pose::motion(animation, model).unwrap();
        motion.validate(&skeleton).unwrap();
        for frame in [0., motion.duration_frames * 0.5, motion.duration_frames] {
            skeleton.sample(&motion, frame).unwrap();
        }
    }
    // Exercise the original modifier stream with a synthetic group attachment;
    // the source program itself uses an ordinary, single-origin emission.
    let command = cooker
        .particle_command_with_scratch(
            bytes,
            bank,
            events::Command::Emit {
                actor: 0,
                attachment:
                    resonance_content::battle::effect_inventory::EffectAttachment::BoneGroup(0),
                modifier: 0x700,
            },
            false,
            [None; 4],
        )
        .unwrap();
    cooker.result.programs[0].emissions = vec![EffectEmission {
        tick: 0,
        repeat: None,
        command,
    }];
    cooker.result.validate().unwrap();
    let bindings = cooker.result.models_for(EffectId { bank, id: 0 });
    assert_eq!(bindings.len(), 9);
    assert!((0..3).all(|animation_model| (0..3).all(|index| {
        bindings.contains(&ModelRef::EnemyAnimated {
            monster: 183,
            index,
            animation_model,
        })
    })));
}
