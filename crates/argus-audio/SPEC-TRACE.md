# argus-audio: spec trace

Every statement in the design document (v0.3) that constrains audio as phase 1 item 3 scopes it (§13 "Audio", §8 "audio is observed as events", §10 clip export, §6 determinism, §14 placeholders, §18 phase 1 and its gate), with where it lives and the test that pins it. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §13 Audio

| Spec | Status | Where | Test |
|---|---|---|---|
| The mixer has named buses (music, sfx, ui, ambience) with a routing tree. | Done | `config.rs`: `[buses] name = { parent, gain_db }`; a bus without a parent hangs off `master`; unknown parents and loops are errors with `audio.toml:line`. Without the file, `master` plus the four named buses. `mixer.rs`: a bus's effective gain is the product down its chain, mute included. | `audio::the_routing_tree_and_its_errors_come_from_audio_toml`, `audio::buses_gain_mute_pan_and_duck_as_declared` |
| Ducking rules declared as data ("dialogue ducks music by 6 dB over 200 ms"). | Done | `[[duck]] when, duck, db, attack_ms, release_ms`: while any audible voice is on `when` or under it, `duck` moves to `db` at the attack rate and back at the release rate, per 256-frame block. The amount in effect is reported per bus (`duck_db`). | `audio::buses_gain_mute_pan_and_duck_as_declared` |
| 2D spatialization by distance from the listener entity. | Done | `AudioListener` component (first active one), else the drawing camera (since item 7: the active `Camera` with the highest priority, the one the renderer uses), else the origin. `[spatial] min_distance, max_distance`: full volume inside `min`, a squared falloff to silence at `max`, equal-power pan by horizontal offset. A voice played on an entity follows it every frame; if the entity dies the sound finishes where it was. `spatial = false` on a sound opts out (UI, the player's own gun). | `audio::spatial_sounds_attenuate_and_pan_from_the_listener_and_follow_their_entity`, `argus-daemon/tests/audio.rs` |
| Sound events carry variation sets (pick one of five footsteps, pitch-jitter ±5%) so the game asks for sfx/step and the mixer chooses. | Done | `[sounds."name"] variations = [...]`, `gain_jitter`, `pitch_jitter`; the choice and both jitters come from a stream seeded by the run seed (`argus_sim::rng::stream(seed, "audio")`), three draws per play whatever the sound, so the same run makes the same choices. | `audio::variations_replay_from_the_seed_and_every_play_is_an_event`; `argus-daemon/tests/audio.rs` (the same run twice gives the same `sound.played` rows) |
| The music system is adaptive: a track is a set of layers; intensity is a parameter a system writes (`Music.intensity = enemies_near / 10`). | Partial | `Music { track, intensity }` component; `[music.name] layers = [{ sound, from, to }]`: layers start together on one block so they stay aligned, loop, and fade in between `from` and `to` (a step at `from` by default); a track change crossfades over `fade_ms`. Swarm's `late.music` writes intensity from `q:within`. Tempo and transitions on beat or bar boundaries are phase 3, as the plan lists "the adaptive music system" there. | `audio::music_layers_follow_intensity_and_are_logged` |
| Hollow's room-to-room music and Outpost's day-night ambience are both this. | Deferred | Phase 3 (room streaming at scale, adaptive music proper). | |

## §8 Observed as events

| Spec | Status | Where | Test |
|---|---|---|---|
| "played sfx/hit_heavy at frame 812, gain 0.8, on entity e:4011". | Done | Gameplay emits `sound.play {sound, entity, gain, pitch, bus, pan, loop}` (`q:play(name, opts)` in Luau); the daemon answers in the log, right after the request and in the same frame, with `sound.played {sound, variation, voice, bus, gain, pitch, pan, entity, attenuation, stole}`. A name that does not exist is one `sound.missing`. `music.started`, `music.stopped`, and `music.layer {track, layer, on}` report the music. | `argus-daemon/tests/audio.rs::scripts_ask_for_sounds_and_the_log_says_what_played`, `audio::music_layers_follow_intensity_and_are_logged` |
| The human hears it, the agent reads it. | Done | `audio.status`: device, buses with gain, mute, ducking, peak and RMS since the last read, voices per bus, the limiter's gain reduction, the music and its layers, sounds known, sounds missing, load warnings. A bus's meter is everything it sends on, so `master` is the whole mix. | `argus-daemon/tests/audio.rs` |

## §10 A clip for the human

| Spec | Status | Where | Test |
|---|---|---|---|
| A clip export exists for the human. | Done | The mixer keeps the last ten seconds of the master; `audio.clip {seconds, path}` writes it as a 16-bit stereo WAV. Headless, the daemon advances the mixer by sim time, so the clip of a run is the same every time. The rendered two-second video-and-audio clip of a mark is phase 2. | `audio::music_layers_follow_intensity_and_are_logged`, `argus-daemon/tests/audio.rs` |

## §14 Assets and placeholders

| Spec | Status | Where | Test |
|---|---|---|---|
| An audio file's gain and variation set in a sidecar. | Done | Gain and variations live in `audio.toml` per sound, or in a sidecar beside the sound (item 4: `parse_sound_sidecar` reads the same table, and `set_asset_sounds` loads the importer's); every WAV under `assets/` is also a sound by its path (`assets/sfx/hit.wav` is `sfx/hit`, on `music` when under `music/`). Precedence: `audio.toml`, then sidecars, then bare WAVs. A sidecar naming a bus that does not exist plays on `sfx` with a warning. WAV is the one format. | `audio::a_wav_under_assets_is_a_sound_by_its_path`, `argus-assets/tests/importer.rs`, `argus-daemon/tests/assets.rs` |
| Gameplay never waits on art. | Done | A variation can be a described tone (`{ wave = "square", freq = 440, ms = 80, slide, attack_ms, release_ms, notes, note_ms }`), synthesised deterministically at load, including note sequences for placeholder music. Both example games sound with no audio files at all. | `audio::the_routing_tree_and_its_errors_come_from_audio_toml` |

## Beyond the text, found by reading the meters

The first run of Swarm's first wave with sound read `sfx peak +18 dB` in `audio.status`: when thirty enemies die in one tick, thirty pops start in the same block and the master hard-clips. The design does not name the fix, so these are decisions:

- **A voice cap per sound.** `max_voices` (default 8): a new voice steals the oldest, and `sound.played` says `stole = <voice>`. A stolen voice that has not sounded yet is dropped outright rather than faded, so a burst in one tick never plays the voices it steals.
- **A master limiter.** A ceiling of 0.95 with immediate attack and 150 ms release; `master.limiter_db` in `audio.status` says how far it turned the mix down. After both, the same wave peaks at -2.5 dB and the limiter never acts.
- **Silence is cheap.** With no device open the daemon mixes as far as the sim clock, so the meters and `audio.clip` have something to read, and a discrete game's turn is one second of it. Mixing a second of zeros through every bus each turn made Delve's tests five times slower (found after the phase-1 gate). A block with no voice playing now takes a short path that leaves exactly the long path's state: meters count the samples, the limiter recovers, the history gets zeros. Delve's tests went from 5.2 s to 2.4 s; `audio::silence_mixes_to_the_same_state_by_the_short_path` compares the two paths.

| Test |
|---|
| `audio::a_burst_is_limited_not_clipped_and_a_capped_sound_steals_its_oldest_voice` |

## Output

| Spec | Status | Where | Test |
|---|---|---|---|
| The core hosts audio (§2). | Done | `device.rs` (cpal 0.15, feature `device`): the output callback pulls blocks straight from the shared mixer, so sound runs in real time even on a discrete scene where nothing ticks between turns. The device prefers 48 kHz and the mixer rebuilds at the device's rate if it differs. `render.window` opens it with the window; `audio.device {open}` by hand; headless runs never open it. | (manual: `audio.device` opened the laptop's speakers and closed them) |

## Known limitations, stated so they are decisions

- WAV only; OGG and other formats come with the importer.
- Resampling is linear. Fine for effects; music is authored at the mixer rate in practice.
- Voices started in one frame start on the next audio block, not at their tick's exact sample. At 60 Hz and 256-frame blocks the error is under 6 ms.
- The mixer is locked briefly by the daemon thread when it starts voices and moves spatial ones; the callback holds the same lock while it renders a block.
- Music layer events are observations of the frame they were noticed in; a headless run and a windowed run of the same inputs can report a layer turning on a frame apart. `sound.played` is exact.
