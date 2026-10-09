Findings:

Temple of Martel:

- I STILL can't move the blocks in the temple of martel properly. Now I can move it from on top of a stacked block but afters if it's on normal ground it's stuck. Look I don't understand what's going on here. These blocks have been a massive source of bugs.

Iselia forest:

- It seems emote animation are playing to fast? Before Genis says "I keep thinking, if he's going to run away, he should at least take us with him." lloyd ball of yarn emote plays really fast. I remmeber it not beeing so fast but maybe I'm wrong.

- The effort/distress sweat emote is wrong. Seen when the party enters the human ranch entrance. Some prisoners have this sweat emote that shows. Clearly visible when the desian says that they should stop slacking. Emotes have been another sources of bugs. For every emote in the game please make a comparison test with dolphin so they match.

- The dot dot dot emoji seems extremely small. Noticed it before Genis says "...Okay!" in the iselia human ranch entrance area.

- The  afterwards genis casts magic at the desians and the game crashes with:

2026-10-06T10:33:00.376294Z ERROR resonance_presentation::field_view: Field update failed: event Some(3004), handle 48, update 31167, source []: SymphoniaScript PC 0x4efc: native handler failed: CreateEffectEmitter (0xbf) [5000, -1700, 1782, 847, 0, 51, 0, 0, 100, 78, 0, 0, -3110, 438, 100, 0, 0, 0]: unsupported emitter 51
2026-10-06T10:33:00.583196Z  INFO resonance_presentation::audio_output: Audio output final: Diagnostics { converted_frames: 38317078, submitted_frames: 38315520, callbacks: 74835, underrun_frames: 0, underrun_callbacks: 0, device_errors: 0, backend_underruns: 0, device_lost: 0, maximum_callback_frames: 512, queued_frames: 1558, maximum_render_ns: 1176728, maximum_callback_ns: 69169, p999_callback_us: 44, callback_deadline_misses: 0, backend_delay_ns: 32000000, worker_realtime_priority: Some(true) }
Error: Resonance exited with an error

Triet:

- When Raine is searching for materials for the keycrest after getting back from the renegade base the key crest effect is a red shining sphere. But it is missing a swirling bright light.
- After Lloyd fixes the key crest I entered Colletes room and then Genis and Raines room. Then in Genis and Raines room the "it is sad" soundtrack is played. That's wrong. How can it be? It continues to be played even when kratos leaves and until Lloyd can exit the Inn room.

Truit ruins:

- The fire looks yellow. That looks wrong. I think it should look like fire. Why did our randomized effect equivalence not catch this? Or am I wrong here that it should not look yellow?
- The effect when the seal is "released" after the boss fight are also much brighter in the gamecube  version.
- The rays of light when  remiel descends from the heavens looks brighter in the gamecube version.
- When collete receive angel powers, the 4 spheres of light enter her. And then there should be a single sphere of light emmitting from her. That last part is missing.
- In general: Please add all these effects to dolphin equivalence, render them with a black background in both resonance and dolphin to prove equivalence. Do this with at least all of the followings effects

- All seal release effects
- All angel effects, including wings for all seraphim, collete and zelos
- All effects of an angel coming from and going to heaven.
- Fire effects in dungeons.
- All sorcerer ring effects
- Yuan lightning orb effect
- teleportation effects
- etc etc.

Do this efficiently we need a parallel dolphin test fixture. Like for example we can spawn 16 dolphin instances as a test fixcture and we can route test cases to it.

The reason being is that effect mismatches have been a huge headache, and it's subtle too. I'd hope our low level effect equivalence would be enough with randomized tests, but clearly that's not true. So before fixing this think hard what we can do here.

Tower of salvation:

- When the screen turns black in the first room some effect of the teleporter remains visible. I reported this last time, but it's not fixed.
- When Kratos is introducing himself as "... I am of Cruxis, the organization that guides this world." his theme is supposed to start playing but it does not. I reported this before. But again not fixed...
- Kratos wings look good. But yggdrasiles wings are flickering a lot. I think this effect exists in the gamecube version but we have implemented it incorrectly.
- The effect of the renegades shooting some kinda magic at yggdrasil is totally wrong. it shows like tiny particles. But it should be a full sphere like thing.
- The teleporter effect when the renegades leave with the party is not correct there are a ton of bright lights at the top.
