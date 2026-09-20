//! Cook martial callback data separately from shared action tracks and hit rules.
mod admission;
mod area;
mod beast;
mod magic_guard;
mod sonic;
mod steal;
use super::*;
use resonance_content::battle::{
    actions::{HammerOpening, HammerPattern, HammerRing},
    effects::{EffectBank, EffectId},
    projectile_modifiers::ProjectileOverride,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct Parameters {
    pub chains: ChainParameters,
    demon_fang: DemonFangParameters,
    tiger_blade: TigerBladeParameters,
    combo_delay: ComboDelay,
    volleys: [VolleyParameters; 2],
    hammers: [HammerParameters; 3],
    hammer_captions: [String; 2],
    area: area::Parameters,
    beasts: [beast::Parameters; 2],
    guard: magic_guard::Parameters,
    contacts: [contact::Parameters; 2],
    seals: [seals::Parameters; 9],
    sonic: sonic::Parameters,
    steal: steal::Parameters,
}

impl Parameters {
    pub fn read(rel: &Rel) -> Result<Self> {
        Ok(Self {
            chains: ChainParameters::read(rel)?,
            demon_fang: read_demon_fang(rel)?,
            tiger_blade: read_tiger_blade(rel)?,
            combo_delay: read_combo_delay(rel)?,
            volleys: [read_volley(rel, 37)?, read_volley(rel, 39)?],
            hammers: [
                read_hammer(rel, 40)?,
                read_hammer(rel, 41)?,
                read_hammer(rel, 42)?,
            ],
            hammer_captions: read_hammer_captions(rel)?,
            area: area::read_parameters(rel)?,
            beasts: [
                beast::read_parameters(rel, 20)?,
                beast::read_parameters(rel, 22)?,
            ],
            guard: magic_guard::read_parameters(rel)?,
            contacts: [
                contact::read_parameters(rel, 85)?,
                contact::read_parameters(rel, 87)?,
            ],
            seals: (63..=71)
                .map(|native| seals::read_parameters(rel, native))
                .collect::<Result<Vec<_>>>()?
                .try_into()
                .ok()
                .context("seal parameter count")?,
            sonic: sonic::read_parameters(rel)?,
            steal: steal::read_parameters(rel)?,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct ChainParameters {
    pub colors: [[u8; 4]; 4],
    pub ground_height: f32,
    pub aerial_height: f32,
    pub regal_aerial_height: f32,
}

impl ChainParameters {
    fn read(rel: &Rel) -> Result<Self> {
        let colors = rel
            .at((4, 0x11b0))?
            .get(..16)
            .context("truncated chain colors")?;
        Ok(Self {
            // The initializer copies four words; feedback consumes RGB from the first three.
            colors: std::array::from_fn(|i| colors[i * 4..i * 4 + 4].try_into().unwrap()),
            ground_height: float(rel.at((4, 0x11c0))?, 0)?,
            aerial_height: float(rel.at((4, 0x11c8))?, 0)?,
            regal_aerial_height: float(rel.at((4, 0x11c4))?, 0)?,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct ComboDelay {
    until_uses: u16,
    ticks: u16,
}

#[derive(Serialize, Deserialize)]
struct VolleyParameters {
    native: u16,
    first_tick: u16,
    interval: u16,
    count: u8,
    step_degrees: f32,
    projectile: EffectId,
}

impl VolleyParameters {
    fn bind(&self, source: &bundle::Bundle) -> Result<MartialCallback> {
        ensure!(
            (1..4).all(|phase| source.phases[phase].duration == 0),
            "unexpected martial volley alternate phase"
        );
        Ok(MartialCallback::ProjectileVolley {
            first_tick: self.first_tick,
            interval: self.interval,
            count: self.count,
            step_degrees: self.step_degrees,
            projectile: self.projectile,
            rule: source.phase_rule(0, 0)?,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct DemonFangParameters {
    delay: u16,
    pulses: ContactVolley,
    projectile: EffectId,
    volley: VolleySchedule,
    alternate: MartialAlternate,
}

impl DemonFangParameters {
    fn bind(&self, source: &bundle::Bundle, variant: u8) -> Result<MartialCallback> {
        ensure!(
            variant < 3
                && source.phases[0].duration == 62
                && source.phases[1].duration == 60
                && source.phases[2].duration == 60
                && source.phases[3].duration == 0,
            "unexpected Fierce Demon Fang phase table"
        );
        let delay = if variant == 0 { 0 } else { self.delay };
        let mut pulses = self.pulses;
        let mut alternate = self.volley;
        pulses.schedule.first_tick += delay;
        alternate.first_tick += delay;
        Ok(MartialCallback::FierceDemonFang {
            pulses,
            projectile: self.projectile,
            alternate,
            rules: [
                source.phase_rule(usize::from(variant), 1)?,
                source.phase_rule(usize::from(variant), 2)?,
            ],
        })
    }
}

#[derive(Serialize, Deserialize)]
struct TigerBladeParameters {
    minimum_uses: u16,
    element: resonance_content::menu_data::Element,
    tick: u16,
    projectile: EffectId,
    origin_height: f32,
    overrides: [ProjectileOverride; 3],
    caption: String,
}

impl TigerBladeParameters {
    fn bind(&self, source: &bundle::Bundle, variant: u8) -> Result<Option<MartialCallback>> {
        ensure!(variant < 2, "unexpected Tiger Blade phase");
        if variant == 0 {
            return Ok(None);
        }
        ensure!(
            (0..2).all(|phase| source.phases[phase].duration == 40)
                && (2..4).all(|phase| source.phases[phase].duration == 0),
            "unexpected Tiger Blade phase table"
        );
        Ok(Some(MartialCallback::TigerBlade {
            minimum_uses: self.minimum_uses,
            element: self.element,
            tick: self.tick,
            projectile: self.projectile,
            rule: source.phase_rule(1, 1)?,
            origin_height: self.origin_height,
            overrides: self.overrides,
        }))
    }
}

#[derive(Serialize, Deserialize)]
struct HammerParameters {
    native: u16,
    first_tick: u16,
    interval: u16,
    count: u8,
    pattern: HammerPatternParameters,
    rule_offset: usize,
    birth_effects: [u8; 3],
    overrides: Option<[ProjectileOverride; 3]>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum HammerPatternParameters {
    Single,
    Repeated,
    Rain {
        tick: u16,
        projectile: EffectId,
        ring: HammerRing,
    },
}

impl HammerParameters {
    fn bind(&self, source: &bundle::Bundle) -> Result<MartialCallback> {
        if self.native == 40 {
            ensure!(
                source.phases[1].duration != 0,
                "missing alternate hammer phase"
            );
        }
        ensure!(
            (if self.native == 40 { 2 } else { 1 }..4)
                .all(|phase| source.phases[phase].duration == 0),
            "unexpected hammer alternate phase"
        );
        let pattern = match self.pattern {
            HammerPatternParameters::Single => HammerPattern::Single,
            HammerPatternParameters::Repeated => HammerPattern::Repeated,
            HammerPatternParameters::Rain {
                tick,
                projectile,
                ring,
            } => HammerPattern::Rain {
                opening: HammerOpening {
                    tick,
                    projectile,
                    rule: source.rule(0)?,
                },
                ring,
            },
        };
        Ok(MartialCallback::HammerVolley {
            first_tick: self.first_tick,
            interval: self.interval,
            count: self.count,
            pattern,
            projectile: EffectId {
                bank: EffectBank::Techniques,
                id: 7,
            },
            rules: [
                source.rule(self.rule_offset)?,
                source.rule(self.rule_offset + 1)?,
                source.rule(self.rule_offset + 2)?,
            ],
            birth_effects: self.birth_effects,
            overrides: self.overrides,
        })
    }
}
pub(super) use steal::continuations as steal_continuations;
mod contact;
mod seals;

pub(super) fn callback(
    parameters: &Parameters,
    rel: &Rel,
    native: u16,
    source: &bundle::Bundle,
    variant: u8,
) -> Result<Option<MartialCallback>> {
    let p = parameters;
    let callback = match native {
        4 => p.demon_fang.bind(source, variant)?,
        6 => return p.tiger_blade.bind(source, variant),
        10 => return sonic::callback(&p.sonic, source, variant),
        11 => area::callback(&p.area, source, variant)?,
        14 => MartialCallback::ComboDelay {
            until_uses: p.combo_delay.until_uses,
            ticks: p.combo_delay.ticks,
        },
        20 | 22 => beast::callback(
            p.beasts
                .iter()
                .find(|p| p.native == native)
                .context("missing Beast parameters")?,
            source,
            variant,
        )?,
        34 => magic_guard::callback(&p.guard, source, variant)?,
        37 | 39 => p
            .volleys
            .iter()
            .find(|p| p.native == native)
            .context("missing volley parameters")?
            .bind(source)?,
        40..=42 => p
            .hammers
            .iter()
            .find(|p| p.native == native)
            .context("missing hammer parameters")?
            .bind(source)?,
        43 | 44 => steal::callback(&p.steal, source, variant)?,
        63..=71 => seals::callback(
            p.seals
                .iter()
                .find(|p| p.native == native)
                .context("missing seal parameters")?,
            source,
            variant,
        )?,
        85 | 87 => contact::callback(
            p.contacts
                .iter()
                .find(|p| p.native == native)
                .context("missing contact parameters")?,
            source,
            variant,
        )?,
        _ => {
            admission::validate(rel, native)?;
            return Ok(None);
        }
    };
    Ok(Some(callback))
}

fn read_combo_delay(rel: &Rel) -> Result<ComboDelay> {
    let dispatch = rel.pointer(DATA, 0xd60 + 14 * 4)?;
    ensure!(
        rel.pointer(dispatch.0, dispatch.1)? == (1, 0x60aec)
            && rel.pointer(dispatch.0, dispatch.1 + 4)? == (1, 0x37fd4),
        "unexpected Sword Rain entry"
    );
    let code = rel.at((1, 0x60aec))?;
    for (offset, instruction) in [
        (0x14, 0x4bfd773d),
        (0x1c, 0x3880000e),
        (0x20, 0x4bfc0dd9),
        (0x24, 0x7c600734),
        (0x28, 0x2c0000c8),
        (0x2c, 0x40800010),
        (0x30, 0xa87f1170),
        (0x34, 0x38030014),
        (0x38, 0xb01f1170),
    ] {
        ensure!(
            word(code, offset)? == instruction,
            "unexpected Sword Rain use-count delay"
        );
    }
    Ok(ComboDelay {
        until_uses: half(code, 0x2a)?,
        ticks: half(code, 0x36)?,
    })
}

fn read_volley(rel: &Rel, native: u16) -> Result<VolleyParameters> {
    let (initializer, update, spread, projectile) = match native {
        37 => (0x91dd8, 0x91d14, 0x97d0, 6),
        39 => (0x91ed4, 0x91e10, 0x9830, 11),
        _ => bail!("unsupported martial projectile volley"),
    };
    let dispatch = rel.pointer(DATA, 0xd60 + usize::from(native) * 4)?;
    ensure!(
        rel.pointer(dispatch.0, dispatch.1)? == (1, initializer)
            && rel.pointer(dispatch.0, dispatch.1 + 4)? == (1, 0x37fd4),
        "unexpected martial volley dispatch"
    );
    let code = rel.at((1, update))?;
    // Bound the native loop and rule source before replacing it with a named recipe.
    ensure!(
        word(code, 0x14)? == 0x2c050014
            && word(code, 0x1c)? == 0x2c050026
            && word(code, 0x28)? == 0x38a5ffec
            && word(code, 0x3c)? == 0x1c030006
            && word(code, 0x80)? == (0x38e00000 | u32::from(projectile))
            && word(code, 0xac)? == 0x8108000c,
        "unexpected martial volley timing or projectile binding"
    );
    let first_tick = half(code, 0x16)?;
    let interval = half(code, 0x3e)?;
    let end_tick = half(code, 0x1e)?;
    let step_degrees = float(rel.at((4, spread))?, 0)?;
    ensure!(step_degrees.is_finite(), "invalid martial volley spread");
    Ok(VolleyParameters {
        native,
        first_tick,
        interval,
        count: ((end_tick - first_tick) / interval).try_into()?,
        step_degrees,
        projectile: EffectId {
            bank: EffectBank::Techniques,
            id: projectile,
        },
    })
}

pub(super) fn magic_guard_variants(
    parameters: &Parameters,
    native: u16,
    variants: &mut Vec<TechniquePhase>,
) -> Result<()> {
    magic_guard::expand(&parameters.guard, native, variants)
}

pub(super) fn caption(p: &Parameters, native: u16, variant: u8) -> Result<Option<String>> {
    if native == 10 {
        return sonic::caption(&p.sonic, variant);
    }
    Ok((native == 6 && variant == 1).then(|| p.tiger_blade.caption.clone()))
}

fn read_tiger_caption(rel: &Rel) -> Result<String> {
    ensure!(
        word(rel.at((4, 0x35b0))?, 0)? == 0,
        "unexpected Tiger Blade title table"
    );
    let text = rel.at(rel.pointer(4, 0x35b4)?)?;
    let end = text
        .iter()
        .position(|&byte| byte == 0)
        .context("unterminated Tiger Blade title")?;
    let caption = std::str::from_utf8(&text[..end])?;
    ensure!(!caption.is_empty(), "empty Tiger Blade title");
    Ok(caption.to_owned())
}

pub(super) fn alternate(p: &Parameters, native: u16) -> Option<MartialAlternate> {
    (native == 4).then(|| p.demon_fang.alternate.clone())
}

fn read_alternate(rel: &Rel) -> Result<MartialAlternate> {
    let entry = fierce_demon_fang_entry(rel)?;
    ensure!(
        word(rel.at((4, 0x34c4))?, 0)? == 0,
        "unexpected Fierce Demon Fang caption table"
    );
    let text = rel.at(rel.pointer(4, 0x34c8)?)?;
    let end = text
        .iter()
        .position(|&byte| byte == 0)
        .context("unterminated martial caption")?;
    let caption = std::str::from_utf8(&text[..end])?.to_owned();
    ensure!(!caption.is_empty(), "empty elemental martial caption");
    Ok(MartialAlternate {
        minimum_uses: half(entry, 0x92)?,
        element: resonance_content::menu_data::Element::ALL[usize::from(half(entry, 0xb2)?) - 1],
        effect: EffectId {
            bank: EffectBank::Techniques,
            id: half(entry, 0xee)?.try_into()?,
        },
        caption,
    })
}

fn fierce_demon_fang_entry(rel: &Rel) -> Result<&[u8]> {
    let dispatch = rel.pointer(DATA, 0xd60 + 4 * 4)?;
    ensure!(
        rel.pointer(dispatch.0, dispatch.1)? == (1, 0x5fc38)
            && rel.pointer(dispatch.0, dispatch.1 + 4)? == (1, 0x37fd4),
        "unexpected Fierce Demon Fang dispatch"
    );
    let entry = rel.at((1, 0x5fc38))?;
    for (offset, instruction) in [
        (0x2c, 0x28060001),
        (0x48, 0x38800004),
        (0x68, 0x38040000),
        (0x6c, 0x388000d1),
        (0x70, 0x901f0018),
        (0x84, 0x38800004),
        (0x90, 0x2c0000c8),
        (0x9c, 0x38800000),
        (0xa0, 0x38a00000),
        (0xb0, 0x20630004),
        (0xc4, 0x5400073f),
        (0xe8, 0x38800001),
        (0xec, 0x38a0000b),
        (0xf0, 0x38c00000),
    ] {
        ensure!(
            word(entry, offset)? == instruction,
            "unexpected Fierce Demon Fang entry at {offset:#x}"
        );
    }
    Ok(entry)
}

fn read_demon_fang(rel: &Rel) -> Result<DemonFangParameters> {
    fierce_demon_fang_entry(rel)?;
    let code = rel.at((1, 0x5fa14))?;
    for (offset, instruction) in [
        (0x38, 0x28000001),
        (0x60, 0x38c6fffb),
        (0x6c, 0x906b15d4),
        (0x74, 0x800b18c8),
        (0x78, 0x900b15dc),
        (0x8c, 0x2c040012),
        (0x94, 0x2c04001e),
        (0xb4, 0x1c000003),
        (0xd0, 0x39050038),
        (0xdc, 0x38e00009),
        (0x100, 0x2c030014),
        (0x108, 0x2c03001e),
        (0x12c, 0x7c831670),
        (0x150, 0x7fa00e70),
        (0x15c, 0x7c0300ae),
        (0x160, 0x3885001c),
        (0x168, 0x39800001),
        (0x188, 0x38c00002),
        (0x18c, 0x39000409),
        (0x190, 0x39200001),
        (0x198, 0x39400000),
        (0x1fc, 0xec2308ba),
        (0x200, 0xd0230068),
        (0x204, 0xd0030064),
    ] {
        ensure!(
            word(code, offset)? == instruction,
            "unexpected Fierce Demon Fang update at {offset:#x}"
        );
    }
    let delay = -(half(code, 0x62)? as i16) as u16;
    let ro = rel.at((4, 0x34cc))?;
    Ok(DemonFangParameters {
        delay,
        pulses: ContactVolley {
            schedule: VolleySchedule {
                first_tick: half(code, 0x102)?,
                interval: 4,
                count: 3,
            },
            lifetime: half(code, 0x18a)?,
            velocity: [float(ro, 0)?, float(ro, 4)?, float(ro, 8)?],
            offset: [0., float(ro, 0x18)?, float(ro, 0x10)?],
            forward_step: float(ro, 0x14)?,
            radius: float(ro, 0x10)?,
            reactions: ro[12..14].try_into()?,
        },
        projectile: EffectId {
            bank: EffectBank::Techniques,
            id: half(code, 0xde)?.try_into()?,
        },
        volley: VolleySchedule {
            first_tick: half(code, 0x8e)?,
            interval: half(code, 0xb6)?,
            count: 5,
        },
        alternate: read_alternate(rel)?,
    })
}

fn read_tiger_blade(rel: &Rel) -> Result<TigerBladeParameters> {
    use resonance_content::battle::projectile_modifiers::{
        Axis, ProjectileOverride, ProjectileVector,
    };
    let dispatch = rel.pointer(DATA, 0xd60 + 6 * 4)?;
    ensure!(
        rel.pointer(dispatch.0, dispatch.1)? == (1, 0x601a0)
            && rel.pointer(dispatch.0, dispatch.1 + 4)? == (1, 0x37fd4),
        "unexpected Tiger Blade dispatch"
    );
    let entry = rel.at((1, 0x601a0))?;
    for (offset, instruction) in [
        (0x30, 0x38800006),
        (0x48, 0x2c0000c8),
        (0x54, 0x38800000),
        (0x58, 0x38a00000),
        (0x68, 0x20630005),
        (0x88, 0x38800006),
        (0x8c, 0x38a00001),
        (0xc4, 0x901f0018),
    ] {
        ensure!(
            word(entry, offset)? == instruction,
            "unexpected Tiger Blade entry at {offset:#x}"
        );
    }
    let code = rel.at((1, 0x600dc))?;
    for (offset, instruction) in [
        (0x14, 0x5400073f),
        (0x20, 0x2c000016),
        (0x3c, 0x38600001),
        (0x44, 0x38e00004),
        (0x70, 0x8108000c),
        (0x78, 0x3908001c),
        (0x88, 0x3800004a),
        (0x90, 0x9803005d),
        (0x94, 0x38000001),
        (0x9c, 0x38800000),
        (0xa4, 0xec01002a),
        (0xa8, 0xd0030068),
        (0xac, 0x98830011),
        (0xb0, 0x98030012),
    ] {
        ensure!(
            word(code, offset)? == instruction,
            "unexpected Tiger Blade emission at {offset:#x}"
        );
    }
    // Native emission explicitly advances one rule beyond the active descriptor's
    // rule-table start; the unused third row is not this projectile's rule.
    let minimum_uses = half(entry, 0x4a)?;
    let element = resonance_content::menu_data::Element::ALL[usize::from(half(entry, 0x6a)?) - 1];
    Ok(TigerBladeParameters {
        minimum_uses,
        element,
        tick: half(code, 0x22)?,
        projectile: EffectId {
            bank: EffectBank::Techniques,
            id: u8::try_from(half(code, 0x46)?)?,
        },
        caption: read_tiger_caption(rel)?,
        origin_height: float(rel.at((4, 0x35b8))?, 0)?,
        overrides: [
            ProjectileOverride::BirthEffect {
                id: u8::try_from(half(code, 0x8a)?)?,
            },
            ProjectileOverride::AddComponent {
                field: ProjectileVector::SpawnOffset,
                axis: Axis::Z,
                value: float(rel.at((4, 0x35bc))?, 0)?,
            },
            ProjectileOverride::HitClassification {
                damage_kind: u8::try_from(half(code, 0x9e)?)?,
                hit_class: u8::try_from(half(code, 0x96)?)?,
            },
        ],
    })
}

fn read_hammer(rel: &Rel, native: u16) -> Result<HammerParameters> {
    use resonance_content::battle::projectile_modifiers::{
        Axis, ProjectileOverride, ProjectileVector,
    };
    let (initializer, update) = match native {
        40 => (0x64ca0, 0x64c08),
        41 => (0x64f60, 0x64dd0),
        42 => (0x5bd88, 0x5ba30),
        _ => bail!("unsupported hammer native"),
    };
    let dispatch = rel.pointer(DATA, 0xd60 + usize::from(native) * 4)?;
    ensure!(
        rel.pointer(dispatch.0, dispatch.1)? == (1, initializer)
            && rel.pointer(dispatch.0, dispatch.1 + 4)? == (1, 0x37fd4),
        "unexpected hammer dispatch"
    );
    let code = rel.at((1, update))?;
    let verify = |code: &[u8], words: &[(usize, u32)]| -> Result<()> {
        for &(offset, instruction) in words {
            ensure!(
                word(code, offset)? == instruction,
                "unexpected hammer native {native} policy at {offset:#x}"
            );
        }
        Ok(())
    };
    let parameter = |offset| float(rel.at((4, offset))?, 0);
    let (first_tick, interval, count, first_effect, rule_offset, pattern, overrides) = match native
    {
        40 => {
            verify(
                rel.at((1, initializer))?,
                &[
                    (0x14, 0x38800028),
                    (0x40, 0x2c0000c8),
                    (0x5c, 0xa8040042),
                    (0x60, 0x7c001e70),
                    (0xa4, 0x20630006),
                    (0xd4, 0x38800028),
                    (0xd8, 0x5406073e),
                    (0xdc, 0x3806ffff),
                    (0xe0, 0x7cc52b38),
                    (0xe4, 0x5400f87e),
                    (0xe8, 0x7c002850),
                    (0xec, 0x54050ffe),
                    (0xf0, 0x4bfd3389),
                    (0x108, 0x7c84002e),
                ],
            )?;
            verify(
                code,
                &[
                    (0x18, 0x2c000016),
                    (0x34, 0x38e00007),
                    (0x58, 0x1c00001c),
                    (0x5c, 0x8108000c),
                    (0x7c, 0x3804000f),
                    (0x80, 0x9803005d),
                ],
            )?;
            (
                half(code, 0x1a)?,
                1,
                1,
                half(code, 0x7e)? as u8,
                0,
                HammerPatternParameters::Single,
                None,
            )
        }
        41 => {
            verify(
                code,
                &[
                    (0x1c, 0x2c040015),
                    (0x24, 0x2c04001d),
                    (0x2c, 0x3804ffeb),
                    (0x34, 0x540007fe),
                    (0x44, 0x38800029),
                    (0x50, 0x2c0000c8),
                    (0x6c, 0xa8040042),
                    (0x70, 0x7c002670),
                    (0x78, 0x1c850064),
                    (0xa8, 0x28000006),
                    (0xb4, 0x546007bf),
                    (0xec, 0x38e00007),
                    (0x104, 0x8108000c),
                    (0x120, 0x381e000f),
                    (0x128, 0x9803005d),
                    (0x140, 0xd0030060),
                    (0x150, 0xd0230064),
                    (0x160, 0xd0030068),
                    (0x168, 0xd023002c),
                    (0x16c, 0xd003006c),
                    (0x170, 0xd0030070),
                    (0x174, 0xd0030074),
                ],
            )?;
            let first_tick = half(code, 0x1e)?;
            (
                first_tick,
                2,
                ((half(code, 0x26)? - first_tick) / 2).try_into()?,
                half(code, 0x122)? as u8,
                0,
                HammerPatternParameters::Repeated,
                Some([
                    ProjectileOverride::Vector {
                        field: ProjectileVector::SpawnOffset,
                        value: [parameter(0x4018)?, parameter(0x401c)?, parameter(0x4020)?],
                    },
                    ProjectileOverride::Component {
                        field: ProjectileVector::Velocity,
                        axis: Axis::Z,
                        value: parameter(0x4024)?,
                    },
                    ProjectileOverride::Vector {
                        field: ProjectileVector::VelocityJitter,
                        value: [parameter(0x4028)?; 3],
                    },
                ]),
            )
        }
        42 => {
            verify(
                code,
                &[
                    (0x34, 0x2c000016),
                    (0x50, 0x38e0000d),
                    (0x70, 0x8108000c),
                    (0x7c, 0x2c00003c),
                    (0x8c, 0x38800029),
                    (0x98, 0x2c0000c8),
                    (0xb4, 0xa8040042),
                    (0xb8, 0x7c002670),
                    (0xc0, 0x1c850064),
                    (0xf0, 0x28000006),
                    (0xfc, 0x546007bf),
                    (0x120, 0x381c0001),
                    (0x124, 0x1c00001c),
                    (0x13c, 0x38e00007),
                    (0x150, 0x8108000c),
                    (0x17c, 0x38bc0040),
                    (0x1ac, 0x98bf005d),
                    (0x1c0, 0xd07f0060),
                    (0x1d4, 0xd03f0064),
                    (0x1dc, 0xd01f0068),
                    (0x22c, 0x1c000032),
                    (0x244, 0xec0200fa),
                    (0x24c, 0xd01f0024),
                    (0x298, 0x1c000032),
                    (0x2b0, 0xec0100ba),
                    (0x2b8, 0xd01f002c),
                    (0x2fc, 0x1c000032),
                    (0x314, 0xec0100ba),
                    (0x318, 0xd01f0028),
                    (0x320, 0x2c1e000a),
                ],
            )?;
            (
                half(code, 0x7e)?,
                1,
                1,
                half(code, 0x17e)? as u8,
                1,
                HammerPatternParameters::Rain {
                    tick: half(code, 0x36)?,
                    projectile: EffectId {
                        bank: EffectBank::Techniques,
                        id: half(code, 0x52)? as u8,
                    },
                    ring: HammerRing {
                        count: half(code, 0x322)? as u8,
                        spawn_offset: [parameter(0x31f0)?, parameter(0x31f4)?, parameter(0x31f8)?],
                        angle_step: parameter(0x3200)?,
                        radians_per_degree: parameter(0x31fc)?,
                        speed: parameter(0x3204)?,
                        speed_jitter: parameter(0x3208)?,
                        lift: parameter(0x320c)?,
                        lift_jitter: parameter(0x3210)?,
                        jitter_range: half(code, 0x22e)?,
                    },
                },
                None,
            )
        }
        _ => unreachable!(),
    };
    for operation in overrides.into_iter().flatten() {
        operation.validate()?;
    }
    Ok(HammerParameters {
        native,
        first_tick,
        interval,
        count,
        pattern,
        rule_offset,
        birth_effects: [first_effect, first_effect + 1, first_effect + 2],
        overrides,
    })
}

/// Native kind 0 uses source phase 0; ice and poison share phase 1's tracks,
/// but keep distinct captions and projectile rules in their cooked variants.
pub(super) fn hammer_variants(
    p: &Parameters,
    native: u16,
    variants: &mut Vec<TechniquePhase>,
) -> Result<()> {
    if native != 40 {
        return Ok(());
    }
    ensure!(
        variants.len() == 2 && variants[0].variant == 0 && variants[1].variant == 1,
        "unexpected hammer source phases"
    );
    let mut poison = variants[1].clone();
    poison.variant = 2;
    variants.push(poison);
    for (variant, caption) in variants[1..].iter_mut().zip(&p.hammer_captions) {
        variant.caption = Some(caption.clone());
    }
    Ok(())
}

fn read_hammer_captions(rel: &Rel) -> Result<[String; 2]> {
    ensure!(
        word(rel.at((4, 0x3fb8))?, 0)? == 0,
        "unexpected default hammer caption"
    );
    let read = |offset| -> Result<String> {
        let bytes = rel.at(rel.pointer(4, offset)?)?;
        let end = bytes
            .iter()
            .position(|&byte| byte == 0)
            .context("unterminated hammer caption")?;
        let (text, _, invalid) = encoding_rs::SHIFT_JIS.decode(&bytes[..end]);
        ensure!(!invalid && !text.is_empty(), "invalid hammer caption");
        Ok(text.into_owned())
    };
    Ok([read(0x3fbc)?, read(0x3fc0)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires locally extracted GameCube assets"]
    fn fierce_demon_fang_recovers_character_delays_inline_contacts_and_grave_blade_closure() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        assert_eq!(float(rel.at((4, 0x15b0)).unwrap(), 0).unwrap(), 0.);
        assert_eq!(float(rel.at((4, 0x15c0)).unwrap(), 0).unwrap(), 0.1);
        assert_eq!(float(rel.at((4, 0x1668)).unwrap(), 0).unwrap(), 0.2);
        let techniques = technique_actions(&extracted, &rel, &usual, &[4, 209]).unwrap();
        for technique in &techniques {
            assert_eq!(technique.native_id, 4);
            let TechniqueProgram::Martial { variants } = &technique.program else {
                panic!()
            };
            assert_eq!(variants.len(), 3);
            for (variant, phase) in variants.iter().enumerate() {
                assert_eq!(phase.effect, Some(if variant == 0 { 9 } else { 10 }));
                assert!(matches!(
                    phase.action.commands.as_slice(),
                    [
                        TimedCommand {
                            tick: 0,
                            command: ActionCommand::Voice { .. }
                        },
                        TimedCommand {
                            tick: 4,
                            command: ActionCommand::ForwardSpeed(8.)
                        },
                        TimedCommand {
                            tick: 20,
                            command: ActionCommand::SecondaryWind {
                                enabled: true,
                                duration: 40
                            }
                        },
                    ]
                ));
                let alternate = phase.alternate.as_ref().unwrap();
                assert_eq!(alternate.minimum_uses, 200);
                assert_eq!(
                    alternate.element,
                    resonance_content::menu_data::Element::Earth
                );
                assert_eq!(alternate.caption, "Grave Blade");
                assert_eq!(
                    alternate.effect,
                    EffectId {
                        bank: EffectBank::Techniques,
                        id: 11
                    }
                );
                let Some(MartialCallback::FierceDemonFang {
                    pulses,
                    projectile,
                    alternate,
                    rules,
                }) = phase.callback
                else {
                    panic!()
                };
                let delay = if variant == 0 { 0 } else { 5 };
                assert_eq!(
                    (
                        pulses.schedule.first_tick,
                        pulses.schedule.interval,
                        pulses.schedule.count
                    ),
                    (20 + delay, 4, 3)
                );
                assert_eq!(
                    (alternate.first_tick, alternate.interval, alternate.count),
                    (18 + delay, 3, 5)
                );
                assert_eq!(pulses.velocity, [0.; 3]);
                assert_eq!(pulses.offset, [0., 50., 90.]);
                assert_eq!(
                    (pulses.radius, pulses.forward_step, pulses.lifetime),
                    (90., 30., 2)
                );
                assert_eq!(pulses.reactions, [1, 7]);
                assert_eq!(rules.map(|rule| rule.power), [60, 70]);
                assert_eq!(
                    projectile,
                    EffectId {
                        bank: EffectBank::Techniques,
                        id: 9
                    }
                );
            }
        }
        let actions = BattleActions {
            chains: None,
            party: vec![],
            enemies: vec![],
            projectiles: vec![],
            techniques,
        };
        actions.validate().unwrap();
        let dependencies = crate::battle::selection::Dependencies::actions(&actions).unwrap();
        assert_eq!(
            dependencies.projectiles,
            [EffectId {
                bank: EffectBank::Techniques,
                id: 9
            }]
            .into()
        );
        for id in [9, 10, 11] {
            assert!(dependencies.programs.contains(&EffectId {
                bank: EffectBank::Techniques,
                id
            }));
        }
        // The alternate's emitted row contributes its own birth timeline when recipes are loaded.
        let effects = crate::battle::effects::cook(&extracted, &dependencies.projectiles).unwrap();
        let projectile = &effects.projectiles[0];
        assert_eq!(
            projectile.spawn_effect,
            Some(EffectId {
                bank: EffectBank::Techniques,
                id: 12
            })
        );
    }

    #[test]
    fn tiger_blade_recovers_second_rule_absolute_height_and_additive_customization() {
        use resonance_content::battle::projectile_modifiers::{
            Axis, ProjectileOverride, ProjectileVector,
        };
        // Complete US native update and initializer through callback installation.
        let instructions: &[u32] = &[
            0x9421ffe0, 0x7c0802a6, 0x7c641b78, 0x90010024, 0x8803117f, 0x5400073f, 0x4182009c,
            0xa80401be, 0x2c000016, 0x40820090, 0x3c600000, 0xc04418c0, 0xc0230000, 0x38c10008,
            0xc00418c8, 0x38600001, 0xd0410014, 0x38e00004, 0xd0210018, 0x81010014, 0xd001001c,
            0x80a10018, 0x8001001c, 0x91010008, 0x90a1000c, 0x90010010, 0x81040014, 0x80a41990,
            0x8108000c, 0xc02418d0, 0x3908001c, 0x4bfc0455, 0x28030000, 0x41820030, 0x3800004a,
            0x3c800000, 0x9803005d, 0x38000001, 0xc0040000, 0x38800000, 0xc0230068, 0xec01002a,
            0xd0030068, 0x98830011, 0x98030012, 0x80010024, 0x7c0803a6, 0x38210020, 0x4e800020,
            0x9421ffe0, 0x7c0802a6, 0x3c800000, 0x90010024, 0x38a40000, 0x38800000, 0x93e1001c,
            0x7c7f1b78, 0x80c50000, 0x8803117f, 0x5080073e, 0x80a50004, 0x38800006, 0x90c10008,
            0x90a1000c, 0x9803117f, 0x4bfc1705, 0x7c600734, 0x2c0000c8, 0x4180002c, 0x7fe3fb78,
            0x38800000, 0x38a00000, 0x4bfc0509, 0x5463063e, 0x881f117f, 0x20630005, 0x7c630034,
            0x5060df3e, 0x981f117f, 0x881f117f, 0x5400073f, 0x41820018, 0x7fe3fb78, 0x38800006,
            0x38a00001, 0x4bfd7ee9, 0x4800000c, 0x7fe3fb78, 0x4bfd8001, 0x881f117f, 0x38810008,
            0x7fe3fb78, 0x38a00000, 0x540016ba, 0x7c84002e, 0x4bfbab49, 0x3c600000, 0x38030000,
            0x901f0018,
        ];
        let mut rel = Rel {
            bytes: vec![0; 0x74000],
            sections: vec![(0, 0), (4, 0x70000), (0, 0), (0, 0), (0x70004, 0x3ffc)],
            pointers: [
                ((DATA, 0xd60 + 6 * 4), (DATA, 0x100)),
                ((DATA, 0x100), (1, 0x601a0)),
                ((DATA, 0x104), (1, 0x37fd4)),
                ((4, 0x35b4), (4, 0x35a0)),
            ]
            .into(),
            local_targets: BTreeSet::new(),
        };
        for (index, instruction) in instructions.iter().enumerate() {
            let at = 4 + 0x600dc + index * 4;
            rel.bytes[at..at + 4].copy_from_slice(&instruction.to_be_bytes());
        }
        rel.bytes[0x70004 + 0x35a0..0x70004 + 0x35af].copy_from_slice(b"Lightning Tiger");
        rel.bytes[0x70004 + 0x35bc..0x70004 + 0x35c0].copy_from_slice(&120_f32.to_be_bytes());
        let mut source = vec![0; 128 + 3 * RULE_BYTES];
        source[..4].copy_from_slice(&128_u32.to_be_bytes());
        for at in [4, 8, 12] {
            let end = source.len() as u32;
            source[at..at + 4].copy_from_slice(&end.to_be_bytes());
        }
        for start in [16, 44] {
            source[start..start + 2].copy_from_slice(&40_u16.to_be_bytes());
        }
        // Deliberately distinct adjacent rules expose off-by-one table selection.
        for (index, power) in [80_u16, 80, 50].into_iter().enumerate() {
            let start = 128 + index * RULE_BYTES;
            source[start..start + 8].copy_from_slice(&[0, 32, 0, 30, 30, 5, 5, 1]);
            source[start + 13] = 1;
            source[start + 14..start + 16].copy_from_slice(&power.to_be_bytes());
        }
        source[128 + 2 * RULE_BYTES + 2] = 5;
        source[128 + RULE_BYTES + 3] = 31;
        let mut source = bundle::Bundle::decode(&source).unwrap();
        let Some(MartialCallback::TigerBlade {
            minimum_uses,
            element,
            tick,
            projectile,
            rule,
            origin_height,
            overrides,
        }) = read_tiger_blade(&rel)
            .and_then(|p| p.bind(&source, 1))
            .unwrap()
        else {
            panic!("missing Tiger Blade callback")
        };
        assert_eq!((minimum_uses, tick, projectile.id), (200, 22, 4));
        assert!(matches!(
            element,
            resonance_content::menu_data::Element::Lightning
        ));
        assert_eq!(origin_height, 0.);
        assert_eq!((rule.power, rule.hitstun), (80, 31));
        assert!(matches!(rule.element, HitElement::Inherit));
        assert!(matches!(
            overrides,
            [
                ProjectileOverride::BirthEffect { id: 74 },
                ProjectileOverride::AddComponent {
                    field: ProjectileVector::SpawnOffset,
                    axis: Axis::Z,
                    value: 120.
                },
                ProjectileOverride::HitClassification {
                    damage_kind: 0,
                    hit_class: 1
                }
            ]
        ));
        assert_eq!(
            Some(read_tiger_caption(&rel).unwrap().as_str()),
            Some("Lightning Tiger")
        );
        assert!(
            read_tiger_blade(&rel)
                .and_then(|p| p.bind(&source, 0))
                .unwrap()
                .is_none()
        );
        source.phases[1].hit_rule_root = RULE_BYTES;
        let Some(MartialCallback::TigerBlade { rule, .. }) = read_tiger_blade(&rel)
            .and_then(|p| p.bind(&source, 1))
            .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(rule.power, 50); // Descriptor base is respected, not fixed to row 1.
        source.phases[1].hit_rule_root = usize::MAX;
        assert!(
            read_tiger_blade(&rel)
                .and_then(|p| p.bind(&source, 1))
                .is_err()
        );
        source.phases[1].hit_rule_root = 0;
        rel.bytes[4 + 0x600dc + 0x23] = 23;
        assert!(
            read_tiger_blade(&rel)
                .and_then(|p| p.bind(&source, 1))
                .is_err()
        );
    }

    #[test]
    fn volley_recovers_native_schedule_spread_and_rule_without_authored_hit_rows() {
        for (native, initializer, update, spread, projectile, step, power, cooldown) in [
            (37, 0x91dd8, 0x91d14, 0x97d0, 6, -20_f32, 120_u16, 30),
            (39, 0x91ed4, 0x91e10, 0x9830, 11, -45., 50, 5),
        ] {
            let mut rel = Rel {
                bytes: vec![0; 0xaa004],
                sections: vec![(0, 0), (4, 0xa0000), (0, 0), (0, 0), (0xa0004, 0xa000)],
                pointers: [
                    ((DATA, 0xd60 + usize::from(native) * 4), (DATA, 0x100)),
                    ((DATA, 0x100), (1, initializer)),
                    ((DATA, 0x104), (1, 0x37fd4)),
                ]
                .into(),
                local_targets: BTreeSet::new(),
            };
            for (offset, instruction) in [
                (0x14, 0x2c050014_u32),
                (0x1c, 0x2c050026),
                (0x28, 0x38a5ffec),
                (0x3c, 0x1c030006),
                (0x80, 0x38e00000 | u32::from(projectile)),
                (0xac, 0x8108000c),
            ] {
                let at = 4 + update + offset;
                rel.bytes[at..at + 4].copy_from_slice(&instruction.to_be_bytes());
            }
            let at = 0xa0004 + spread;
            rel.bytes[at..at + 4].copy_from_slice(&step.to_be_bytes());
            let mut source = vec![0; 128 + RULE_BYTES];
            source[..4].copy_from_slice(&128_u32.to_be_bytes());
            for at in [4, 8, 12] {
                let end = source.len() as u32;
                source[at..at + 4].copy_from_slice(&end.to_be_bytes());
            }
            source[16..18].copy_from_slice(&95_u16.to_be_bytes());
            source[128..136].copy_from_slice(&[0, 32, 0, 30, cooldown, 0, 5, 1]);
            source[141] = 1;
            source[142..144].copy_from_slice(&power.to_be_bytes());
            let mut source = bundle::Bundle::decode(&source).unwrap();
            let Some(MartialCallback::ProjectileVolley {
                first_tick,
                interval,
                count,
                step_degrees,
                projectile: id,
                rule,
            }) = read_volley(&rel, native)
                .and_then(|p| p.bind(&source))
                .map(Some)
                .unwrap()
            else {
                panic!("missing volley")
            };
            assert_eq!((first_tick, interval, count), (20, 6, 3));
            assert_eq!(step_degrees, step);
            assert_eq!(
                id,
                EffectId {
                    bank: EffectBank::Techniques,
                    id: projectile
                }
            );
            assert_eq!((rule.power, rule.contact_cooldown), (power, cooldown));
            source.phases[0].hit_rule_root = usize::MAX;
            assert!(
                read_volley(&rel, native)
                    .and_then(|p| p.bind(&source))
                    .map(Some)
                    .is_err()
            );
            source.phases[0].hit_rule_root = 0;
            rel.bytes[4 + update + 0x17] = 21;
            assert!(
                read_volley(&rel, native)
                    .and_then(|p| p.bind(&source))
                    .map(Some)
                    .is_err()
            );
        }
    }
    #[test]
    #[ignore = "requires the privately extracted US disc; parses records without cooking textures"]
    fn original_hammer_family_keeps_phase_tracks_rules_and_resource_closure() {
        let extracted = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted/disc1");
        let rel = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        let usual = fs::read(extracted.join("files/BTL/BTLusual.dat")).unwrap();
        let sword_rain = technique_actions(&extracted, &rel, &usual, &[14]).unwrap();
        let TechniqueProgram::Martial { variants } = &sword_rain[0].program else {
            panic!()
        };
        assert!(variants.iter().all(|phase| matches!(
            phase.callback,
            Some(MartialCallback::ComboDelay {
                until_uses: 200,
                ticks: 20
            })
        )));
        // The original threshold is read after the shared successful-use increment.
        let mut changed = Rel::read(&extracted.join("files/US_r_Top2Btl.rel")).unwrap();
        let threshold = changed.sections[1].0 + 0x60b17;
        changed.bytes[threshold] = 201;
        assert!(read_combo_delay(&changed).is_err());
        let techniques = technique_actions(&extracted, &rel, &usual, &[40, 41, 42]).unwrap();
        for (technique, durations, powers, births) in [
            (
                &techniques[0],
                vec![45, 45, 45],
                [160, 200, 200],
                [15, 16, 17],
            ),
            (&techniques[1], vec![45], [60, 70, 70], [15, 16, 17]),
            (&techniques[2], vec![65], [100, 125, 125], [64, 65, 66]),
        ] {
            let TechniqueProgram::Martial { variants } = &technique.program else {
                panic!()
            };
            assert_eq!(
                variants
                    .iter()
                    .map(|p| p.action.duration)
                    .collect::<Vec<_>>(),
                durations
            );
            let Some(MartialCallback::HammerVolley {
                rules,
                birth_effects,
                ..
            }) = variants[0].callback
            else {
                panic!()
            };
            assert_eq!(rules.map(|r| r.power), powers);
            assert_eq!(birth_effects, births);
        }
        let TechniqueProgram::Martial { variants } = &techniques[0].program else {
            panic!()
        };
        assert_eq!(
            variants
                .iter()
                .map(|p| p.caption.as_deref())
                .collect::<Vec<_>>(),
            [None, Some("Ice Hammer"), Some("Toss Hammer")]
        );
        assert_eq!(
            serde_json::to_value(&variants[1].action).unwrap(),
            serde_json::to_value(&variants[2].action).unwrap()
        );
        let TechniqueProgram::Martial { variants } = &techniques[2].program else {
            panic!()
        };
        let Some(MartialCallback::HammerVolley {
            first_tick,
            pattern: HammerPattern::Rain { opening, ring },
            ..
        }) = variants[0].callback
        else {
            panic!()
        };
        assert_eq!((opening.tick, first_tick, ring.count), (22, 60, 10));
        assert_eq!((opening.projectile.id, opening.rule.power), (13, 50));
        assert_eq!(ring.radians_per_degree.to_bits(), 0x3c8efa33);
        assert_eq!(ring.spawn_offset, [0., 380., 230.]);
        let actions = BattleActions {
            chains: None,
            party: vec![],
            enemies: vec![],
            projectiles: vec![],
            techniques,
        };
        actions.validate().unwrap();
        let dependencies = super::super::super::selection::Dependencies::actions(&actions).unwrap();
        assert_eq!(
            dependencies.projectiles,
            [7, 13]
                .map(|id| EffectId {
                    bank: EffectBank::Techniques,
                    id
                })
                .into()
        );
        for id in [15, 16, 17, 19, 64, 65, 66] {
            assert!(dependencies.programs.contains(&EffectId {
                bank: EffectBank::Techniques,
                id
            }));
        }
    }
}
