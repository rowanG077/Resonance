//! Resolve party casting from the verified technique, actor and scene tables.
use anyhow::{Context, Result, ensure};
use resonance_battle::{CastMotion, CastingDefinition, MotionBinding, ResourceBinding};
use resonance_content::{arte, battle_effect, battle_profile, prepared::Files};

/// The encounter's character, technique slot and prepared body model.
pub struct CastingResource {
    pub character: u8,
    pub technique: u16,
    pub model: u32,
    /// The prepared resident scene, when the maintained entry uses stored casting.
    pub stored_scene: Option<u16>,
}

/// Append motion bindings after the module's declared assets. Scripts receive
/// these same module-local references through the typed parameter natives.
pub(super) fn load(
    files: &Files,
    request: CastingResource,
    first_motion: usize,
    motions: &mut Vec<ResourceBinding>,
) -> Result<CastingDefinition> {
    let techniques: arte::Catalogue = files.json(arte::PATH)?;
    techniques.validate()?;
    let technique = techniques.definition(usize::from(request.technique))?;
    let tints: battle_effect::Tints = files.json(battle_effect::TINTS_PATH)?;
    let element = usize::from(technique.element);
    let palette = *tints
        .palettes
        .get(element)
        .context("invalid casting element")?;
    ensure!(
        (technique.flags & 1 != 0) == request.stored_scene.is_some(),
        "stored-scene casting binding differs from technique"
    );
    if let Some(scene) = request.stored_scene {
        ensure!(
            i32::from(scene) == i32::from(technique.native_id),
            "stored-scene technique differs"
        );
        let _: resonance_content::battle_scene::Scene =
            files.json(&resonance_content::battle_scene::path(scene))?;
    }
    ensure!(
        technique.flags & 0x60000020 == 0,
        "technique requires special casting"
    );
    let table: battle_profile::Table = files.json(battle_profile::PARTY_PATH)?;
    ensure!(table.records.len() == 11, "invalid party profile table");
    let profile = request
        .character
        .checked_sub(1)
        .and_then(|index| table.records.get(usize::from(index)))
        .context("missing party casting profile")?;
    let casting = &profile.casting;
    let rate = casting.animation_rate.finite()?;
    ensure!(
        rate > 0.,
        "ordinary casting requires a positive animation rate"
    );
    ensure!(
        i16::try_from(i32::from(casting.base_ticks) + i32::from(technique.cast_time_adjustment))
            .is_ok(),
        "casting duration exceeds signed clock"
    );
    let mut bind = |clip| {
        let index = first_motion + motions.len();
        motions.push(ResourceBinding::Motion(MotionBinding {
            model: request.model,
            clip,
        }));
        index
    };
    let release = CastMotion {
        age: 0,
        motion: bind(12),
        blend: casting.resume_blend,
        frame: f32::from(casting.resume_start) * rate,
        rate,
        repeat: casting.motion_flags & 2 != 0,
        loop_start: f32::from(casting.resume_loop_start) * rate,
    };
    let chant = if request.character == 3 {
        ensure!(
            table.chant.len() == 4 && table.chant[3].time == -2,
            "invalid Genis casting motion table"
        );
        table.chant[..3]
            .iter()
            .map(|row| {
                ensure!(
                    row.end == 0 && row.layer_flags & !0x40 == 8 && row.resource == -1,
                    "unsupported casting motion binding"
                );
                let rate = row.rate.finite()?;
                let frame = f32::from(row.start) * rate.abs();
                Ok(CastMotion {
                    age: u16::try_from(row.time)?,
                    motion: bind(u16::from(row.clip)),
                    blend: row.blend,
                    frame,
                    rate,
                    repeat: row.layer_flags & 0x40 != 0,
                    loop_start: frame,
                })
            })
            .collect::<Result<_>>()?
    } else {
        vec![CastMotion {
            age: 0,
            motion: bind(11),
            blend: 8,
            frame: 0.,
            rate,
            repeat: casting.motion_flags & 4 == 0,
            loop_start: f32::from(casting.loop_start) * rate,
        }]
    };
    Ok(CastingDefinition {
        base: casting.base_ticks,
        extra: technique.cast_time_adjustment,
        recovery: u16::try_from(technique.recovery_ticks).context("negative casting recovery")?,
        tp_cost: u16::from(technique.tp_cost),
        release,
        chant,
        pulse_member: if technique.flags & 0x00400000 != 0 {
            3
        } else if technique.flags & 0x00800000 != 0 {
            4
        } else {
            5
        },
        effect_scale: profile.effect_scale.finite()?,
        tint: battle_effect::EffectTint {
            enabled: element != 0,
            palette,
            rgb: tints.colors[element][..3].try_into()?,
        },
    })
}
