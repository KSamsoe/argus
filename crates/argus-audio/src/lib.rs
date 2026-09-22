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

pub use config::{AudioConfig, BusDef, DuckDef, LayerDef, SoundDef, Source, Tone, TrackDef, Wave};
pub use mixer::{db_to_lin, lin_to_db, BusMeter, Mixer};

use argus_ecs::{
    ArgusError, Component, Entity, GlobalTransform, Result, StrId, Transform, Value, World,
};
use argus_sim::Pcg32;
use sound::Clip;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
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
    layers: Vec<Layer>,
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
    spatial: Vec<SpatialVoice>,
    music: Option<Playing>,
    missing: BTreeSet<String>,
    /// Voices of each sound, oldest first, for the per-sound cap.
    by_sound: BTreeMap<String, std::collections::VecDeque<u64>>,
    /// Problems found loading sounds: file:line and a sentence each.
    pub warnings: Vec<String>,
    last_time: Option<f64>,
    /// Sounds started since load.
    pub played: u64,
}

fn round3(x: f32) -> f64 {
    ((x as f64) * 1000.0).round() / 1000.0
}

fn wav_files(dir: &Path, base: &Path, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            wav_files(&p, base, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("wav")) {
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
        let config = AudioConfig::load(root)?;
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
            spatial: Vec::new(),
            music: None,
            missing: BTreeSet::new(),
            by_sound: BTreeMap::new(),
            warnings: Vec::new(),
            last_time: None,
            played: 0,
        };
        e.load_sounds();
        Ok(e)
    }

    fn load_sounds(&mut self) {
        self.sounds.clear();
        self.warnings.clear();
        let assets = self.root.join("assets");
        let mut defs: Vec<SoundDef> = self.config.sounds.values().cloned().collect();
        // Every WAV under assets/ is a sound by its path, unless defined.
        let mut files = Vec::new();
        wav_files(&assets, &assets, &mut files);
        for f in files {
            let name = f
                .trim_end_matches(".wav")
                .trim_end_matches(".WAV")
                .to_string();
            if !self.config.sounds.contains_key(&name) {
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
        for def in defs {
            let mut clips = Vec::new();
            for v in &def.variations {
                match v {
                    Source::Tone(t) => clips.push(sound::synth(t, self.rate)),
                    Source::File(f) => match sound::load_wav(&assets.join(f), self.rate) {
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
                if !self.sounds.contains_key(&l.sound) {
                    self.warnings.push(format!(
                        "{}: music.{} layer {:?} names no sound",
                        self.config.file, t.name, l.sound
                    ));
                }
            }
        }
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
    /// the first active `Camera`, else the origin.
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
        if let Some(c) = world.component_id("Camera") {
            for e in world.query().has(c).entities() {
                if matches!(world.get_field(e, "Camera.active"), Ok(Value::Bool(true))) {
                    if let Some(p) = Self::position(world, e) {
                        return p;
                    }
                }
            }
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
        let current = self.music.as_ref().map(|p| p.track.clone());
        let want_track = want.as_ref().map(|w| w.0.clone());
        if current != want_track {
            if let Some(p) = self.music.take() {
                let fade = self
                    .config
                    .tracks
                    .get(&p.track)
                    .map(|t| t.fade_ms / 1000.0)
                    .unwrap_or(1.0);
                let mut m = self.lock();
                for l in &p.layers {
                    m.stop(l.voice, fade);
                }
                drop(m);
                events.push((
                    "music.stopped".to_string(),
                    vec![("track".to_string(), Value::Str(p.track))],
                ));
            }
            if let Some(name) = want_track {
                match self.config.tracks.get(&name).cloned() {
                    None => {
                        if self.missing.insert(format!("music:{name}")) {
                            events.push((
                                "sound.missing".to_string(),
                                vec![("track".to_string(), Value::Str(name))],
                            ));
                        }
                    }
                    Some(t) => {
                        let mut m = self.lock();
                        let bus = m.bus(&t.bus).unwrap_or(0);
                        let mut layers = Vec::new();
                        // One lock: every layer starts on the same block, so they stay aligned.
                        for d in &t.layers {
                            if let Some(s) = self.sounds.get(&d.sound) {
                                let v = m.play(s.clips[0].clone(), bus, 0.0, 0.0, 1.0, true, 0.0);
                                m.set_ramp(v, t.fade_ms / 1000.0);
                                layers.push(Layer {
                                    voice: v,
                                    def: d.clone(),
                                    on: false,
                                });
                            }
                        }
                        drop(m);
                        events.push((
                            "music.started".to_string(),
                            vec![
                                ("track".to_string(), Value::Str(name.clone())),
                                ("layers".to_string(), Value::Int(layers.len() as i64)),
                            ],
                        ));
                        self.music = Some(Playing {
                            track: name,
                            layers,
                        });
                    }
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
                let on = g > 0.01;
                if on != l.on {
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

    /// Advance the mixer to sim time `t` seconds when no device is pulling
    /// it (headless). Capped at the clip history length per call; a longer
    /// jump just moves on. A time earlier than the last means a new run.
    pub fn advance_to(&mut self, t: f64) {
        if self.device_open() {
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
            std::fs::create_dir_all(d).map_err(|x| e(x.to_string()))?;
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
        #[cfg(feature = "device")]
        {
            self.device.as_ref().map(|d| d.name.clone())
        }
        #[cfg(not(feature = "device"))]
        {
            None
        }
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
            if rate != self.rate {
                self.rate = rate;
                *self.lock() = Mixer::new(rate, &self.config);
                self.spatial.clear();
                self.music = None;
                self.load_sounds();
            }
            self.device = Some(device::open(self.mixer.clone(), rate).map_err(e)?);
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
