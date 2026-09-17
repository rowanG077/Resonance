# Audio and video

Title music, field audio and movies share one persistent mixer worker. Bevy
controls lifecycle and presentation; the audio callback consumes prepared PCM.
The file-only recorder uses the same sources and mixer with an explicit clock.
Customize previews BGM independently of committed preferences. New cues, voice
settings and stereo use the saved values until commit; the commit cue uses the
new values. The field adapter sends these controls through its existing queue.
Mono centers individual synthesized voices before shared effects and the final
channel fold. Authored pan remains intact, including pan events received in mono,
so switching back to stereo preserves playback and positioning.
Speech uses a cooked amplitude curve for saved volume settings. Rust cooking
resolves CRI attenuation and stream-mixer tables into `voice_gains` in the field
audio JSON; the runtime applies that gain to decoded PCM. Level 64 is about
22.15% amplitude. Muting keeps speech progression and completion callbacks alive.

| Owner | Responsibility |
|---|---|
| `resonance-audio` | Resumable native synthesis in 160-frame blocks at 32028 Hz; no device or thread |
| `resonance-playback` | Source epochs, scheduled starts, pause/stop/drain, bounded queues, timeline mapping and offline execution |
| `resonance-media` | Rust FFV1/FLAC decoding, independent movie feeding and final band-limited device-rate conversion |
| `resonance-audio-device` | Negotiated CPAL stream, platform audio priority, timestamp/error forwarding and permanent mute |
| `resonance-presentation` | Device recovery, movie deadlines, subtitles and audible voice-completion acknowledgements |

## Delivery and clocks

The callback copies from a preallocated queue, converts format/channels, applies
mute and updates atomics. It performs no synthesis, decoding, allocation, file
I/O, logging, application locking or worker joins. The mixer requests audio
priority; failures are logged and exposed in diagnostics. Linux uses RTKit.

Device output prefers stereo f32 at 48 kHz and requests 512-frame callbacks,
clamped to supported limits. The ring targets three actual callback periods
with a 768-frame minimum. This is queued PCM, not total speaker latency. Movie
PCM has half a second of decoded lookahead; video queues are bounded. A full
video queue discards its oldest decoded frame without blocking audio. Compressed
packet dependencies remain inside the stateful Rust FFV1 decoder.

The callback publishes output positions and predicted playback timestamps.
Timeline spans map the monotonic audible estimate to each source, accounting
for pauses. A sink owns a unique epoch, so an old stop cannot stop a replacement.
Decoder EOF, source EOF and audible completion are distinct. Already committed
device audio drains before pause/stop is audible.

Native synthesis retains its own control quantization. The final Rubato sinc
resampler advances its cursor only for emitted samples and drains filter tails.
Immediate starts use the earliest unwritten mixer frame; scheduled starts use
absolute native frames. Field cues retain their next-block control semantics.

## Presentation and recovery

Gameplay updates at 60000/1001 Hz with uncapped rendering. Fullscreen movies wake
at video deadlines derived from audible time. Presentation holds early frames
and selects the newest due frame; subtitles use the same clock. Input/window
events can wake the loop between video deadlines. Paused movies poll for
controller input. Gameplay restores the continuous loop after the movie.

Automatic voiced dialogue waits for estimated audible completion. Explicit
player skip and authored mouth animations retain their own behavior. Movie skip
cancels feeding; decoder teardown happens off the presentation thread.

Device loss freezes the audible clock and game time, suspends mixing, and retries
the default device once per second. Recovery re-primes from the last half second
of native PCM and preserves source epochs and user pause. Insufficient retained
history is an explicit error. Decoded starvation, output underruns, backend xruns
and mixer failures have separate diagnostics. The callback supplies bounded
silence on starvation; the controlling thread reports failure, including release.

`--silent` is an immutable final output mute and survives device replacement.
Offline recorders write unmuted PCM to files without an output device.

See [silent probes](performance.md) and [PCM comparisons](../tools/oracle/README.md).
Hardware validation currently covers ARM Linux. Other target devices, real
hotplug/rate changes and physical speaker/display latency need validation.
Seeking, arbitrary track-length mismatch and tighter video buffer pooling are
future work; current playback supports the two cooked movies.
