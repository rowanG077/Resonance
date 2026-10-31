use super::audio_output::Sink as AudioSink;
use super::{PendingInput, RunOptions, materials::TitleText};
use crate::audio_output::{PlaybackSettings, Player as AudioPlayer};
use anyhow::{Context, Result, ensure};
use bevy::{
    camera::{MsaaWriteback, RenderTarget, ScalingMode, visibility::RenderLayers},
    core_pipeline::tonemapping::Tonemapping,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use resonance_content::{HEIGHT, WIDTH};
use resonance_media::{Format, MovieDecoder, MovieEvent, MovieStream, VideoFrame};
use resonance_playback::Decodable;
use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

pub(super) mod pacing;
#[cfg(test)]
pub(crate) mod tests;

type AudioBuffer = resonance_playback::Pcm;

#[derive(Asset, TypePath, Clone)]
pub(super) struct MovieAudio {
    buffer: Arc<AudioBuffer>,
    mono: bool,
    gain: f32,
}

impl Decodable for MovieAudio {
    type Decoder = resonance_playback::PcmSource;
    fn decoder(&self) -> Self::Decoder {
        self.buffer.source(self.mono, self.gain)
    }
}

#[derive(Resource, Default)]
pub(super) struct Playback {
    pub active: bool,
    pub completed_naturally: bool,
    pub resource: Option<u32>,
    pub asset: Option<Format>,
    pub final_frame: Option<u32>,
    pub presented_frame: Option<u32>,
    pub presented_timestamp: Option<Duration>,
    decoder: Option<MovieDecoder>,
    pending_events: VecDeque<MovieEvent>,
    stream: Option<MovieStream>,
    pub dropped_frames: u64,
    frames: VecDeque<VideoFrame>,
    buffer: Arc<AudioBuffer>,
    reported_underruns: u64,
    audio_entity: Option<Entity>,
    texture: Handle<Image>,
    ended: bool,
    paused: bool,
    started: Option<Instant>,
    prebuffer_started: Option<Instant>,
    completion: Option<resonance_events::Operation>,
    ignore_initial_skip: bool,
}

/// Bounded decoder startup, performed by the field worker before activation.
/// The decoder blocks on its own bounded queue until the script plays it.
pub(super) struct Prepared {
    decoder: MovieDecoder,
    events: VecDeque<MovieEvent>,
}
impl Prepared {
    pub(super) fn capture(
        snapshot: &mut resonance_content::library::Snapshot,
        id: u32,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self> {
        ensure!(!cancelled(), "movie preparation cancelled");
        Self::prepare(MovieDecoder::load(snapshot, id)?, cancelled)
    }

    fn prepare(decoder: MovieDecoder, cancelled: impl Fn() -> bool) -> Result<Self> {
        let format = decoder.format();
        let mut events = VecDeque::new();
        let (mut video, mut audio, mut chunks) = (0, 0, 0);
        let video_target = resonance_media::VIDEO_LOOKAHEAD;
        let audio_target = u64::from(format.sample_rate) as usize;
        let started = Instant::now();
        loop {
            ensure!(!cancelled(), "movie preparation cancelled");
            ensure!(
                started.elapsed() < Duration::from_secs(30),
                "movie preparation timed out"
            );
            if video < video_target
                && let Some(frame) = decoder.try_video()?
            {
                events.push_back(MovieEvent::Video(frame));
                video += 1;
            }
            if audio < audio_target
                && let Some(chunk) = decoder.try_audio()?
            {
                audio += chunk.samples.len();
                chunks += 1;
                events.push_back(MovieEvent::Audio(chunk));
            }
            if (video >= video_target || decoder.video_complete())
                && (audio >= audio_target || decoder.audio_complete())
            {
                break;
            }
            ensure!(chunks < 64, "movie exceeds startup buffer limits");
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(Self { decoder, events })
    }
}

impl Playback {
    pub(super) fn video_timestamps(&self) -> Result<Vec<Duration>> {
        self.decoder
            .as_ref()
            .context("movie is no longer available for capture planning")?
            .video_timestamps()
    }

    /// Reuse the movie surface for a field-script request. The decoder and
    /// audio queues belong to this playback; the token belongs to the scene.
    pub(super) fn start_script_movie(
        &mut self,
        resource: u32,
        prepared: Prepared,
        completion: resonance_events::Operation,
        images: &mut Assets<Image>,
    ) -> Result<()> {
        ensure!(!self.active, "another movie is still playing");
        let asset = prepared.decoder.format();
        // The preceding movie's final image must not flash while this one
        // prebuffers. Its surface remains black until the first decoded frame.
        // A resized Image gets a new GPU texture, but an existing material can
        // retain its old binding. Give each playback its own image handle so
        // the surface explicitly binds the texture receiving decoded frames.
        let texture = images.add(surface_image(asset.width, asset.height));
        *self = Self {
            active: true,
            resource: Some(resource),
            asset: Some(asset),
            decoder: Some(prepared.decoder),
            pending_events: prepared.events,
            texture,
            completion: Some(completion),
            ignore_initial_skip: true,
            ..Default::default()
        };
        Ok(())
    }
    pub fn load(
        library: &resonance_content::library::Library,
        options: &RunOptions,
    ) -> Result<Self> {
        if options.skip_intro
            || matches!(options.capture_at, Some(crate::CaptureAt::TitleTick(_)))
            || options.replay.is_some()
            || matches!(options.capture_at, Some(crate::CaptureAt::BootTick(_)))
        {
            return Ok(Self::default());
        }
        let mut snapshot = library.snapshot();
        let decoder = MovieDecoder::load(&mut snapshot, 0).context("prepare opening movie")?;
        let asset = decoder.format();
        Ok(Self {
            active: true,
            resource: Some(0),
            asset: Some(asset),
            decoder: Some(decoder),
            ..Default::default()
        })
    }

    pub(super) fn is_presenting(&self) -> bool {
        self.active && self.started.is_some() && !self.paused
    }

    fn finish(&mut self, commands: &mut Commands, pending: &mut PendingInput) {
        self.active = false;
        self.buffer.finish();
        self.stream.take();
        self.decoder.take();
        self.pending_events.clear();
        self.frames.clear();
        if let Some(entity) = self.audio_entity.take() {
            commands.entity(entity).despawn();
        }
        if let Some(completion) = self.completion.take()
            && completion.is_pending()
        {
            // Skip and natural completion both release the script's movie wait.
            if let Err(error) = completion.complete(None) {
                error!("movie completion failed: {error}");
            }
        }
        // The skip press belongs to the movie, not the next menu update.
        let held = pending.held;
        *pending = PendingInput::default();
        pending.held = held;
    }
}

#[derive(Component)]
pub(super) struct MovieCamera;

#[derive(Component)]
pub(super) struct MovieSurface;

fn surface_image(width: u32, height: u32) -> Image {
    let mut image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![0; width as usize * height as usize * 4],
        TextureFormat::Rgba8Unorm,
        default(),
    );
    image.sampler = ImageSampler::linear();
    image
}

pub(super) fn setup(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<TitleText>,
    movie: &mut Playback,
    source: Handle<Image>,
) {
    // Keep a movie surface available even when startup used --skip-intro.
    let (width, height) = movie
        .asset
        .as_ref()
        .map_or((WIDTH, HEIGHT), |a| (a.width, a.height));
    movie.texture = images.add(surface_image(width, height));
    commands.spawn((
        MovieSurface,
        Mesh2d(meshes.add(Rectangle::new(1., 1.))),
        Transform::from_scale(Vec3::new(width as f32, height as f32, 1.)),
        MeshMaterial2d(materials.add(TitleText {
            source: movie.texture.clone(),
            opacity_pulse: Vec4::new(1., 0., 0., 0.),
        })),
        RenderLayers::layer(3),
    ));
    commands.spawn((
        Camera2d,
        Tonemapping::None,
        Msaa::Off,
        RenderLayers::layer(3),
        MovieCamera,
        Camera {
            order: -2,
            is_active: false,
            clear_color: ClearColorConfig::Custom(Color::BLACK),
            // Movies replace the field, including their black borders. MSAA
            // writeback would preserve the field fade and suppress this clear.
            msaa_writeback: MsaaWriteback::Off,
            ..default()
        },
        super::camera::overlay_alignment(),
        RenderTarget::Image(source.into()),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed {
                width: WIDTH as f32,
                height: HEIGHT as f32,
            },
            ..OrthographicProjection::default_2d()
        }),
    ));
}

#[allow(clippy::too_many_arguments)] // Device input and movie ownership.
/// Input policy shared by render-frame player input and fixed-update scenarios.
pub(super) fn controls(
    mut commands: Commands,
    mut movie: ResMut<Playback>,
    input: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut pending: ResMut<PendingInput>,
    mut cameras: Query<&mut Camera, With<MovieCamera>>,
    ready: Res<super::timing::Ready>,
    boot: Res<super::boot::Playback>,
    scenario: Option<ResMut<super::saves::ScenarioInput>>,
    session: Option<Res<crate::new_game::Session>>,
    preferences: Option<Res<crate::config::Preferences>>,
    mut previous_inputs: Local<std::collections::BTreeSet<resonance_content::input::PhysicalInput>>,
) {
    if !ready.0 || !movie.active || boot.active() {
        return;
    }
    if let Some(mut scenario) = scenario {
        scenario.acknowledge_input();
    }
    use resonance_content::input::StandardAction as A;
    let settings = crate::input::settings(session.as_deref(), preferences.as_deref());
    let mut physical = crate::input::PhysicalState::keyboard(&input);
    for pad in &gamepads {
        physical.merge(&crate::input::PhysicalState::gamepad(pad));
    }
    physical.track_edges(&mut previous_inputs);
    let skip_pressed = [A::Confirm, A::Cancel, A::Start]
        .into_iter()
        .any(|a| physical.edge(&settings.keymap, a));
    let ignore_skip = std::mem::take(&mut movie.ignore_initial_skip);
    let skip = skip_pressed && !ignore_skip;
    if skip {
        info!("Movie skipped");
        movie.finish(&mut commands, &mut pending);
        for mut camera in &mut cameras {
            camera.is_active = false;
        }
        return;
    }
    if physical.edge(&settings.keymap, A::Menu) {
        movie.paused = !movie.paused;
    }
}

#[allow(clippy::too_many_arguments)] // Movie decoding, texture upload, and playback ownership.
pub(super) fn update(
    mut commands: Commands,
    mut movie: ResMut<Playback>,
    mut pending: ResMut<PendingInput>,
    mut cameras: Query<&mut Camera, With<MovieCamera>>,
    mut surfaces: Query<(&mut Transform, &MeshMaterial2d<TitleText>), With<MovieSurface>>,
    mut materials: ResMut<Assets<TitleText>>,
    mut images: ResMut<Assets<Image>>,
    mut audio_assets: ResMut<Assets<MovieAudio>>,
    sinks: Query<&AudioSink>,
    mut exit: MessageWriter<AppExit>,
    ready: Res<super::timing::Ready>,
    boot: Res<super::boot::Playback>,
    preferences: Option<Res<super::config::Preferences>>,
    session: Option<Res<super::new_game::Session>>,
) {
    if !ready.0 {
        return;
    }
    for mut camera in &mut cameras {
        camera.is_active = movie.active && !boot.active();
    }
    if !movie.active || boot.active() {
        return;
    }
    if let Some(asset) = &movie.asset {
        for (mut surface, material) in &mut surfaces {
            surface.scale = Vec3::new(asset.width as f32, asset.height as f32, 1.);
            if materials
                .get(&material.0)
                .is_some_and(|material| material.source != movie.texture)
            {
                materials.get_mut(&material.0).unwrap().source = movie.texture.clone();
            }
        }
    }
    let settings = session
        .as_ref()
        .and_then(|session| session.events().world.party.as_ref())
        .map(|party| &party.settings.preferences)
        .or_else(|| {
            preferences
                .as_ref()
                .map(|preferences| &preferences.settings)
        });
    if let Err(error) = advance(
        &mut movie,
        &mut commands,
        &mut images,
        &mut audio_assets,
        &sinks,
        settings,
    ) {
        error!("movie playback failed: {error:#}");
        if let Some(completion) = movie.completion.take() {
            completion.cancel();
        }
        movie.finish(&mut commands, &mut pending);
        exit.write(AppExit::error());
    } else if movie.ended
        && let Some(entity) = movie.audio_entity
        && let Ok(sink) = sinks.get(entity)
        && sink.empty()
    {
        info!("Movie playback complete");
        movie.completed_naturally = true;
        movie.finish(&mut commands, &mut pending);
        for mut camera in &mut cameras {
            camera.is_active = false;
        }
    }
}

fn advance(
    movie: &mut Playback,
    commands: &mut Commands,
    images: &mut Assets<Image>,
    audio_assets: &mut Assets<MovieAudio>,
    sinks: &Query<&AudioSink>,
    settings: Option<&resonance_content::menu_data::CustomizeSettings>,
) -> Result<()> {
    movie.prebuffer_started.get_or_insert_with(Instant::now);
    let asset = movie
        .asset
        .as_ref()
        .context("active movie has no prepared format")?;
    if movie.stream.is_none() {
        let decoder = movie.decoder.take().context("movie decoder missing")?;
        let stream = MovieStream::start(
            decoder,
            std::mem::take(&mut movie.pending_events),
            asset.sample_rate,
        )?;
        movie.buffer = stream.audio();
        movie.stream = Some(stream);
    }
    let stream = movie.stream.as_ref().unwrap();
    stream.check()?;
    let underruns = movie.buffer.underruns();
    if underruns != movie.reported_underruns {
        warn!(
            underruns,
            "Movie decoded audio underrun; rebuffering preserves the movie clock"
        );
        movie.reported_underruns = underruns;
    }
    // Free stale presentation frames before draining the producer. Otherwise a
    // long stall with a full local queue would flash an old frame for one update.
    let mut latest = None;
    if let Some(position) = movie
        .audio_entity
        .and_then(|entity| sinks.get(entity).ok())
        .map(AudioSink::position)
    {
        while movie
            .frames
            .front()
            .is_some_and(|frame| frame.timestamp <= position)
        {
            if latest.is_some() {
                movie.dropped_frames += 1;
            }
            latest = movie.frames.pop_front();
        }
    }
    while movie.frames.len() < 32 {
        let Some(frame) = stream.try_video()? else {
            break;
        };
        movie.frames.push_back(frame);
    }
    if stream.video_complete() {
        movie.final_frame = movie
            .frames
            .back()
            .map(|frame| frame.index)
            .or_else(|| latest.as_ref().map(|frame| frame.index))
            .or(movie.presented_frame);
    }
    movie.ended = stream.complete();
    if movie.audio_entity.is_none() {
        let buffered = movie.buffer.buffered();
        if (movie.frames.len() >= resonance_media::VIDEO_LOOKAHEAD || stream.video_complete())
            && (buffered >= u64::from(asset.sample_rate) / 2 || movie.buffer.finished())
        {
            let audio = MovieAudio {
                buffer: movie.buffer.clone(),
                mono: settings.is_some_and(|settings| !settings.stereo),
                // Movies have one premixed soundtrack, like the credits.
                gain: settings.map_or(1., |settings| f32::from(settings.volumes.music) / 127.),
            };
            movie.audio_entity = Some(
                commands
                    .spawn((
                        AudioPlayer(audio_assets.add(audio)),
                        PlaybackSettings {
                            paused: movie.paused,
                        },
                    ))
                    .id(),
            );
            movie.started = Some(Instant::now());
        } else {
            ensure!(
                movie
                    .prebuffer_started
                    .is_none_or(|loaded| loaded.elapsed() < Duration::from_secs(30)),
                "movie prebuffer timed out"
            );
        }
    }
    let Some(entity) = movie.audio_entity else {
        return Ok(());
    };
    let Ok(sink) = sinks.get(entity) else {
        ensure!(
            movie
                .started
                .is_none_or(|started| started.elapsed() < Duration::from_secs(10)),
            "movie audio output did not start"
        );
        return Ok(());
    };
    if movie.paused {
        sink.pause();
    } else {
        sink.play();
    }
    let position = sink.position();
    while movie
        .frames
        .front()
        .is_some_and(|frame| frame.timestamp <= position)
    {
        if latest.is_some() {
            movie.dropped_frames += 1;
        }
        latest = movie.frames.pop_front();
    }
    if let Some(frame) = latest {
        movie.presented_frame = Some(frame.index);
        movie.presented_timestamp = Some(frame.timestamp);
        images
            .get_mut(&movie.texture)
            .context("movie texture missing")?
            .data = Some(frame.rgba);
    }
    Ok(())
}

impl Playback {
    pub(super) fn wait_for_audio(&self, frames: u64) -> Result<()> {
        if self.is_presenting() {
            // The offline mixer pulls exactly these native stereo frames;
            // source attachment rejects other rates, so no resampler lookahead.
            self.stream
                .as_ref()
                .context("presenting movie has no decoder stream")?
                .wait_for_audio(frames, Duration::from_secs(30))?;
        }
        Ok(())
    }

    /// Authored subtitle cues retain their original frame numbers, but advance
    /// from media time even when the renderer is holding an older video frame.
    pub(super) fn timeline_frame(&self, position: Option<Duration>) -> Option<u32> {
        if !self.active {
            return None;
        }
        match (position, self.asset.as_ref()) {
            (Some(position), Some(asset)) => Some(
                (position.as_nanos() / asset.frame_duration.as_nanos())
                    .min(u128::from(self.final_frame.unwrap_or(u32::MAX))) as u32,
            ),
            _ => self.presented_frame,
        }
    }
    pub(super) fn position(&self, world: &World) -> Option<Duration> {
        self.audio_entity
            .and_then(|entity| world.get::<AudioSink>(entity))
            .map(AudioSink::position)
    }
    pub(super) fn audio_sink<'a>(&self, sinks: &'a Query<&AudioSink>) -> Option<&'a AudioSink> {
        self.audio_entity.and_then(|entity| sinks.get(entity).ok())
    }
}
