#[derive(bevy::prelude::Resource, Clone, Default)]
pub(crate) struct Diagnostics(pub resonance_content::diagnostics::Diagnostics);

pub(crate) fn policy(world: &bevy::prelude::World) -> resonance_content::diagnostics::Diagnostics {
    world.get_resource::<Diagnostics>().map_or_else(
        || resonance_content::diagnostics::Diagnostics::new(true),
        |diagnostics| diagnostics.0.clone(),
    )
}

use bevy::prelude::*;
use std::{
    collections::VecDeque,
    fmt::{self, Write as _},
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, RwLock},
    time::{Duration, Instant},
};

static CRASH_CONTEXT: OnceLock<RwLock<CrashContext>> = OnceLock::new();
const STARTUP: &str = "No game snapshot yet (startup), or snapshot is being updated";

/// Retained outside the app so its original error and backtrace survive world teardown.
#[derive(Resource, Clone, Default)]
pub(super) struct FatalErrors(Arc<Mutex<Option<anyhow::Error>>>);

impl FatalErrors {
    fn record(&self, scope: &str, error: anyhow::Error) {
        let error = report_error(scope, error);
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get_or_insert(error);
    }

    pub(super) fn finish(&self, result: anyhow::Result<()>) -> anyhow::Result<()> {
        if let Some(error) = self
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            return Err(error);
        }
        result
    }
}

fn report_error(scope: &str, error: anyhow::Error) -> anyhow::Error {
    let error = error.context(scope.to_owned());
    remember_failure("fatal", &format_args!("{error:#}"));
    error!("{error:#}");
    error
}

/// A fatal system failure always records its cause before requesting shutdown.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Fatal<'w> {
    errors: Option<Res<'w, FatalErrors>>,
    exit: MessageWriter<'w, AppExit>,
}

impl Fatal<'_> {
    pub(super) fn fail(&mut self, scope: &str, error: anyhow::Error) {
        if let Some(errors) = &self.errors {
            errors.record(scope, error);
        } else {
            // Small standalone tools/tests can install systems without the game app.
            let _ = report_error(scope, error);
        }
        self.exit.write(AppExit::error());
    }

    pub(super) fn success(&mut self) {
        self.exit.write(AppExit::Success);
    }
}

pub(super) fn fatal(world: &mut World, scope: &str, error: anyhow::Error) {
    world.init_resource::<FatalErrors>();
    world.resource::<FatalErrors>().record(scope, error);
    record(world, true);
    world.write_message(AppExit::error());
}

/// Stop formatters at the byte budget instead of allocating an entire VM or
/// content inventory and truncating it afterward. All output stays valid UTF-8.
struct Text {
    value: String,
    limit: usize,
    truncated: bool,
}

impl Text {
    fn new(limit: usize) -> Self {
        Self {
            value: String::new(),
            limit,
            truncated: false,
        }
    }
    fn format(limit: usize, args: fmt::Arguments<'_>) -> String {
        let mut text = Self::new(limit);
        let _ = text.write_fmt(args);
        text.value
    }
}

impl fmt::Write for Text {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if self.truncated {
            return Err(fmt::Error);
        }
        if text.len() <= self.limit - self.value.len() {
            self.value.push_str(text);
            return Ok(());
        }
        const MARKER: &str = "\n[context truncated]";
        let budget = self.limit.saturating_sub(MARKER.len());
        let mut end = self.value.len().min(budget);
        while !self.value.is_char_boundary(end) {
            end -= 1;
        }
        self.value.truncate(end);
        let mut end = text.len().min(budget - self.value.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        self.value.push_str(&text[..end]);
        self.value.push_str(MARKER);
        self.truncated = true;
        Err(fmt::Error)
    }
}

#[derive(Default)]
struct CrashContext {
    scene: String,
    failures: VecDeque<String>,
    file: Option<PathBuf>,
}

impl CrashContext {
    fn record(&mut self, scene: String) -> String {
        self.scene = scene;
        let snapshot = self.snapshot();
        let _ = self.publish(&snapshot);
        snapshot
    }

    fn snapshot(&self) -> String {
        // Put recent failures first so large scene inventories cannot hide them.
        let mut snapshot = Text::new(16 * 1024);
        for failure in self.failures.iter().rev() {
            let _ = writeln!(snapshot, "Recent failure: {failure}");
        }
        let _ = snapshot.write_str(if self.scene.is_empty() {
            STARTUP
        } else {
            &self.scene
        });
        snapshot.value
    }

    fn remember_failure(&mut self, failure: String) {
        let failure = Text::format(1024, format_args!("{failure}"));
        if self.failures.back() == Some(&failure) {
            return;
        }
        if self.failures.len() == 8 {
            self.failures.pop_front();
        }
        self.failures.push_back(failure);
        let _ = self.publish(&self.snapshot());
    }

    fn publish(&self, snapshot: &str) -> std::io::Result<()> {
        let Some(path) = &self.file else {
            return Ok(());
        };
        // Serialize publications under the context lock and replace atomically.
        // The monitor always reads a complete snapshot, without game locks.
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
        file.write_all(snapshot.as_bytes())?;
        file.persist(path).map_err(|error| error.error)?;
        Ok(())
    }
}

fn context() -> &'static RwLock<CrashContext> {
    CRASH_CONTEXT.get_or_init(|| RwLock::new(CrashContext::default()))
}

pub(super) fn remember_failure(scope: &str, error: &dyn std::fmt::Display) {
    let failure = Text::format(1024, format_args!("{scope}: {error}"));
    context()
        .write()
        .unwrap_or_else(|error| error.into_inner())
        .remember_failure(failure);
}

/// Publish the latest completed snapshot for the separate native crash monitor.
/// The path belongs to its private temporary directory, removed on normal exit.
pub fn write_crash_context_to(path: PathBuf) -> std::io::Result<()> {
    let mut state = context().write().unwrap_or_else(|error| error.into_inner());
    state.file = Some(path);
    state.publish(&state.snapshot())
}

/// Copied ahead of time, never inspect Bevy worlds from a panic/signal handler.
/// A render or preparation thread can panic while the main world is borrowed.
pub fn crash_context() -> String {
    context()
        .try_read()
        .map_or_else(|_| STARTUP.into(), |state| state.snapshot())
}

#[derive(Resource)]
struct SnapshotClock {
    sample: Instant,
    log: Instant,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<FatalErrors>();
    let now = Instant::now();
    app.insert_resource(SnapshotClock {
        sample: now - Duration::from_secs(1),
        log: now - Duration::from_secs(30),
    });
    app.add_systems(First, sample).add_systems(Last, sample);
}

fn sample(world: &mut World) {
    let now = Instant::now();
    let mut timer = world.resource_mut::<SnapshotClock>();
    if now.duration_since(timer.sample) < Duration::from_secs(1) {
        return;
    }
    timer.sample = now;
    let log = now.duration_since(timer.log) >= Duration::from_secs(30);
    if log {
        timer.log = now;
    }
    record(world, log);
}

/// Force a breadcrumb at a publication or recovery boundary.
fn describe(world: &World) -> String {
    let mut snapshot = Text::new(16 * 1024);
    let _ = writeln!(snapshot, "Sampled: {}", time::OffsetDateTime::now_utc());
    let mut owners = None;
    if let Some(clock) = world.get_resource::<crate::Clock>() {
        let _ = writeln!(snapshot, "Presentation tick: {}", clock.0.tick());
    }
    if let Some(session) = world.get_resource::<crate::new_game::Session>() {
        let events = session.events();
        owners = Some(events);
        let player = events.world.actors.get(&events.world.controlled_actor);
        let _ = writeln!(
            snapshot,
            "Actors: {}; controlled actor: {}; position: {:?}; party: {:?}",
            events.world.actors.len(),
            events.world.controlled_actor,
            player.map(|actor| actor.position),
            events.world.party.as_ref().map(|party| &party.formation)
        );
        let _ = writeln!(
            snapshot,
            "Scene: {}; map: {}; event tick: {}; story: {:?}; player control: {}; ready: {}\nMenu: {:?}\nTransitions: field={:?}, world={:?}",
            if session.is_field() {
                "field"
            } else {
                "overworld"
            },
            session.map_id(),
            events.tick(),
            events.memory().read(0x40, symphonia_script::Width::S32),
            events.player_has_control(),
            session.ready_for_field,
            session.menu().map(|menu| menu.page),
            events.world.field_transition,
            events.world.world_transition
        );
    } else if let Some(events) = world.get_resource::<crate::Events>() {
        let _ = writeln!(snapshot, "Scene: title; tick: {}", events.0.tick());
        owners = Some(&events.0);
    } else {
        let _ = snapshot.write_str("Scene: starting or returning to title\n");
    }
    if let Some(battle) = world.get_resource::<crate::battle::Owner>() {
        let _ = battle.write_crash_context(&mut snapshot);
    }
    if let Some(movie) = world.get_resource::<crate::movie::Playback>() {
        let _ = writeln!(
            snapshot,
            "Movie: resource={:?}, active={}, frame={:?}, timestamp={:?}, dropped={}",
            movie.resource,
            movie.active,
            movie.presented_frame,
            movie.presented_timestamp,
            movie.dropped_frames
        );
    }
    if let Some(resident) = world.get_resource::<crate::loading::Resident>() {
        use std::sync::atomic::Ordering;
        let _ = writeln!(
            snapshot,
            "Loading: active={}, battle={}, memory reads={}, unprepared reads={}",
            resident.active.load(Ordering::Relaxed),
            resident.battle.load(Ordering::Relaxed),
            resident.memory_reads.load(Ordering::Relaxed),
            resident.unprepared_reads.load(Ordering::Relaxed)
        );
    }
    if let Some(diagnostics) = world.get_resource::<Diagnostics>() {
        let mut errors = Text::new(4 * 1024);
        let _ = diagnostics.0.write_summary(&mut errors);
        let _ = snapshot.write_str(&errors.value);
    }
    if let Some(events) = owners {
        let mut details = Text::new(8 * 1024);
        let _ = events.write_diagnostics(&mut details);
        let _ = snapshot.write_str(&details.value);
    }
    snapshot.value
}

/// Force a breadcrumb at a publication or recovery boundary.
pub(super) fn record(world: &World, log: bool) {
    let scene = describe(world);
    let snapshot = context()
        .write()
        .unwrap_or_else(|error| error.into_inner())
        .record(scene);
    if log {
        info!(target: "resonance::context", "Game context:\n{snapshot}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_fatal_error_survives_world_teardown_with_its_cause_and_backtrace() {
        let mut app = App::new();
        install(&mut app);
        let failures = app.world().resource::<FatalErrors>().clone();
        let error = anyhow::Error::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "bad movie frame",
        ));
        let backtrace = std::ptr::from_ref(error.backtrace());
        fatal(app.world_mut(), "movie playback", error);
        fatal(
            app.world_mut(),
            "cleanup",
            anyhow::anyhow!("secondary failure"),
        );
        drop(app);
        let error = failures
            .finish(Err(anyhow::anyhow!("application exited")))
            .unwrap_err();
        assert_eq!(format!("{error:#}"), "movie playback: bad movie frame");
        assert!(error.downcast_ref::<std::io::Error>().is_some());
        assert!(std::ptr::eq(backtrace, error.backtrace()));
    }

    #[test]
    fn system_failure_preserves_the_error_without_a_logging_subscriber() {
        let mut app = App::new();
        install(&mut app);
        let failures = app.world().resource::<FatalErrors>().clone();
        app.add_systems(Update, |mut failure: Fatal| {
            failure.fail(
                "title script",
                anyhow::anyhow!("invalid opcode").context("owner 7"),
            );
        });
        app.update();
        assert_eq!(app.should_exit(), Some(AppExit::error()));
        drop(app);
        let error = failures.finish(Ok(())).unwrap_err();
        assert_eq!(
            format!("{error:#}"),
            "title script: owner 7: invalid opcode"
        );
    }

    #[test]
    fn byte_budget_stops_the_formatter_before_it_walks_the_inventory() {
        struct Inventory(std::cell::Cell<usize>);
        impl fmt::Display for Inventory {
            fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
                for _ in 0..100_000 {
                    self.0.set(self.0.get() + 1);
                    out.write_str("éééééééééé")?;
                }
                Ok(())
            }
        }
        let inventory = Inventory(std::cell::Cell::new(0));
        let text = Text::format(64, format_args!("{inventory}"));
        assert!(text.len() <= 64);
        assert!(text.ends_with("[context truncated]"));
        assert!(inventory.0.get() < 5);
    }

    #[test]
    fn failure_notes_reach_native_reports_and_survive_scene_retirement() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("context");
        let mut context = CrashContext {
            file: Some(path.clone()),
            ..Default::default()
        };
        context.record("Scene: field; map: 332".into());
        // Failure paths can hold a removed owner, then retire its scene.
        context.remember_failure("battle: state=Active; setup=Formation(77); seed=1234".into());
        let failure = std::fs::read_to_string(&path).unwrap();
        assert_eq!(failure, context.snapshot());
        assert!(failure.contains("map: 332"));
        context.record("Scene: title".into());
        let snapshot = context.snapshot();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), snapshot);
        for detail in ["Scene: title", "state=Active", "Formation(77)", "seed=1234"] {
            assert!(snapshot.contains(detail), "{snapshot}");
        }
    }

    #[test]
    fn recent_failures_stay_visible_with_large_scenes_and_utf8_errors() {
        let mut context = CrashContext::default();
        context.record("large scene inventory".repeat(2000));
        for sequence in 0..12 {
            context.remember_failure(format!("failure {sequence}: {}", "é".repeat(2000)));
        }
        let snapshot = context.snapshot();
        assert_eq!(context.failures.len(), 8);
        assert!(snapshot.starts_with("Recent failure: failure 11:"));
        assert!(snapshot.contains("failure 4:"));
        assert!(!snapshot.contains("failure 3:"));
        assert!(snapshot.len() < 17 * 1024);
    }

    #[test]
    fn snapshot_identifies_clock_loading_and_content_failure_without_a_live_scene() {
        let mut world = World::new();
        world.insert_resource(crate::Clock(resonance_game::clock::PresentationClock::new(
            91,
        )));
        world.insert_resource(crate::loading::Resident::default());
        let diagnostics = resonance_content::diagnostics::Diagnostics::default();
        diagnostics
            .report("field 332", anyhow::anyhow!("missing dragon.glb"))
            .unwrap();
        world.insert_resource(Diagnostics(diagnostics));
        let snapshot = describe(&world);
        for detail in [
            "Sampled:",
            "Presentation tick: 91",
            "Scene: starting",
            "Loading:",
            "field 332",
            "missing dragon.glb",
        ] {
            assert!(snapshot.contains(detail), "{snapshot}");
        }
    }

    #[test]
    fn very_large_utf8_diagnostics_are_bounded_without_splitting_characters() {
        let mut world = World::new();
        let diagnostics = resonance_content::diagnostics::Diagnostics::default();
        diagnostics
            .report(
                "oversized content",
                anyhow::anyhow!("{}", "é".repeat(10_000)),
            )
            .unwrap();
        world.insert_resource(Diagnostics(diagnostics));
        let snapshot = describe(&world);
        assert!(snapshot.ends_with("[context truncated]"));
        assert!(snapshot.len() < 17 * 1024);
    }
}
