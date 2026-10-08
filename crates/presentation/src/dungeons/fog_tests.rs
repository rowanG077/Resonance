//! Fog updates the camera without invalidating scene materials.
use super::super::destinations::*;
use super::{configured, enter, mission, skip_battle};
use crate::{field_view, materials::TitleSurface};
use anyhow::Result;
use bevy::{asset::AssetPlugin, prelude::*};
use resonance_events::input::{Button, Buttons};
use resonance_game::field::{FieldInput, FieldSession};

#[test]
#[ignore = "requires locally cooked fields; no devices"]
fn radar_updates_camera_fog_and_restores_the_room_without_material_changes() -> Result<()> {
    let mut field = enter(PALMACOSTA_RANCH, 201, None)?;
    field.advance_until(FieldSession::player_has_control)?;
    field.party_mut().travel.sorcerers_ring = resonance_events::ring::SorcerersRing::Radar;
    let original = field.events.world.fog().cloned();
    let mut app = App::new();
    app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default()))
        .init_asset::<TitleSurface>()
        .insert_resource(field_view::Session(field.field))
        .add_systems(Update, field_view::fog);
    let camera = app.world_mut().spawn(crate::FieldCamera).id();
    let _material = app
        .world_mut()
        .resource_mut::<Assets<TitleSurface>>()
        .add(TitleSurface {
            field_fog: true,
            ..default()
        });
    let mut saw_radar = false;
    for tick in 0..1800 {
        let fog = {
            let mut session = app.world_mut().resource_mut::<field_view::Session>();
            skip_battle(&mut session.0)?;
            session.0.step(FieldInput {
                pressed_buttons: Buttons::default().with(Button::Ring, tick == 0),
                ..Default::default()
            })?;
            session.0.events.world.fog().cloned()
        };
        saw_radar |= fog.as_ref().is_some_and(|f| f.color == [10, 255, 10]);
        app.update();
        let view = app.world().get::<DistanceFog>(camera).unwrap();
        let (color, start, end) = fog.as_ref().map_or(([0.; 4], 0., 0.), |f| {
            (
                [
                    f32::from(f.color[0]) / 255.,
                    f32::from(f.color[1]) / 255.,
                    f32::from(f.color[2]) / 255.,
                    1.,
                ],
                f.start,
                f.end,
            )
        });
        assert_eq!(view.color.to_linear().to_f32_array(), color);
        assert!(
            matches!(view.falloff, FogFalloff::Linear { start: a, end: b } if a == start && b == end)
        );
        assert!(
            !app.world_mut()
                .resource_mut::<Messages<AssetEvent<TitleSurface>>>()
                .drain()
                .any(|e| matches!(e, AssetEvent::Modified { .. }))
        );
        if saw_radar && fog == original {
            return Ok(());
        }
    }
    anyhow::bail!("radar did not restore the room fog")
}

#[test]
#[ignore = "requires locally cooked Mana seal; no devices"]
fn mana_seal_finishes_and_clears_its_fog() -> Result<()> {
    let mut field = configured(MANA_START, 369, |entry| {
        entry
            .persistent
            .memory
            .write(0xcc, symphonia_script::Width::S32, 13_600)?;
        entry.position = [0.; 3];
        entry.heading = 180.;
        Ok(())
    })?;
    let mut saw_fog = false;
    let battles = field.replay(|f| {
        saw_fog |= f.events.world.fog().is_some_and(|fog| fog.end > fog.start);
        Ok(f.player_has_control() && mission(f, 0xcc) == 21_000)
    })?;
    assert!(saw_fog && battles > 0);
    assert!(
        field
            .events
            .world
            .fog()
            .is_none_or(|fog| fog.end == fog.start)
    );
    assert!(field.events.world.event_flags.contains(&203));
    Ok(())
}
