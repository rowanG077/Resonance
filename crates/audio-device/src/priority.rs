use anyhow::Result;
use resonance_playback::{Converter, SOURCE_BLOCK, SOURCE_RATE};

struct Guard(Option<audio_thread_priority::RtPriorityHandle>);
impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            let _ = audio_thread_priority::demote_current_thread_from_real_time(handle);
        }
    }
}
struct Prioritized {
    converter: Box<dyn Converter>,
    priority: Option<bool>,
}
impl Converter for Prioritized {
    fn prepare(&mut self) -> Result<Option<Box<dyn std::any::Any>>> {
        match audio_thread_priority::promote_current_thread_to_real_time(
            SOURCE_BLOCK as u32,
            SOURCE_RATE,
        ) {
            Ok(handle) => {
                self.priority = Some(true);
                Ok(Some(Box::new(Guard(Some(handle)))))
            }
            Err(error) => {
                self.priority = Some(false);
                eprintln!("Audio mixer priority unavailable: {error}");
                Ok(None)
            }
        }
    }
    fn realtime_priority(&self) -> Option<bool> {
        self.priority
    }
    fn convert(&mut self, input: &[[f32; 2]], output: &mut Vec<[f32; 2]>) -> Result<()> {
        self.converter.convert(input, output)
    }
}
pub fn prioritize(converter: Box<dyn Converter>) -> Box<dyn Converter> {
    Box::new(Prioritized {
        converter,
        priority: None,
    })
}
