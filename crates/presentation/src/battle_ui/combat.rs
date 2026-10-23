//! Battle notices, enemy roster, combo summaries, and meters.
use super::*;
#[path = "combo.rs"]
mod combo;
#[path = "markers.rs"]
mod markers;
#[path = "technical_smash.rs"]
mod technical_smash;

const ENEMY_NUMBERS: usize = 0;
const ENEMY_PAGE_SIZE: usize = 8;
const ENEMY_COLUMNS: usize = 4;
const COMBO_PANEL: usize = 1;
const COMBO_TEXT: usize = 2;
const METERS: usize = 3;
const TARGET_ORNAMENTS: usize = 5;

pub(super) struct Hud {
    pub(super) layers: Vec<HudLayer>,
    pub(super) enemies: Vec<EnemyHud>,
    stack_enemies: bool,
    icon_layers: Vec<Option<usize>>,
    marker_layers: [Option<usize>; markers::COUNT],
    roster_marker_layers: [Option<usize>; TARGET_ORNAMENTS],
    combos: combo::Combos,
    technical_smash: technical_smash::Notice,
    markers: markers::Markers,
    motion_tick: u16,
    motion_steps: u32,
    ui_steps: u32,
}

impl Hud {
    pub(super) fn omit_images(&mut self, images: &[Handle<Image>]) {
        for layer in self
            .icon_layers
            .iter_mut()
            .chain(&mut self.marker_layers)
            .chain(&mut self.roster_marker_layers)
        {
            if layer.is_some_and(|index| images.contains(&self.layers[index].definition.image)) {
                *layer = None;
            }
        }
    }

    pub fn load(
        art: &Art,
        enemies: &[EnemyHud],
        stack_enemies: bool,
        palette: &Palette,
        materials: &mut Assets<Surface>,
        mut load: impl FnMut(&resonance_content::font::UiTexture, bool, bool) -> Handle<Image>,
    ) -> Result<Self> {
        art.font
            .validate_text("ITEM 0123456789.ESCAPEUNISONREADYHITSDAMAGE TECHNICAL SMASH!% AERIAL SUMMON HIGH SPEED CONTINUE COUNTER SKILL")?;
        let mut layers = vec![
            palette.font.clone().at(Depth::Enemies).offset_depth(1.),
            palette.solid.clone().at(Depth::Combo),
            palette.font.clone().at(Depth::Combo).offset_depth(1.),
            palette.solid.clone().at(Depth::Meters),
            palette.font.clone().at(Depth::Meters).offset_depth(4.),
        ];
        for (index, texture) in std::iter::once(&art.feedback.unison_frame)
            .chain(&art.feedback.escape)
            .enumerate()
        {
            let image = load(texture, false, true);
            let mut definition =
                LayerDefinition::new(image, [texture.width, texture.height], Depth::Meters);
            definition.depth += [1., 2., 3., 1.][index];
            layers.push(definition);
        }
        let mut unison_shadow = layers[METERS + 2].clone();
        unison_shadow.depth = Depth::Meters.value() - 1.;
        layers.push(unison_shadow);
        let mut icon_layers = vec![None; enemy_group_count(enemies)];
        for enemy in enemies {
            let layer = icon_layers
                .get_mut(usize::from(enemy.group))
                .context("unknown battle HUD enemy group")?;
            if layer.is_none()
                && let Some(icon) = &enemy.icon
            {
                let image = load(icon, false, false);
                *layer = Some(layers.len());
                layers.push(
                    LayerDefinition::new(image, [icon.width, icon.height], Depth::Enemies)
                        .nearest(),
                );
            }
        }
        let mut marker_layers = [None; markers::COUNT];
        let mut roster_marker_layers = [None; TARGET_ORNAMENTS];
        for (index, (slot, texture)) in marker_layers.iter_mut().zip(art.targets.iter()).enumerate()
        {
            let image = load(texture, false, false);
            *slot = Some(layers.len());
            let mut definition =
                LayerDefinition::new(image, [texture.width, texture.height], Depth::Labels);
            definition.depth += index as f32;
            layers.push(definition.clone());
            roster_marker_layers[index] = Some(layers.len());
            let mut roster = definition.at(Depth::Enemies);
            roster.depth += 2. + index as f32;
            layers.push(roster);
        }
        marker_layers[markers::LABELS] = Some(layers.len());
        let mut labels = palette.font.clone().at(Depth::Labels);
        labels.depth += markers::LABELS as f32;
        layers.push(labels);
        marker_layers[markers::PARALYSIS] = Some(layers.len());
        let mut paralysis = LayerDefinition::new(
            load(&art.stun, false, true),
            [art.stun.width, art.stun.height],
            Depth::Labels,
        );
        paralysis.depth += markers::PARALYSIS as f32;
        layers.push(paralysis);
        Ok(Self {
            layers: palette.layers(layers, materials),
            enemies: enemies.to_vec(),
            stack_enemies,
            icon_layers,
            marker_layers,
            roster_marker_layers,
            combos: Default::default(),
            technical_smash: Default::default(),
            markers: Default::default(),
            motion_tick: 0,
            motion_steps: 0,
            ui_steps: 0,
        })
    }

    pub fn advance(&mut self, frame: &BattleFrame, elapsed: u32) -> Result<()> {
        self.motion_steps = self.motion_steps.saturating_add(elapsed);
        self.ui_steps = self.ui_steps.saturating_add(1);
        self.technical_smash.advance(frame, elapsed);
        self.combos.advance(frame, elapsed)
    }

    fn draw(
        &mut self,
        frame: &BattleFrame,
        art: &Art,
        batch: &mut [Batch],
        command: Option<&resonance_game::battle::command::Frame>,
        marker_anchors: &[ActorMarkers],
    ) -> Result<()> {
        // Selection keeps its own ornaments moving while gameplay waits.
        let elapsed = if command.is_some() || frame.target_selector.is_some() {
            self.ui_steps
        } else {
            self.motion_steps
        };
        self.ui_steps = 0;
        self.motion_steps = 0;
        self.motion_tick = ((u32::from(self.motion_tick) + elapsed) % 360) as u16;
        meters::draw(frame, art, &mut batch[METERS..METERS + meters::COUNT])?;
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
        draw_enemies(
            &self.enemies,
            self.stack_enemies,
            &self.icon_layers,
            &self.roster_marker_layers,
            selection,
            frame,
            &art.font,
            self.motion_tick,
            batch,
        )?;
        for (layer, markers) in self.marker_layers.into_iter().zip(self.markers.draw(
            frame,
            selection,
            marker_anchors,
            elapsed,
        )?) {
            if let Some(layer) = layer {
                batch[layer] = markers;
            }
        }
        if frame.recognized_result.is_none() && frame.target_selector.is_none() {
            let (left, right) = batch.split_at_mut(COMBO_TEXT);
            self.combos
                .draw(&art.font, &art.combo, &mut left[COMBO_PANEL], &mut right[0])?;
        }
        self.technical_smash
            .draw(&art.font, &mut batch[COMBO_TEXT])?;
        Ok(())
    }
}

struct EnemyPortrait<'a> {
    enemy: &'a EnemyHud,
    number: usize,
}

fn enemy_portraits<'a>(
    enemies: &'a [EnemyHud],
    frame: &BattleFrame,
    selected: Option<resonance_battle::ActorId>,
    stacked: bool,
) -> Vec<EnemyPortrait<'a>> {
    if stacked {
        let mut groups: std::collections::BTreeMap<_, EnemyPortrait<'_>> = Default::default();
        for enemy in enemies {
            let actor = &frame.actors[enemy.actor];
            if !actor.available() || actor.hp <= 0 {
                continue;
            }
            let portrait = groups
                .entry(enemy.group)
                .or_insert(EnemyPortrait { enemy, number: 0 });
            portrait.number += 1;
            if selected.is_some_and(|target| target.index() == enemy.actor) {
                portrait.enemy = enemy;
            }
        }
        return groups.into_values().collect();
    }
    let mut ordinals = std::collections::BTreeMap::new();
    enemies
        .iter()
        .map(|enemy| {
            let ordinal = ordinals.entry(enemy.group).or_default();
            *ordinal += 1;
            EnemyPortrait {
                enemy,
                number: *ordinal,
            }
        })
        .collect()
}

#[expect(
    clippy::too_many_arguments,
    reason = "Enemy markers borrow their artwork, frame, animation clock, and output batches independently."
)]
fn draw_enemies(
    enemies: &[EnemyHud],
    stacked: bool,
    icon_layers: &[Option<usize>],
    marker_layers: &[Option<usize>; TARGET_ORNAMENTS],
    selection: Option<(resonance_battle::ActorId, resonance_battle::ActorId)>,
    frame: &BattleFrame,
    font: &BitmapFont,
    motion_tick: u16,
    batch: &mut [Batch],
) -> Result<()> {
    if frame.recognized_result.is_some() {
        return Ok(());
    }
    let selection = selection.or_else(|| {
        let owner = frame.target_selector?;
        frame
            .targets
            .get(owner.index())?
            .map(|target| (owner, target))
    });
    let selected = selection.map(|(_, target)| target).or_else(|| {
        let owner = frame.actors.iter().position(|actor| {
            actor.side == Side::Party && actor.control_slot == resonance_battle::ControllerPort::One
        })?;
        frame.targets.get(owner).copied().flatten()
    });
    let portraits = enemy_portraits(enemies, frame, selected, stacked);
    let group_count = portraits
        .iter()
        .map(|portrait| portrait.enemy.group)
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let columns = if stacked || (group_count > 2 && portraits.len() <= 4) {
        ENEMY_COLUMNS
    } else {
        portraits.len().div_ceil(2)
    };
    let page = selected
        .and_then(|target| {
            portraits
                .iter()
                .position(|p| p.enemy.actor == target.index())
        })
        .unwrap_or(0)
        / ENEMY_PAGE_SIZE;
    if portraits.len() > ENEMY_PAGE_SIZE {
        text::glyphs(
            &mut batch[ENEMY_NUMBERS],
            font,
            &format!("{}/{}", page + 1, portraits.len().div_ceil(ENEMY_PAGE_SIZE)),
            [464., 370.],
            [12., 12.],
            [128, 128, 128, 255],
        )?;
    }
    let separate_rows = !stacked
        && group_count == 2
        && (0..icon_layers.len()).all(|group| {
            enemies
                .iter()
                .filter(|enemy| usize::from(enemy.group) == group)
                .count()
                <= ENEMY_COLUMNS
        });
    for (index, portrait) in portraits.iter().enumerate() {
        let enemy = portrait.enemy;
        let group = usize::from(enemy.group);
        let actor = &frame.actors[enemy.actor];
        if !actor.available()
            || actor.hp <= 0
            || (portraits.len() > ENEMY_PAGE_SIZE && index / ENEMY_PAGE_SIZE != page)
        {
            continue;
        }
        let (column, row) = if portraits.len() > ENEMY_PAGE_SIZE {
            let slot = index % ENEMY_PAGE_SIZE;
            (slot % ENEMY_COLUMNS, slot / ENEMY_COLUMNS)
        } else if separate_rows {
            (portrait.number - 1, group)
        } else {
            (index % columns, index / columns)
        };
        let x = 464. + column as f32 * 36.;
        let y = 388. + row as f32 * 36.;
        if let Some(layer) = icon_layers[group] {
            let mut rect = [x, y, x + 32., y + 32.];
            let mut uv = [0., 0., 32., 32.];
            if selected.is_some_and(|target| target.index() == enemy.actor) {
                let pulse =
                    (4. + 4. * (f32::from(motion_tick % 60) * 6.).to_radians().sin()) as u16;
                let size = 32. + f32::from(pulse);
                let shift = f32::from(pulse / 2);
                let (sine, cosine) = (f32::from(motion_tick % 72) * 5.).to_radians().sin_cos();
                let center = x - shift + size / 2. - sine / 2.;
                let half_width = size * cosine.abs() / 2.;
                rect = [
                    center - half_width,
                    y - shift,
                    center + half_width,
                    y - shift + size,
                ];
                if cosine < 0. {
                    uv = [32., 0., 0., 32.];
                }
            }
            batch[layer].quad(rect, uv, [1.; 4]);
        }
        let number = portrait.number.to_string();
        text::glyphs(
            &mut batch[ENEMY_NUMBERS],
            font,
            &number,
            [x + 34. - number.len() as f32 * 12., y + 20.],
            [12., 12.],
            [128, 128, 128, 255],
        )?;
        if let Some((owner, _)) = selection.filter(|(_, target)| target.index() == enemy.actor) {
            let owner = frame
                .actors
                .get(owner.index())
                .context("roster target owner is not in battle frame")?;
            let slot = owner.control_slot.index();
            let pointer = marker_layers[slot + 1];
            if let Some(layer) = marker_layers[0] {
                batch[layer].quad(
                    [x + 4., y - 16., x + 32., y + 20.],
                    [0., 256., 48., 320.],
                    hud_color([0, 0, 0, 128]),
                );
                batch[layer].quad(
                    [x + 2., y - 18., x + 30., y + 18.],
                    [0., 256., 48., 320.],
                    hud_color([128, 128, 128, 255]),
                );
            }
            if let Some(layer) = pointer {
                batch[layer].quad(
                    [x + 2., y - 18., x + 30., y + 18.],
                    [48., 256., 96., 320.],
                    hud_color([128, 128, 128, 255]),
                );
            }
        }
    }
    Ok(())
}

impl Artwork {
    pub(super) fn render_combat(
        &mut self,
        frame: &BattleFrame,
        command: Option<&resonance_game::battle::command::Frame>,
        marker_anchors: &[ActorMarkers],
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
    ) -> Result<()> {
        let mut batches: Vec<_> = (0..self.combat.layers.len())
            .map(|_| Batch::default())
            .collect();
        self.combat
            .draw(frame, &self.art, &mut batches, command, marker_anchors)?;
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
    fn combined_enemy_portraits_count_survivors_and_follow_individual_targets() -> Result<()> {
        use resonance_battle::{ActorAvailability, ActorId};

        let mut frame = fixtures::frame(fixtures::actor(Default::default()));
        frame.actors = vec![frame.actors[0].clone(); 14];
        // Interleave two enemy types, as when their formations are combined.
        let enemies = (0..14)
            .map(|actor| EnemyHud {
                actor,
                group: u8::from(actor % 7 == 1),
                name: String::new(),
                icon: None,
            })
            .collect::<Vec<_>>();
        let selected = Some(ActorId::from_index(13)?);
        let roster = |frame: &BattleFrame| {
            enemy_portraits(&enemies, frame, selected, true)
                .iter()
                .map(|p| (p.enemy.group, p.enemy.actor, p.number))
                .collect::<Vec<_>>()
        };
        assert_eq!(roster(&frame), [(0, 13, 12), (1, 1, 2)]);
        frame.actors[0].hp = 0;
        frame.actors[8].availability = ActorAvailability::Absent;
        assert_eq!(roster(&frame), [(0, 13, 11), (1, 1, 1)]);
        frame.actors[13].hp = 0;
        assert_eq!(roster(&frame), [(0, 2, 10), (1, 1, 1)]);
        frame.actors[1].hp = 0;
        assert_eq!(roster(&frame), [(0, 2, 10)]);
        frame.actors[0].hp = 100;
        assert_eq!(roster(&frame), [(0, 0, 11)]);
        for actor in &mut frame.actors {
            actor.hp = 0;
        }
        assert!(roster(&frame).is_empty());
        Ok(())
    }

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
            characters: &[Character::Lloyd],
            queued_techniques: &[false],
            names: &[Some("R%s")],
        };
        let combos = |hud: &mut Artwork, frame: &BattleFrame| -> Result<[Batch; 2]> {
            let mut batches = (0..hud.combat.layers.len())
                .map(|_| Batch::default())
                .collect::<Vec<_>>();
            hud.combat.draw(frame, &hud.art, &mut batches, None, &[])?;
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
        hud.notice_request(0, "Indignation Judgment", 12, &frame)?;
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
        frame.actors[1].hp = 400_000;
        frame.actors[1].equipment.max_hp = 400_000;
        frame.actors[1].tp = 999;
        frame.actors[1].equipment.max_tp = 999;
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
        hud.overlay_request(Request::EscapeNotice)?;
        assert!(
            hud.draw_notices(&frame, party)?
                .iter()
                .all(|batch| batch.indices.is_empty())
        );
        hud.overlay_request(Request::DefeatNotice)?;
        hud.notices.advance(&frame, 8);
        assert!(
            hud.draw_notices(&frame, party)?
                .iter()
                .any(|batch| !batch.indices.is_empty())
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires the battle font; CPU enemy HUD drawing only"]
    fn enemy_icons_keep_their_slots_when_an_enemy_is_defeated() -> Result<()> {
        use resonance_battle::ActorId;

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
        let owner = ActorId::from_index(frame.actors.len())?;
        let mut party = frame.actors[0].clone();
        party.control_slot = resonance_battle::ControllerPort::Three;
        frame.actors.push(party);
        frame.targets = vec![None; frame.actors.len()];
        let draw = |frame: &BattleFrame, selection| -> Result<[Batch; 8]> {
            let mut batches = std::array::from_fn(|_| Batch::default());
            draw_enemies(
                &enemies,
                false,
                &[Some(1), Some(2), None, None],
                &[Some(3), Some(4), Some(5), Some(6), Some(7)],
                selection,
                frame,
                &font,
                0,
                &mut batches,
            )?;
            Ok(batches)
        };
        let all = draw(&frame, None)?;
        assert_eq!(all[1].positions.len(), 8);
        assert_eq!(all[2].positions.len(), 4);
        assert!(all[3..].iter().all(|batch| batch.indices.is_empty()));
        frame.target_selector = Some(owner);
        frame.targets[owner.index()] = Some(ActorId::from_index(1)?);
        let selected = draw(&frame, None)?;
        assert_eq!(selected[3].positions.len(), 8, "frame and shadow");
        assert_eq!(selected[6].positions.len(), 4, "controller three pointer");
        assert_eq!(selected[3].positions[4..], selected[6].positions);
        assert!(selected[0] == all[0], "selection retains the ordinals");
        assert_eq!(selected[1].positions[..4], all[1].positions[..4]);
        assert!(
            selected[1].positions[4..] != all[1].positions[4..],
            "selected thumbnail pulses"
        );
        frame.target_selector = None;
        assert!(
            draw(&frame, None)?[3..]
                .iter()
                .all(|batch| batch.indices.is_empty()),
            "release hides the ornament"
        );
        let recipient = Some((owner, ActorId::from_index(2)?));
        let command = draw(&frame, recipient)?;
        assert!(command[6].positions != selected[6].positions);
        assert!(command[0] == all[0]);
        assert!(command[2].positions != all[2].positions);
        let hp = frame.actors[2].hp;
        frame.actors[2].hp = 0;
        assert!(
            draw(&frame, recipient)?[3..]
                .iter()
                .all(|batch| batch.indices.is_empty()),
            "a defeated recipient has no target ornament"
        );
        frame.actors[2].hp = hp;
        frame.actors[0].hp = 0;
        let remaining = draw(&frame, None)?;
        assert_eq!(remaining[1].positions, all[1].positions[4..]);
        assert_eq!(remaining[2].positions, all[2].positions);
        frame.recognized_result = Some(resonance_battle::BattleResult::Victory);
        assert!(
            draw(&frame, recipient)?
                .iter()
                .all(|batch| batch.indices.is_empty())
        );
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
            draw(&frame)?[3..6]
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
