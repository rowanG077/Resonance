//! Inspect a sound's resolved layers without decoding or opening audio.
use anyhow::Result;
use resonance_audio_cook::{
    bank::{Bank, ObjectKind, Page},
    instrument,
};
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    anyhow::ensure!(args.len() >= 2, "expected BANK SOUND_ID...");
    let bytes = std::fs::read(&args[0])?;
    let bank = Bank::parse(&bytes)?;
    for id in &args[1..] {
        let sound = bank.sound(id.parse()?)?;
        println!("sound {id}: {sound:?}");
        for note in instrument::resolve(
            &bank,
            Page {
                object: sound.object,
                priority: 64,
                max_voices: 255,
            },
            sound.key,
            sound.volume,
            sound.pan,
        )? {
            println!("  {note:?}");
            for (pc, words) in bank
                .object(ObjectKind::Macro, note.macro_id)?
                .chunks_exact(8)
                .enumerate()
            {
                println!(
                    "    {pc}: {:08x} {:08x}",
                    u32::from_be_bytes(words[..4].try_into()?),
                    u32::from_be_bytes(words[4..].try_into()?)
                );
            }
        }
    }
    Ok(())
}
