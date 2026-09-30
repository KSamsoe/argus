# argus-audio: spec trace

Every statement in the design document (v0.3) that constrains audio as phase 1 item 3 scopes it (§13 "Audio", §8 "audio is observed as events", §10 clip export, §6 determinism, §14 placeholders, §18 phase 1 and its gate), with where it lives and the test that pins it. Statuses as in the other traces: **Done**, **Partial**, **Deferred**.

## §13 Audio

| Spec | Status | Where | Test |
|---|---|---|---|
| The mixer has named buses (music, sfx, ui, ambience) with a routing tree. | Done | `config.rs`: `[buses] name = { parent, gain_db }`; a bus without a parent hangs off `master`; unknown parents and loops are errors with `audio.toml:line`. Without the file, `master` plus the four named buses. `mixer.rs`: a bus's effective gain is the product down its chain, mute included. | `audio::the_routing_tree_and_its_errors_come_from_audio_toml`, `audio::buses_gain_mute_pan_and_duck_as_declared` |
| Ducking rules declared as data ("dialogue ducks music by 6 dB over 200 ms"). | Done | `[[duck]] when, duck, db, attack_ms, release_ms`: while any audible voice is on `when` or under it, `duck` moves to `db` at the attack rate and back at the release rate, per 256-frame block. The amount in effect is reported per bus (`duck_db`). | `audio::buses_gain_mute_pan_and_duck_as_declared` |
| 2D spatialization by distance from the listener entity. | Done | `AudioListener` component (first active one), else the drawing camera (since item 7: the active `Camera` with the highest priority, the one the renderer uses), else the origin. `[spatial] min_distance, max_distance`: full volume inside `min`, a squared falloff to silence at `max`, equal-power pan by horizontal offset. A voice played on an entity follows it every frame; if the entity dies the sound finishes where it was. `spatial = false` on a sound opts out (UI, the player's own gun). | `audio::spatial_sounds_attenuate_and_pan_from_the_listener_and_follow_their_entity`, `argus-daemon/tests/audio.rs` |
| Sound events carry variation sets (pick one of five footsteps, pitch-jitter ±5%) so the game asks for sfx/step and the mixer chooses. | Done | `[sounds."name"] variations = [...]`, `gain_jitter`, `pitch_jitter`; the choice and both jitters come from a stream seeded by the run seed (`argus_sim::rng::stream(seed, "audio")`), three draws per play whatever the sound, so the same run makes the same choices. | `audio::variations_replay_from_the_seed_and_every_play_is_an_event`; `argus-daemon/tests/audio.rs` (the same run twice gives the same `sound.played` rows) |
| The music system is adaptive: a track is a set of layers; intensity is a parameter a system writes (`Music.intensity = enemies_near / 10`). | Done | `Music { track, intensity }` component; `[music.name] layers = [{ sound, from, to }]`: layers start together on one frame so they stay aligned, loop, and fade in between `from` and `to` (a step at `from` by default); a track change crossfades over `fade_ms`. Swarm's `late.music` writes intensity from `q:within`. Tempo and transitions on the grid: phase 3 item 7, below. | `audio::music_layers_follow_intensity_and_are_logged` |
| Hollow's room-to-room music and Outpost's day-night ambience are both this. | Done | Phase 3 item 7: Hollow's `systems/music.luau` picks the cave's, the hall's, or the deep's track by the hero's room (`InRoom`), intensity up the shaft and down the deep; Outpost's `systems/ambience.luau` the day's or the night's by the colony's clock, intensity from 1 at dawn to 0 at dusk (the birds). | Hollow's `tests/music.luau`, Outpost's `tests/ambience.luau` |

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
- **Variations replay from any frame.** A seek replays a stretch of the run, and `sound.played` has to name the same variations: the daemon calls `at_frame` before each request, and the variation stream is seeded by the run seed and the frame (phase 2 item 0). After a seek, `silence` fades out the sound effects of the moment left behind; music follows the restored world's `Music`.
- **Silence is cheap.** With no device open the daemon mixes as far as the sim clock, so the meters and `audio.clip` have something to read, and a discrete game's turn is one second of it. Mixing a second of zeros through every bus each turn made Delve's tests five times slower (found after the phase-1 gate). A block with no voice playing now takes a short path that leaves exactly the long path's state: meters count the samples, the limiter recovers, the history gets zeros. Delve's tests went from 5.2 s to 2.4 s; `audio::silence_mixes_to_the_same_state_by_the_short_path` compares the two paths.

| Test |
|---|
| `audio::a_burst_is_limited_not_clipped_and_a_capped_sound_steals_its_oldest_voice` |

## Output

| Spec | Status | Where | Test |
|---|---|---|---|
| (Phase 2 item 1.) Audio in the browser. | Partial (the spike) | The same `device.rs` over cpal's Web Audio backend (`argus-web` turns on cpal's `wasm-bindgen` feature). A browser allows sound only after a key press or a click, so the page opens the device on the first one (`WebPlayer::open_audio`). `rendered_frames` reports what the device pulled. cpal mixes on the page's main thread two 2048-frame buffers ahead (about 85 ms). Moving the mixer into an AudioWorklet was on the web spike's list for phase 2 item 7 (docs/web-spike.md) and was not done there or carried into a checklist; found in phase 3 item 7 and left open, since the design asks nothing of the audio thread: a frame slower than 85 ms on the page would starve the sound. | `tools/web/check.mjs`: 2.4 s mixed at 48 kHz, peak -24.7 dBFS from the footsteps, with Chrome muted |
| The core hosts audio (§2). | Done | `device.rs` (cpal 0.15, feature `device`): the output callback pulls blocks straight from the shared mixer, so sound runs in real time even on a discrete scene where nothing ticks between turns. The device prefers 48 kHz and the mixer rebuilds at the device's rate if it differs. `render.window` opens it with the window; `audio.device {open}` by hand; headless runs never open it. | (manual: `audio.device` opened the laptop's speakers and closed them) |

## Phase 3, item 7: tempo and changes on the grid

| Spec | Status | Where | Test |
|---|---|---|---|
| (§13) "A track is a set of layers with a tempo." | Done | `[music.name] bpm, beats_per_bar` (4 by default): `TrackDef::beat_s`, `bar_s`. A layer that is not a whole number of bars long drifts off the grid a little every loop; that is a load warning naming the layer, its length, and the bar's. | `audio::a_change_waits_for_the_bar_and_starts_on_it` (a 3.1 s loop at 120 bpm is said) |
| (§13) "Transitions between tracks happen on beat or bar boundaries." | Done | `change_on = "bar"` (the default with a tempo), `"beat"`, or `"now"`, on the track being left. When the world asks for another track, the one sounding stops fading at its next bar or beat and the new one starts on that very frame: the mixer holds a voice until its frame (`Mixer::play_at`) and starts a stop on one (`stop_at`), and measures where a track is by the frames it has sounded (`played`). Asked back to the sounding track before the bar, the change is undone (`cancel_stop`; the waiting voices never sound); asked for a third, it replaces the waiting one. Silence waits for the grid too. Without a tempo a change is at once, as before; a grid without a tempo is an `audio.toml` error. | `audio::a_change_waits_for_the_bar_and_starts_on_it` (asked for 0.5 s into a 2 s bar, the new track is exactly 0.25 s in at 2.25 s; on the beat; undone; silence) |
| (§13) "Intensity is a parameter a system writes." | Done | As in phase 1; a track waiting for its bar takes its layers' levels but does not report them until it sounds. | `audio::a_change_waits_for_the_bar_and_starts_on_it` |
| (§8) "All of it is observable as the event log; the human hears it, the agent reads it." | Done | `music.queued {track, from, on, in_ms}` when a change is asked for and waits; `music.started {track, layers, bpm}` when it sounds; `music.stopped {track}` when the old one starts to fade; `music.cancelled {track}`; `music.layer` as before. `AudioEngine::music_position`: the track sounding, its tempo, bar and beat (from 1), seconds, and a change waiting with the milliseconds to go; `audio.status` reports it. | Hollow's `tests/music.luau` (into the hall, asked for 2,967 ms before the cave's bar, started 2,950 ms later: within a frame); Outpost's `tests/ambience.luau` (dusk: 1,502 ms, started 1,517 ms later) |
| Headless, the change is measured from where the music is now. | Done | The daemon and the player advance the mixer to the sim's time before they read the world's `Music` (found in Outpost's test: measured from the last step's start, a 10-tick step put the logged time 167 ms off). | Outpost's `tests/ambience.luau` |

## Phase 3, item 8: mods' sounds and music

| Spec | Status | Where | Test |
|---|---|---|---|
| (§15) A mod carries sounds and music. | Done | `AudioConfig::lay_over`: a mod's `audio.toml` over the game's: buses, sounds, and tracks named in its namespace are added, a sound or track with the name of one before it replaced (reported), ducking rules added; a change to a game's bus, or a `[spatial]` table, is refused. `AudioEngine::load_layers` reads each mod's in load order, and a mod's WAVs named in its namespace are sounds. | Hollow's `tests/mods.luau` (the vigil's track changes on the bar); argus-player `mods::a_mod_adds_under_its_id_and_changes_what_came_before` |

## The phase 3 gate: tones made at once

| Spec | Status | Where | Test |
|---|---|---|---|
| (§17) Hollow's cold open under 300 ms, with item 7's music. | Done | `sound::synth_all`: every described tone of a load made on every core, longest first to the least loaded (in order, one by one, on the web, which has no threads); `AudioEngine::load_sounds` puts the sounds together from them. Hollow's music, several seconds of samples a layer, was 52 ms of every open made in turn; the open went from 267 to 305 ms to 226 to 238. | `audio` tests; argus-daemon `perf_rooms` |

## Phase 4: the sound waits for the game

| Spec | Status | Where | Test |
|---|---|---|---|
| (Kevin's play in the editor) The music does not play before Play. | Done | `AudioEngine::set_hold`: while held, the device's callback (`device.rs`) fills its buffer with silence and does not pull the mixer, so voices and music stay where they are and go on from there. The daemon holds it while its clock is stopped or paused. Headless the mixer follows sim time and is never held. | argus-daemon `stream::the_music_waits_for_play` |

## Known limitations, stated so they are decisions

- WAV only; OGG and other formats come with the importer.
- Resampling is linear. Fine for effects; music is authored at the mixer rate in practice.
- Voices started in one frame start on the next audio block, not at their tick's exact sample. At 60 Hz and 256-frame blocks the error is under 6 ms. A music change on the grid starts on its exact frame; the stop of the track it leaves begins on the block holding that frame.
- Layers come and go with intensity by a smooth fade, not on the grid.
- There is no event per bar or beat; `audio.status` says where the music is. With a device open the grid is the device's clock, so music events are the mixer's answers (as `sound.played` is), not part of a replay's check.
- The web page mixes on its main thread (see Output).
- The mixer is locked briefly by the daemon thread when it starts voices and moves spatial ones; the callback holds the same lock while it renders a block.
- Music layer events are observations of the frame they were noticed in; a headless run and a windowed run of the same inputs can report a layer turning on a frame apart. `sound.played` is exact.
