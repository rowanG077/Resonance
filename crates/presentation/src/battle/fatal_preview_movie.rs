//! Ordered asynchronous framebuffer readback into a bounded video encoder pipe.
use super::*;
use std::{
    io::Write,
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub(super) struct Movie(Arc<Mutex<Writer>>);
struct Writer {
    child: Option<Child>,
    input: Option<ChildStdin>,
    pending: BTreeMap<u64, Image>,
    next: u64,
    error: Option<String>,
}

impl Movie {
    pub fn new(output: &Path) -> Result<Self> {
        let encoder = std::env::var_os("RESONANCE_FATAL_FFMPEG").unwrap_or_else(|| "ffmpeg".into());
        let mut child = Command::new(encoder)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "rawvideo",
                "-pixel_format",
                "rgba",
                "-video_size",
                "1280x720",
                "-framerate",
                "60",
                "-i",
                "pipe:0",
                "-an",
                "-c:v",
                "libx264",
                "-threads",
                "2",
                "-preset",
                "veryfast",
                "-crf",
                "18",
                "-pix_fmt",
                "yuv420p",
                "-movflags",
                "+faststart",
            ])
            .arg(output.join("video-silent.mp4"))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::from(std::fs::File::create(
                output.join("encoder.log"),
            )?))
            .spawn()
            .context("start frame encoder")?;
        let input = child.stdin.take().context("encoder has no input")?;
        Ok(Self(Arc::new(Mutex::new(Writer {
            child: Some(child),
            input: Some(input),
            pending: BTreeMap::new(),
            next: 0,
            error: None,
        }))))
    }

    /// Requests precede simulation so the render submission depicts this update,
    /// even though its GPU readback arrives on a later app update.
    pub fn request(&self, app: &mut App, frame: u64) {
        use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
        let framebuffer = app.world().resource::<crate::Framebuffer>().0.clone();
        let writer = self.0.clone();
        app.world_mut().spawn(Screenshot(framebuffer)).observe(
            move |shot: On<ScreenshotCaptured>| {
                let mut writer = writer.lock().unwrap();
                if writer.error.is_some() {
                    return;
                }
                writer.pending.insert(frame, shot.image.clone());
                if let Err(error) = writer.flush() {
                    writer.error = Some(format!("{error:#}"));
                }
            },
        );
    }

    pub fn finish(
        self,
        scenario: &mut crate::field_ui::resident::scenario::Scenario,
        frames: u64,
    ) -> Result<()> {
        scenario
            .app
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                Duration::ZERO,
            ));
        let began = Instant::now();
        loop {
            {
                let writer = self.0.lock().unwrap();
                ensure!(writer.error.is_none(), "video encoder: {:?}", writer.error);
                if writer.next == frames {
                    break;
                }
                ensure!(
                    began.elapsed().as_secs() < 30,
                    "GPU readback incomplete: {}/{} frames",
                    writer.next,
                    frames
                );
            }
            scenario.app.update();
            crate::playthrough::check_exit(&scenario.app)?;
            std::thread::sleep(Duration::from_millis(1));
        }
        let mut writer = self.0.lock().unwrap();
        writer.input.take();
        let status = writer
            .child
            .take()
            .context("missing video encoder")?
            .wait()?;
        ensure!(status.success(), "video encoder exited {status}");
        Ok(())
    }
}

impl Writer {
    fn flush(&mut self) -> Result<()> {
        while let Some(image) = self.pending.remove(&self.next) {
            let rgba = image.try_into_dynamic()?.into_rgba8();
            ensure!(
                rgba.dimensions() == (1280, 720),
                "unexpected capture dimensions"
            );
            self.input
                .as_mut()
                .context("encoder input closed")?
                .write_all(rgba.as_raw())?;
            self.next += 1;
        }
        Ok(())
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        self.input.take();
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
