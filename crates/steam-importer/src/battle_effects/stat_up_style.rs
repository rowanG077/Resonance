//! Keep Living Armor's stat-up energy in the GameCube presentation style.
//! Only local sprite palette/fade settings change; the native action and every
//! effect birth, lifetime, geometry and gameplay event retain their clocks.
use crate::pe;
use anyhow::{Context, Result, ensure};
use resonance_content::battle_effect::{
    EffectOperation, declaration::Declaration, document::Definition, visual::ParticleTexture,
};
use std::collections::BTreeMap;

struct Component {
    actor: usize,
    kind: u8,
    slot: u8,
    stride: u8,
    palettes: [u8; 2],
    uv: [i16; 4],
    last_age: u16,
    flags: u32,
    fade_from: u8,
    colors: [[i16; 4]; 2],
    fade: [u8; 4],
    name: &'static str,
}

const COMPONENTS: [Component; 3] = [
    Component {
        actor: 8,
        kind: 5,
        slot: 0,
        stride: 16,
        palettes: [11, 0],
        uv: [129, 177, 30, 30],
        last_age: 20,
        flags: 0x1042,
        fade_from: 0,
        colors: [[128, 128, 128, 160], [128, 128, 128, 0]],
        fade: [4, 4, 0, 8],
        name: "stat-up-style-core",
    },
    Component {
        actor: 7,
        kind: 18,
        slot: 1,
        stride: 32,
        palettes: [26, 2],
        uv: [1, 160, 62, 32],
        last_age: 32,
        flags: 0x4000048,
        fade_from: 0,
        colors: [[128, 96, 96, 128], [96, 96, 128, 255]],
        fade: [4, 4, 0, 8],
        name: "stat-up-style-energy",
    },
    Component {
        actor: 9,
        kind: 4,
        slot: 0,
        stride: 16,
        palettes: [11, 1],
        uv: [65, 33, 30, 30],
        last_age: 24,
        flags: 0x100a,
        fade_from: 9,
        colors: [[128, 128, 64, 64], [128, 128, 128, 255]],
        fade: [6, 6, 0, 16],
        name: "stat-up-style-ground",
    },
];

fn source_guard(bytes: &[u8]) -> Result<()> {
    ensure!(
        bytes.get(..4) == Some(b"ef1\0") && bytes.get(4) == Some(&8),
        "Living Armor effect directory changed"
    );
    let actors = pe::word(bytes, 8)? as usize;
    for component in &COMPONENTS {
        let start = actors + component.actor * 360;
        let row = bytes
            .get(start..start + 360)
            .context("missing Living Armor stat-up source particle")?;
        let uv = (0..4)
            .map(|i| pe::half(row, 8 + i * 2).map(|v| v as i16))
            .collect::<Result<Vec<_>>>()?;
        let colors = (0..8)
            .map(|i| pe::half(row, 32 + i * 2).map(|v| v as i16))
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            row[0] == component.kind
                && row[2] == component.slot
                && row[3..5] == component.palettes
                && row[6] == component.stride
                && uv == component.uv
                && pe::half(row, 16)? == component.last_age
                && pe::word(row, 24)? == component.flags
                && row[57] == component.fade_from
                && colors == component.colors.into_iter().flatten().collect::<Vec<_>>()
                && row[52..56] == component.fade,
            "Living Armor stat-up source particle {} changed",
            component.actor
        );
    }
    let commands = pe::word(bytes, 12)? as usize;
    let roots = pe::word(bytes, 24)? as usize;
    let start = usize::from(pe::half(bytes, roots + 6 * 2)?);
    let end = usize::from(pe::half(bytes, roots + 7 * 2)?);
    const EXPECTED: [(u16, u8, u8, u32); 12] = [
        (20, 252, 0, 0x141),
        (20, 255, 3, 6),
        (0, 9, 0, 0),
        (20, 8, 0, 0),
        (20, 7, 0, 0),
        (23, 7, 0, 0xfd8),
        (23, 255, 5, 3),
        (0, 8, 0, 0x1044),
        (26, 7, 0, 0xfec),
        (29, 255, 2, 3),
        (0, 7, 0, 0x1000),
        (38, 254, 0, 0),
    ];
    ensure!(
        end.checked_sub(start) == Some(EXPECTED.len() * 8),
        "Living Armor stat-up source program changed"
    );
    for (index, &(at, opcode, count, value)) in EXPECTED.iter().enumerate() {
        let row = bytes
            .get(commands + start + index * 8..commands + start + (index + 1) * 8)
            .context("truncated Living Armor stat-up source program")?;
        ensure!(
            pe::half(row, 0)? == at && row[2..4] == [opcode, count] && pe::word(row, 4)? == value,
            "Living Armor stat-up source command {index} changed"
        );
    }
    Ok(())
}

/// Call after native rate normalization and before sprite atlas publication.
pub(super) fn normalize(enemy: u16, native_bank: &[u8], definition: &mut Definition) -> Result<()> {
    if enemy != 44 {
        return Ok(());
    }
    source_guard(native_bank)?;
    let program = definition
        .programs
        .get(6)
        .context("missing Living Armor stat-up program")?;
    let mut selected = BTreeMap::new();
    let mut births = BTreeMap::<(u32, usize), usize>::new();
    for event in program {
        if let EffectOperation::Spawn {
            particle,
            palette,
            birth,
            ..
        } = &event.operation
        {
            ensure!(
                palette.is_none()
                    && birth.palette.is_none()
                    && birth.alpha_palette.is_none()
                    && birth.fade.iter().all(Option::is_none),
                "Living Armor stat-up birth overrides changed"
            );
            let declaration = definition
                .templates
                .get(usize::from(*particle))
                .context("missing stat-up declaration")?;
            let Declaration::Particle { template, visual } = declaration else {
                anyhow::bail!("Living Armor stat-up sprite declaration changed");
            };
            let component = COMPONENTS
                .iter()
                .position(|component| {
                    template.state.uv == component.uv
                        && template.state.palettes == component.palettes
                        && template.state.colors == component.colors
                        && template.lifetime == Some(u32::from(component.last_age) + 1)
                        && template.fade_from == u32::from(component.fade_from)
                        && template.fade == component.fade.map(|v| v.saturating_mul(2))
                        && matches!(visual.texture, ParticleTexture::Atlas { slot, palette_set, .. }
                        if slot == component.slot && palette_set == component.stride)
                })
                .context("Living Armor stat-up sprite settings changed")?;
            selected.insert(*particle, component);
            *births.entry((event.at, component)).or_default() += 1;
        }
    }
    let expected = [
        (0, &[20, 23, 26, 29, 32, 35][..]),
        (1, &[20, 23, 26, 29, 32][..]),
        (2, &[20, 26, 32][..]),
    ]
    .into_iter()
    .flat_map(|(component, ticks)| ticks.iter().map(move |&tick| ((tick, component), 1)))
    .collect::<BTreeMap<_, _>>();
    ensure!(
        selected.len() == 3 && births == expected,
        "Living Armor stat-up births changed"
    );

    // Private clones ensure another program using an interned source template
    // keeps its native appearance. Only program6 is rebound to these variants.
    let mut replacements = BTreeMap::new();
    for (old, component) in selected {
        let source = &COMPONENTS[component];
        ensure!(
            !definition
                .template_names
                .iter()
                .any(|name| name == source.name),
            "stat-up style was already applied"
        );
        let mut declaration = definition.templates[usize::from(old)].clone();
        let template = declaration.template_mut()?;
        template.fade = source.fade;
        if component == 1 {
            // Native selector26*stride32/16 selects table52, whose remaps do
            // not cover this energy rectangle. Selector34 selects table68's
            // authored purple version of the exact same rectangle (PE8fe2f8).
            template.state.palettes[0] = 34;
        }
        let new = u8::try_from(definition.templates.len())
            .context("too many stat-up style declarations")?;
        definition.template_names.push(source.name.into());
        definition.templates.push(declaration);
        replacements.insert(old, new);
    }
    for event in &mut definition.programs[6] {
        if let EffectOperation::Spawn { particle, .. } = &mut event.operation {
            *particle = *replacements
                .get(particle)
                .context("unmapped stat-up sprite")?;
        }
    }
    Ok(())
}
