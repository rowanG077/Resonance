//! Battle notices, enemy roster, combo summaries, and meters.
use super::*;
#[path = "combo.rs"]
mod combo;
#[path = "markers.rs"]
mod markers;

const ENEMY_NUMBERS: usize = 0;
const COMBO_PANEL: usize = 1;
const COMBO_TEXT: usize = 2;
const METERS: usize = 3;

pub(super) struct Hud {
    pub(super) layers: Vec<HudLayer>,
    pub(super) enemies: Vec<EnemyHud>,
    icon_layers: [Option<usize>; 4],
    marker_layers: [Option<usize>; markers::COUNT],
    combos: combo::Combos,
}

impl Hud {
    pub fn load(
        art: &Art,
        enemies: &[EnemyHud],
        palette: &Palette,
        materials: &mut Assets<Surface>,
        mut load: impl FnMut(&resonance_content::font::UiTexture, bool) -> Result<Option<Handle<Image>>>,
    ) -> Result<Self> {
        art.font
            .validate_text("ITEM 0123456789.ESCAPEUNISONREADYHITSDAMAGE")?;
        let mut layers = vec![
            palette.font.clone().at(Depth::Enemies).foreground(),
            palette.solid.clone().at(Depth::Combo),
            palette.font.clone().at(Depth::Combo).foreground(),
            palette.solid.clone().at(Depth::Meters),
            palette
                .font
                .clone()
                .at(Depth::Meters)
                .foreground()
                .foreground()
                .foreground()
                .foreground(),
        ];
        for (index, texture) in std::iter::once(&art.feedback.unison_frame)
            .chain(&art.feedback.escape)
            .enumerate()
        {
            let image = load(texture, false)?.context("battle meter atlas is missing")?;
            let mut definition =
                LayerDefinition::new(image, [texture.width, texture.height], Depth::Meters);
            definition.depth += [1., 2., 3., 1.][index];
            layers.push(definition);
        }
        ensure!(enemies.len() <= 8, "too many battle HUD enemies");
        let mut icon_layers = [None; 4];
        for enemy in enemies {
            let layer = icon_layers
                .get_mut(usize::from(enemy.group))
                .context("unknown battle HUD enemy group")?;
            if layer.is_none()
                && let Some(icon) = &enemy.icon
                && let Some(image) = load(icon, false)?
            {
                *layer = Some(layers.len());
                layers.push(
                    LayerDefinition::new(image, [icon.width, icon.height], Depth::Enemies)
                        .foreground()
                        .nearest(),
                );
            }
        }
        let mut marker_layers = [None; markers::COUNT];
        for (index, (slot, texture)) in marker_layers
            .iter_mut()
            .zip(art.targets.iter().chain([&art.stun]))
            .enumerate()
        {
            if let Some(image) = load(texture, false)? {
                *slot = Some(layers.len());
                let mut definition =
                    LayerDefinition::new(image, [texture.width, texture.height], Depth::Labels);
                if index > 0 {
                    definition = definition.foreground();
                }
                layers.push(definition);
            }
        }
        Ok(Self {
            layers: palette.layers(layers, materials),
            enemies: enemies.to_vec(),
            icon_layers,
            marker_layers,
            combos: Default::default(),
        })
    }

    pub fn advance(&mut self, frame: &BattleFrame, elapsed: u32) -> Result<()> {
        self.combos.advance(frame, elapsed)
    }

    fn draw(
        &mut self,
        frame: &BattleFrame,
        art: &Art,
        batch: &mut [Batch],
        command: Option<&resonance_game::battle::command::Frame>,
    ) -> Result<()> {
        meters::draw(frame, art, &mut batch[METERS..METERS + meters::COUNT])?;
        draw_enemies(&self.enemies, &self.icon_layers, frame, &art.font, batch)?;
        let selection = command.and_then(|command| {
            use resonance_game::battle::command::View;
            match command.view {
                View::Enemy { target } => Some((command.actor, target)),
                View::TechTarget {
                    target: Some(target),
                } => Some((command.actor, target)),
                _ => None,
            }
        });
        for (layer, markers) in self
            .marker_layers
            .into_iter()
            .zip(markers::draw(frame, selection)?)
        {
            if let Some(layer) = layer {
                batch[layer] = markers;
            }
        }
        if frame.recognized_result.is_none() && frame.target_selector.is_none() {
            let (left, right) = batch.split_at_mut(COMBO_TEXT);
            self.combos
                .draw(&art.font, &art.combo, &mut left[COMBO_PANEL], &mut right[0])?;
        }
        Ok(())
    }
}

fn draw_enemies(
    enemies: &[EnemyHud],
    icon_layers: &[Option<usize>; 4],
    frame: &BattleFrame,
    font: &BitmapFont,
    batch: &mut [Batch],
) -> Result<()> {
    if frame.recognized_result.is_some() {
        return Ok(());
    }
    let group_count = enemies
        .iter()
        .map(|enemy| enemy.group)
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let columns = if group_count > 2 && enemies.len() <= 4 {
        4
    } else {
        enemies.len().div_ceil(2)
    };
    let mut ordinals = [0; 4];
    for (index, enemy) in enemies.iter().enumerate() {
        let group = usize::from(enemy.group);
        let ordinal = ordinals[group];
        ordinals[group] += 1;
        let actor = &frame.actors[enemy.actor];
        if !actor.available() || actor.hp <= 0 {
            continue;
        }
        let (column, row) = if group_count == 2 {
            (ordinal, group)
        } else {
            (index % columns, index / columns)
        };
        let x = 464. + column as f32 * 36.;
        let y = 388. + row as f32 * 36.;
        if let Some(layer) = icon_layers[group] {
            batch[layer].quad([x, y, x + 32., y + 32.], [0., 0., 32., 32.], [1.; 4]);
        }
        text::glyphs(
            &mut batch[ENEMY_NUMBERS],
            font,
            &(ordinal + 1).to_string(),
            [x + 22., y + 20.],
            [12., 12.],
            [128, 128, 128, 255],
        )?;
    }
    Ok(())
}

impl Artwork {
    pub(super) fn render_combat(
        &mut self,
        frame: &BattleFrame,
        command: Option<&resonance_game::battle::command::Frame>,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let mut batches: Vec<_> = (0..self.combat.layers.len())
            .map(|_| Batch::default())
            .collect();
        self.combat.draw(frame, &self.art, &mut batches, command)?;
        for (layer, batch) in self.combat.layers.iter_mut().zip(batches) {
            layer.upload(batch, commands, meshes)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires current HUD assets; CPU feedback integration only"]
    fn feedback_draws_replaces_holds_and_expires() -> Result<()> {
        use resonance_battle::{ActorId, Affinity, BattleResult, Cue};
        use resonance_game::battle::lifecycle::Request;

        let (mut hud, _, _app) = test_artwork()?;
        let mut frame = fixtures::frame(fixtures::actor(Default::default()));
        let mut enemy = frame.actors[0].clone();
        enemy.side = Side::Enemy;
        frame.actors.extend([enemy.clone(), enemy]);
        frame.targets = vec![None; frame.actors.len()];
        frame.camera = None;
        hud.combat.enemies = (1..=2)
            .map(|actor| EnemyHud {
                actor,
                group: 0,
                name: format!("Slime {actor} %s"),
                icon: None,
            })
            .collect();
        let party = PartyHudInput {
            characters: &[1],
            queued_techniques: &[false],
            names: &[Some("R%s")],
        };
        let combos = |hud: &mut Artwork, frame: &BattleFrame| -> Result<[Batch; 2]> {
            let mut batches = (0..hud.combat.layers.len())
                .map(|_| Batch::default())
                .collect::<Vec<_>>();
            hud.combat.draw(frame, &hud.art, &mut batches, None)?;
            Ok([
                std::mem::take(&mut batches[COMBO_PANEL]),
                std::mem::take(&mut batches[COMBO_TEXT]),
            ])
        };
        hud.item_notice_request(0, 1, 12, &frame)?;
        assert_eq!(
            hud.party_notice_text(),
            Some(hud.menu_data.item_text(1)?.name.as_str())
        );
        hud.notice_request(
            0,
            "A deliberately long action name that must stay inside its notice",
            12,
            &frame,
        )?;
        hud.notice_request(1, "Enemy action", 12, &frame)?;
        frame.cues = vec![
            Cue::Combo {
                actor: ActorId::from_index(1)?,
                hits: 3,
                damage: 27,
            },
            Cue::Combo {
                actor: ActorId::from_index(0)?,
                hits: 2,
                damage: 10,
            },
        ];
        hud.advance(&frame, true)?;
        let notices = hud.draw_notices(&frame, party)?;
        let first = combos(&mut hud, &frame)?;
        assert!(notices.iter().any(|b| !b.indices.is_empty()));
        assert!(
            notices
                .iter()
                .flat_map(|b| &b.positions)
                .all(|p| (-320. ..=320.).contains(&p[0]) && (152. ..=240.).contains(&p[1]))
        );
        assert_eq!(
            first[0].positions.len(),
            8,
            "both corners have a hit-count strip"
        );
        assert!(!first[1].indices.is_empty());
        assert!(
            first
                .iter()
                .flat_map(|b| &b.positions)
                .all(|p| (-320. ..=320.).contains(&p[0]) && (128. ..=232.).contains(&p[1]))
        );
        assert!(
            combos(&mut hud, &frame)? == first,
            "drawing consumes no time"
        );
        frame.cues = vec![Cue::Combo {
            actor: ActorId::from_index(1)?,
            hits: 12,
            damage: 140,
        }];
        hud.advance(&frame, true)?;
        frame.cues.clear();
        let replaced = combos(&mut hud, &frame)?;
        assert!(
            first != replaced,
            "the count and its strip expand for another digit"
        );
        for _ in 0..100 {
            hud.advance(&frame, true)?;
        }
        assert!(hud.draw_notices(&frame, party)? == notices);
        assert!(combos(&mut hud, &frame)? == replaced);
        for _ in 0..12 {
            hud.advance(&frame, false)?;
        }
        let entered = hud.draw_notices(&frame, party)?;
        assert!(
            entered != notices,
            "notices fade in while the battle advances"
        );
        for _ in 0..10 {
            hud.advance(&frame, false)?;
        }
        let fading = hud.draw_notices(&frame, party)?;
        assert!(fading != entered && !fading[1].indices.is_empty());
        for _ in 0..120 {
            hud.advance(&frame, false)?;
        }
        assert!(
            hud.draw_notices(&frame, party)?
                .iter()
                .all(|b| b.indices.is_empty())
        );
        assert!(
            combos(&mut hud, &frame)?
                .iter()
                .all(|b| b.indices.is_empty())
        );

        hud.notice_request(0, "Magic Lens", 90, &frame)?;
        frame.cues.push(Cue::EnemyScanned {
            actor: ActorId::from_index(1)?,
        });
        hud.advance(&frame, false)?;
        frame.cues.clear();
        for _ in 0..15 {
            hud.advance(&frame, false)?;
        }
        let mut scanned = hud.scan_batches(&frame)?;
        assert!(scanned[..2].iter().all(|b| !b.indices.is_empty()));
        assert!(
            hud.draw_notices(&frame, party)?
                .iter()
                .all(|b| b.indices.is_empty()),
            "scan details take precedence over notices"
        );
        for _ in 0..300 {
            hud.advance(&frame, true)?;
        }
        assert!(
            hud.scan_batches(&frame)? == scanned,
            "a five-second hold preserves the scan"
        );
        frame.actors[1].hp = i32::MAX;
        frame.actors[1].equipment.max_hp = i32::MAX;
        frame.actors[1].tp = u16::MAX;
        frame.actors[1].equipment.max_tp = u16::MAX;
        for affinity in [Affinity::Normal, Affinity::Weak, Affinity::Resistant] {
            frame.actors[1].equipment.affinities.fill(affinity);
            let current = hud.scan_batches(&frame)?;
            assert!(
                current != scanned,
                "live vitals and affinities change the display"
            );
            assert!(
                current
                    .iter()
                    .flat_map(|b| &b.positions)
                    .all(|p| (464. ..=640.).contains(&(p[0] + 320.))
                        && (328. ..=454.).contains(&(240. - p[1])))
            );
            scanned = current;
        }
        frame.cues.push(Cue::EnemyScanned {
            actor: ActorId::from_index(2)?,
        });
        hud.advance(&frame, false)?;
        frame.cues.clear();
        assert!(
            hud.scan_batches(&frame)? != scanned,
            "a new target replaces the scan"
        );
        for _ in 0..4 * 60 + 30 {
            hud.advance(&frame, false)?;
        }
        assert!(
            hud.scan_batches(&frame)?
                .iter()
                .all(|b| b.indices.is_empty())
        );

        hud.notice_request(0, "Victory", 90, &frame)?;
        frame.cues = vec![
            Cue::EnemyScanned {
                actor: ActorId::from_index(1)?,
            },
            Cue::Combo {
                actor: ActorId::from_index(1)?,
                hits: 2,
                damage: 8,
            },
        ];
        hud.advance(&frame, false)?;
        frame.cues.clear();
        frame.recognized_result = Some(BattleResult::Victory);
        hud.advance(&frame, false)?;
        assert!(
            hud.scan_batches(&frame)?
                .iter()
                .all(|b| b.indices.is_empty())
        );
        assert!(
            combos(&mut hud, &frame)?
                .iter()
                .all(|b| b.indices.is_empty())
        );
        assert!(
            hud.draw_notices(&frame, party)?
                .iter()
                .all(|b| b.indices.is_empty())
        );
        for request in [Request::DefeatNotice, Request::EscapeNotice] {
            hud.overlay_request(request)?;
            assert!(
                hud.draw_notices(&frame, party)?
                    .iter()
                    .all(|batch| batch.indices.is_empty())
            );
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires the battle font; CPU enemy HUD drawing only"]
    fn enemy_icons_keep_their_slots_when_an_enemy_is_defeated() -> Result<()> {
        let font = fixtures::battle_font()?;
        let mut frame = fixtures::frame(fixtures::actor(Default::default()));
        frame.actors = vec![frame.actors[0].clone(); 3];
        let enemies = (0..3)
            .map(|actor| EnemyHud {
                actor,
                group: if actor < 2 { 0 } else { 1 },
                name: String::new(),
                icon: None,
            })
            .collect::<Vec<_>>();
        let draw = |frame: &BattleFrame| -> Result<[Batch; 3]> {
            let mut batches = std::array::from_fn(|_| Batch::default());
            draw_enemies(
                &enemies,
                &[Some(1), Some(2), None, None],
                frame,
                &font,
                &mut batches,
            )?;
            Ok(batches)
        };
        let all = draw(&frame)?;
        assert_eq!(all[1].positions.len(), 8);
        assert_eq!(all[2].positions.len(), 4);
        frame.actors[0].hp = 0;
        let remaining = draw(&frame)?;
        assert_eq!(remaining[1].positions, all[1].positions[4..]);
        assert_eq!(remaining[2].positions, all[2].positions);
        frame.recognized_result = Some(resonance_battle::BattleResult::Victory);
        assert!(draw(&frame)?.iter().all(|batch| batch.indices.is_empty()));
        Ok(())
    }

    #[test]
    #[ignore = "requires the battle font; CPU meter drawing only"]
    fn native_meters_draw_live_progress_and_dismiss_at_results() -> Result<()> {
        use resonance_battle::item::ITEM_COOLDOWN_TICKS;
        use resonance_battle::{BattleResult, EscapeFrame, MAX_ESCAPE_GAUGE, MAX_UNISON_GAUGE};
        let art: Art = serde_json::from_slice(&std::fs::read(
            fixtures::asset_root().join(resonance_content::battle_ui::PATH),
        )?)?;
        let draw = |frame: &BattleFrame| -> Result<[Batch; meters::COUNT]> {
            let mut batches = std::array::from_fn(|_| Batch::default());
            meters::draw(frame, &art, &mut batches)?;
            Ok(batches)
        };
        let mut frame = fixtures::frame(fixtures::actor(Default::default()));
        assert!(draw(&frame)?.iter().all(|batch| batch.indices.is_empty()));
        frame.item_cooldown = ITEM_COOLDOWN_TICKS / 2;
        frame.escape = Some(EscapeFrame {
            requested: true,
            gauge: MAX_ESCAPE_GAUGE / 2,
        });
        frame.unison_available = true;
        frame.unison_gauge = MAX_UNISON_GAUGE / 2;
        let half = draw(&frame)?;
        assert!(half.iter().all(|batch| !batch.indices.is_empty()));
        assert!(draw(&frame)? == half, "drawing does not advance meters");
        frame.item_cooldown = 0;
        let cooling = draw(&frame)?;
        assert!(cooling[0].positions.len() < half[0].positions.len());
        frame.escape.as_mut().unwrap().requested = false;
        let charging = draw(&frame)?;
        assert!(
            charging == cooling,
            "canceled escape retains its remaining progress"
        );
        frame.unison_gauge = MAX_UNISON_GAUGE;
        let ready = draw(&frame)?;
        assert!(ready[0] != charging[0], "meter follows live progress");
        assert!(
            ready[1].positions.len() > charging[1].positions.len(),
            "ready caption is added"
        );
        frame.escape.as_mut().unwrap().gauge = 0;
        assert!(
            draw(&frame)?[3..]
                .iter()
                .all(|batch| batch.indices.is_empty())
        );
        frame.unison_available = false;
        assert!(draw(&frame)?.iter().all(|batch| batch.indices.is_empty()));
        frame.unison_available = true;
        for result in [
            BattleResult::Victory,
            BattleResult::Defeat,
            BattleResult::Escaped,
        ] {
            frame.recognized_result = Some(result);
            assert!(draw(&frame)?.iter().all(|batch| batch.indices.is_empty()));
        }
        Ok(())
    }
}
