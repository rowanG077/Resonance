use super::*;

#[test]
#[ignore = "requires both original extracted discs; private texture output"]
fn original_hud_pixels_and_tables_match_both_discs() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/extracted");
    let mut previous = None;
    for disc in [1, 2] {
        let extracted = local.join(format!("disc{disc}"));
        let output = tempfile::tempdir()?;
        let paths = publish(&extracted, output.path())?;
        let sources = crate::source_assets::Sources::read(&extracted)?;
        let usual = fs::read(extracted.join("files").join(sources.usual))?;
        let module = Rel::read(&extracted.join("files").join(sources.module))?;
        let art: Art = serde_json::from_slice(&fs::read(
            output.path().join(resonance_content::battle_ui::PATH),
        )?)?;
        art.validate()?;
        assert_eq!(
            art.sine
                .iter()
                .flat_map(|v| v.to_be_bytes())
                .collect::<Vec<_>>(),
            section(&usual, 5)?[..1800]
        );
        // Native cosine addresses +90 directly; the extension is not a copy
        // of the first quarter-turn and must not be synthesized by wrapping.
        assert_eq!(art.sine[360].to_bits(), 0xb5a8_885a);
        assert_eq!(art.sine[449].to_bits(), 0x3f7f_f604);
        assert_ne!(art.sine[360].to_bits(), art.sine[0].to_bits());
        assert_ne!(art.sine[449].to_bits(), art.sine[89].to_bits());
        for (actual, offset) in [
            (art.intro.panel_colors.concat(), 0x4224),
            (art.orders.panel_colors.concat(), 0x4214),
            (art.radar.panel_colors.concat(), 0x4240),
            (art.combo.colors.concat(), 0x4308),
            (art.combo.panel_colors.concat(), 0x4328),
            (art.commands.player_colors.concat(), 0x35c),
        ] {
            assert_eq!(actual, module.at((4, offset))?[..actual.len()]);
        }
        for (actual, offset) in [
            (art.intro.text_color, 0x4234),
            (art.orders.shadow, 0x4220),
            (art.radar.number_color, 0x4238),
            (art.radar.initial_ordinals, 0x423c),
            (art.radar.shadow, 0x4250),
            (art.commands.shadow_color, 0x384),
            (art.commands.text_color, 0x2758),
        ] {
            assert_eq!(actual, module.at((4, offset))?[..4]);
        }
        for (actual, offset) in [
            (art.radar.pulse_amplitude, 0x44ec),
            (art.radar.shade_center, 0x455c),
            (art.radar.target_size_amplitude, 0x4554),
            (art.radar.effect_size_amplitude, 0x4550),
            (art.radar.effect_size_center, 0x4568),
            (art.radar.selection_color_amplitude, 0x4530),
            (art.radar.selection_blue_center, 0x4578),
            (art.radar.radians_per_degree, 0x261c),
            (art.radar.depth, 0x2620),
            (art.commands.motion.selected_shade_center, 0x38c),
            (art.commands.motion.selected_shade_amplitude, 0x390),
            (art.commands.motion.bob_amplitude, 0x394),
            (art.commands.motion.y_rotation, 0x398),
            (art.commands.motion.small_bob_amplitude, 0x39c),
            (art.commands.motion.strategy_rotation_amplitude, 0x3a0),
            (art.commands.motion.lift_amplitude, 0x3a4),
            (art.commands.motion.label_bob_amplitude, 0x3a8),
            (art.commands.motion.item_rotation_amplitude, 0x3a8),
            (art.commands.motion.item_sway_amplitude, 0x3ac),
            (art.commands.motion.escape_rotation_amplitude, 0x3b0),
            (art.commands.cursor_amplitude, 0x4554),
        ] {
            assert_eq!(actual.to_be_bytes(), module.at((4, offset))?[..4]);
        }
        assert_eq!(
            art.radar.target_size_center.to_be_bytes(),
            module.at((4, 0x4570))?[..8]
        );
        assert_eq!(
            art.combo.anchors,
            <[i16; 2]>::read(module.at((4, 0x4348))?, 0)?
        );
        assert_eq!(
            art.combo.destinations,
            <[i16; 2]>::read(module.at((4, 0x4304))?, 0)?
        );
        for (actual, offset) in [
            (&art.intro.hidden_name_symbol, 0x2400),
            (&art.intro.group_count_format, 0x457c),
            (&art.orders.cancel_text, 0x4584),
            (&art.orders.name_format, 0x44bc),
            (&art.combo.hits, 0x43b4),
            (&art.combo.count_format, 0x44e8),
            (&art.combo.damage_format, 0x44f4),
            (&art.combo.damage_suffix, 0x4500),
            (&art.commands.player_format, 0x3d8),
        ] {
            assert_eq!(actual, &module.text((4, offset))?);
        }
        for (lines, index) in art.overlays.actor_lines.iter().zip([6, 12, 0]) {
            for (line, offset) in lines.iter().zip([0, 4]) {
                assert_eq!(
                    line,
                    &module.text(module.pointer(4, 0x14f4 + index * 8 + offset)?)?
                );
            }
        }
        assert_eq!(art.overlays.actor_lines[0], ["LEVEL", "UP"]);
        assert_eq!(art.overlays.actor_lines[1], ["NEW", "EX SKILL"]);
        assert_eq!(art.overlays.actor_lines[2], ["CRITICAL", "DAMAGE"]);
        assert_eq!(art.version, 5);
        assert_eq!(art.intro.hidden_name_symbol, "？");
        assert_eq!(art.intro.group_count_format, "    %d");
        assert_eq!(art.orders.cancel_text, "Cancel orders");
        assert_eq!(art.combo.anchors, [80, 552]);
        for (index, name) in art.commands.names.iter().enumerate() {
            assert_eq!(name, &module.text(module.pointer(5, 0x90 + index * 4)?)?);
        }
        assert_eq!(
            art.combat_number_colors.concat().concat(),
            module.at((4, 0x4350))?[..24]
        );
        assert_eq!(
            art.combat_number_sizes,
            <[[i16; 2]; 3]>::read(module.at((4, 0x4368))?, 0)?
        );
        assert_eq!(
            art.recovery.colors.concat().concat(),
            module.at((4, 0x4290))?[..16]
        );
        assert_eq!(
            art.recovery.rect,
            <[u16; 4]>::read(module.at((5, 0x3d18))?, 0)?
        );
        assert_eq!(art.recovery.rect, [0, 0, 16, 24]);
        for (vertex, samples) in art.markers.shadow_circle.iter().enumerate() {
            let source = section(&usual, 5)?;
            for (sample, index) in samples.iter().zip([90 + vertex * 24, vertex * 24]) {
                assert_eq!(sample.to_be_bytes(), source[index * 4..index * 4 + 4]);
            }
        }
        assert!(paths.iter().all(|path| output.path().join(path).is_file()));
        let portraits = section(section(&usual, 4)?, 2)?;
        let decoded = crate::tpl::decode(portraits)?;
        for (image, (width, height, pixels)) in art.portraits.iter().zip(decoded) {
            assert_eq!([image.width, image.height], [width, height]);
            assert_eq!(
                *crate::texture::pixels(&output.path().join(&image.path))?.as_raw(),
                pixels
            );
        }
        let atlas = section(section(&usual, 4)?, 3)?;
        let texture = &crate::tpl::parse_tpl(atlas)?[0];
        for (palette, path) in [
            (0, art.font.texture.as_str()),
            (24, &art.results.cook_button[0].texture.path),
            (25, &art.results.next_button[0].texture.path),
            (80, &art.overlays.notice_background.path),
            (81, &art.overlays.notice_shadow.path),
            (82, &art.overlays.notice_symbols[0].texture.path),
            (83, &art.overlays.notice_symbols[1].texture.path),
            (84, &art.overlays.notice_symbols[2].texture.path),
            (85, &art.overlays.notice_symbols[3].texture.path),
            (22, &art.radar.effect.texture.path),
            (16, &art.markers.target_background.texture.path),
            (17, &art.markers.target_foregrounds[0].path),
            (18, &art.markers.target_foregrounds[1].path),
            (19, &art.markers.target_foregrounds[2].path),
            (20, &art.markers.target_foregrounds[3].path),
            (62, &art.markers.stun.path),
            (2, &art.commands.background.path),
            (47, &art.commands.disabled.path),
            (58, &art.commands.plate.path),
            (86, &art.commands.shadow.path),
            (21, &art.commands.cursor.path),
            (0, &art.commands.cursor_shadow.path),
        ] {
            let mut texture = texture.clone();
            texture.palette_offset =
                Some(texture.palette_offset.context("missing atlas palette")? + palette * 32);
            texture.palette_entries = 16;
            assert_eq!(
                *crate::texture::pixels(&output.path().join(path))?.as_raw(),
                crate::tpl::decode_texture(atlas, &texture)?
            );
        }
        // 63F8's original layer branches select these palettes independently
        // of their UV coordinates; verify the published RGBA pixels, not names.
        for (images, palettes) in art.commands.icons.iter().zip([
            &[3, 4, 5][..],
            &[75, 76, 77][..],
            &[6, 7, 8, 9][..],
            &[10, 11][..],
            &[12, 13, 14][..],
            &[15, 15][..],
        ]) {
            for (image, palette) in images.iter().zip(palettes) {
                let mut source = texture.clone();
                source.palette_offset =
                    Some(source.palette_offset.context("missing command palette")? + palette * 32);
                source.palette_entries = 16;
                assert_eq!(
                    *crate::texture::pixels(&output.path().join(&image.path))?.as_raw(),
                    crate::tpl::decode_texture(atlas, &source)?
                );
                assert!(paths.contains(&image.path));
            }
        }
        for (index, symbol) in art.overlays.notice_symbols.iter().enumerate() {
            let [x, y] = <[u16; 2]>::read(module.at((4, 0x4280 + index * 4))?, 0)?.map(u32::from);
            assert_eq!(symbol.rect, [x, y, 24, 24]);
        }
        assert_eq!(art.radar.effect.rect, [128, 72, 32, 32]);
        for (character, sprite) in art.results.character_icons.iter().enumerate() {
            let mut source = texture.clone();
            source.palette_offset =
                Some(source.palette_offset.context("missing palette")? + (character + 32) * 32);
            source.palette_entries = 16;
            assert_eq!(
                *crate::texture::pixels(&output.path().join(&sprite.texture.path))?.as_raw(),
                crate::tpl::decode_texture(atlas, &source)?
            );
            assert_eq!(
                sprite.rect,
                <[u16; 4]>::read(module.at((4, 0x4450 + character * 8))?, 0)?.map(u32::from)
            );
        }
        assert_eq!(
            art.results.strips,
            <[[i16; 4]; 6]>::read(module.at((4, 0x2d64))?, 0)?
        );
        assert_eq!(art.font.glyphs[&'0'].rect, [0, 0, 16, 23]);
        assert_eq!(art.font.glyphs[&'Z'].rect, [48, 48, 16, 23]);
        assert_eq!(art.font.glyphs[&'('].rect, [0, 0, 16, 23]);
        assert_eq!(
            art.overlay_punctuation[usize::from(b'(' - b'!')],
            [320, 136]
        );
        assert_eq!(art.party.hp.colors.concat(), module.at((4, 0x4190))?[..16]);
        assert_eq!(art.party.tp.colors.concat(), module.at((4, 0x41a0))?[..16]);
        assert_eq!(
            art.party.hp.number_colors.concat(),
            module.at((4, 0x4180))?[..8]
        );
        assert_eq!(
            art.party.tp.number_colors.concat(),
            module.at((4, 0x4188))?[..8]
        );
        assert_eq!(
            art.results
                .positions
                .map(|p| p.map(i16::to_be_bytes))
                .concat()
                .concat(),
            module.at((4, 0x2d94))?[..24]
        );
        assert_eq!(art.results.formats[0], "EXP   %7d");
        assert_eq!(art.results.notice_formats[1], "Earned the title, \"%s\"");
        assert_eq!(art.results.headings[0], "ITEM(S) FOUND");
        // The decompiled consumers are partial. Pin their original immediate
        // arguments as well as the data tables, independently of that C source.
        for (offset, instruction) in [
            (0x6f858, 0x3880000c), // party x = 12
            (0x6f85c, 0x38a00184), // party y = 388
            (0x6d344, 0x38c00040), // portrait width = 64
            (0x6d348, 0x38e00040), // portrait height = 64
            (0x6d354, 0x3940003e), // source inset leaves 62 pixels
            (0x6d384, 0x3a730070), // portrait spacing = 112
            (0x6d3c8, 0x3aba0022), // HP bar y = origin + 34
            (0x6d3d4, 0x3afa003e), // TP bar y = origin + 62
            (0x6d470, 0x1c630038), // width = integer percentage * 56 / 100
            (0x6da90, 0x38100032), // number x = origin + 50
            (0x6da8c, 0x3860000a), // number slant = 10
            (0x6da98, 0x3860000d), // number advance = 13
            (0x4c910, 0x38600017), // glyph source height = 23
            (0x4c950, 0x39400010), // glyph source width = 16
            (0x55a58, 0x3860000e), // result number slant = 14
            (0x55a5c, 0x38c00012), // result number advance = 18
            (0x55a7c, 0x38e00014), // result glyph width = 20
            (0x55a80, 0x3900001c), // result glyph height = 28
            (0x688ec, 0x1ca3006e), // recovery party spacing = 110
            (0x688fc, 0x38a50068), // recovery right edge = 104
            (0x686a8, 0x3b40018c), // recovery first row y = 396
            (0x68710, 0x3b5a0018), // recovery row spacing = 24
            (0x686e4, 0x3940000c), // recovery glyph skew = 12
            (0x71230, 0x57bd06f7), // recovery hold retains only menu bit0x10
            (0x44ea8, 0xa01e0006), // hidden name uses enemy statistics header
            (0x44ec4, 0x38790004), // original name starts at header +4
            (0x44ed0, 0x5478f87e), // hidden units = original byte strlen >>1
            (0x6b23c, 0x39400008), // intro first row y = 8
            (0x6b248, 0x39000140), // intro text starts x = 320
            (0x6b394, 0x3880015c), // orders panel y = 348
            (0x6b3d8, 0x38a0014e), // orders text y = 334
            (0x704bc, 0x388001d0), // radar x = 464
            (0x704c0, 0x38a00184), // radar y = 388
            (0x6a950, 0x38c00016), // casting halo palette = 22
            (0x6a9ac, 0x38c00020), // casting halo texture height = 32
            (0x6a9d8, 0x90c10008), // texture height is first stacked operand
            (0x6a9e0, 0x39000080), // casting halo texture u = 128
            (0x6a9f8, 0x39200048), // casting halo texture v = 72
            (0x6aa00, 0x39400020), // casting halo texture width = 32
        ] {
            assert_eq!(u32::read(module.at((1, offset))?, 0)?, instruction);
        }
        let value = serde_json::to_value(&art)?;
        if let Some(previous) = &previous {
            assert_eq!(previous, &value);
        }
        previous = Some(value);
    }
    Ok(())
}
