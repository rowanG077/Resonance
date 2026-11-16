//! Fullscreen movies request the next frame deadline. Input/window events wake
//! Winit immediately; gameplay restores the existing uncapped update policy.
use super::*;
use bevy::winit::{EventLoopProxyWrapper, UpdateMode, WinitSettings, WinitUserEvent};
use std::sync::{Condvar, Mutex};

pub(crate) fn update(
    movie: Res<Playback>,
    boot: Res<crate::boot::Playback>,
    sinks: Query<&AudioSink>,
    proxy: Option<Res<EventLoopProxyWrapper>>,
    mut wake: Local<Option<Wake>>,
    mut settings: ResMut<WinitSettings>,
) {
    if !movie.active || boot.active() {
        if let Some(wake) = wake.as_ref() {
            wake.schedule(None);
        }
        *settings = WinitSettings::continuous();
        return;
    }
    let Some(proxy) = proxy else {
        // A live offscreen runner has its own continuous schedule, not Winit.
        wake.take();
        *settings = WinitSettings::continuous();
        return;
    };
    let position = movie
        .audio_entity
        .and_then(|entity| sinks.get(entity).ok())
        .map(AudioSink::position);
    let wait = if movie.paused {
        // Gilrs polls on app updates; retain responsive controller input too.
        Duration::from_millis(33)
    } else if let Some(position) = position {
        movie
            .frames
            .front()
            .map_or(Duration::from_millis(4), |frame| {
                frame
                    .timestamp
                    .saturating_sub(position)
                    .clamp(Duration::from_millis(1), Duration::from_millis(34))
            })
    } else {
        Duration::from_millis(4)
    };
    // Changing Reactive.wait every update forces an immediate redraw in
    // Bevy 0.19. Keep the mode stable and wake its proxy at the actual deadline.
    let wake = wake.get_or_insert_with(|| {
        let proxy = (**proxy).clone();
        Wake::new(move || {
            let _ = proxy.send_event(WinitUserEvent::WakeUp);
        })
    });
    wake.schedule(Some(Instant::now() + wait));
    let mode = UpdateMode::reactive(Duration::MAX);
    settings.focused_mode = mode;
    settings.unfocused_mode = mode;
}

#[derive(Default)]
struct Deadline {
    at: Option<Instant>,
    stopped: bool,
}
pub(crate) struct Wake {
    state: Arc<(Mutex<Deadline>, Condvar)>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Wake {
    fn new(wake: impl Fn() + Send + 'static) -> Self {
        let state = Arc::new((Mutex::new(Deadline::default()), Condvar::new()));
        let worker = state.clone();
        let thread = std::thread::Builder::new()
            .name("resonance-video-deadline".into())
            .spawn(move || {
                let (lock, condition) = &*worker;
                let mut deadline = lock.lock().unwrap();
                while !deadline.stopped {
                    if let Some(at) = deadline.at {
                        let remaining = at.saturating_duration_since(Instant::now());
                        if remaining.is_zero() {
                            deadline.at = None;
                            drop(deadline);
                            wake();
                            deadline = lock.lock().unwrap();
                        } else {
                            deadline = condition.wait_timeout(deadline, remaining).unwrap().0;
                        }
                    } else {
                        deadline = condition.wait(deadline).unwrap();
                    }
                }
            })
            .expect("could not start video deadline worker");
        Self {
            state,
            thread: Some(thread),
        }
    }
    fn schedule(&self, at: Option<Instant>) {
        let (lock, condition) = &*self.state;
        let mut deadline = lock.lock().unwrap();
        if deadline.at != at {
            deadline.at = at;
            condition.notify_one();
        }
    }
}
impl Drop for Wake {
    fn drop(&mut self) {
        self.state.0.lock().unwrap().stopped = true;
        self.state.1.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn windowless_playback_does_not_require_an_event_loop_proxy() {
        let mut world = World::new();
        world.init_resource::<Playback>();
        world.init_resource::<crate::boot::Playback>();
        world.init_resource::<WinitSettings>();
        for active in [false, true] {
            world.resource_mut::<Playback>().active = active;
            world.run_system_once(update).unwrap();
            let settings = world.resource::<WinitSettings>();
            assert!(matches!(settings.focused_mode, UpdateMode::Continuous));
            assert!(matches!(settings.unfocused_mode, UpdateMode::Continuous));
        }
    }
}
