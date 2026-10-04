Findings:

Iselia forest:

- Genis initial casting magic now works. But soon after it still crashes with:

2026-10-06T17:01:24.275144Z ERROR resonance_presentation::field_view: Field update failed: event Some(5100), handle 58, update 26502, source []: SymphoniaScript PC 0xadb5: native handler failed: CreateEffectObject (0xd3) [2, 10, -3110, 438, 100, 0, 0, 0, 200, 150, 255, -22, 2, 0]: effect recipe is not implemented
2026-10-06T17:01:24.624978Z  INFO resonance_presentation::audio_output: Audio output final: Diagnostics { converted_frames: 18523358, submitted_frames: 18521600, callbacks: 36175, underrun_frames: 0, underrun_callbacks: 0, device_errors: 0, backend_underruns: 0, device_lost: 0, maximum_callback_frames: 512, queued_frames: 1758, maximum_render_ns: 1344843, maximum_callback_ns: 84459, p999_callback_us: 53, callback_deadline_misses: 0, backend_delay_ns: 196979166, worker_realtime_priority: Some(true) }
Error: Resonance exited with an error

Look I really am pretty mad that you just don't catch this. A straightforward read of the script can catch this. Why didn't you? You even run an equivalence test between dolphin.

Triet:

- When Raine is searching for materials for the keycrest after getting back from the renegade base the key crest effect is a red shining sphere. But it is missing a swirling bright light.
- After Lloyd fixes the key crest I entered Colletes room and then Genis and Raines room. Then in Genis and Raines room the "it is sad" soundtrack is played. That's wrong. How can it be? It continues to be played even when kratos leaves and until Lloyd can exit the Inn room.

Truit ruins:

- We again cannot move a block over another block in the center room of triet ruins. The blocks are terribly implemented. I ask you to fix this, you destroy temple of martel block puzzle. I aks you to fix temple of martel and you break triet ruins. That's not even to say future block puzzles we are not even testing. This is unaccetable.

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
