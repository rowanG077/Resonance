//! Page, text-reveal, and input behavior shared by live UI and silent records.
use anyhow::{Result, ensure};
use resonance_events::{
    Operation,
    dialogue::{Dialogue, ResolvedMessage, TextToken, flags},
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const OPENING_STEPS: u8 = 5;
const CLOSE_DELAY: u8 = 3;
const GLYPH_ALPHA_STEP: u8 = 32;

/// Playback creates a distinct completion token for each requested line.
/// Logic-only tests can retain duration-based progression without a device.
pub trait VoiceFeedback: Send + Sync {
    fn begin(&self, resource: u32) -> Arc<AtomicBool>;
}

#[derive(Debug, Clone)]
pub struct Glyph {
    pub character: char,
    pub color: [u8; 3],
    pub delay: u16,
    pub followed_by_control: bool,
}
impl Glyph {
    /// Single-byte box sizing samples the next encoded character, including controls.
    pub fn measured_character(&self, next: Option<char>) -> char {
        if !resonance_content::font::is_single_byte(self.character) {
            self.character
        } else if self.followed_by_control {
            ' '
        } else {
            next.filter(|&character| character > ' ').unwrap_or(' ')
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceAction {
    Play(u32),
    Stop,
}
#[derive(Debug, Clone, Default)]
pub struct Page {
    pub glyphs: Vec<Glyph>,
    pub voices: Vec<(usize, VoiceAction)>,
}
impl Page {
    pub fn text(&self) -> String {
        self.glyphs.iter().map(|g| g.character).collect()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowPhase {
    Opening(u8),
    Text,
    Closing(u8),
    Closed,
}
pub struct DialoguePlayer {
    pub operation: Operation,
    pub pages: Vec<Page>,
    pub page: usize,
    pub visible: usize,
    pub closed: bool,
    pub persistent: bool,
    phase: WindowPhase,
    auto_pages: bool,
    delay: u16,
    voice_cursor: usize,
    voice_remaining: u32,
    voice_feedback: Option<Arc<dyn VoiceFeedback>>,
    voice_completion: Option<Arc<AtomicBool>>,
    voice_durations: Arc<BTreeMap<u32, u32>>,
    glyph_alpha: Vec<u8>,
    instant_glyphs: bool,
}
impl DialoguePlayer {
    /// Scripts can release a persistent notice after its text has appeared.
    pub fn sync_flags(&mut self, dialogue: &Dialogue) {
        self.persistent = dialogue.persistent();
        self.auto_pages = dialogue.flags & flags::AUTO_PAGES != 0;
    }
    pub fn fully_revealed(&self) -> bool {
        self.visible == self.current().glyphs.len()
    }
    pub fn new(dialogue: &Dialogue, default_delay: u16) -> Result<Self> {
        Ok(Self {
            operation: dialogue.operation.clone(),
            pages: pages(&dialogue.body, default_delay)?,
            page: 0,
            visible: 0,
            closed: false,
            persistent: dialogue.persistent(),
            phase: if dialogue.flags & flags::INSTANT != 0 {
                WindowPhase::Text
            } else {
                WindowPhase::Opening(0)
            },
            auto_pages: dialogue.flags & flags::AUTO_PAGES != 0,
            delay: 0,
            voice_cursor: 0,
            voice_remaining: 0,
            voice_feedback: None,
            voice_completion: None,
            voice_durations: Default::default(),
            glyph_alpha: Vec::new(),
            instant_glyphs: dialogue.flags & flags::INSTANT != 0,
        })
    }
    pub fn current(&self) -> &Page {
        &self.pages[self.page]
    }
    pub fn with_voice_durations(mut self, durations: Arc<BTreeMap<u32, u32>>) -> Self {
        self.voice_durations = durations;
        self
    }
    pub fn with_voice_feedback(mut self, feedback: Option<Arc<dyn VoiceFeedback>>) -> Self {
        self.voice_feedback = feedback;
        self
    }
    pub fn voice_finished(&self) -> bool {
        self.voice_completion
            .as_ref()
            .map_or(self.voice_remaining == 0, |token| {
                token.load(Ordering::Acquire)
            })
    }
    /// Expand over five samples before revealing text; instant dialogue skips expansion.
    pub fn opening_fraction(&self) -> Option<f32> {
        match self.phase {
            WindowPhase::Opening(step) => {
                Some(f32::from(step.saturating_sub(1)) / f32::from(OPENING_STEPS - 1))
            }
            _ => None,
        }
    }
    pub fn window_visible(&self) -> bool {
        matches!(
            self.phase,
            WindowPhase::Opening(_) | WindowPhase::Text | WindowPhase::Closing(0)
        )
    }
    /// Selection and ordinary confirmation share the same window retirement.
    pub fn close(&mut self) {
        self.phase = WindowPhase::Closing(0);
    }
    pub fn accepts_input(&self) -> bool {
        self.phase == WindowPhase::Text
            && (self.visible < self.current().glyphs.len()
                || self
                    .current()
                    .glyphs
                    .iter()
                    .zip(&self.glyph_alpha)
                    .rfind(|(glyph, _)| glyph.character != '\n')
                    // Input sees the opacity update after drawing; stored glyph
                    // opacity is the value presented during this update.
                    .is_none_or(|(_, alpha)| alpha.saturating_add(GLYPH_ALPHA_STEP) == 255))
    }
    /// Glyph opacity continues rising after text readiness, so a choice can appear
    /// before the final glyph becomes opaque enough to dismiss.
    pub fn glyph_alpha(&self, index: usize) -> u8 {
        self.glyph_alpha.get(index).copied().unwrap_or(0)
    }
    /// Move the speaker’s mouth through text reveal and until its voice finishes.
    pub fn is_talking(&self) -> bool {
        self.phase == WindowPhase::Text
            && self.operation.is_pending()
            && (self.visible < self.current().glyphs.len() || !self.voice_finished())
    }
    /// A press advances a revealed page; holding accept accelerates its text.
    pub fn step(&mut self, advance: bool, accelerate: bool) -> Result<Vec<VoiceAction>> {
        if self.closed {
            return Ok(Vec::new());
        }
        self.voice_remaining = self.voice_remaining.saturating_sub(1);
        if self.operation.progress().outcome.is_some() {
            self.closed = true;
            self.phase = WindowPhase::Closed;
            return Ok(vec![VoiceAction::Stop]);
        }
        match self.phase {
            WindowPhase::Opening(step) if step < OPENING_STEPS => {
                self.phase = WindowPhase::Opening(step + 1);
                return Ok(Vec::new());
            }
            WindowPhase::Opening(_) => self.phase = WindowPhase::Text,
            WindowPhase::Closing(step) => {
                // Hold completion until the VM has observed window retirement.
                // Closing hides the window without shrinking it.
                if step == CLOSE_DELAY {
                    self.phase = WindowPhase::Closed;
                    self.closed = true;
                    self.operation.complete(None).map_err(anyhow::Error::msg)?;
                } else {
                    self.phase = WindowPhase::Closing(step + 1);
                }
                return Ok(Vec::new());
            }
            WindowPhase::Text | WindowPhase::Closed => {}
        }
        for alpha in &mut self.glyph_alpha {
            *alpha = alpha.saturating_add(GLYPH_ALPHA_STEP);
        }
        let advance = advance && !self.persistent && self.accepts_input();
        let next_page =
            self.auto_pages && self.page + 1 < self.pages.len() && self.voice_finished();
        if (advance || next_page) && self.fully_revealed() {
            if self.page + 1 == self.pages.len() {
                self.close();
                return Ok(vec![VoiceAction::Stop]);
            }
            self.page += 1;
            self.visible = 0;
            self.glyph_alpha.clear();
            self.delay = 0;
            self.voice_cursor = 0;
        } else {
            while self.visible < self.current().glyphs.len() {
                let glyph = &self.current().glyphs[self.visible];
                if glyph.character == '\n' {
                    self.visible += 1;
                    continue;
                }
                if self.delay > 0 {
                    self.delay -= 1;
                    break;
                }
                self.delay = if accelerate && !self.auto_pages && glyph.delay > 0 {
                    1
                } else {
                    glyph.delay
                };
                self.visible += 1;
            }
        }
        for glyph in &self.pages[self.page].glyphs[self.glyph_alpha.len()..self.visible] {
            self.glyph_alpha
                .push(if self.instant_glyphs || glyph.character == '\n' {
                    255
                } else {
                    0
                });
        }
        let mut voices = Vec::new();
        while let Some(&(position, action)) = self.current().voices.get(self.voice_cursor) {
            if position > self.visible {
                break;
            }
            voices.push(action);
            self.voice_completion = match action {
                VoiceAction::Play(id) => self
                    .voice_feedback
                    .as_ref()
                    .map(|feedback| feedback.begin(id)),
                VoiceAction::Stop => None,
            };
            self.voice_remaining = match action {
                VoiceAction::Play(id) => self.voice_durations.get(&id).copied().unwrap_or(0),
                VoiceAction::Stop => 0,
            };
            self.voice_cursor += 1;
        }
        // Publish readiness only after the final page and voice finish, so the next
        // speaker cannot replace a line that is still being revealed or spoken.
        if self.page + 1 == self.pages.len() && self.fully_revealed() && self.voice_finished() {
            self.operation
                .advance(self.page as u32 + 1)
                .map_err(anyhow::Error::msg)?;
        }
        Ok(voices)
    }
}

/// Advance scene-owned notices through the ordinary text reveal and close rules.
pub fn step_requests(
    world: &mut resonance_events::GameWorld,
    players: &mut std::collections::BTreeMap<u8, DialoguePlayer>,
    confirm: bool,
    accelerate: bool,
) -> Result<()> {
    players.retain(|slot, p| {
        world
            .dialogue
            .get(slot)
            .is_some_and(|d| d.operation.id() == p.operation.id())
    });
    for (&slot, request) in &world.dialogue {
        if request.operation.is_pending() && !players.contains_key(&slot) {
            players.insert(
                slot,
                DialoguePlayer::new(
                    request,
                    world
                        .party
                        .as_ref()
                        .map_or(3, |p| u16::from(p.settings.preferences.message_speed)),
                )?,
            );
        }
        if let Some(player) = players.get_mut(&slot) {
            player.sync_flags(request);
        }
    }
    let focus = players
        .iter()
        .find(|(_, p)| !p.closed && !p.persistent && p.operation.is_pending())
        .map(|(&slot, _)| slot);
    for (&slot, player) in players.iter_mut() {
        for voice in player.step(
            confirm && focus == Some(slot),
            accelerate && focus == Some(slot),
        )? {
            world.audio_commands.push(match voice {
                VoiceAction::Play(id) => resonance_events::AudioCommand::Voice(id),
                VoiceAction::Stop => resonance_events::AudioCommand::StopVoice,
            });
        }
    }
    Ok(())
}
pub fn pages(message: &ResolvedMessage, default_delay: u16) -> Result<Vec<Page>> {
    ensure!(default_delay <= 120, "text delay exceeds supported range");
    let mut pages = vec![Page::default()];
    let mut color = [255; 3];
    let mut delay = default_delay;
    let mut count = 0;
    for token in &message.tokens {
        if matches!(token, TextToken::Control { .. })
            && let Some(glyph) = pages.last_mut().unwrap().glyphs.last_mut()
        {
            glyph.followed_by_control = true;
        }
        match token {
            TextToken::Text { text } => {
                for character in text.chars() {
                    if character == '\u{c}' {
                        ensure!(pages.len() < 80, "too many dialogue pages");
                        pages.push(Page::default());
                    } else {
                        count += 1;
                        ensure!(count <= 8192, "dialogue is too long");
                        pages.last_mut().unwrap().glyphs.push(Glyph {
                            character,
                            color,
                            delay: if character == '\n' { 0 } else { delay },
                            followed_by_control: false,
                        });
                    }
                }
            }
            TextToken::Control { opcode, value } => match opcode {
                2 => {
                    delay = if *value < 0 {
                        default_delay
                    } else {
                        u16::try_from(*value)
                            .ok()
                            .filter(|d| *d <= 120)
                            .ok_or_else(|| anyhow::anyhow!("unsupported text delay"))?
                    }
                }
                3 => {
                    color = match value {
                        0 => [255; 3],
                        1 => [0, 0, 255],
                        2 => [255, 0, 0],
                        3 => [159, 64, 206],
                        4 => [52, 255, 17],
                        5 => [20, 250, 250],
                        6 => [235, 255, 17],
                        _ => anyhow::bail!("unsupported text color"),
                    }
                }
                9 => {
                    let page = pages.last_mut().unwrap();
                    let voice = if *value == -1 {
                        VoiceAction::Stop
                    } else {
                        VoiceAction::Play(u32::try_from(*value)?)
                    };
                    page.voices.push((page.glyphs.len(), voice));
                }
                7 => {} // Original text renderer skips this parameter.
                _ => anyhow::bail!("dialogue control {opcode:#x} requires a UI service"),
            },
        }
    }
    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn player(text: &str, flags: u16) -> (DialoguePlayer, resonance_events::EventRuntime) {
        let bytes = [4u16, 0, 0, 0, 1, 0x3000, 0x4000, 0x2054, 0x20ff]
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>();
        let resources = resonance_events::ResourceLibrary {
            movies: [1].into(),
            ..Default::default()
        };
        let events = resonance_events::EventRuntime::new(
            Arc::new(symphonia_script::Program::decode(&bytes).unwrap()),
            Arc::new(resources),
        )
        .unwrap();
        let dialogue = Dialogue {
            operation: events.world.movie.as_ref().unwrap().operation.clone(),
            speaker: ResolvedMessage { tokens: vec![] },
            body: ResolvedMessage {
                tokens: vec![
                    TextToken::Control {
                        opcode: 9,
                        value: 7,
                    },
                    TextToken::Text { text: text.into() },
                ],
            },
            anchor: resonance_events::dialogue::DialogueAnchor::Screen([0., 0.]),
            speaker_actor: None,
            opening_actor: None,
            flags,
            dimensions: None,
            height_offset: 0,
        };
        (DialoguePlayer::new(&dialogue, 1).unwrap(), events)
    }
    #[test]
    fn attached_window_expands_before_speech_and_finishes_before_vm_resume() {
        let (mut player, _scope) = player("AB", 0);
        for fraction in [0., 0.25, 0.5, 0.75, 1.] {
            assert!(player.step(true, false).unwrap().is_empty());
            assert_eq!(player.opening_fraction(), Some(fraction));
            assert_eq!(player.visible, 0);
            assert!(!player.is_talking());
            assert!(!player.accepts_input());
        }
        assert_eq!(player.step(false, false).unwrap(), [VoiceAction::Play(7)]);
        assert_eq!(player.visible, 1);
        assert!(player.is_talking());
        player.step(false, false).unwrap();
        for _ in 0..8 {
            player.step(false, false).unwrap();
        }
        assert_eq!(player.step(true, false).unwrap(), [VoiceAction::Stop]);
        assert!(
            player.window_visible(),
            "dismissal update still draws the page"
        );
        assert!(!player.is_talking());
        for _ in 0..3 {
            player.step(true, false).unwrap();
            assert!(!player.window_visible());
            assert!(!player.closed);
            assert!(player.operation.is_pending());
        }
        player.step(false, false).unwrap();
        assert!(player.closed);
        assert!(player.operation.progress().outcome.is_some());
    }
    #[test]
    fn final_glyph_accepts_confirm_on_its_last_fade_update() {
        let (mut player, _scope) = player("A", 0);
        for _ in 0..6 {
            player.step(false, false).unwrap();
        }
        assert_eq!(player.visible, 1);
        assert!(player.operation.progress().ready);
        assert_eq!(player.glyph_alpha(0), 0);
        for alpha in [32, 64, 96, 128, 160, 192] {
            player.step(true, false).unwrap();
            assert_eq!(player.glyph_alpha(0), alpha);
            assert!(player.window_visible());
            assert!(!player.accepts_input());
        }
        assert_eq!(player.step(true, false).unwrap(), [VoiceAction::Stop]);
        assert_eq!(player.glyph_alpha(0), 224);
        assert!(player.window_visible());
        assert!(!player.is_talking());
    }
    #[test]
    fn speaker_moves_mouth_through_reveal_and_voice_then_stops() {
        let (player, _scope) = player("AB", flags::INSTANT);
        let mut player = player.with_voice_durations(Arc::new([(7, 10)].into()));
        assert!(player.is_talking());
        for _ in 0..10 {
            player.step(false, false).unwrap();
            assert!(player.is_talking(), "voice outlasts text reveal");
        }
        player.step(false, false).unwrap();
        assert!(!player.is_talking());
        assert!(
            !player.closed,
            "mouth closes while the finished page stays open"
        );
    }

    #[test]
    fn ready_waits_for_final_page_and_spoken_line_to_finish() {
        let (player, _scope) = player("AB\u{c}CD", flags::INSTANT | flags::AUTO_PAGES);
        let mut player = player.with_voice_durations(Arc::new([(7, 10)].into()));
        player.step(false, false).unwrap();
        assert!(
            !player.operation.progress().ready,
            "opening a window is not readiness"
        );
        for _ in 0..9 {
            player.step(false, false).unwrap();
        }
        assert_eq!(player.page, 0, "automatic page must wait for its voice");
        assert!(!player.operation.progress().ready);
        for _ in 0..4 {
            player.step(false, false).unwrap();
        }
        assert_eq!(player.page, 1);
        assert!(player.operation.progress().ready);
        assert!(
            !player.closed,
            "persistent text remains until its script closes it"
        );
    }
    #[test]
    fn confirm_waits_for_revealed_text_then_can_interrupt_an_unfinished_voice() {
        let (player, _scope) = player("ABCDE\u{c}F", 0);
        let mut player = player.with_voice_durations(Arc::new([(7, 100)].into()));
        for _ in 0..6 {
            player.step(false, false).unwrap();
        }
        assert_eq!(player.visible, 1);
        assert!(player.step(true, true).unwrap().is_empty());
        assert_eq!((player.page, player.visible), (0, 2));
        assert_eq!(player.glyph_alpha(1), 0);
        while !player.fully_revealed() {
            player.step(false, true).unwrap();
        }
        assert!(!player.operation.progress().ready);
        for _ in 0..6 {
            assert!(player.step(true, true).unwrap().is_empty());
            assert_eq!(player.page, 0, "confirmation waits for glyph opacity");
        }
        assert!(!player.voice_finished());
        assert!(player.is_talking());
        player.step(false, true).unwrap();
        assert_eq!(player.page, 0, "holding confirm must not advance the page");
        player.step(true, true).unwrap();
        assert_eq!((player.page, player.visible), (1, 0));
        player.step(true, false).unwrap();
        assert_eq!(player.visible, 1);
        assert!(!player.voice_finished());
        for _ in 0..6 {
            assert!(player.step(true, false).unwrap().is_empty());
            assert_eq!(player.phase, WindowPhase::Text);
        }
        assert!(
            !player.operation.progress().ready,
            "voice still owns readiness"
        );
        assert_eq!(player.step(true, false).unwrap(), [VoiceAction::Stop]);
        assert!(!player.is_talking());
        for _ in 0..4 {
            player.step(false, false).unwrap();
        }
        assert!(player.closed);
        assert!(player.operation.progress().outcome.is_some());
    }
    #[test]
    fn held_accept_accelerates_new_glyphs_without_skipping_a_pending_delay() {
        let (mut player, _scope) = player("ABCDE\u{c}F", flags::INSTANT);
        for glyph in &mut player.pages[0].glyphs {
            glyph.delay = 3;
        }
        player.step(false, false).unwrap();
        assert_eq!((player.visible, player.delay), (1, 2));
        player.step(false, true).unwrap();
        assert_eq!((player.visible, player.delay), (1, 1));
        player.step(false, true).unwrap();
        assert_eq!((player.visible, player.delay), (1, 0));
        player.step(false, true).unwrap();
        assert_eq!((player.visible, player.delay), (2, 0));
        player.step(false, false).unwrap();
        assert_eq!((player.visible, player.delay), (3, 2));
        while !player.fully_revealed() {
            player.step(false, true).unwrap();
        }
        player.step(false, true).unwrap();
        assert_eq!(player.page, 0, "holding accept never advances a page");
        player.step(true, false).unwrap();
        assert_eq!(player.page, 1);
    }
    #[test]
    fn automatic_pages_keep_authored_glyph_delays_while_accept_is_held() {
        let (mut player, _scope) = player("AB", flags::INSTANT | flags::AUTO_PAGES);
        player.pages[0].glyphs[0].delay = 3;
        player.step(true, true).unwrap();
        assert_eq!((player.visible, player.delay), (1, 2));
        player.step(true, true).unwrap();
        assert_eq!((player.visible, player.delay), (1, 1));
    }
    #[test]
    fn page_breaks_preserve_voice_boundaries_color_and_speed() {
        let message = ResolvedMessage {
            tokens: vec![
                TextToken::Control {
                    opcode: 9,
                    value: 0xa0000,
                },
                TextToken::Text {
                    text: "Hello\nLloyd!\u{c}".into(),
                },
                TextToken::Control {
                    opcode: 3,
                    value: 2,
                },
                TextToken::Control {
                    opcode: 2,
                    value: 0,
                },
                TextToken::Control {
                    opcode: 9,
                    value: 0xa0001,
                },
                TextToken::Text {
                    text: "Wake".into(),
                },
                TextToken::Control {
                    opcode: 3,
                    value: 2,
                },
                TextToken::Text {
                    text: " up!".into(),
                },
            ],
        };
        let pages = pages(&message, 2).unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].voices, [(0, VoiceAction::Play(0xa0000))]);
        assert_eq!(pages[1].voices, [(0, VoiceAction::Play(0xa0001))]);
        assert_eq!(pages[1].glyphs[0].color, [255, 0, 0]);
        assert_eq!(pages[1].glyphs[0].delay, 0);
        assert_eq!(pages[1].text(), "Wake up!");
        assert_eq!(pages[1].glyphs[3].color, pages[1].glyphs[4].color);
        assert!(pages[1].glyphs[3].followed_by_control);
        assert_eq!(pages[1].glyphs[3].measured_character(Some('X')), ' ');
        assert_eq!(pages[1].glyphs[2].measured_character(Some('X')), 'X');
        assert_eq!(pages[1].glyphs[2].measured_character(Some('\n')), ' ');
        let mut kana = pages[1].glyphs[2].clone();
        for (character, measured) in [('ｵ', 'X'), ('･', 'X'), ('オ', 'オ')] {
            kana.character = character;
            assert_eq!(kana.measured_character(Some('X')), measured);
        }
        assert_eq!(
            pages[0]
                .glyphs
                .iter()
                .find(|g| g.character == '\n')
                .unwrap()
                .delay,
            0
        );
        let (mut player, _scope) = player("\nA\n\nB\n", 0);
        for glyph in &mut player.pages[0].glyphs {
            if glyph.character != '\n' {
                glyph.delay = 3;
            }
        }
        for _ in 0..6 {
            player.step(false, false).unwrap();
        }
        assert_eq!(
            player.visible, 4,
            "line breaks parse with the preceding glyph"
        );
        for _ in 0..2 {
            player.step(false, false).unwrap();
            assert_eq!(
                player.visible, 4,
                "line breaks preserve the pending glyph delay"
            );
        }
        player.step(false, false).unwrap();
        assert!(player.fully_revealed());
        assert_eq!(player.glyph_alpha(4), 0);
        assert_eq!(player.glyph_alpha(5), 255);
        for _ in 0..6 {
            player.step(true, false).unwrap();
            assert_eq!(player.phase, WindowPhase::Text);
        }
        player.step(true, false).unwrap();
        assert_eq!(player.glyph_alpha(4), 224);
        assert_eq!(player.phase, WindowPhase::Closing(0));
    }
    #[test]
    fn playback_acknowledgement_outlives_nominal_voice_duration() {
        struct Feedback(Arc<AtomicBool>);
        impl VoiceFeedback for Feedback {
            fn begin(&self, resource: u32) -> Arc<AtomicBool> {
                assert_eq!(resource, 7);
                self.0.clone()
            }
        }
        let completed = Arc::new(AtomicBool::new(false));
        let (player, _scope) = player("AB", flags::INSTANT);
        let mut player = player
            .with_voice_durations(Arc::new([(7, 2)].into()))
            .with_voice_feedback(Some(Arc::new(Feedback(completed.clone()))));
        for _ in 0..100 {
            player.step(false, false).unwrap();
        }
        assert!(!player.voice_finished());
        assert!(!player.operation.progress().ready);
        assert!(player.is_talking());
        completed.store(true, Ordering::Release);
        player.step(false, false).unwrap();
        assert!(player.voice_finished());
        assert!(player.operation.progress().ready);
        assert!(!player.is_talking());
    }
}
