//! Argus audio, phase 1 item 3: a mixer with named buses in a routing
//! tree, ducking rules as data, 2D spatialization from a listener entity,
//! variation sets with pitch and gain jitter, a first layered-music model,
//! and every sound observable as an event (§8, §13).
//!
//! Gameplay asks for a sound by name with a `sound.play` event
//! (`q:play("sfx/step", {entity = e})` from Luau); the engine picks the
//! variation with a stream seeded from the run seed, starts a voice, and
//! answers with a `sound.played` event naming the variation, gain, pitch,
//! bus, entity, pan, and attenuation. The agent reads that log; the human
//! hears the device. A `Music` component names the playing track and its
//! intensity, which a system writes; layers fade in and out with it.
//!
//! With a device open the output callback pulls from the mixer in real
//! time. Headless, the daemon advances the mixer by sim time, so meters
//! and `audio.clip` are the same on every run.

#![warn(missing_docs)]

pub mod config;
#[cfg(feature = "device")]
pub mod device;
pub mod mixer;
pub mod sound;

pub use config::{
    parse_sound_sidecar, AudioConfig, BusDef, ChangeOn, DuckDef, LayerDef, SoundDef, Source, Tone,
    TrackDef, Wave,
};
pub use mixer::{db_to_lin, lin_to_db, BusMeter, Mixer};

use argus_ecs::{
    ArgusError, Component, Entity, GlobalTransform, Result, StrId, Transform, Value, World,
};
use argus_sim::Pcg32;
use sound::Clip;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Error codes this crate adds (51xx).
pub mod codes {
    /// `audio.toml` is malformed.
    pub const BAD_AUDIO_TOML: u32 = 5101;
    /// A sound file could not be decoded.
    pub const BAD_SOUND: u32 = 5102;
    /// No output device.
    pub const NO_DEVICE: u32 = 5103;
    /// No sound by that name.
    pub const UNKNOWN_SOUND: u32 = 5104;
    /// No bus by that name.
    pub const UNKNOWN_BUS: u32 = 5105;
    /// A clip could not be written.
    pub const CLIP: u32 = 5106;
}

/// The mixer's rate when no device dictates another.
pub const RATE: u32 = 48_000;

/// Marks the entity sounds are heard from. Without one, the active
/// `Camera` is the listener, then the origin.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "Hears the world: spatial sounds are attenuated and panned by their distance from this entity"
)]
pub struct AudioListener {
    /// The first active listener is used.
    #[argus(doc = "Whether this listener is the one in use")]
    pub active: bool,
}

impl Default for AudioListener {
    fn default() -> Self {
        AudioListener { active: true }
    }
}

/// The music playing: a track from `audio.toml` and the intensity its
/// layers follow (§13 "intensity is a parameter a system writes").
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(doc = "Plays a music track from audio.toml; layers fade in as intensity rises")]
pub struct Music {
    /// Track name; empty is silence.
    #[argus(doc = "Track from audio.toml [music.<name>]; empty is silence")]
    pub track: StrId,
    /// 0 to 1.
    #[argus(doc = "How intense the music is, 0 to 1", range(0, 1), tune)]
    pub intensity: f32,
}

impl Default for Music {
    fn default() -> Self {
        Music {
            track: StrId::EMPTY,
            intensity: 0.0,
        }
    }
}

/// Register the audio components.
pub fn register(world: &mut World) -> Result<()> {
    world.register::<AudioListener>()?;
    world.register::<Music>()?;
    Ok(())
}

struct Loaded {
    def: SoundDef,
    clips: Vec<Clip>,
}

struct SpatialVoice {
    voice: u64,
    entity: Entity,
    base_gain: f32,
    last: (f32, f32),
}

struct Layer {
    voice: u64,
    def: LayerDef,
    on: bool,
}

struct Playing {
    track: String,
    def: TrackDef,
    layers: Vec<Layer>,
    /// Sounding (not waiting for a bar).
    started: bool,
}

fn started_event(p: &Playing) -> AudioEvent {
    let mut f = vec![
        ("track".to_string(), Value::Str(p.track.clone())),
        ("layers".to_string(), Value::Int(p.layers.len() as i64)),
    ];
    if let Some(b) = p.def.bpm {
        f.push(("bpm".to_string(), Value::Float(b as f64)));
    }
    ("music.started".to_string(), f)
}

/// Where the music is (`AudioEngine::music_position`).
#[derive(Clone, Debug, PartialEq)]
pub struct MusicPosition {
    /// The track sounding.
    pub track: Option<String>,
    /// Its tempo.
    pub bpm: Option<f32>,
    /// Bar, from 1, when it has a tempo.
    pub bar: Option<u64>,
    /// Beat in the bar, from 1.
    pub beat: Option<u64>,
    /// Seconds the track has sounded.
    pub seconds: f64,
    /// A change waiting for its bar: the track, and milliseconds to go.
    pub queued: Option<(String, f32)>,
}

/// An event the engine wants in the log: a name and its fields.
pub type AudioEvent = (String, Vec<(String, Value)>);

/// The engine: sounds, the shared mixer, the listener, the music.
pub struct AudioEngine {
    root: PathBuf,
    /// The configuration in use.
    pub config: AudioConfig,
    sounds: BTreeMap<String, Loaded>,
    mixer: Arc<Mutex<Mixer>>,
    rate: u32,
    seed: u64,
    rng: Pcg32,
    #[cfg(feature = "device")]
    device: Option<device::Output>,
    /// An output the host drives and pulls blocks for (the browser's
    /// AudioWorklet, phase 5 item 10): its name.
    external: Option<String>,
    /// Set while the game is not playing: the device sends silence and the
    /// mixer waits (phase 4 item 1: the editor opened the sound before its
    /// Play, and the music played).
    hold: Arc<AtomicBool>,
    spatial: Vec<SpatialVoice>,
    /// The track the world asks for (waiting for its bar while `due`).
    music: Option<Playing>,
    /// The track that plays until a change's bar.
    outgoing: Option<Playing>,
    /// The mixer frame a waiting change happens on.
    due: Option<u64>,
    missing: BTreeSet<String>,
    /// Sounds from asset sidecars (the importer), below audio.toml and above bare WAVs.
    extra: Vec<SoundDef>,
    /// Voices of each sound, oldest first, for the per-sound cap.
    by_sound: BTreeMap<String, std::collections::VecDeque<u64>>,
    /// Problems found loading sounds: file:line and a sentence each.
    pub warnings: Vec<String>,
    last_time: Option<f64>,
    /// Sounds started since load.
    pub played: u64,
    /// The frame `rng` was seeded for (`at_frame`).
    rng_frame: Option<u64>,
    /// The game and its mods (phase 3 item 8).
    layers: argus_ecs::Layers,
    /// What the mods changed of the game's sounds and tracks.
    changes: Vec<argus_ecs::layers::Change>,
    /// Mods' audio.toml files refused.
    mod_problems: Vec<ArgusError>,
}

fn round3(x: f32) -> f64 {
    ((x as f64) * 1000.0).round() / 1000.0
}

/// A sound file's name: its path without the extension.
fn sound_name(f: &str) -> &str {
    let lower = f.to_ascii_lowercase();
    match lower.rfind('.') {
        Some(i) if matches!(&lower[i..], ".wav" | ".ogg") => &f[..i],
        _ => f,
    }
}

/// The sound files (WAV and Ogg Vorbis) under `dir`, relative to `base`.
fn sound_files(dir: &Path, base: &Path, out: &mut Vec<String>) {
    let Ok(rd) = argus_os::fs::read_dir(dir) else {
        return;
    };
    for e in rd {
        let p = e.path;
        if e.is_dir {
            sound_files(&p, base, out);
        } else if p
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("wav") || x.eq_ignore_ascii_case("ogg"))
        {
            if let Ok(rel) = p.strip_prefix(base) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
}

fn field<'a>(fields: &'a [(String, Value)], k: &str) -> Option<&'a Value> {
    fields.iter().find(|(n, _)| n == k).map(|(_, v)| v)
}

fn num(v: Option<&Value>) -> Option<f32> {
    match v? {
        Value::Float(f) => Some(*f as f32),
        Value::Int(i) => Some(*i as f32),
        Value::UInt(u) => Some(*u as f32),
        _ => None,
    }
}

fn text(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::Str(s) | Value::Ref(s) => Some(s.clone()),
        _ => None,
    }
}

impl AudioEngine {
    /// Load `audio.toml` and every sound under `assets/`, seeded from the
    /// run seed so variation choices replay.
    pub fn load(root: &Path, seed: u64) -> Result<AudioEngine> {
        AudioEngine::load_layers(&argus_ecs::Layers::game(root), seed)
    }

    /// [`load`](AudioEngine::load) for the game and its mods (phase 3
    /// item 8): each mod's `audio.toml` over the game's
    /// ([`AudioConfig::lay_over`]), and each mod's WAVs named in its
    /// namespace as sounds. A mod whose `audio.toml` is refused adds
    /// nothing, and the refusal is in [`mod_problems`](Self::mod_problems).
    pub fn load_layers(layers: &argus_ecs::Layers, seed: u64) -> Result<AudioEngine> {
        let root = layers.game.as_path();
        let mut config = AudioConfig::load(root)?;
        let mut changes = Vec::new();
        let mut problems = Vec::new();
        for l in layers.each().into_iter().skip(1) {
            let id = l.id.unwrap_or_default();
            let p = l.root.join("audio.toml");
            let Ok(text) = argus_os::fs::read_to_string(&p) else {
                continue;
            };
            match config.lay_over(id, &l.show("audio.toml"), &text, &l.root.join("assets")) {
                Ok((c, ch)) => {
                    config = c;
                    changes.extend(ch);
                }
                Err(e) => problems.push(e),
            }
        }
        let mixer = Arc::new(Mutex::new(Mixer::new(RATE, &config)));
        let mut e = AudioEngine {
            root: root.to_path_buf(),
            config,
            sounds: BTreeMap::new(),
            mixer,
            rate: RATE,
            seed,
            rng: argus_sim::rng::stream(seed, "audio"),
            #[cfg(feature = "device")]
            device: None,
            external: None,
            hold: Arc::new(AtomicBool::new(false)),
            spatial: Vec::new(),
            music: None,
            outgoing: None,
            due: None,
            missing: BTreeSet::new(),
            by_sound: BTreeMap::new(),
            extra: Vec::new(),
            warnings: Vec::new(),
            last_time: None,
            played: 0,
            rng_frame: None,
            layers: argus_ecs::Layers::game(root),
            changes: Vec::new(),
            mod_problems: Vec::new(),
        };
        e.layers = layers.clone();
        e.changes = changes;
        e.mod_problems = problems;
        e.load_sounds();
        Ok(e)
    }

    /// What the mods changed of the game's sounds and tracks (phase 3
    /// item 8).
    pub fn changes(&self) -> &[argus_ecs::layers::Change] {
        &self.changes
    }

    /// Mods' `audio.toml` files that were refused, each with why.
    pub fn mod_problems(&self) -> &[ArgusError] {
        &self.mod_problems
    }

    fn load_sounds(&mut self) {
        self.sounds.clear();
        self.warnings.clear();
        let assets = self.root.join("assets");
        let mut defs: Vec<SoundDef> = self.config.sounds.values().cloned().collect();
        // Sidecar sounds, unless audio.toml defines the same name.
        let buses = self.lock().bus_names();
        for d in &self.extra {
            if self.config.sounds.contains_key(&d.name) {
                continue;
            }
            let mut d = d.clone();
            if !buses.contains(&d.bus) {
                self.warnings.push(format!(
                    "{}: bus {:?} is not a bus in audio.toml; using sfx",
                    d.at, d.bus
                ));
                d.bus = "sfx".to_string();
            }
            defs.push(d);
        }
        // Every sound file under assets/ is a sound by its path, unless
        // defined; a mod's, when named in its namespace, by its whole path.
        let mut files = Vec::new();
        sound_files(&assets, &assets, &mut files);
        let mut named: Vec<(String, String)> = files
            .into_iter()
            .map(|f| (sound_name(&f).to_string(), f))
            .collect();
        for l in self.layers.each().into_iter().skip(1) {
            let dir = l.root.join("assets");
            let mut mine = Vec::new();
            sound_files(&dir, &dir, &mut mine);
            for f in mine {
                let name = sound_name(&f).to_string();
                if l.id.is_some_and(|id| argus_ecs::layers::owns(id, &name)) {
                    named.push((name, dir.join(&f).to_string_lossy().into_owned()));
                }
            }
        }
        for (name, f) in named {
            if !defs.iter().any(|d| d.name == name) {
                defs.push(SoundDef {
                    bus: if name.starts_with("music/") {
                        "music"
                    } else {
                        "sfx"
                    }
                    .to_string(),
                    name,
                    gain: 1.0,
                    gain_jitter: 0.0,
                    pitch_jitter: 0.0,
                    variations: vec![Source::File(f)],
                    spatial: true,
                    looping: false,
                    max_voices: 8,
                    at: "assets/".to_string(),
                });
            }
        }
        // Every tone at once, then the sounds put together from them.
        let tones: Vec<&config::Tone> = defs
            .iter()
            .flat_map(|d| d.variations.iter())
            .filter_map(|v| match v {
                Source::Tone(t) => Some(t),
                Source::File(_) => None,
            })
            .collect();
        let mut made = sound::synth_all(&tones, self.rate).into_iter();
        for def in defs {
            let mut clips = Vec::new();
            for v in &def.variations {
                match v {
                    Source::Tone(_) => clips.extend(made.next()),
                    Source::File(f) => match sound::load(&assets.join(f), self.rate) {
                        Ok(c) => clips.push(c),
                        Err(e) => self.warnings.push(format!("{}: {}", def.at, e.message)),
                    },
                }
            }
            if clips.is_empty() {
                continue;
            }
            self.sounds.insert(def.name.clone(), Loaded { def, clips });
        }
        for t in self.config.tracks.values() {
            for l in &t.layers {
                match self.sounds.get(&l.sound) {
                    None => self.warnings.push(format!(
                        "{}: music.{} layer {:?} names no sound",
                        self.config.file, t.name, l.sound
                    )),
                    // A layer that is not a whole number of bars drifts off
                    // the grid its changes wait for, a little every loop.
                    Some(s) => {
                        if let Some(bar) = t.bar_s() {
                            let secs = s.clips[0].len() as f32 / self.rate as f32;
                            let bars = secs / bar;
                            if (bars - bars.round()).abs() * bar > 0.005 || bars.round() < 1.0 {
                                self.warnings.push(format!(
                                    "{}: music.{} layer {:?} is {secs:.3} s long, not a whole number of {bar:.3} s bars at {} bpm; its loop drifts off the bar",
                                    self.config.file, t.name, l.sound, t.bpm.unwrap_or(0.0)
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    /// Replace the sounds that come from asset sidecars and reload every
    /// sound. The mixer keeps running: voices playing keep their old
    /// samples, new plays get the new ones (hot reload).
    pub fn set_asset_sounds(&mut self, defs: Vec<SoundDef>) {
        self.extra = defs;
        self.load_sounds();
    }

    /// Reload every sound from disk (a WAV changed).
    pub fn reload_sounds(&mut self) {
        self.load_sounds();
    }

    /// Sounds by name.
    pub fn sound_names(&self) -> Vec<String> {
        self.sounds.keys().cloned().collect()
    }

    /// Sounds asked for that do not exist.
    pub fn missing(&self) -> Vec<String> {
        self.missing.iter().cloned().collect()
    }

    /// The mixer rate.
    pub fn rate(&self) -> u32 {
        self.rate
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Mixer> {
        match self.mixer.lock() {
            Ok(m) => m,
            Err(p) => p.into_inner(),
        }
    }

    // ----- the listener and the distance model ------------------------------

    fn position(world: &World, e: Entity) -> Option<(f32, f32)> {
        if let Ok(g) = world.get::<GlobalTransform>(e) {
            return Some((g.pos.x, g.pos.y));
        }
        world.get::<Transform>(e).ok().map(|t| (t.pos.x, t.pos.y))
    }

    /// Where sounds are heard from: the first active `AudioListener`, else
    /// the drawing camera (the active one with the highest priority, item
    /// 7), else the origin.
    pub fn listener(world: &World) -> (f32, f32) {
        if let Some(l) = world.component_id_of::<AudioListener>() {
            for e in world.query().has(l).entities() {
                if world
                    .get::<AudioListener>(e)
                    .map(|a| a.active)
                    .unwrap_or(false)
                {
                    if let Some(p) = Self::position(world, e) {
                        return p;
                    }
                }
            }
        }
        if let Some(p) = argus_camera::current(world).and_then(|e| Self::position(world, e)) {
            return p;
        }
        (0.0, 0.0)
    }

    /// Attenuation and pan for a source at `p` heard from `l`.
    pub fn spatialize(&self, l: (f32, f32), p: (f32, f32)) -> (f32, f32) {
        let s = self.config.spatial;
        let (dx, dy) = (p.0 - l.0, p.1 - l.1);
        let d = (dx * dx + dy * dy).sqrt();
        let att = if d <= s.min_distance {
            1.0
        } else if d >= s.max_distance {
            0.0
        } else {
            let t = (d - s.min_distance) / (s.max_distance - s.min_distance);
            (1.0 - t) * (1.0 - t)
        };
        let pan = (dx / s.max_distance * 2.0).clamp(-1.0, 1.0);
        (att, pan)
    }

    // ----- requests -----------------------------------------------------------

    /// The requests that follow came from `frame`: variations are drawn
    /// from a stream seeded by the run seed and the frame, so a seek that
    /// replays the frame draws the same ones (phase 2). Without a call, one
    /// stream serves the whole run.
    pub fn at_frame(&mut self, frame: u64) {
        if self.rng_frame != Some(frame) {
            self.rng = argus_sim::rng::stream(self.seed, &format!("audio@{frame}"));
            self.rng_frame = Some(frame);
        }
    }

    /// Handle one `sound.play` or `sound.stop` request; the events to log.
    pub fn request(
        &mut self,
        world: &World,
        name: &str,
        fields: &[(String, Value)],
    ) -> Vec<AudioEvent> {
        match name {
            "sound.play" => self.play(world, fields),
            "sound.stop" => self.stop(fields),
            _ => Vec::new(),
        }
    }

    fn play(&mut self, world: &World, fields: &[(String, Value)]) -> Vec<AudioEvent> {
        let Some(name) = text(field(fields, "sound")) else {
            return vec![(
                "sound.missing".to_string(),
                vec![("sound".to_string(), Value::Str(String::new()))],
            )];
        };
        let Some(loaded) = self.sounds.get(&name) else {
            if self.missing.insert(name.clone()) {
                return vec![(
                    "sound.missing".to_string(),
                    vec![("sound".to_string(), Value::Str(name))],
                )];
            }
            return Vec::new();
        };
        let n_clips = loaded.clips.len() as u32;
        let def = loaded.def.clone();
        // Always three draws, so the stream stays aligned whatever the sound.
        let pick = self.rng.below(n_clips) as usize;
        let rg = self.rng.next_f64() as f32 * 2.0 - 1.0;
        let rp = self.rng.next_f64() as f32 * 2.0 - 1.0;
        let clip = self.sounds[&name].clips[pick].clone();
        let gain =
            (def.gain * num(field(fields, "gain")).unwrap_or(1.0) * (1.0 + def.gain_jitter * rg))
                .max(0.0);
        let pitch =
            (num(field(fields, "pitch")).unwrap_or(1.0) * (1.0 + def.pitch_jitter * rp)).max(0.05);
        let bus_name = text(field(fields, "bus")).unwrap_or_else(|| def.bus.clone());
        let looping = matches!(field(fields, "loop"), Some(Value::Bool(true))) || def.looping;
        let entity = match field(fields, "entity") {
            Some(Value::Entity(e)) if !e.is_null() => Some(*e),
            Some(Value::Str(s)) => world.entity(s).ok(),
            _ => None,
        };
        let spatial = def.spatial;
        let mixer = self.mixer.clone();
        let mut m = match mixer.lock() {
            Ok(m) => m,
            Err(p) => p.into_inner(),
        };
        let Some(bus) = m.bus(&bus_name) else {
            drop(m);
            return vec![(
                "sound.missing".to_string(),
                vec![
                    ("sound".to_string(), Value::Str(name)),
                    ("bus".to_string(), Value::Str(bus_name)),
                ],
            )];
        };
        let pos = entity.and_then(|e| Self::position(world, e));
        let (att, pan) = match (spatial, pos) {
            (true, Some(p)) => {
                let l = Self::listener(world);
                let (a, p2) = self.spatialize(l, p);
                (Some(a), p2)
            }
            _ => (None, num(field(fields, "pan")).unwrap_or(0.0)),
        };
        // The per-sound cap: a burst of the same sound steals its oldest voice
        // rather than stacking into a clip.
        let q = self.by_sound.entry(name.clone()).or_default();
        q.retain(|v| m.is_playing(*v));
        let stole = if q.len() >= def.max_voices as usize {
            let old = q.pop_front();
            if let Some(o) = old {
                m.stop(o, 0.005);
            }
            old
        } else {
            None
        };
        let voice = m.play(
            clip,
            bus,
            gain * att.unwrap_or(1.0),
            pan,
            pitch,
            looping,
            0.0,
        );
        q.push_back(voice);
        drop(m);
        if let (Some(e), Some(p), true) = (entity, pos, spatial) {
            self.spatial.push(SpatialVoice {
                voice,
                entity: e,
                base_gain: gain,
                last: p,
            });
        }
        self.played += 1;
        let mut out = vec![
            ("sound".to_string(), Value::Str(name)),
            ("variation".to_string(), Value::Int(pick as i64)),
            ("voice".to_string(), Value::Int(voice as i64)),
            ("bus".to_string(), Value::Str(bus_name)),
            ("gain".to_string(), Value::Float(round3(gain))),
            ("pitch".to_string(), Value::Float(round3(pitch))),
            ("pan".to_string(), Value::Float(round3(pan))),
        ];
        if let Some(e) = entity {
            out.push(("entity".to_string(), Value::Entity(e)));
        }
        if let Some(a) = att {
            out.push(("attenuation".to_string(), Value::Float(round3(a))));
        }
        if looping {
            out.push(("loop".to_string(), Value::Bool(true)));
        }
        if let Some(s) = stole {
            out.push(("stole".to_string(), Value::Int(s as i64)));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        vec![("sound.played".to_string(), out)]
    }

    fn stop(&mut self, fields: &[(String, Value)]) -> Vec<AudioEvent> {
        let fade = num(field(fields, "fade_ms")).unwrap_or(0.0) / 1000.0;
        let mut ids: Vec<u64> = Vec::new();
        if let Some(v) = num(field(fields, "voice")) {
            ids.push(v as u64);
        }
        if let Some(Value::Entity(e)) = field(fields, "entity") {
            ids.extend(
                self.spatial
                    .iter()
                    .filter(|s| s.entity == *e)
                    .map(|s| s.voice),
            );
        }
        let mut m = self.lock();
        let stopped: Vec<u64> = ids.into_iter().filter(|id| m.stop(*id, fade)).collect();
        drop(m);
        stopped
            .into_iter()
            .map(|v| {
                (
                    "sound.stopped".to_string(),
                    vec![("voice".to_string(), Value::Int(v as i64))],
                )
            })
            .collect()
    }

    // ----- per frame ------------------------------------------------------------

    /// Follow moving sources and the music component. Returns the events
    /// to log (music starting, stopping, layers coming in and out).
    pub fn update(&mut self, world: &World) -> Vec<AudioEvent> {
        let l = Self::listener(world);
        let mut events = Vec::new();
        {
            let playing: Vec<bool> = {
                let m = self.lock();
                self.spatial.iter().map(|s| m.is_playing(s.voice)).collect()
            };
            let mut keep = Vec::new();
            for (s, alive) in std::mem::take(&mut self.spatial).into_iter().zip(playing) {
                if alive {
                    keep.push(s);
                }
            }
            for s in &mut keep {
                if world.is_alive(s.entity) {
                    if let Some(p) = Self::position(world, s.entity) {
                        s.last = p;
                    }
                }
            }
            let targets: Vec<(u64, f32, f32)> = keep
                .iter()
                .map(|s| {
                    let (a, p) = self.spatialize(l, s.last);
                    (s.voice, s.base_gain * a, p)
                })
                .collect();
            self.spatial = keep;
            let mut m = self.lock();
            for (v, g, p) in targets {
                m.set(v, g, p);
            }
        }

        // Music: the first entity with a Music component.
        let mut want: Option<(String, f32)> = None;
        if let Some(mc) = world.component_id_of::<Music>() {
            if let Some(e) = world.query().has(mc).entities().into_iter().next() {
                if let Ok(mu) = world.get::<Music>(e) {
                    let name = world.strings().resolve(mu.track);
                    if !name.is_empty() {
                        want = Some((name, mu.intensity.clamp(0.0, 1.0)));
                    }
                }
            }
        }
        let want_track = want.as_ref().map(|w| w.0.clone());
        self.change_music(want_track, &mut events);
        // A change whose bar has come: the new track sounds, the old fades.
        if let Some(due) = self.due {
            if self.lock().rendered_frames() >= due {
                self.due = None;
                if let Some(o) = self.outgoing.take() {
                    events.push((
                        "music.stopped".to_string(),
                        vec![("track".to_string(), Value::Str(o.track))],
                    ));
                }
                if let Some(p) = &mut self.music {
                    p.started = true;
                    events.push(started_event(p));
                }
            }
        }
        if let (Some(p), Some((_, intensity))) = (&mut self.music, &want) {
            let mut m = match self.mixer.lock() {
                Ok(m) => m,
                Err(e) => e.into_inner(),
            };
            for l in &mut p.layers {
                let span = (l.def.to - l.def.from).max(1e-6);
                let x = ((intensity - l.def.from) / span).clamp(0.0, 1.0);
                let g = if l.def.to <= l.def.from {
                    if *intensity >= l.def.from {
                        1.0
                    } else {
                        0.0
                    }
                } else {
                    x * x * (3.0 - 2.0 * x)
                };
                m.set(l.voice, g, 0.0);
                // A track waiting for its bar is not heard yet; its layers
                // come in when it is.
                let on = g > 0.01;
                if p.started && on != l.on {
                    l.on = on;
                    events.push((
                        "music.layer".to_string(),
                        vec![
                            ("layer".to_string(), Value::Str(l.def.sound.clone())),
                            ("on".to_string(), Value::Bool(on)),
                            ("track".to_string(), Value::Str(p.track.clone())),
                        ],
                    ));
                }
            }
        }
        events
    }

    /// The music the world asks for, if it is not what plays: the track
    /// sounding now leaves on its next bar or beat (its `change_on`, when
    /// it has a tempo) and the new one starts on that very sample; with no
    /// grid, at once. A change asked for again before its bar replaces the
    /// waiting track; asked back to the one still sounding, it is undone.
    fn change_music(&mut self, want: Option<String>, events: &mut Vec<AudioEvent>) {
        let wanted = self.music.as_ref().map(|p| p.track.clone());
        if want == wanted {
            return;
        }
        let back = self.due.is_some() && self.outgoing.as_ref().map(|o| &o.track) == want.as_ref();
        if back {
            // Undone before its bar: the waiting track never sounds.
            let pending = self.music.take();
            let mut m = self.lock();
            if let Some(p) = &pending {
                for l in &p.layers {
                    m.stop(l.voice, 0.0);
                }
            }
            if let Some(o) = &self.outgoing {
                for l in &o.layers {
                    m.cancel_stop(l.voice);
                }
            }
            drop(m);
            events.push((
                "music.cancelled".to_string(),
                vec![(
                    "track".to_string(),
                    Value::Str(pending.map(|p| p.track).unwrap_or_default()),
                )],
            ));
            self.music = self.outgoing.take();
            self.due = None;
            return;
        }
        // The track sounding now; a waiting one is replaced.
        let sounding = if self.due.is_some() {
            if let Some(p) = self.music.take() {
                let mut m = self.lock();
                for l in &p.layers {
                    m.stop(l.voice, 0.0);
                }
                drop(m);
                events.push((
                    "music.cancelled".to_string(),
                    vec![("track".to_string(), Value::Str(p.track))],
                ));
            }
            self.due = None;
            self.outgoing.take()
        } else {
            self.music.take()
        };
        let next = match &want {
            None => None,
            Some(name) => match self.config.tracks.get(name).cloned() {
                Some(t) => Some(t),
                None => {
                    if self.missing.insert(format!("music:{name}")) {
                        events.push((
                            "sound.missing".to_string(),
                            vec![("track".to_string(), Value::Str(name.clone()))],
                        ));
                    }
                    None
                }
            },
        };
        let mut m = self.lock();
        // Frames to the sounding track's next bar or beat.
        let delay = sounding
            .as_ref()
            .and_then(|s| {
                let unit = match s.def.change_on {
                    ChangeOn::Bar => s.def.bar_s()?,
                    ChangeOn::Beat => s.def.beat_s()?,
                    ChangeOn::Now => return None,
                };
                let unit = (unit as f64 * self.rate as f64).round() as u64;
                let played = m.played(s.layers.first()?.voice)?;
                Some((unit - played % unit) % unit)
            })
            .unwrap_or(0);
        if let Some(s) = &sounding {
            for l in &s.layers {
                m.stop_at(l.voice, delay, s.def.fade_ms / 1000.0);
            }
        }
        let playing = next.map(|t| {
            let bus = m.bus(&t.bus).unwrap_or(0);
            let mut layers = Vec::new();
            // One lock: every layer starts on the same frame, so they stay aligned.
            for d in &t.layers {
                if let Some(snd) = self.sounds.get(&d.sound) {
                    let v = m.play_at(snd.clips[0].clone(), bus, 0.0, 0.0, 1.0, true, 0.0, delay);
                    m.set_ramp(v, t.fade_ms / 1000.0);
                    layers.push(Layer {
                        voice: v,
                        def: d.clone(),
                        on: false,
                    });
                }
            }
            Playing {
                track: t.name.clone(),
                def: t,
                layers,
                started: delay == 0,
            }
        });
        let now = m.rendered_frames();
        drop(m);
        if delay == 0 {
            if let Some(s) = sounding {
                events.push((
                    "music.stopped".to_string(),
                    vec![("track".to_string(), Value::Str(s.track))],
                ));
            }
            if let Some(p) = &playing {
                events.push(started_event(p));
            }
            self.music = playing;
        } else {
            let on = sounding
                .as_ref()
                .map(|s| s.def.change_on.name())
                .unwrap_or("now");
            events.push((
                "music.queued".to_string(),
                vec![
                    ("track".to_string(), Value::Str(want.unwrap_or_default())),
                    (
                        "from".to_string(),
                        Value::Str(
                            sounding
                                .as_ref()
                                .map(|s| s.track.clone())
                                .unwrap_or_default(),
                        ),
                    ),
                    ("on".to_string(), Value::Str(on.to_string())),
                    (
                        "in_ms".to_string(),
                        Value::Float(
                            (delay as f64 * 1000.0 / self.rate as f64 * 10.0).round() / 10.0,
                        ),
                    ),
                ],
            ));
            self.music = playing;
            self.outgoing = sounding;
            self.due = Some(now + delay);
        }
    }

    /// Where the music is: its track, tempo, and bar and beat (from 1),
    /// and a change waiting for its bar.
    pub fn music_position(&self) -> Option<MusicPosition> {
        let sounding = if self.due.is_some() {
            self.outgoing.as_ref()
        } else {
            self.music.as_ref()
        };
        let m = self.lock();
        let mut seconds = 0.0;
        let (track, bpm, bar, beat) = match sounding {
            Some(s) => {
                let played = s
                    .layers
                    .first()
                    .and_then(|l| m.played(l.voice))
                    .unwrap_or(0);
                let secs = played as f64 / self.rate as f64;
                seconds = secs;
                let (bar, beat) = match s.def.beat_s() {
                    Some(b) => {
                        let n = (secs / b as f64).floor() as u64;
                        let per = s.def.beats_per_bar as u64;
                        (Some(n / per + 1), Some(n % per + 1))
                    }
                    None => (None, None),
                };
                (Some(s.track.clone()), s.def.bpm, bar, beat)
            }
            None => (None, None, None, None),
        };
        let queued = self.due.map(|d| {
            (
                self.music
                    .as_ref()
                    .map(|p| p.track.clone())
                    .unwrap_or_default(),
                (d.saturating_sub(m.rendered_frames()) as f64 * 1000.0 / self.rate as f64) as f32,
            )
        });
        if track.is_none() && queued.is_none() {
            return None;
        }
        Some(MusicPosition {
            track,
            bpm,
            bar,
            beat,
            seconds,
            queued,
        })
    }

    /// Advance the mixer to sim time `t` seconds when no device is pulling
    /// it (headless). Capped at the clip history length per call; a longer
    /// jump just moves on. A time earlier than the last means a new run.
    /// An external output (a page's AudioWorklet) pulls it too: advanced
    /// as well, a page's mixer ran at twice its rate (Hollow's and
    /// Outpost's music at 96,000 frames a second to a 48 kHz output) and a
    /// turn-based game's skipped a turn's time at each press (Delve's music
    /// on the public site, 2026-10-10).
    pub fn advance_to(&mut self, t: f64) {
        if self.device_open() || self.external.is_some() {
            self.last_time = Some(t);
            return;
        }
        let last = self.last_time.unwrap_or(t);
        self.last_time = Some(t);
        if t <= last {
            return;
        }
        let secs = (t - last).min(mixer::HISTORY_SECONDS as f64);
        let frames = (secs * self.rate as f64) as usize;
        self.lock().advance(frames);
    }

    /// Meters since the last read: per bus, then the output's peak and RMS
    /// after the limiter (dBFS), and the most the limiter turned it down (dB).
    pub fn meters(&self) -> (Vec<BusMeter>, f32, f32, f32) {
        let mut m = self.lock();
        let (b, p, r) = m.take_meters();
        (b, p, r, m.take_limiter_db())
    }

    /// Frames the mixer has rendered: pulled by the device when one is
    /// open, else advanced in step with the sim.
    pub fn rendered_frames(&self) -> u64 {
        self.lock().rendered_frames()
    }

    /// Voices playing.
    pub fn voices(&self) -> usize {
        self.lock().voice_count()
    }

    /// Voices playing on each bus, by name.
    pub fn bus_voices(&self) -> Vec<(String, usize)> {
        let m = self.lock();
        m.bus_names()
            .into_iter()
            .enumerate()
            .map(|(i, n)| (n, m.voices_on(i)))
            .collect()
    }

    /// The music playing: track and each layer's name and on state.
    pub fn music(&self) -> Option<(String, Vec<(String, bool)>)> {
        self.music.as_ref().map(|p| {
            (
                p.track.clone(),
                p.layers
                    .iter()
                    .map(|l| (l.def.sound.clone(), l.on))
                    .collect(),
            )
        })
    }

    /// Set a bus's gain or mute.
    pub fn set_bus(&mut self, name: &str, gain_db: Option<f32>, mute: Option<bool>) -> Result<()> {
        let mut m = self.lock();
        let b = m.bus(name).ok_or_else(|| {
            ArgusError::new(codes::UNKNOWN_BUS, format!("no bus {name:?}"))
                .hint(format!("Buses are {}.", m.bus_names().join(", ")))
        })?;
        m.set_bus(b, gain_db, mute);
        Ok(())
    }

    /// Write the last `seconds` of master output as a 16-bit stereo WAV.
    /// Returns frames written and the clip's peak in dBFS.
    pub fn clip(&self, seconds: f32, path: &Path) -> Result<(usize, f32)> {
        let data = self.lock().history(seconds);
        let e = |m: String| ArgusError::new(codes::CLIP, format!("{}: {m}", path.display()));
        if let Some(d) = path.parent() {
            argus_os::fs::create_dir_all(d).map_err(|x| e(x.to_string()))?;
        }
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: self.rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).map_err(|x| e(x.to_string()))?;
        let mut peak = 0.0f32;
        for s in &data {
            peak = peak.max(s.abs());
            w.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                .map_err(|x| e(x.to_string()))?;
        }
        w.finalize().map_err(|x| e(x.to_string()))?;
        Ok((data.len() / 2, lin_to_db(peak)))
    }

    /// Whether the output device is open.
    pub fn device_open(&self) -> bool {
        #[cfg(feature = "device")]
        {
            self.device.is_some()
        }
        #[cfg(not(feature = "device"))]
        {
            false
        }
    }

    /// The device's name, when open.
    pub fn device_name(&self) -> Option<String> {
        if let Some(n) = &self.external {
            return Some(n.clone());
        }
        #[cfg(feature = "device")]
        {
            self.device.as_ref().map(|d| d.name.clone())
        }
        #[cfg(not(feature = "device"))]
        {
            None
        }
    }

    /// Run at the output's rate: sounds rebuilt at it (voices playing are
    /// dropped), as for a device that wants another rate.
    fn use_rate(&mut self, rate: u32) {
        if rate != self.rate {
            self.rate = rate;
            *self.lock() = Mixer::new(rate, &self.config);
            self.spatial.clear();
            self.music = None;
            self.outgoing = None;
            self.due = None;
            self.load_sounds();
        }
    }

    /// Play to an output the host drives (phase 5 item 10: the browser's
    /// AudioWorklet, whose own thread plays from a queue the page keeps
    /// full): sounds rebuilt at its rate; the host takes blocks with
    /// [`AudioEngine::pull`].
    pub fn open_external(&mut self, rate: u32, name: &str) {
        self.use_rate(rate.max(8000));
        self.external = Some(name.to_string());
    }

    /// The next block for an external output, stereo interleaved: the
    /// mixer's, or silence while the game is not playing (the mixer waits).
    pub fn pull(&self, stereo: &mut [f32]) {
        if self.hold.load(std::sync::atomic::Ordering::Relaxed) {
            stereo.fill(0.0);
            return;
        }
        self.lock().render(stereo);
    }

    /// Open the system's default output. If the device wants another rate,
    /// sounds are rebuilt at that rate first (voices playing are dropped).
    pub fn open_device(&mut self) -> Result<()> {
        #[cfg(feature = "device")]
        {
            if self.device.is_some() {
                return Ok(());
            }
            let e = |m: String| {
                ArgusError::new(codes::NO_DEVICE, m)
                    .hint("Headless runs need no device: sounds are still events, and audio.clip renders them.")
            };
            let rate = device::probe(RATE).map_err(e)?;
            self.use_rate(rate);
            self.device =
                Some(device::open(self.mixer.clone(), rate, self.hold.clone()).map_err(e)?);
            Ok(())
        }
        #[cfg(not(feature = "device"))]
        {
            Err(ArgusError::new(
                codes::NO_DEVICE,
                "this build has no audio device support",
            ))
        }
    }

    /// Stop the sound effects playing, after a seek: they belong to a moment
    /// the sim left. Music keeps playing; it follows the world's `Music`,
    /// which the seek restored.
    pub fn silence(&mut self) {
        let keep: Vec<u64> = self
            .music
            .iter()
            .chain(self.outgoing.iter())
            .flat_map(|p| p.layers.iter().map(|l| l.voice))
            .collect();
        self.lock().stop_all_but(&keep, 0.03);
        self.spatial.clear();
    }

    /// Hold the output (the game is not playing): silence out, and the
    /// mixer where it is until released. Headless, the mixer follows sim
    /// time and nothing needs holding.
    pub fn set_hold(&self, held: bool) {
        self.hold.store(held, Ordering::Relaxed);
    }

    /// Whether the output is held.
    pub fn held(&self) -> bool {
        self.hold.load(Ordering::Relaxed)
    }

    /// Close the output device.
    pub fn close_device(&mut self) {
        #[cfg(feature = "device")]
        {
            self.device = None;
        }
    }

    /// The run seed variation choices come from.
    pub fn seed(&self) -> u64 {
        self.seed
    }
}
