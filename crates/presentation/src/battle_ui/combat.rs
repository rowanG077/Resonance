//! Battle notices, enemy roster, combo summaries, and meters.
use super::*;
#[path = "combo.rs"]
mod combo;
#[path = "markers.rs"]
mod markers;

const ENEMY_PANEL: usize = 0;
const ENEMY_TEXT: usize = 1;
const COMBO_PANEL: usize = 2;
const COMBO_TEXT: usize = 3;
const METERS: usize = 4;

pub(super) struct Hud {
    pub(super) layers: Vec<HudLayer>,
    pub(super) enemies: Vec<EnemyHud>,
    icon_layers: [Option<usize>; 4],
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
            .validate_text("ITEM 0123456789.ESCAPEUNISONREADY")?;
        let mut layers = vec![
            palette.solid.clone().at(Depth::Enemies),
            palette.main_font.clone().at(Depth::Enemies).foreground(),
            palette.solid.clone().at(Depth::Combo),
            palette.main_font.clone().at(Depth::Combo).foreground(),
            palette.solid.clone().at(Depth::Meters),
            palette.font.clone().at(Depth::Meters).foreground(),
        ];
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
        Ok(Self {
            layers: palette.layers(layers, materials),
            enemies: enemies.to_vec(),
            icon_layers,
            combos: Default::default(),
        })
    }

    pub fn advance(&mut self, frame: &BattleFrame, elapsed: u32) -> Result<()> {
        self.combos.advance(frame, elapsed)
    }

    fn draw(
        &mut self,
        frame: &BattleFrame,
        battle_font: &BitmapFont,
        font: &BitmapFont,
        batch: &mut [Batch],
    ) -> Result<()> {
        meters::draw(
            frame,
            battle_font,
            &mut batch[METERS..METERS + meters::COUNT],
        )?;
        draw_enemies(&self.enemies, &self.icon_layers, frame, font, batch)?;
        let (left, right) = batch.split_at_mut(COMBO_TEXT);
        markers::draw(frame, font, &mut left[COMBO_PANEL], &mut right[0])?;
        if frame.recognized_result.is_none() && frame.target_selector.is_none() {
            let (left, right) = batch.split_at_mut(COMBO_TEXT);
            self.combos
                .draw(font, &mut left[COMBO_PANEL], &mut right[0])?;
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
    let leader = frame
        .target_selector
        .map(|id| id.index())
        .or_else(|| {
            frame.actors.iter().position(|actor| {
                actor.side == Side::Party && actor.control != resonance_battle::Control::Auto
            })
        })
        .or_else(|| {
            frame
                .actors
                .iter()
                .position(|actor| actor.side == Side::Party)
        });
    let target = leader
        .and_then(|leader| frame.targets[leader])
        .map(|id| id.index());
    for (index, enemy) in enemies.iter().enumerate() {
        let actor = &frame.actors[enemy.actor];
        if !actor.available() || actor.hp <= 0 {
            continue;
        }
        let x = 12. + (index % 2) as f32 * 140.;
        let y = 16. + (index / 2) as f32 * 34.;
        if target == Some(enemy.actor) {
            batch[ENEMY_PANEL].quad(
                [x - 2., y - 2., x + 134., y + 32.],
                [0.5; 4],
                [0.95, 0.75, 0.3, 1.],
            );
        }
        batch[ENEMY_PANEL].quad([x, y, x + 132., y + 30.], [0.5; 4], [0.04, 0.05, 0.08, 0.9]);
        let icon = icon_layers[usize::from(enemy.group)];
        if let Some(layer) = icon {
            batch[layer].quad(
                [x + 2., y + 3., x + 26., y + 27.],
                [0., 0., 32., 32.],
                hud_color([128, 128, 128, 255]),
            );
        }
        let inset = if icon.is_some() { 30. } else { 4. };
        let name = format!("{} {}", index + 1, enemy.name);
        text::fit(
            &mut batch[ENEMY_TEXT],
            font,
            &name,
            [x + inset, y + 3., 128. - inset, 12.],
            [128, 128, 128, 255],
        )?;
        if matches!(actor.activity, Activity::Casting { .. }) {
            text::fit(
                &mut batch[ENEMY_TEXT],
                font,
                "CASTING",
                [x + inset, y + 18., 128. - inset, 10.],
                [90, 110, 128, 255],
            )?;
        }
    }
    Ok(())
}

impl Artwork {
    pub(super) fn render_combat(
        &mut self,
        frame: &BattleFrame,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let mut batches: Vec<_> = (0..self.combat.layers.len())
            .map(|_| Batch::default())
            .collect();
        self.combat
            .draw(frame, &self.art.font, &self.font, &mut batches)?;
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
            hud.combat
                .draw(frame, &hud.art.font, &hud.font, &mut batches)?;
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
        assert!(notices.iter().all(|b| !b.indices.is_empty()));
        assert!(
            notices
                .iter()
                .flat_map(|b| &b.positions)
                .all(|p| (-16. ..=308.).contains(&p[0]) && (156. ..=224.).contains(&p[1]))
        );
        assert_eq!(first[0].positions.len(), 8, "both sides have a combo panel");
        assert!(!first[1].indices.is_empty());
        assert!(
            first
                .iter()
                .flat_map(|b| &b.positions)
                .all(|p| (-320. ..=0.).contains(&p[0]) && (32. ..=92.).contains(&p[1]))
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
        assert!(first[0] == replaced[0] && first[1] != replaced[1]);
        for _ in 0..100 {
            hud.advance(&frame, true)?;
        }
        assert!(hud.draw_notices(&frame, party)? == notices);
        assert!(combos(&mut hud, &frame)? == replaced);
        for _ in 0..12 {
            hud.advance(&frame, false)?;
        }
        assert!(hud.draw_notices(&frame, party)? == notices);
        for _ in 0..10 {
            hud.advance(&frame, false)?;
        }
        let fading = hud.draw_notices(&frame, party)?;
        assert!(fading != notices && !fading[1].indices.is_empty());
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
        let mut scanned = hud.scan_batches(&frame)?;
        assert!(scanned.iter().all(|b| !b.indices.is_empty()));
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
        for affinity in [
            Affinity::Normal,
            Affinity::Weak,
            Affinity::Resistant,
            Affinity::Absorb,
            Affinity::Immune,
        ] {
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
                    .all(|p| (8. ..=632.).contains(&(p[0] + 320.))
                        && (8. ..party::TOP).contains(&(240. - p[1])))
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
        for _ in 0..4 * 60 {
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
            let banner = hud.draw_notices(&frame, party)?;
            assert!(banner.iter().all(|b| !b.indices.is_empty()));
            assert!(
                banner
                    .iter()
                    .flat_map(|b| &b.positions)
                    .all(|p| (-160. ..=160.).contains(&p[0]) && (24. ..=68.).contains(&p[1]))
            );
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires the dialogue font; CPU enemy HUD drawing only"]
    fn enemy_roster_shows_live_targets_and_casting_without_icons() -> Result<()> {
        use resonance_battle::{ActorId, BattleResult, Control};
        let font = fixtures::dialogue_font()?;
        let mut frame = fixtures::frame(fixtures::actor(Default::default()));
        frame.actors = vec![frame.actors[0].clone(); 10];
        frame.actors[0].control = Control::Manual;
        for actor in &mut frame.actors[2..] {
            actor.side = Side::Enemy;
        }
        frame.targets = vec![None; 10];
        frame.targets[0] = Some(ActorId::from_index(2)?);
        frame.targets[1] = Some(ActorId::from_index(9)?);
        let enemies: Vec<_> = (0..8)
            .map(|index| EnemyHud {
                actor: index + 2,
                group: (index % 4) as u8,
                name: "Long enemy name".into(),
                icon: None,
            })
            .collect();
        let draw = |frame: &BattleFrame| -> Result<[Batch; 2]> {
            let mut batches = std::array::from_fn(|_| Batch::default());
            draw_enemies(&enemies, &[None; 4], frame, &font, &mut batches)?;
            Ok(batches)
        };
        let ordinary = draw(&frame)?;
        assert!(!ordinary[ENEMY_TEXT].indices.is_empty());
        assert!(
            ordinary
                .iter()
                .flat_map(|batch| &batch.positions)
                .all(|p| { (-320. ..=320.).contains(&p[0]) && (-122. ..=240.).contains(&p[1]) }),
            "enemy rows stay on screen above the meters and party cards"
        );
        frame.target_selector = Some(ActorId::from_index(1)?);
        let targeting = draw(&frame)?;
        assert!(
            ordinary[ENEMY_PANEL] != targeting[ENEMY_PANEL],
            "selection follows its actual owner"
        );
        assert!(ordinary[ENEMY_TEXT] == targeting[ENEMY_TEXT]);
        frame.actors[9].activity = Activity::Casting { held: false };
        let casting = draw(&frame)?;
        assert!(casting[ENEMY_TEXT].positions.len() > targeting[ENEMY_TEXT].positions.len());
        frame.actors[9].hp = 0;
        let defeated = draw(&frame)?;
        assert!(defeated[ENEMY_PANEL].positions.len() < ordinary[ENEMY_PANEL].positions.len());
        assert!(defeated[ENEMY_TEXT].positions.len() < ordinary[ENEMY_TEXT].positions.len());
        frame.recognized_result = Some(BattleResult::Victory);
        assert!(draw(&frame)?.iter().all(|batch| batch.indices.is_empty()));
        Ok(())
    }

    #[test]
    #[ignore = "requires the battle font; CPU meter drawing only"]
    fn native_meters_draw_live_progress_and_dismiss_at_results() -> Result<()> {
        use resonance_battle::item::ITEM_COOLDOWN_TICKS;
        use resonance_battle::{BattleResult, EscapeFrame, MAX_ESCAPE_GAUGE, MAX_UNISON_GAUGE};
        let font = fixtures::battle_font()?;
        let draw = |frame: &BattleFrame| -> Result<[Batch; meters::COUNT]> {
            let mut batches = std::array::from_fn(|_| Batch::default());
            meters::draw(frame, &font, &mut batches)?;
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
        assert!(draw(&frame)?[0].positions.len() < ready[0].positions.len());
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
