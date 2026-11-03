use super::*;
use crate::effect_atlas::{Remap, Table};
use resonance_asset_writer::texture::Pixels;
use resonance_content::battle_effect::{ScheduledEvent, declaration::Declaration};

fn bank(rect: Rect) -> Result<Definition> {
    let actor: Declaration = serde_json::from_value(serde_json::json!({
        "kind":"particle", "template":{"lifetime":400,"state":{
            "geometry":{"Size":{"value":[1.,1.,1.],"velocity":[0.,0.,0.],"acceleration":[0.,0.,0.]}},
            "colors":[[128,128,128,128],[128,128,128,128]], "uv":rect
        }},
        "visual":{"geometry":{"kind":"quad"},"orientation":"world",
            "texture":{"kind":"atlas","slot":"scene","dual":true},"blend":"alpha"}
    }))?;
    Ok(Definition {
        templates: vec![actor],
        template_names: vec!["sprite".into()],
        programs: vec![],
        artwork: None,
    })
}

#[test]
fn packed_cells_at_another_resolution_restore_gc_texel_centers() -> Result<()> {
    let bank = bank([1, 1, 2, 2])?;
    let plan = Plan::read(&bank, 0)?;
    let source = Pixels {
        width: 6,
        height: 6,
        rgba: (0..6u8)
            .flat_map(|y| (0..6u8).flat_map(move |x| [x * 40, y * 40, 80, x * 20 + y * 5]))
            .collect(),
    };
    let table = Table {
        rows: vec![Remap {
            palette: 7,
            source: [1., 1., 3., 3.],
            packed: [0., 0., 4., 4.],
        }],
    };
    let image = plan.image(&source, Some(&table), 7)?;
    let target = plan.rectangle([1, 1, 2, 2])?;
    let pixel = |x, y| &image.rgba[((y as usize * image.width as usize + x as usize) * 4)..][..4];
    assert_eq!(pixel(target[0], target[1]), [20, 20, 80, 13]);
    assert_eq!(pixel(target[0] + 1, target[1] + 1), [100, 100, 80, 63]);
    assert_eq!(&target[2..], &[2, 2]);
    Ok(())
}

#[test]
fn inherited_uv_sums_keep_each_draw_and_sample_the_repacked_frames() -> Result<()> {
    use resonance_content::battle_effect::BirthSum;
    let mut bank = bank([1, 1, 6, 6])?;
    let terms = vec![
        ValueRange {
            min: 1.,
            max: 97.,
            step: 32.,
            draw: Some(-1),
            reverse: true,
        },
        ValueRange {
            min: 0.,
            max: 96.,
            step: 32.,
            draw: Some(7),
            reverse: false,
        },
    ];
    let mut birth = ParticleBirth {
        random_draws: 8,
        sums: vec![BirthSum {
            field: BirthField::Uv(0),
            terms: terms.clone(),
        }],
        ..Default::default()
    };
    bank.programs.push(vec![ScheduledEvent {
        at: 0,
        operation: EffectOperation::Spawn {
            particle: 0,
            blend: None,
            palette: None,
            birth: Box::new(birth.clone()),
        },
    }]);
    let plan = Plan::read(&bank, 0)?;
    plan.rewrite_birth(&mut birth)?;
    assert_eq!(birth.random_draws, 8);
    let result = &birth.sums[0].terms;
    for (a, b) in terms.iter().zip(result) {
        assert_eq!((a.draw, a.reverse), (b.draw, b.reverse));
        assert_eq!(a.choice_count(), b.choice_count());
    }
    for a in [0., 0.3, 0.6, 0.9] {
        for b in [0., 0.3, 0.6, 0.9] {
            let source = terms[0].sample(a) + terms[1].sample(b);
            let target = result[0].sample(a) + result[1].sample(b);
            assert_eq!(
                target.round() as i16,
                plan.rectangle([source.round() as i16, 1, 6, 6])?[0]
            );
        }
    }
    Ok(())
}

#[test]
fn atlas_remaps_chain_in_source_order_and_select_the_native_palette() {
    let table = Table {
        rows: vec![
            Remap {
                palette: 2,
                source: [1., 1., 3., 3.],
                packed: [5., 1., 7., 3.],
            },
            Remap {
                palette: 2,
                source: [5., 1., 7., 3.],
                packed: [9., 1., 11., 3.],
            },
        ],
    };
    let quad = [[1., 1.], [3., 1.], [1., 3.], [3., 3.]];
    let mut output = [quad];
    table.remap(1, &mut output);
    assert_eq!(output[0], quad);
    table.remap(2, &mut output);
    assert_eq!(output[0], [[9., 1.], [11., 1.], [9., 3.], [11., 3.]]);
}

#[test]
fn random_uv_choices_keep_shared_draw_identity_and_direction() -> Result<()> {
    let mut bank = bank([257, 129, 30, 30])?;
    let mut birth = ParticleBirth::default();
    birth.random_draws = 1;
    birth.uv[0] = Some(ValueRange {
        min: 257.,
        max: 264.,
        step: 1.,
        draw: Some(17),
        reverse: true,
    });
    bank.programs.push(vec![ScheduledEvent {
        at: 0,
        operation: EffectOperation::Spawn {
            particle: 0,
            blend: None,
            palette: None,
            birth: Box::new(birth.clone()),
        },
    }]);
    let plan = Plan::read(&bank, 0)?;
    let source = birth.uv[0].unwrap();
    plan.rewrite_birth(&mut birth)?;
    let result = birth.uv[0].unwrap();
    assert_eq!(
        (birth.random_draws, result.draw, result.reverse),
        (1, Some(17), true)
    );
    for i in 0..4096 {
        let unit = i as f32 / 4096.;
        assert_eq!(
            result.sample(unit) as i16,
            plan.rectangle([source.sample(unit) as i16, 129, 30, 30])?[0]
        );
    }
    Ok(())
}

#[test]
fn overlapping_random_origins_fit_without_changing_sampling() -> Result<()> {
    let mut bank = bank([257, 145, 190, 46])?;
    let mut birth = ParticleBirth::default();
    birth.random_draws = 1;
    birth.uv[0] = Some(ValueRange {
        min: 257.,
        max: 320.,
        step: 1.,
        draw: Some(0),
        reverse: true,
    });
    bank.programs.push(vec![ScheduledEvent {
        at: 0,
        operation: EffectOperation::Spawn {
            particle: 0,
            blend: None,
            palette: None,
            birth: Box::new(birth.clone()),
        },
    }]);
    let plan = Plan::read(&bank, 0)?;
    assert_eq!(plan.size, [255, 48]);
    let source = Pixels {
        width: 512,
        height: 256,
        rgba: (0..512 * 256)
            .flat_map(|i| [(i % 251) as u8, (i / 512) as u8, 9, 255])
            .collect(),
    };
    let image = plan.image(&source, None, 19)?;
    let original = birth.uv[0].unwrap();
    plan.rewrite_birth(&mut birth)?;
    assert_eq!(
        (
            birth.random_draws,
            birth.uv[0].unwrap().draw,
            birth.uv[0].unwrap().reverse
        ),
        (1, Some(0), true)
    );
    for x in 257..=320 {
        let target = plan.rectangle([x, 145, 190, 46])?;
        for row in 0..46usize {
            let a = ((target[1] as usize + row) * image.width as usize + target[0] as usize) * 4;
            let b = ((145 + row) * source.width as usize + x as usize) * 4;
            assert_eq!(&image.rgba[a..a + 190 * 4], &source.rgba[b..b + 190 * 4]);
        }
    }
    for i in 0..4096 {
        let unit = i as f32 / 4096.;
        assert_eq!(
            birth.uv[0].unwrap().sample(unit) as i16,
            plan.rectangle([original.sample(unit) as i16, 145, 190, 46])?[0]
        );
    }
    let incompatible = Table {
        rows: vec![Remap {
            palette: 19,
            source: [257., 145., 447., 191.],
            packed: [258., 145., 448., 191.],
        }],
    };
    assert!(plan.image(&source, Some(&incompatible), 19).is_err());
    Ok(())
}

#[test]
fn birth_uv_extents_keep_random_choices_and_all_source_texels() -> Result<()> {
    for shape in [ParticleShape::Quad, ParticleShape::Disc { segments: 16 }] {
        let mut bank = bank([1, 1, 2, 2])?;
        if let Declaration::Particle { visual, .. } = &mut bank.templates[0] {
            visual.geometry = shape;
        }
        let mut birth = ParticleBirth::default();
        birth.random_draws = 1;
        birth.uv[0] = Some(ValueRange::fixed(8.));
        birth.uv[1] = Some(ValueRange::fixed(1.));
        birth.uv[2] = Some(ValueRange {
            min: 2.,
            max: 4.,
            step: 2.,
            draw: Some(7),
            reverse: true,
        });
        birth.uv[3] = Some(ValueRange::fixed(2.));
        bank.programs.push(vec![ScheduledEvent {
            at: 0,
            operation: EffectOperation::Spawn {
                particle: 0,
                blend: None,
                palette: None,
                birth: Box::new(birth.clone()),
            },
        }]);
        let plan = Plan::read(&bank, 0)?;
        let source = Pixels {
            width: 24,
            height: 4,
            rgba: (0..96).flat_map(|i| [i as u8, 7, 9, 255]).collect(),
        };
        let table = Table {
            rows: vec![
                Remap {
                    palette: 2,
                    source: [8., 1., 10., 3.],
                    packed: [16., 1., 18., 3.],
                },
                Remap {
                    palette: 2,
                    source: [8., 1., 12., 3.],
                    packed: [16., 1., 20., 3.],
                },
            ],
        };
        let image = plan.image(&source, Some(&table), 2)?;
        // A different remap for overlapping sizes cannot share one image.
        let incompatible = Table {
            rows: vec![table.rows[1].clone()],
        };
        assert!(plan.image(&source, Some(&incompatible), 2).is_err());
        let original = birth.clone();
        plan.rewrite_birth(&mut birth)?;
        assert_eq!(birth.random_draws, original.random_draws);
        assert_eq!(
            serde_json::to_value(&birth.uv[2..])?,
            serde_json::to_value(&original.uv[2..])?
        );
        for width in [2, 4] {
            let target = plan.rectangle([8, 1, width, 2])?;
            assert_eq!(&target[2..], &[width, 2]);
            for y in 0..2 {
                for x in 0..width as usize {
                    let at =
                        ((target[1] as usize + y) * image.width as usize + target[0] as usize + x)
                            * 4;
                    let original = ((1 + y) * source.width as usize + 16 + x) * 4;
                    assert_eq!(
                        &image.rgba[at..at + 4],
                        &source.rgba[original..original + 4]
                    );
                }
            }
        }
    }
    Ok(())
}

#[test]
fn overlapping_source_frames_get_distinct_pixels_and_filter_borders() -> Result<()> {
    let mut bank = bank([1, 1, 2, 2])?;
    bank.templates[0].template_mut()?.uv_animation = Some(UvAnimation::Frames {
        frames: vec![
            UvFrame {
                duration: 1,
                change: UvChange::Rectangle { rect: [1, 1, 2, 2] },
            },
            UvFrame {
                duration: 1,
                change: UvChange::Rectangle { rect: [2, 1, 2, 2] },
            },
        ],
        loop_start: Some(0),
    });
    let plan = Plan::read(&bank, 0)?;
    let source = Pixels {
        width: 16,
        height: 4,
        rgba: (0..64).flat_map(|i| [i as u8, 7, 9, 255]).collect(),
    };
    let table = Table {
        rows: vec![
            Remap {
                palette: 2,
                source: [1., 1., 3., 3.],
                packed: [5., 1., 7., 3.],
            },
            Remap {
                palette: 2,
                source: [2., 1., 4., 3.],
                packed: [9., 1., 11., 3.],
            },
        ],
    };
    let image = plan.image(&source, Some(&table), 2)?;
    for (rect, shift) in [([1, 1, 2, 2], 4), ([2, 1, 2, 2], 7)] {
        let target = plan.rectangle(rect)?;
        for y in 0..4 {
            for x in 0..4 {
                let at = ((target[1] as usize - 1 + y) * image.width as usize + target[0] as usize
                    - 1
                    + x)
                    * 4;
                let src = (y * 16 + rect[0] as usize - 1 + x + shift) * 4;
                assert_eq!(&image.rgba[at..at + 4], &source.rgba[src..src + 4]);
            }
        }
    }
    Ok(())
}

#[test]
fn adjacent_remapped_panels_keep_their_own_texels_at_shared_seams() -> Result<()> {
    let mut bank = bank([1, 1, 4, 2])?;
    if let Declaration::Particle { visual, .. } = &mut bank.templates[0] {
        visual.geometry = ParticleShape::Ring {
            segments: 2,
            flared: false,
            hidden: false,
            uv_layout: UvLayout::HalfWidthCycle,
        };
    }
    let plan = Plan::read(&bank, 0)?;
    let source = Pixels {
        width: 16,
        height: 4,
        rgba: (0..64).flat_map(|i| [i as u8, 0, 0, 255]).collect(),
    };
    let table = Table {
        rows: vec![
            Remap {
                palette: 2,
                source: [1., 1., 3., 3.],
                packed: [5., 1., 7., 3.],
            },
            Remap {
                palette: 2,
                source: [3., 1., 5., 3.],
                packed: [9., 1., 11., 3.],
            },
        ],
    };
    let image = plan.image(&source, Some(&table), 2)?;
    let target = plan.rectangle([1, 1, 4, 2])?;
    for (x, source_x) in [(0, 5), (1, 6), (2, 9), (3, 10)] {
        for y in 0..2 {
            let at = ((target[1] as usize + y) * image.width as usize + target[0] as usize + x) * 4;
            let original = ((y + 1) * source.width as usize + source_x) * 4;
            assert_eq!(
                &image.rgba[at..at + 4],
                &source.rgba[original..original + 4]
            );
        }
    }
    plan.image(&source, Some(&table), 0)?;
    Ok(())
}

#[test]
fn full_width_ring_frames_fit_without_optional_filter_padding() -> Result<()> {
    let mut bank = bank([192, 448, 64, 64])?;
    let template = bank.templates[0].template_mut()?;
    template.state.geometry_count = 255;
    template.uv_animation = Some(UvAnimation::Frames {
        frames: [192, 208, 224, 240]
            .map(|x| UvFrame {
                duration: 1,
                change: UvChange::Rectangle {
                    rect: [x, 448, 32, 64],
                },
            })
            .into(),
        loop_start: Some(0),
    });
    if let Declaration::Particle { visual, .. } = &mut bank.templates[0] {
        visual.geometry = ParticleShape::Ring {
            segments: 16,
            flared: false,
            hidden: false,
            uv_layout: UvLayout::Cycle,
        };
    }
    let plan = Plan::read(&bank, 0)?;
    assert_eq!(plan.size[0], 4096);
    let source = Pixels {
        width: 1216,
        height: 512,
        rgba: vec![255; 1216 * 512 * 4],
    };
    plan.image(&source, None, 0)?;
    Ok(())
}
