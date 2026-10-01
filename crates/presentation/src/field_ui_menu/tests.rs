use super::test_support::Fixture;
use super::*;

#[test]
#[ignore = "requires locally cooked menus; CPU page admission and drawing only"]
fn optional_battle_menu_text_is_admitted_when_drawn() -> Result<()> {
    use resonance_content::diagnostics::Diagnostics;
    use resonance_game::menu::{strategy::Strategy, unison::Unison};
    let mut fixture = Fixture::load()?;
    let strategy = Strategy::default();
    let unison = Unison::opening(0, &fixture.party)?;
    let selected = unison
        .page(&fixture.party, &fixture.session, &fixture.data)
        .selection()
        .unwrap()
        .technique;
    let valid = fixture.data.clone();
    let data = std::sync::Arc::make_mut(&mut fixture.data);
    // These descriptions are absent from the opening pages. They must not be
    // scanned while admitting the encounter or rendering a different selection.
    let choice = usize::from(fixture.party.members[0].strategy[0]);
    data.presentation.strategy.as_mut().unwrap().groups[0][choice]
        .as_mut()
        .unwrap()
        .details
        .push('🙂');
    for (id, tech) in data
        .presentation
        .techniques
        .as_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        if id != usize::from(selected) {
            tech.as_mut().unwrap().description.push('🙂');
        }
    }
    for key in ["strategy_title", "unison_title"] {
        data.presentation.labels.remove(key);
    }
    data.validate_gameplay()?;
    for malformed in [None, Some("🙂")] {
        for key in ["strategy_title", "unison_title"] {
            if let Some(text) = malformed {
                std::sync::Arc::make_mut(&mut fixture.data)
                    .presentation
                    .labels
                    .insert(key.into(), text.into());
            }
        }
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            for (scope, result) in [
                (
                    "battle Strategy page",
                    fixture
                        .drawing(0)
                        .strategy(strategy.page(&fixture.party, &fixture.data)),
                ),
                (
                    "battle U. Attack page",
                    fixture.drawing(0).unison(
                        unison.page(&fixture.party, &fixture.session, &fixture.data),
                        false,
                    ),
                ),
            ] {
                assert_eq!(diagnostics.attempt(scope, result).is_err(), paranoid);
            }
            assert_eq!(diagnostics.entries().len(), 2);
            assert!(
                diagnostics
                    .entries()
                    .iter()
                    .all(|entry| entry.message.contains(if malformed.is_some() {
                        "uncooked menu glyph"
                    } else {
                        "title was not prepared"
                    }))
            );
        }
    }
    for key in ["strategy_title", "unison_title"] {
        std::sync::Arc::make_mut(&mut fixture.data)
            .presentation
            .labels
            .insert(key.into(), valid.presentation.labels[key].clone());
    }
    fixture
        .drawing(1)
        .strategy(strategy.page(&fixture.party, &fixture.data))?;
    fixture.drawing(1).unison(
        unison.page(&fixture.party, &fixture.session, &fixture.data),
        false,
    )?;
    let setting = Strategy {
        focus: StrategyFocus::Setting,
        ..strategy
    };
    let other = Unison {
        slot: fixture.party.members[0]
            .shortcuts
            .iter()
            .position(|&id| id != 0 && id != selected)
            .unwrap(),
        ..unison
    };
    for paranoid in [false, true] {
        let diagnostics = Diagnostics::new(paranoid);
        for (scope, result) in [
            (
                "battle Strategy page",
                fixture
                    .drawing(2)
                    .strategy(setting.page(&fixture.party, &fixture.data)),
            ),
            (
                "battle U. Attack page",
                fixture.drawing(2).unison(
                    other.page(&fixture.party, &fixture.session, &fixture.data),
                    false,
                ),
            ),
        ] {
            assert_eq!(diagnostics.attempt(scope, result).is_err(), paranoid);
        }
        assert_eq!(diagnostics.entries().len(), 2);
        assert!(
            diagnostics
                .entries()
                .iter()
                .all(|entry| entry.message.contains("uncooked menu glyph"))
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires locally cooked menus; CPU mesh/visibility preparation only"]
fn menu_submission_retires_layers_and_requires_a_new_draw() -> Result<()> {
    let fixture = Fixture::load()?;
    let mut world = World::new();
    let mut artwork = fixture.artwork(&mut world)?;
    let mut queue = bevy::ecs::world::CommandQueue::default();
    let mut meshes = Assets::<Mesh>::default();
    artwork.prepare_windows();
    queue.apply(&mut world);
    assert!(meshes.is_empty(), "unused menu planes allocated meshes");
    assert!(artwork.layers.is_empty());
    let state = resonance_game::menu::strategy::Strategy {
        focus: StrategyFocus::Rename,
        preset_opacity: 255,
        rename_opacity: 255,
        ..Default::default()
    };
    artwork.render_battle_strategy(
        state.page(&fixture.party, &fixture.data),
        &fixture.font,
        &fixture.dialogue,
        &fixture.party.settings.preferences,
        100,
        &mut Commands::new(&mut queue, &world),
        &mut meshes,
    )?;
    queue.apply(&mut world);
    assert!(
        artwork
            .layers
            .values()
            .all(|layer| world.get::<RenderLayers>(layer.entity)
                == Some(&RenderLayers::layer(crate::battle_view::LAYER)))
    );
    let mut menu = fixture.field_menu();
    menu.page = Page::Tech;
    menu.tech.character = 3;
    menu.at_save_point = true;
    menu.tech.return_to = TechFocus::List;
    for focus in [
        TechFocus::Target,
        TechFocus::CannotForget,
        TechFocus::Forget { yes: true },
        TechFocus::List,
    ] {
        menu.tech.focus = focus;
        artwork.render(
            Source::Title(Some(&menu)),
            &fixture.font,
            &fixture.dialogue,
            101,
            crate::Resolution::default(),
            &mut Commands::new(&mut queue, &world),
            &mut meshes,
        )?;
        queue.apply(&mut world);
        let submitted: Vec<_> = artwork
            .layers
            .values()
            .filter(|layer| layer.visible)
            .collect();
        assert!(!submitted.is_empty());
        for layer in submitted {
            assert!(meshes.get(&layer.mesh).unwrap().count_vertices() > 0);
            assert!(matches!(
                world.get::<Visibility>(layer.entity),
                Some(Visibility::Visible | Visibility::Inherited)
            ));
        }
    }
    let list = resonance_game::battle::command::ListFrame {
        rows: vec![],
        selected: 0,
        first: 0,
        scroll: 0,
        fade: 0,
        description_previous: 0,
        description_blend: 255,
    };
    artwork.render_battle_items(
        &list,
        &fixture.data,
        &fixture.font,
        &fixture.dialogue,
        &fixture.party.settings.preferences,
        101,
        &mut Commands::new(&mut queue, &world),
        &mut meshes,
    )?;
    queue.apply(&mut world);
    assert!(
        artwork
            .layers
            .range((2, DrawRole::Background, MaterialKey::Texture(0))..)
            .map(|(_, layer)| layer)
            .all(|layer| !layer.visible)
    );
    let mut images = world.remove_resource::<Assets<Image>>().unwrap();
    for image in artwork.drawn_images() {
        images.insert(image.id(), Image::default())?;
    }
    let previous_completion = artwork.draws.0.lock().unwrap().completed.clone();
    previous_completion.store(true, std::sync::atomic::Ordering::Release);
    assert!(artwork.ready(&images));
    artwork.render(
        Source::Title(None),
        &fixture.font,
        &fixture.dialogue,
        102,
        crate::Resolution::default(),
        &mut Commands::new(&mut queue, &world),
        &mut meshes,
    )?;
    queue.apply(&mut world);
    assert!(artwork.layers.values().all(|layer| !layer.visible
        && world.get::<Visibility>(layer.entity) == Some(&Visibility::Hidden)));
    artwork.render_battle_items(
        &list,
        &fixture.data,
        &fixture.font,
        &fixture.dialogue,
        &fixture.party.settings.preferences,
        103,
        &mut Commands::new(&mut queue, &world),
        &mut meshes,
    )?;
    queue.apply(&mut world);
    assert!(
        !artwork.ready(&images),
        "reopening must wait for its own drawing"
    );
    previous_completion.store(true, std::sync::atomic::Ordering::Release);
    assert!(
        !artwork.ready(&images),
        "a late old callback must not admit the new visit"
    );
    artwork
        .draws
        .0
        .lock()
        .unwrap()
        .completed
        .store(true, std::sync::atomic::Ordering::Release);
    assert!(artwork.ready(&images));
    Ok(())
}

#[test]
#[ignore = "requires locally cooked menus; CPU drawing only"]
fn equipment_focus_always_has_a_visible_cursor() -> Result<()> {
    let fixture = Fixture::load()?;
    let mut world = World::new();
    let mut artwork = fixture.artwork(&mut world)?;
    let mut queue = bevy::ecs::world::CommandQueue::default();
    let mut meshes = Assets::<Mesh>::default();
    artwork.prepare_windows();
    queue.apply(&mut world);
    for focus in [
        EquipmentFocus::Character,
        EquipmentFocus::Slots,
        EquipmentFocus::List,
        EquipmentFocus::Optimal { thrust: true },
    ] {
        let state = resonance_game::menu::equipment::Equipment {
            focus,
            ..Default::default()
        };
        let page = resonance_game::menu::equipment::Page {
            state: &state,
            party: &fixture.party,
            session: &fixture.session,
            data: &fixture.data,
            names: &fixture.data.initial_names,
            character: 0,
        };
        artwork.render_battle_equipment(
            page,
            &fixture.font,
            &fixture.dialogue,
            &fixture.party.settings.preferences,
            100,
            &mut Commands::new(&mut queue, &world),
            &mut meshes,
        )?;
        queue.apply(&mut world);
        assert!(
            artwork.layers[&(2, DrawRole::Cursor, FONT)].visible,
            "{focus:?}"
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires locally cooked menus; serialized prompt admission, input and CPU drawing"]
fn serialized_item_prompts_fail_on_selection_and_recover_without_panicking() -> Result<()> {
    use resonance_content::{
        diagnostics::Diagnostics,
        menu_data::{ItemUse, MenuData},
    };
    use resonance_game::{field::FieldInput, menu::items::Focus};
    let mut fixture = Fixture::load()?;
    let healthy = fixture.data.clone();
    let id = healthy
        .items
        .iter()
        .position(|item| {
            matches!(
                item.field_use,
                Some(ItemUse::Recover {
                    hp: 1..,
                    party: true,
                    ..
                })
            )
        })
        .context("fixture has no party healing item")? as u16;
    for prompt in [
        serde_json::Value::Null,
        serde_json::Value::Bool(false),
        serde_json::json!({"lines":[]}),
        serde_json::json!({"lines":[[{"kind":"button","sprite":255}]]}),
        serde_json::json!({"lines":[[{"kind":"text","text":"Use","color":255}]]}),
    ] {
        let mut document = serde_json::to_value(healthy.as_ref())?;
        if prompt.is_null() {
            document["presentation"]["items"]
                .as_object_mut()
                .unwrap()
                .remove("item_group_prompt");
        } else {
            document["presentation"]["items"]["item_group_prompt"] = prompt;
        }
        for key in ["confirm_discard", "discarded", "transformed"] {
            document["presentation"]["labels"]
                .as_object_mut()
                .unwrap()
                .remove(key);
        }
        let data = MenuData::decode(&serde_json::to_vec(&document)?, &Diagnostics::new(false))?;
        data.validate_gameplay()?;
        assert_eq!(data.item_text(id)?.name, healthy.item_text(id)?.name);
        fixture.data = std::sync::Arc::new(data);
        let mut menu = fixture.field_menu();
        menu.page = Page::Items;
        menu.inventory.category = healthy.items[usize::from(id)].inventory_category().unwrap();
        let party = &mut menu.checkpoint.as_mut().unwrap().progress_mut().party;
        party.items = [(id, 1)].into();
        party.members[0].hp = 1;
        let before = serde_json::to_vec(party)?;
        let mut animation = items::Animation::default();
        fixture
            .drawing(0)
            .items(&menu, animation.sample(menu.inventory.focus, 0))?;
        assert_eq!(
            menu.step(FieldInput {
                pressed_buttons: [resonance_events::input::Button::Accept].into(),
                ..Default::default()
            }),
            Some(4)
        );
        assert_eq!(menu.inventory.focus, Focus::List);
        assert!(menu.notice.is_some());
        assert_eq!(
            serde_json::to_vec(&menu.checkpoint.as_ref().unwrap().progress().party)?,
            before
        );
        // Direct rendering is checked too, including callers that already held a target page.
        menu.inventory.focus = Focus::Target;
        menu.inventory.target_all = true;
        for paranoid in [false, true] {
            let diagnostics = Diagnostics::new(paranoid);
            let mut animation = items::Animation::default();
            let result = fixture
                .drawing(0)
                .items(&menu, animation.sample(menu.inventory.focus, 0));
            assert!(result.is_err());
            assert_eq!(
                diagnostics.attempt("item target prompt", result).is_err(),
                paranoid
            );
            assert_eq!(diagnostics.entries().len(), 1);
        }
        fixture.data = healthy.clone();
        std::sync::Arc::get_mut(menu.resources.as_mut().unwrap())
            .unwrap()
            .data = healthy.clone();
        menu.inventory.focus = Focus::List;
        menu.notice = None;
        menu.take_failure();
        assert_eq!(
            menu.step(FieldInput {
                pressed_buttons: [resonance_events::input::Button::Accept].into(),
                ..Default::default()
            }),
            Some(2)
        );
        assert_eq!(menu.inventory.focus, Focus::Target);
        let mut animation = items::Animation::default();
        fixture
            .drawing(1)
            .items(&menu, animation.sample(menu.inventory.focus, 1))?;
        assert_eq!(
            serde_json::to_vec(&menu.checkpoint.as_ref().unwrap().progress().party)?,
            before
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires locally cooked menus; cooking and popup drawing only"]
fn cooking_commits_before_optional_popup_drawing() -> Result<()> {
    use resonance_content::menu_data::Ingredient;
    use resonance_game::field::FieldInput;
    use std::sync::Arc;
    let mut fixture = Fixture::load()?;
    let recipe = usize::from(fixture.party.cooking.recipe);
    for ingredient in &fixture.data.cooking.recipes[recipe].required {
        let id = match *ingredient {
            Ingredient::None => continue,
            Ingredient::Item(id) => id,
            Ingredient::Any(group) => fixture.data.cooking.groups[usize::from(group)].items[0],
        };
        fixture
            .party
            .change_item(&fixture.session, id, 3)
            .map_err(anyhow::Error::msg)?;
    }
    fixture.party.members[0].hp = 1;
    let healthy_art = fixture.art.clone();
    for failure in [Some("glyph"), Some("Items"), None] {
        fixture.art = healthy_art.clone();
        let mut menu = fixture.field_menu();
        menu.page = Page::Cooking;
        let mut expected = menu.checkpoint.as_ref().unwrap().progress().clone();
        expected
            .cook(&fixture.data)
            .map_err(|error| anyhow::anyhow!("{error:?}"))?;
        if failure == Some("glyph") {
            Arc::make_mut(&mut Arc::get_mut(menu.resources.as_mut().unwrap()).unwrap().data)
                .presentation
                .cooking
                .as_mut()
                .unwrap()
                .labels
                .insert("result_join".into(), "\u{10ffff}".into());
        } else if failure == Some("Items") {
            fixture.art.sprites.rects.remove(&Sprite::Items);
        }
        assert_eq!(
            menu.step(FieldInput {
                pressed_buttons: [resonance_events::input::Button::Ring].into(),
                ..Default::default()
            }),
            Some(2)
        );
        assert_eq!(
            serde_json::to_vec(&menu.checkpoint.as_ref().unwrap().progress())?,
            serde_json::to_vec(&expected)?
        );
        for tick in 0..3 {
            let result = fixture.drawing(tick).cooking_popup(&menu);
            if let Some(failure) = failure {
                assert!(result.unwrap_err().to_string().contains(failure));
            } else {
                result?;
            }
            // Holding the cook command while the result is open cannot pay again.
            menu.step(FieldInput {
                pressed_buttons: [resonance_events::input::Button::Ring].into(),
                ..Default::default()
            });
            assert_eq!(
                serde_json::to_vec(&menu.checkpoint.as_ref().unwrap().progress())?,
                serde_json::to_vec(&expected)?
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires locally cooked menus; selected artwork admission only"]
fn selected_menu_art_is_checked_without_admitting_unrelated_pages() -> Result<()> {
    let mut fixture = Fixture::load()?;
    fixture.art.sprites.rects.remove(&Sprite::Recipes);
    fixture.art.sprites.names.clear();
    fixture.art.sprites.number_colors.clear();
    fixture.art.sprites.bar_colors.clear();
    fixture.art.sprites.rects.remove(&Sprite::ItemImages);
    fixture
        .art
        .textures
        .get_mut(&MenuArt::PORTRAIT_TEXTURES.start)
        .unwrap()
        .width = 0;
    let atlas = fixture.art.textures[&MenuArt::ATLAS_TEXTURE].clone();
    fixture.art.textures.insert(usize::MAX, atlas);
    let mut world = World::new();
    let mut artwork = fixture.artwork(&mut world)?;
    let mut slots = Menu::new(Page::Slots(Mode::Load), None, false);
    slots.take_command();
    slots.finish(None);
    let mut queue = bevy::ecs::world::CommandQueue::default();
    let mut meshes = Assets::<Mesh>::default();
    artwork.render(
        Source::Title(Some(&slots)),
        &fixture.font,
        &fixture.dialogue,
        0,
        crate::Resolution::default(),
        &mut Commands::new(&mut queue, &world),
        &mut meshes,
    )?;
    assert!(!artwork.layers.is_empty());
    let mut draw = artwork.begin_drawing(&fixture.font, &fixture.dialogue, None, 0)?;
    let mut status = fixture.field_menu();
    status.page = Page::Status;
    assert!(draw.status(&status).is_err());
    assert!(
        draw.item_description_data(&fixture.data, 1)
            .unwrap_err()
            .to_string()
            .contains("ItemImages")
    );
    fixture.art.sprites.rects.get_mut(&Sprite::Buttons).unwrap()[0] = [0; 4];
    assert!(fixture.drawing(0).button(0, [0., 0.]).is_err());
    fixture.art.sprites.rects.remove(&Sprite::Buttons);
    assert!(fixture.drawing(0).button(0, [0., 0.]).is_err());
    fixture.art.sprites.rects.remove(&Sprite::ConditionIcons);
    fixture
        .art
        .sprites
        .rects
        .remove(&Sprite::PetrifiedPortraits);
    let mut member = fixture.party.members[0].clone();
    member.hp = 0;
    member.ailments.petrified = true;
    member.ailments.poison = resonance_events::party::Poison::Both;
    // Knockout uses the normal portrait without any ailment art.
    fixture.drawing(0).portrait(0, &member, [0., 0.])?;
    member.hp = 1;
    assert!(
        fixture
            .drawing(0)
            .portrait(0, &member, [0., 0.])
            .unwrap_err()
            .to_string()
            .contains("PetrifiedPortraits")
    );
    member.ailments.petrified = false;
    assert!(
        fixture
            .drawing(0)
            .portrait(0, &member, [0., 0.])
            .unwrap_err()
            .to_string()
            .contains("ConditionIcons")
    );
    let window = artwork.spec.windows.get_mut(&1).unwrap();
    window.heading = Some(usize::MAX - 1);
    window.slices.as_mut().unwrap()[9] = usize::MAX - 1;
    let mut frame = artwork.begin_drawing(&fixture.font, &fixture.dialogue, None, 0)?;
    frame.frame([16., 60., 200., 100.])?;
    assert!(frame.heading("Heading").is_err());
    assert!(frame.framed([16., 60., 200., 100.], true).is_err());
    artwork.spec.windows.clear();
    artwork.render(
        Source::Title(None),
        &fixture.font,
        &fixture.dialogue,
        1,
        crate::Resolution::default(),
        &mut Commands::new(&mut queue, &world),
        &mut meshes,
    )?;
    assert!(artwork.layers.values().all(|layer| !layer.visible));
    assert!(
        artwork
            .begin_drawing(&fixture.font, &fixture.dialogue, None, 1)
            .is_err()
    );
    Ok(())
}
