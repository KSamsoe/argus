//! §13 Audio: buses in a routing tree, ducking as data, 2D spatialization
//! from a listener, variation sets with jitter, layered music with an
//! intensity a system writes; §8 audio observed as events.

use argus_audio::{
    db_to_lin, lin_to_db, AudioConfig, AudioEngine, AudioListener, Mixer, Music, Tone, Wave,
};
use argus_ecs::{Transform, Value, World};

const TOML: &str = r#"
[buses]
master = { gain_db = 0 }
music  = { parent = "master", gain_db = -6 }
sfx    = { parent = "master" }
ui     = { parent = "master" }

[[duck]]
when = "ui"
duck = "music"
db = -12
attack_ms = 10
release_ms = 100

[spatial]
min_distance = 2
max_distance = 20

[sounds."sfx/step"]
gain = 0.8
pitch_jitter = 0.05
gain_jitter = 0.1
variations = [
  { wave = "noise", ms = 60 },
  { wave = "square", freq = 220, ms = 60 },
  { wave = "sine", freq = 330, ms = 60 },
]

[sounds."ui/click"]
bus = "ui"
tone = { wave = "sine", freq = 880, ms = 300 }

[sounds."music/pad"]
tone = { wave = "sine", notes = [220, 262, 330], note_ms = 200 }

[sounds."music/drums"]
tone = { wave = "noise", ms = 100, gain = 0.3 }

[music.explore]
layers = [ { sound = "music/pad" }, { sound = "music/drums", from = 0.5, to = 0.8 } ]
fade_ms = 50
"#;

/// A temp directory removed on drop, used as its path.
struct Temp(std::path::PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl std::ops::Deref for Temp {
    type Target = std::path::Path;
    fn deref(&self) -> &std::path::Path {
        &self.0
    }
}

fn project(name: &str, toml: &str) -> Temp {
    let dir = std::env::temp_dir().join(format!("argus-audio-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets").join("sfx")).unwrap();
    std::fs::write(dir.join("audio.toml"), toml).unwrap();
    Temp(dir)
}

fn peak(buf: &[f32], ch: usize) -> f32 {
    buf.iter()
        .skip(ch)
        .step_by(2)
        .fold(0.0f32, |m, s| m.max(s.abs()))
}

#[test]
fn the_routing_tree_and_its_errors_come_from_audio_toml() {
    let cfg = AudioConfig::parse("audio.toml", TOML).unwrap();
    assert_eq!(cfg.buses[0].name, "master", "the root first");
    assert_eq!(cfg.buses.len(), 4);
    assert_eq!(cfg.ducks[0].db, -12.0);
    assert_eq!(cfg.sounds["sfx/step"].variations.len(), 3);
    assert_eq!(
        cfg.sounds["sfx/step"].bus, "sfx",
        "sfx/ names default to the sfx bus"
    );
    assert_eq!(
        cfg.sounds["music/pad"].bus, "music",
        "music/ names default to music"
    );
    assert_eq!(cfg.tracks["explore"].layers[1].from, 0.5);
    assert!(AudioConfig::default()
        .buses
        .iter()
        .any(|b| b.name == "ambience"));

    for (bad, says, line) in [
        (
            "[buses]\na = { parent = \"b\" }\nb = { parent = \"a\" }\n",
            "loops",
            1,
        ),
        ("[buses]\na = { parent = \"nowhere\" }\n", "not a bus", 1),
        (
            "[sounds.x]\n\ntone = { wave = \"cello\" }\n",
            "unknown wave",
            1,
        ),
        ("\n\n[sounds.x]\nbus = \"sfx\"\n", "nothing to play", 3),
        (
            "[[duck]]\nwhen = \"dialogue\"\nduck = \"music\"\n",
            "not a bus",
            1,
        ),
    ] {
        let e = AudioConfig::parse("audio.toml", bad).unwrap_err();
        assert!(e.message.contains(says), "{bad:?}: {e}");
        assert_eq!(
            e.r#ref.as_deref(),
            Some(format!("audio.toml:{line}").as_str()),
            "{bad:?}"
        );
    }
}

#[test]
fn buses_gain_mute_pan_and_duck_as_declared() {
    let cfg = AudioConfig::parse("audio.toml", TOML).unwrap();
    let tone = argus_audio::sound::synth(
        &Tone {
            wave: Wave::Square,
            freq: 100.0,
            ms: 2000.0,
            attack_ms: 0.0,
            release_ms: 0.0,
            gain: 0.5,
            slide: 0.0,
            notes: vec![],
            note_ms: 1.0,
            ..Default::default()
        },
        48_000,
    );
    let mut m = Mixer::new(48_000, &cfg);
    let (sfx, music, ui) = (
        m.bus("sfx").unwrap(),
        m.bus("music").unwrap(),
        m.bus("ui").unwrap(),
    );

    // Centre pan: equal power, 0.5 * cos(pi/4) per channel.
    let v = m.play(tone.clone(), sfx, 1.0, 0.0, 1.0, false, 0.0);
    let mut out = vec![0.0; 4800 * 2];
    m.render(&mut out);
    assert!((peak(&out, 0) - 0.5 * std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3);
    // Hard left: nothing on the right.
    m.set(v, 1.0, -1.0);
    m.render(&mut out); // the ramp block
    m.render(&mut out);
    assert!(peak(&out, 1) < 1e-4, "right {}", peak(&out, 1));
    assert!((peak(&out, 0) - 0.5).abs() < 1e-3);
    m.stop(v, 0.0);
    m.render(&mut out);
    assert_eq!(m.voice_count(), 0, "a stopped voice fades and leaves");

    // The music bus sits at -6 dB.
    m.play(tone.clone(), music, 1.0, -1.0, 1.0, true, 0.0);
    m.render(&mut out);
    let base = peak(&out, 0);
    assert!(
        (lin_to_db(base / 0.5) + 6.0).abs() < 0.05,
        "{} dB",
        lin_to_db(base / 0.5)
    );
    // Anything on ui ducks music by 12 dB within the attack time...
    m.play(tone.clone(), ui, 1.0, 1.0, 1.0, false, 0.0); // hard right: not on the left channel
    m.render(&mut out); // 100 ms, attack is 10 ms
    m.render(&mut out);
    let ducked = peak(&out, 0);
    assert!(
        (lin_to_db(ducked / base) + 12.0).abs() < 0.1,
        "{} dB",
        lin_to_db(ducked / base)
    );
    let (meters, _, _) = m.take_meters();
    let mm = meters.iter().find(|b| b.name == "music").unwrap();
    assert!(
        (mm.duck_db + 12.0).abs() < 0.1 && (mm.gain_db + 6.0).abs() < 0.01,
        "{mm:?}"
    );
    // ...and comes back once ui is quiet (release 100 ms).
    let mut long = vec![0.0; 48_000 * 2 * 3];
    m.render(&mut long); // the ui tone ends after 2 s, then 100 ms release
    m.render(&mut out);
    assert!(
        (peak(&out, 0) - base).abs() < 1e-3,
        "released: {} vs {}",
        peak(&out, 0),
        base
    );
    // Mute silences a bus and everything under it.
    m.set_bus(m.bus("master").unwrap(), None, Some(true));
    m.render(&mut out);
    assert_eq!(peak(&out, 0), 0.0);
    // The history holds what was heard.
    assert_eq!(m.history(0.5).len(), 48_000);
    assert!(db_to_lin(-6.0) > 0.5 && db_to_lin(-6.0) < 0.51);
}

fn world_with_listener() -> (World, argus_ecs::Entity) {
    let mut w = World::new();
    w.register::<Transform>().unwrap();
    argus_audio::register(&mut w).unwrap();
    let l = w.spawn_authored("ears").unwrap();
    w.insert(l, Transform::at(0.0, 0.0)).unwrap();
    w.insert(l, AudioListener { active: true }).unwrap();
    (w, l)
}

fn f<'a>(fields: &'a [(String, Value)], k: &str) -> &'a Value {
    &fields
        .iter()
        .find(|(n, _)| n == k)
        .unwrap_or_else(|| panic!("no {k} in {fields:?}"))
        .1
}

#[test]
fn variations_replay_from_the_seed_and_every_play_is_an_event() {
    let dir = project("seed", TOML);
    let (w, _) = world_with_listener();
    let play = |seed: u64| {
        let mut e = AudioEngine::load(&dir, seed).unwrap();
        (0..12)
            .map(|_| {
                let ev = e.request(
                    &w,
                    "sound.play",
                    &[("sound".into(), Value::Str("sfx/step".into()))],
                );
                assert_eq!(ev.len(), 1);
                assert_eq!(ev[0].0, "sound.played");
                (
                    f(&ev[0].1, "variation").clone(),
                    f(&ev[0].1, "pitch").clone(),
                    f(&ev[0].1, "gain").clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    let a = play(7);
    assert_eq!(a, play(7), "same seed, same choices (section 6)");
    assert_ne!(a, play(8));
    let picked: std::collections::BTreeSet<String> =
        a.iter().map(|x| format!("{:?}", x.0)).collect();
    assert!(picked.len() >= 2, "a variation set varies: {picked:?}");
    for (_, pitch, gain) in &a {
        let (Value::Float(p), Value::Float(g)) = (pitch, gain) else {
            panic!()
        };
        assert!((0.95..=1.05).contains(p), "pitch jitter 5%: {p}");
        assert!((0.72..=0.88).contains(g), "gain 0.8 with 10% jitter: {g}");
    }

    let mut e = AudioEngine::load(&dir, 1).unwrap();
    let ev = e.request(
        &w,
        "sound.play",
        &[("sound".into(), Value::Str("sfx/nope".into()))],
    );
    assert_eq!(ev[0].0, "sound.missing");
    assert!(
        e.request(
            &w,
            "sound.play",
            &[("sound".into(), Value::Str("sfx/nope".into()))]
        )
        .is_empty(),
        "reported once"
    );
    assert_eq!(e.missing(), vec!["sfx/nope"]);
}

#[test]
fn spatial_sounds_attenuate_and_pan_from_the_listener_and_follow_their_entity() {
    let dir = project("spatial", TOML);
    let (mut w, ears) = world_with_listener();
    let src = w.spawn();
    w.insert(src, Transform::at(11.0, 0.0)).unwrap();
    let mut e = AudioEngine::load(&dir, 1).unwrap();
    let ev = e.request(
        &w,
        "sound.play",
        &[
            ("sound".into(), Value::Str("sfx/step".into())),
            ("entity".into(), Value::Entity(src)),
        ],
    );
    let fields = &ev[0].1;
    // 11 units: half way between 2 and 20, so (1 - 0.5)^2.
    assert_eq!(f(fields, "attenuation"), &Value::Float(0.25));
    assert_eq!(
        f(fields, "pan"),
        &Value::Float(1.0),
        "right of the listener"
    );
    assert_eq!(f(fields, "entity"), &Value::Entity(src));
    // Inside min_distance: full volume; left of the listener pans left.
    assert_eq!(e.spatialize((0.0, 0.0), (-1.0, 0.0)), (1.0, -0.1));
    assert_eq!(e.spatialize((0.0, 0.0), (25.0, 0.0)).0, 0.0);
    // The listener moves onto the source: the voice follows on update.
    w.set_field(ears, "Transform.pos", argus_ecs::Vec2::new(11.0, 0.0))
        .unwrap();
    assert!(e.update(&w).is_empty());
    assert_eq!(e.voices(), 1);
    // Without a listener component the active camera would be used; with none, the origin.
    assert_eq!(AudioEngine::listener(&World::new()), (0.0, 0.0));
}

#[test]
fn a_sound_is_stopped_by_its_name_spatial_or_not() {
    // A loop played from no entity could not be stopped from a script:
    // q:play names no voice, and sound.stop took a voice or an entity.
    let dir = project("stop", TOML);
    let (w, _) = world_with_listener();
    let mut e = AudioEngine::load(&dir, 1).unwrap();
    let play = [("sound".into(), Value::Str("sfx/step".into()))];
    for _ in 0..2 {
        assert_eq!(e.request(&w, "sound.play", &play)[0].0, "sound.played");
    }
    let ev = e.request(&w, "sound.stop", &play);
    assert_eq!(ev.len(), 2, "{ev:?}");
    assert!(ev.iter().all(|x| x.0 == "sound.stopped"));
    // Stopped already: nothing more to stop.
    assert!(e.request(&w, "sound.stop", &play).is_empty());
}

#[test]
fn music_layers_follow_intensity_and_are_logged() {
    let dir = project("music", TOML);
    let (mut w, _) = world_with_listener();
    let jukebox = w.spawn_authored("jukebox").unwrap();
    let track = w.strings().intern("explore");
    w.insert(
        jukebox,
        Music {
            track,
            intensity: 0.0,
        },
    )
    .unwrap();
    let mut e = AudioEngine::load(&dir, 1).unwrap();
    assert!(e.warnings.is_empty(), "{:?}", e.warnings);
    let names =
        |evs: &[argus_audio::AudioEvent]| evs.iter().map(|x| x.0.clone()).collect::<Vec<_>>();
    let ev = e.update(&w);
    assert_eq!(names(&ev), ["music.started", "music.layer"], "{ev:?}");
    assert_eq!(f(&ev[1].1, "layer"), &Value::Str("music/pad".into()));
    assert_eq!(
        e.music().unwrap().1,
        vec![("music/pad".into(), true), ("music/drums".into(), false)]
    );
    // A system raises intensity: the drums come in, and that is an event.
    w.set_field(jukebox, "Music.intensity", 0.9).unwrap();
    let ev = e.update(&w);
    assert_eq!(names(&ev), ["music.layer"]);
    assert_eq!(f(&ev[0].1, "on"), &Value::Bool(true));
    assert!(e.update(&w).is_empty(), "no change, no event");
    // Headless, sim time drives the mixer; the clip is what played.
    e.advance_to(0.0);
    e.advance_to(0.5);
    let (meters, master_peak, _, _) = e.meters();
    assert!(master_peak > -40.0, "{master_peak}");
    assert!(meters.iter().find(|b| b.name == "music").unwrap().peak_db > -60.0);
    let wav = dir.join("clip.wav");
    let (frames, peak_db) = e.clip(0.5, &wav).unwrap();
    assert_eq!(frames, 24_000);
    assert!(peak_db > -40.0);
    assert!(std::fs::metadata(&wav).unwrap().len() > 90_000);
    // Silence: clear the track.
    w.set_field(jukebox, "Music.track", Value::Str(String::new()))
        .unwrap();
    assert_eq!(names(&e.update(&w)), ["music.stopped"]);
    assert!(e.music().is_none());
}

#[test]
fn a_wav_under_assets_is_a_sound_by_its_path() {
    let dir = project("wav", "");
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 22_050,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut wr = hound::WavWriter::create(dir.join("assets/sfx/hit.wav"), spec).unwrap();
    for i in 0..2205 {
        let s = ((i as f32 * 0.1).sin() * 16000.0) as i16;
        wr.write_sample(s).unwrap();
        wr.write_sample(s).unwrap();
    }
    wr.finalize().unwrap();
    std::fs::write(dir.join("assets/sfx/broken.wav"), b"not a wav").unwrap();
    let e = AudioEngine::load(&dir, 1).unwrap();
    assert!(
        e.sound_names().contains(&"sfx/hit".to_string()),
        "{:?}",
        e.sound_names()
    );
    assert!(!e.sound_names().contains(&"sfx/broken".to_string()));
    assert_eq!(e.warnings.len(), 1, "{:?}", e.warnings);
    assert!(e.warnings[0].contains("broken.wav"));
    let (w, _) = world_with_listener();
    let mut e = e;
    let ev = e.request(
        &w,
        "sound.play",
        &[("sound".into(), Value::Str("sfx/hit".into()))],
    );
    assert_eq!(f(&ev[0].1, "bus"), &Value::Str("sfx".into()));
}

#[test]
fn a_burst_is_limited_not_clipped_and_a_capped_sound_steals_its_oldest_voice() {
    let cfg = AudioConfig::parse("audio.toml", TOML).unwrap();
    let loud = argus_audio::sound::synth(
        &Tone {
            wave: Wave::Square,
            freq: 100.0,
            ms: 500.0,
            attack_ms: 0.0,
            release_ms: 0.0,
            gain: 0.5,
            slide: 0.0,
            notes: vec![],
            note_ms: 1.0,
            ..Default::default()
        },
        48_000,
    );
    let mut m = Mixer::new(48_000, &cfg);
    let sfx = m.bus("sfx").unwrap();
    for _ in 0..10 {
        m.play(loud.clone(), sfx, 1.0, 0.0, 1.0, false, 0.0);
    }
    let mut out = vec![0.0; 4800 * 2];
    m.render(&mut out);
    assert!(
        peak(&out, 0) <= argus_audio::mixer::CEILING + 1e-4,
        "{}",
        peak(&out, 0)
    );
    let (meters, _, _) = m.take_meters();
    let master = meters.iter().find(|b| b.name == "master").unwrap();
    assert!(
        master.peak_db > 10.0,
        "the master row is the mix before the limiter: {master:?}"
    );
    let sfx_m = meters.iter().find(|b| b.name == "sfx").unwrap();
    assert!(
        (sfx_m.peak_db - master.peak_db).abs() < 0.01,
        "master contains its children"
    );
    assert!(m.take_limiter_db() < -10.0);
    // After the burst the limiter lets go.
    let mut long = vec![0.0; 48_000 * 2];
    m.render(&mut long);
    m.take_limiter_db();
    m.render(&mut out);
    assert_eq!(m.take_limiter_db(), 0.0);

    let dir = project(
        "steal",
        "[sounds.\"sfx/pop\"]\nmax_voices = 2\ntone = { wave = \"noise\", ms = 500 }\n",
    );
    let (w, _) = world_with_listener();
    let mut e = AudioEngine::load(&dir, 1).unwrap();
    let pop = || vec![("sound".to_string(), Value::Str("sfx/pop".into()))];
    let first = e.request(&w, "sound.play", &pop());
    let v1 = f(&first[0].1, "voice").clone();
    e.request(&w, "sound.play", &pop());
    let third = e.request(&w, "sound.play", &pop());
    assert_eq!(
        f(&third[0].1, "stole"),
        &v1,
        "the oldest voice goes, and the log says so"
    );
}

/// Found after the phase-1 gate: with no device open the daemon mixes as far
/// as the sim clock, and a discrete game's turn is a second, so every turn
/// mixed a second of zeros through every bus. Silence now takes a short
/// path, and it must leave exactly what the long one did.
#[test]
fn silence_mixes_to_the_same_state_by_the_short_path() {
    let cfg = AudioConfig::parse("audio.toml", TOML).unwrap();
    let tone = |gain: f32| {
        argus_audio::sound::synth(
            &Tone {
                wave: Wave::Square,
                freq: 220.0,
                ms: 300.0,
                attack_ms: 0.0,
                release_ms: 0.0,
                gain,
                slide: 0.0,
                notes: vec![],
                note_ms: 1.0,
                ..Default::default()
            },
            48_000,
        )
    };
    let run = |hold_open: bool| {
        let mut m = Mixer::new(48_000, &cfg);
        let (sfx, ui) = (m.bus("sfx").unwrap(), m.bus("ui").unwrap());
        if hold_open {
            // Silent and inaudible (gain 0, so it ducks nothing), but playing:
            // every block takes the long path.
            m.play(tone(0.0), sfx, 0.0, 0.0, 1.0, true, 0.0);
        }
        // Loud enough for the limiter, and a ui sound that ducks the music.
        for _ in 0..8 {
            m.play(tone(0.5), sfx, 1.0, 0.0, 1.0, false, 0.0);
        }
        m.play(tone(0.5), ui, 1.0, 0.3, 1.0, false, 0.0);
        m.advance(48_000 / 10);
        let early = (m.take_meters(), m.take_limiter_db());
        m.advance(48_000 * 3);
        let late = (m.take_meters(), m.take_limiter_db());
        (early, late, m.history(10.0), m.rendered_frames())
    };
    let (long, short) = (run(true), run(false));
    assert!(
        long.0 .1 < -0.1,
        "the burst reached the limiter: {:?}",
        long.0 .1
    );
    assert_eq!(
        format!("{:?}", short.0),
        format!("{:?}", long.0),
        "during the sounds"
    );
    assert_eq!(
        format!("{:?}", short.1),
        format!("{:?}", long.1),
        "after them"
    );
    assert_eq!(short.3, long.3);
    assert_eq!(short.2.len(), long.2.len());
    assert!(short.2 == long.2, "the history differs");
}

/// Two tracks at 120 beats a minute, four to the bar: a bar is 2 s, 96,000
/// frames at 48 kHz. Each loops two bars of eight half-second notes.
const TEMPO: &str = r#"
[buses]
master = { gain_db = 0 }
music  = { parent = "master" }

[sounds."music/calm"]
tone = { wave = "sine", notes = [220, 262, 330, 262, 220, 196, 220, 262], note_ms = 500 }

[sounds."music/fight"]
tone = { wave = "square", notes = [110, 110, 131, 110, 147, 110, 131, 98], note_ms = 500, gain = 0.2 }

[sounds."music/odd"]
tone = { wave = "sine", freq = 440, ms = 3100 }

[music.calm]
layers = [ { sound = "music/calm" } ]
bpm = 120
fade_ms = 20

[music.fight]
layers = [ { sound = "music/fight" } ]
bpm = 120
change_on = "beat"
fade_ms = 20
"#;

#[test]
fn a_change_waits_for_the_bar_and_starts_on_it() {
    let dir = project("tempo", TEMPO);
    let (mut w, _) = world_with_listener();
    let jukebox = w.spawn_authored("jukebox").unwrap();
    let calm = w.strings().intern("calm");
    w.insert(
        jukebox,
        Music {
            track: calm,
            intensity: 0.0,
        },
    )
    .unwrap();
    let mut e = AudioEngine::load(&dir, 1).unwrap();
    assert!(e.warnings.is_empty(), "{:?}", e.warnings);
    let names =
        |evs: &[argus_audio::AudioEvent]| evs.iter().map(|x| x.0.clone()).collect::<Vec<_>>();
    // From silence, at once.
    let ev = e.update(&w);
    assert_eq!(names(&ev)[0], "music.started");
    assert_eq!(f(&ev[0].1, "bpm"), &Value::Float(120.0));
    e.advance_to(0.0);
    e.advance_to(0.5);
    let p = e.music_position().unwrap();
    assert_eq!(
        (p.bar, p.beat),
        (Some(1), Some(2)),
        "half a second in: the second beat"
    );

    // Asked for in the first bar, the fight waits for the second.
    w.set_field(jukebox, "Music.track", Value::Str("fight".into()))
        .unwrap();
    let ev = e.update(&w);
    assert_eq!(names(&ev), ["music.queued"], "{ev:?}");
    assert_eq!(f(&ev[0].1, "from"), &Value::Str("calm".into()));
    assert_eq!(f(&ev[0].1, "on"), &Value::Str("bar".into()));
    assert_eq!(f(&ev[0].1, "in_ms"), &Value::Float(1500.0));
    let p = e.music_position().unwrap();
    assert_eq!(
        p.track.as_deref(),
        Some("calm"),
        "calm plays on until the bar"
    );
    assert_eq!(p.queued, Some(("fight".to_string(), 1500.0)));
    e.advance_to(1.75);
    assert!(
        !names(&e.update(&w)).contains(&"music.started".to_string()),
        "not yet"
    );
    // Past the bar: the calm stops, the fight sounds, and it started on the
    // bar's very frame: a quarter of a second in, exactly.
    e.advance_to(2.25);
    let ev = e.update(&w);
    assert_eq!(
        names(&ev)[..2],
        ["music.stopped", "music.started"],
        "{ev:?}"
    );
    assert_eq!(f(&ev[0].1, "track"), &Value::Str("calm".into()));
    let p = e.music_position().unwrap();
    assert_eq!(p.track.as_deref(), Some("fight"));
    assert_eq!(p.seconds, 0.25);
    assert_eq!((p.bar, p.beat), (Some(1), Some(1)));

    // The fight changes on the beat: asked for a quarter of a second into
    // a beat, the calm waits a quarter of a second. Asked back before it,
    // the change is undone and the fight plays on.
    w.set_field(jukebox, "Music.track", Value::Str("calm".into()))
        .unwrap();
    let ev = e.update(&w);
    assert_eq!(names(&ev), ["music.queued"]);
    assert_eq!(f(&ev[0].1, "on"), &Value::Str("beat".into()));
    assert_eq!(f(&ev[0].1, "in_ms"), &Value::Float(250.0));
    w.set_field(jukebox, "Music.track", Value::Str("fight".into()))
        .unwrap();
    let ev = e.update(&w);
    assert_eq!(names(&ev), ["music.cancelled"]);
    assert_eq!(f(&ev[0].1, "track"), &Value::Str("calm".into()));
    e.advance_to(2.75);
    assert!(e.update(&w).is_empty(), "nothing waits");
    let p = e.music_position().unwrap();
    assert_eq!(p.track.as_deref(), Some("fight"));
    assert_eq!(p.seconds, 0.75, "the fight never stopped");

    // Silence waits for the grid too.
    w.set_field(jukebox, "Music.track", Value::Str(String::new()))
        .unwrap();
    assert_eq!(names(&e.update(&w)), ["music.queued"]);
    e.advance_to(3.25);
    assert_eq!(names(&e.update(&w)), ["music.stopped"]);
    assert!(e.music().is_none());
    drop(dir);

    // A loop that is not a whole number of bars is said at load.
    let dir = project(
        "tempo-odd",
        &(TEMPO.to_string() + "\n[music.odd]\nlayers = [ { sound = \"music/odd\" } ]\nbpm = 120\n"),
    );
    let e = AudioEngine::load(&dir, 1).unwrap();
    assert!(
        e.warnings
            .iter()
            .any(|w| w.contains("music.odd") && w.contains("not a whole number")),
        "{:?}",
        e.warnings
    );
    // And a grid without a tempo is an error.
    let bad = AudioConfig::parse(
        "audio.toml",
        "[sounds.\"a\"]\ntone = { wave = \"sine\", ms = 100 }\n[music.x]\nlayers = [ { sound = \"a\" } ]\nchange_on = \"bar\"\n",
    )
    .unwrap_err();
    assert!(bad.message.contains("needs a bpm"), "{bad:?}");
}

/// Phase 5 item 10: the web player's AudioWorklet is an output the host
/// drives. Opening it rebuilds the sounds at its rate, it is the device's
/// name, its blocks are the mixer's, and while the game is not playing
/// they are silence and the mixer waits.
#[test]
fn an_external_output_runs_the_mixer_at_its_rate_and_pulls_its_blocks() {
    let dir = project("external", TOML);
    let (w, _) = world_with_listener();
    let mut e = AudioEngine::load(&dir, 1).unwrap();
    e.open_external(44_100, "AudioWorklet");
    assert_eq!(e.rate(), 44_100);
    assert_eq!(e.device_name().as_deref(), Some("AudioWorklet"));
    e.request(
        &w,
        "sound.play",
        &[("sound".into(), Value::Str("ui/click".into()))],
    );
    let mut block = vec![0.0f32; 4410 * 2];
    e.pull(&mut block);
    assert!(peak(&block, 0) > 0.01, "{}", peak(&block, 0));
    assert_eq!(e.rendered_frames(), 4410);
    // The sim's time does not move a mixer an output pulls: advanced by
    // it as well, a page's mixer ran at twice its rate, and a turn-based
    // game's skipped a turn's time at each press (2026-10-10).
    e.advance_to(0.0);
    e.advance_to(0.5);
    e.advance_to(1.5);
    assert_eq!(e.rendered_frames(), 4410, "only what the output pulled");

    e.set_hold(true);
    let mut held = vec![1.0f32; 128 * 2];
    e.pull(&mut held);
    assert!(held.iter().all(|s| *s == 0.0));
    assert_eq!(e.rendered_frames(), 4410, "held, the mixer waits");
    e.set_hold(false);

    // The click is 300 ms at 44.1 kHz: 13230 frames, then no voice.
    let mut rest = vec![0.0f32; 9000 * 2];
    e.pull(&mut rest);
    assert_eq!(e.voices(), 0);
    std::fs::remove_dir_all(&*dir).ok();
}
