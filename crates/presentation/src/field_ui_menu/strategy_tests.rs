use super::super::test_support::Fixture;
use super::*;

#[test]
fn strategy_cursor_anchors_follow_page_slides_and_nested_focus() -> Result<()> {
    let mut state = Strategy::opening();
    let mut previous_x = main_anchor(&state)?[0];
    assert!(previous_x < 0.);
    assert_eq!(main_opacity(&state), 0);
    // Opening starts hidden and must reach its resting position within a second.
    for _ in 0..60 {
        state.transition.advance();
        let [x, y] = main_anchor(&state)?;
        assert!(x >= previous_x && x <= 32.);
        assert_eq!(y, 100.);
        previous_x = x;
        if !state.transition.animating() {
            break;
        }
    }
    assert_eq!(main_anchor(&state)?, [32., 100.]);
    assert_eq!(main_opacity(&state), 255);
    state.character = 5;
    state.first = 2;
    assert_eq!(main_anchor(&state)?, [32., 307.]);
    state.focus = Focus::Setting;
    state.group = 2;
    assert_eq!(main_anchor(&state)?, [240., 308.]);
    state.focus = Focus::PresetSetting;
    assert_eq!(main_anchor(&state)?, [264., 308.]);
    state.focus = Focus::Options;
    state.option = 4;
    assert_eq!(main_anchor(&state)?, [452., 184.]);
    state.focus = Focus::Presets;
    state.preset = 2;
    state.preset_opacity = 0;
    assert_eq!(main_anchor(&state)?, [396., -27.]);
    assert_eq!((main_plane(&state), main_alpha(&state)), (3, 255));
    state.focus = Focus::Rename;
    state.preset_opacity = 255;
    assert_eq!(main_anchor(&state)?, [396., 28.]);
    assert_eq!((main_plane(&state), main_alpha(&state)), (3, 127));
    assert_eq!(key_position(5, 4), [250., 240.]);
    assert_eq!(key_position(10, 8), [414., 352.]);
    state.focus = Focus::Presets;
    state.rename_opacity = 1;
    assert!(rename_visible(&state));
    assert_eq!(main_plane(&state), 3);
    state.rename_opacity = 0;
    assert!(!rename_visible(&state));
    assert_eq!(main_plane(&state), 3);
    Ok(())
}
#[test]
#[ignore = "requires locally cooked menus; CPU drawing only"]
fn preset_backing_covers_its_heading_and_fades_in() -> Result<()> {
    let fixture = Fixture::load()?;
    let mut previous_alpha = 0.;
    for opacity in [0, 127, 255] {
        let state = Strategy {
            focus: Focus::PresetCharacter,
            preset_opacity: opacity,
            ..Default::default()
        };
        let mut drawing = fixture.drawing(0);
        drawing.strategy(state.page(&fixture.party, &fixture.data))?;
        let backing = &drawing.batches[&(2, DrawRole::Background, FONT)];
        let start = backing.positions.len() - 4;
        let alpha = backing.colors[start..]
            .iter()
            .map(|color| color[3])
            .fold(0., f32::max);
        assert!((previous_alpha..=1.).contains(&alpha));
        if opacity == 0 {
            assert_eq!(alpha, 0.);
        } else {
            assert!(alpha > previous_alpha);
        }
        previous_alpha = alpha;
        if opacity == 255 {
            let [left, top, _] = backing.positions[start];
            let [right, bottom, _] = backing.positions[start + 2];
            assert!(left <= 100. - 320. && right >= 540. - 320.);
            let screen_y = |y: f32| 240. - y * HEIGHT as f32 / SCENE_HEIGHT as f32;
            assert!(bottom <= screen_y(48.) + 0.01 && top >= screen_y(16.) - 0.01);
        }
    }
    let mut personal = fixture.drawing(0);
    personal.strategy(Strategy::default().page(&fixture.party, &fixture.data))?;
    assert!(
        personal
            .batches
            .range((2, DrawRole::Background, MaterialKey::Texture(0))..)
            .all(|(_, batch)| batch.indices.is_empty())
    );
    Ok(())
}
