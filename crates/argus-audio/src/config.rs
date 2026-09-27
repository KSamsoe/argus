//! `audio.toml`: buses and their routing tree, ducking rules, sounds with
//! variation sets, and music tracks made of layers (§13 "Audio"). All of
//! it is data; a project without the file gets the four default buses.
//!
//! ```toml
//! [buses]
//! master   = { gain_db = 0 }
//! music    = { parent = "master", gain_db = -6 }
//! sfx      = { parent = "master" }
//!
//! [[duck]]                       # "dialogue ducks music by 6 dB over 200 ms"
//! when = "ui"
//! duck = "music"
//! db = -6
//! attack_ms = 50
//! release_ms = 200
//!
//! [spatial]
//! min_distance = 2               # full volume inside this
//! max_distance = 30              # silent beyond this
//!
//! [sounds."sfx/step"]
//! bus = "sfx"
//! gain = 0.8
//! pitch_jitter = 0.05            # plus or minus 5 percent
//! max_voices = 8                 # a ninth steals the oldest
//! variations = ["sfx/step1.wav", "sfx/step2.wav", { wave = "noise", ms = 60 }]
//!
//! [music.explore]
//! layers = [ { sound = "music/pad" }, { sound = "music/drums", from = 0.5, to = 0.8 } ]   # on at from, fully in by to
//! bpm = 96                       # a tempo: changes wait for the grid
//! beats_per_bar = 4
//! change_on = "bar"              # leaving this track: on the next "bar", "beat", or "now"
//! fade_ms = 60                   # in and out; on the bar, a short one is a cut
//! ```
//!
//! A variation is a file under `assets/` or a described tone, so sound
//! work never waits on audio files (the §14 placeholder rule, for ears).

use argus_ecs::{ArgusError, Result};
use std::collections::BTreeMap;

/// A bus in the routing tree.
#[derive(Clone, Debug, PartialEq)]
pub struct BusDef {
    /// Name.
    pub name: String,
    /// Parent bus; `None` only for the root.
    pub parent: Option<String>,
    /// Gain in dB.
    pub gain_db: f32,
}

/// "When anything plays on `when`, lower `duck` by `db`."
#[derive(Clone, Debug, PartialEq)]
pub struct DuckDef {
    /// The bus whose activity triggers the rule.
    pub when: String,
    /// The bus that is lowered.
    pub duck: String,
    /// How far, in dB (negative).
    pub db: f32,
    /// Time to reach the ducked level.
    pub attack_ms: f32,
    /// Time to come back.
    pub release_ms: f32,
}

/// A waveform for a described tone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wave {
    /// Sine.
    Sine,
    /// Square.
    Square,
    /// Sawtooth.
    Saw,
    /// Triangle.
    Triangle,
    /// White noise (deterministic).
    Noise,
}

/// A described placeholder sound: a wave, a length, an envelope, and
/// optionally a note sequence (placeholder music loops are tones too).
#[derive(Clone, Debug, PartialEq)]
pub struct Tone {
    /// Waveform.
    pub wave: Wave,
    /// Frequency in Hz (ignored for noise).
    pub freq: f32,
    /// Length in ms (a note sequence sets its own length).
    pub ms: f32,
    /// Attack in ms.
    pub attack_ms: f32,
    /// Release in ms.
    pub release_ms: f32,
    /// Peak amplitude, 0..1.
    pub gain: f32,
    /// Frequency change in Hz per second (a laser zap is a negative slide).
    pub slide: f32,
    /// Notes in Hz; when present the tone is the sequence, `note_ms` each.
    pub notes: Vec<f32>,
    /// Length of each note in ms.
    pub note_ms: f32,
}

/// One variation of a sound.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// A WAV file, relative to `assets/`.
    File(String),
    /// A described tone.
    Tone(Tone),
}

/// A sound the game asks for by name; the mixer picks the variation.
#[derive(Clone, Debug, PartialEq)]
pub struct SoundDef {
    /// Name, as gameplay spells it (`sfx/step`).
    pub name: String,
    /// Bus.
    pub bus: String,
    /// Base gain, linear.
    pub gain: f32,
    /// Random gain variation, fraction (0.1 is plus or minus 10 percent).
    pub gain_jitter: f32,
    /// Random pitch variation, fraction.
    pub pitch_jitter: f32,
    /// The variation set; one is picked per play.
    pub variations: Vec<Source>,
    /// Attenuate and pan by distance from the listener when played on an entity.
    pub spatial: bool,
    /// Loop until stopped.
    pub looping: bool,
    /// Most voices of this sound at once; a new one steals the oldest.
    pub max_voices: u32,
    /// `audio.toml:line` of the definition.
    pub at: String,
}

/// A music layer: a looping sound whose gain follows the track's intensity.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerDef {
    /// Sound name.
    pub sound: String,
    /// Intensity at which the layer starts to fade in.
    pub from: f32,
    /// Intensity at which it is fully in.
    pub to: f32,
}

/// Where a change away from a track may happen (§13 "transitions between
/// tracks happen on beat or bar boundaries").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeOn {
    /// The next bar line.
    Bar,
    /// The next beat.
    Beat,
    /// At once.
    Now,
}

impl ChangeOn {
    /// Its name in `audio.toml`.
    pub fn name(self) -> &'static str {
        match self {
            ChangeOn::Bar => "bar",
            ChangeOn::Beat => "beat",
            ChangeOn::Now => "now",
        }
    }
}

/// A music track: layers played in lockstep (§13 adaptive music), with a
/// tempo that changes wait for (phase 3 item 7).
#[derive(Clone, Debug, PartialEq)]
pub struct TrackDef {
    /// Name.
    pub name: String,
    /// Bus.
    pub bus: String,
    /// Layers, started together so they stay aligned.
    pub layers: Vec<LayerDef>,
    /// Crossfade when the track starts or stops, ms.
    pub fade_ms: f32,
    /// Beats a minute; `None` has no grid, and changes happen at once.
    pub bpm: Option<f32>,
    /// Beats a bar.
    pub beats_per_bar: u32,
    /// Where a change away from it waits for.
    pub change_on: ChangeOn,
}

impl TrackDef {
    /// Seconds a beat, when it has a tempo.
    pub fn beat_s(&self) -> Option<f32> {
        self.bpm.map(|b| 60.0 / b)
    }
    /// Seconds a bar, when it has a tempo.
    pub fn bar_s(&self) -> Option<f32> {
        self.beat_s().map(|b| b * self.beats_per_bar as f32)
    }
}

/// Distance model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spatial {
    /// Full volume within this distance.
    pub min_distance: f32,
    /// Silent beyond this distance.
    pub max_distance: f32,
}

/// Everything in `audio.toml`.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioConfig {
    /// Buses, root first.
    pub buses: Vec<BusDef>,
    /// Ducking rules.
    pub ducks: Vec<DuckDef>,
    /// Sounds by name.
    pub sounds: BTreeMap<String, SoundDef>,
    /// Music tracks by name.
    pub tracks: BTreeMap<String, TrackDef>,
    /// Distance model.
    pub spatial: Spatial,
    /// Where it came from.
    pub file: String,
}

impl Default for AudioConfig {
    fn default() -> Self {
        let bus = |n: &str| BusDef {
            name: n.to_string(),
            parent: Some("master".to_string()),
            gain_db: 0.0,
        };
        AudioConfig {
            buses: vec![
                BusDef {
                    name: "master".to_string(),
                    parent: None,
                    gain_db: 0.0,
                },
                bus("music"),
                bus("sfx"),
                bus("ui"),
                bus("ambience"),
            ],
            ducks: Vec::new(),
            sounds: BTreeMap::new(),
            tracks: BTreeMap::new(),
            spatial: Spatial {
                min_distance: 2.0,
                max_distance: 30.0,
            },
            file: "audio.toml".to_string(),
        }
    }
}

type Doc = toml_edit::ImDocument<String>;

fn line_of(text: &str, item: &toml_edit::Item) -> u32 {
    let at = item.span().map(|s| s.start).unwrap_or(0).min(text.len());
    text[..at].matches('\n').count() as u32 + 1
}

fn err(file: &str, line: u32, msg: impl Into<String>) -> ArgusError {
    ArgusError::new(crate::codes::BAD_AUDIO_TOML, msg.into())
        .hint("audio.toml has [buses], [[duck]], [spatial], [sounds.\"name\"], and [music.name]; see the argus-audio crate docs for every key.")
        .at(Some(format!("{file}:{line}")))
}

fn num(v: &toml_edit::Value) -> Option<f32> {
    v.as_float()
        .map(|f| f as f32)
        .or_else(|| v.as_integer().map(|i| i as f32))
}

/// Read a key of a table-like as a number.
fn get_num(t: &dyn toml_edit::TableLike, key: &str) -> Option<f32> {
    t.get(key).and_then(|i| i.as_value()).and_then(num)
}

fn get_str(t: &dyn toml_edit::TableLike, key: &str) -> Option<String> {
    t.get(key)
        .and_then(|i| i.as_value())
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn get_bool(t: &dyn toml_edit::TableLike, key: &str) -> Option<bool> {
    t.get(key)
        .and_then(|i| i.as_value())
        .and_then(|v| v.as_bool())
}

fn tone_from(t: &dyn toml_edit::TableLike, file: &str, line: u32) -> Result<Tone> {
    let wave = match get_str(t, "wave").as_deref().unwrap_or("sine") {
        "sine" => Wave::Sine,
        "square" => Wave::Square,
        "saw" => Wave::Saw,
        "triangle" => Wave::Triangle,
        "noise" => Wave::Noise,
        other => {
            return Err(err(
                file,
                line,
                format!("unknown wave {other:?}; waves are sine, square, saw, triangle, noise"),
            ))
        }
    };
    let notes: Vec<f32> = t
        .get("notes")
        .and_then(|i| i.as_value())
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(num).collect())
        .unwrap_or_default();
    Ok(Tone {
        wave,
        freq: get_num(t, "freq").unwrap_or(440.0),
        ms: get_num(t, "ms").unwrap_or(120.0).max(1.0),
        attack_ms: get_num(t, "attack_ms").unwrap_or(3.0).max(0.0),
        release_ms: get_num(t, "release_ms").unwrap_or(40.0).max(0.0),
        gain: get_num(t, "gain").unwrap_or(0.5).clamp(0.0, 1.0),
        slide: get_num(t, "slide").unwrap_or(0.0),
        notes,
        note_ms: get_num(t, "note_ms").unwrap_or(250.0).max(1.0),
    })
}

/// A sound from a table: the body of `[sounds."name"]` in `audio.toml`, or
/// a whole sound sidecar (`assets/sfx/step.toml`). The bus is checked by
/// the caller, who knows the buses.
fn sound_from(name: &str, st: &dyn toml_edit::TableLike, file: &str, sl: u32) -> Result<SoundDef> {
    let bus = get_str(st, "bus").unwrap_or_else(|| {
        if name.starts_with("music/") {
            "music"
        } else {
            "sfx"
        }
        .to_string()
    });
    let mut variations = Vec::new();
    if let Some(a) = st
        .get("variations")
        .and_then(|i| i.as_value())
        .and_then(|v| v.as_array())
    {
        for v in a.iter() {
            if let Some(p) = v.as_str() {
                variations.push(Source::File(p.to_string()));
            } else if let Some(tt) = v.as_inline_table() {
                variations.push(Source::Tone(tone_from(tt, file, sl)?));
            } else {
                return Err(err(
                    file,
                    sl,
                    format!("sounds.{name:?}.variations: each item is a file path or a tone table"),
                ));
            }
        }
    }
    if let Some(tv) = st.get("tone") {
        let tt = tv
            .as_table_like()
            .ok_or_else(|| err(file, sl, format!("sounds.{name:?}.tone must be a table")))?;
        variations.push(Source::Tone(tone_from(tt, file, sl)?));
    }
    if let Some(f) = get_str(st, "file") {
        variations.push(Source::File(f));
    }
    if variations.is_empty() {
        return Err(err(
            file,
            sl,
            format!("sounds.{name:?} has nothing to play: give variations = [...], tone = {{...}}, or file = \"...\""),
        ));
    }
    Ok(SoundDef {
        name: name.to_string(),
        bus,
        gain: get_num(st, "gain").unwrap_or(1.0).max(0.0),
        gain_jitter: get_num(st, "gain_jitter").unwrap_or(0.0).clamp(0.0, 1.0),
        pitch_jitter: get_num(st, "pitch_jitter").unwrap_or(0.0).clamp(0.0, 0.9),
        variations,
        spatial: get_bool(st, "spatial").unwrap_or(true),
        looping: get_bool(st, "loop").unwrap_or(false),
        max_voices: get_num(st, "max_voices").unwrap_or(8.0).max(1.0) as u32,
        at: format!("{file}:{sl}"),
    })
}

/// Parse a sound sidecar: the whole file is one sound's table, with file
/// variations relative to `assets/` (the importer rewrites them).
pub fn parse_sound_sidecar(name: &str, file: &str, text: &str) -> Result<SoundDef> {
    let doc: Doc = Doc::parse(text.to_string()).map_err(|e| {
        let line = e
            .span()
            .map(|s| text[..s.start.min(text.len())].matches('\n').count() as u32 + 1)
            .unwrap_or(1);
        err(file, line, format!("not valid TOML: {}", e.message()))
    })?;
    sound_from(name, doc.as_table(), file, 1)
}

impl AudioConfig {
    /// Read `<root>/audio.toml`, or the defaults when there is none.
    pub fn load(root: &std::path::Path) -> Result<AudioConfig> {
        let p = root.join("audio.toml");
        match argus_os::fs::read_to_string(&p) {
            Ok(text) => AudioConfig::parse("audio.toml", &text),
            Err(_) => Ok(AudioConfig::default()),
        }
    }

    /// Parse the text of an `audio.toml`.
    pub fn parse(file: &str, text: &str) -> Result<AudioConfig> {
        let doc: Doc = Doc::parse(text.to_string()).map_err(|e| {
            let line = e
                .span()
                .map(|s| text[..s.start.min(text.len())].matches('\n').count() as u32 + 1)
                .unwrap_or(1);
            err(file, line, format!("not valid TOML: {}", e.message()))
        })?;
        let mut cfg = AudioConfig {
            file: file.to_string(),
            ..AudioConfig::default()
        };

        if let Some(item) = doc.get("buses") {
            let line = line_of(text, item);
            let t = item.as_table_like().ok_or_else(|| {
                err(
                    file,
                    line,
                    "buses must be a table of bus = { parent, gain_db }",
                )
            })?;
            let mut buses = Vec::new();
            for (name, b) in t.iter() {
                let bl = line_of(text, b);
                let bt = b.as_table_like().ok_or_else(|| {
                    err(file, bl, format!("buses.{name} must be a table like {{ parent = \"master\", gain_db = -6 }}"))
                })?;
                let parent = get_str(bt, "parent");
                if parent.is_none() && name != "master" {
                    // Anything without a parent hangs off master.
                    buses.push(BusDef {
                        name: name.to_string(),
                        parent: Some("master".to_string()),
                        gain_db: get_num(bt, "gain_db").unwrap_or(0.0),
                    });
                } else {
                    buses.push(BusDef {
                        name: name.to_string(),
                        parent,
                        gain_db: get_num(bt, "gain_db").unwrap_or(0.0),
                    });
                }
            }
            if !buses.iter().any(|b| b.name == "master") {
                buses.insert(
                    0,
                    BusDef {
                        name: "master".to_string(),
                        parent: None,
                        gain_db: 0.0,
                    },
                );
            }
            // Parents must exist and the tree must not loop.
            for b in &buses {
                if let Some(p) = &b.parent {
                    if !buses.iter().any(|x| &x.name == p) {
                        return Err(err(
                            file,
                            line,
                            format!("bus {} has parent {p:?}, which is not a bus", b.name),
                        ));
                    }
                }
                let mut seen = vec![b.name.clone()];
                let mut cur = b.parent.clone();
                while let Some(c) = cur {
                    if seen.contains(&c) {
                        return Err(err(
                            file,
                            line,
                            format!("bus routing loops: {} -> {c}", seen.join(" -> ")),
                        ));
                    }
                    seen.push(c.clone());
                    cur = buses
                        .iter()
                        .find(|x| x.name == c)
                        .and_then(|x| x.parent.clone());
                }
            }
            buses.sort_by_key(|b| b.parent.is_some());
            cfg.buses = buses;
        }
        let bus_exists = |cfg: &AudioConfig, n: &str| cfg.buses.iter().any(|b| b.name == n);

        if let Some(item) = doc.get("duck") {
            let line = line_of(text, item);
            let arr = item
                .as_array_of_tables()
                .ok_or_else(|| err(file, line, "duck must be an array of tables: [[duck]] when, duck, db, attack_ms, release_ms"))?;
            for t in arr.iter() {
                let when = get_str(t, "when").unwrap_or_default();
                let duck = get_str(t, "duck").unwrap_or_default();
                for (k, v) in [("when", &when), ("duck", &duck)] {
                    if !bus_exists(&cfg, v) {
                        return Err(err(file, line, format!("duck.{k} = {v:?} is not a bus")));
                    }
                }
                cfg.ducks.push(DuckDef {
                    when,
                    duck,
                    db: get_num(t, "db").unwrap_or(-6.0).min(0.0),
                    attack_ms: get_num(t, "attack_ms").unwrap_or(50.0).max(1.0),
                    release_ms: get_num(t, "release_ms").unwrap_or(200.0).max(1.0),
                });
            }
        }

        if let Some(t) = doc.get("spatial").and_then(|i| i.as_table_like()) {
            cfg.spatial.min_distance = get_num(t, "min_distance").unwrap_or(2.0).max(0.0);
            cfg.spatial.max_distance = get_num(t, "max_distance")
                .unwrap_or(30.0)
                .max(cfg.spatial.min_distance + 0.001);
        }

        if let Some(item) = doc.get("sounds") {
            let t = item.as_table_like().ok_or_else(|| {
                err(
                    file,
                    line_of(text, item),
                    "sounds must be a table of [sounds.\"name\"]",
                )
            })?;
            for (name, s) in t.iter() {
                let sl = line_of(text, s);
                let st = s
                    .as_table_like()
                    .ok_or_else(|| err(file, sl, format!("sounds.{name:?} must be a table")))?;
                let def = sound_from(name, st, file, sl)?;
                if !bus_exists(&cfg, &def.bus) {
                    return Err(err(
                        file,
                        sl,
                        format!("sounds.{name:?}.bus = {:?} is not a bus", def.bus),
                    ));
                }
                cfg.sounds.insert(name.to_string(), def);
            }
        }

        if let Some(item) = doc.get("music") {
            let t = item.as_table_like().ok_or_else(|| {
                err(
                    file,
                    line_of(text, item),
                    "music must be a table of [music.name]",
                )
            })?;
            for (name, m) in t.iter() {
                let ml = line_of(text, m);
                let mt = m
                    .as_table_like()
                    .ok_or_else(|| err(file, ml, format!("music.{name} must be a table")))?;
                let bus = get_str(mt, "bus").unwrap_or_else(|| "music".to_string());
                if !bus_exists(&cfg, &bus) {
                    return Err(err(
                        file,
                        ml,
                        format!("music.{name}.bus = {bus:?} is not a bus"),
                    ));
                }
                let mut layers = Vec::new();
                if let Some(a) = mt
                    .get("layers")
                    .and_then(|i| i.as_value())
                    .and_then(|v| v.as_array())
                {
                    for v in a.iter() {
                        let lt = v.as_inline_table().ok_or_else(|| {
                            err(file, ml, format!("music.{name}.layers: each layer is {{ sound = \"...\", from = 0.0, to = 0.2 }}"))
                        })?;
                        let sound = get_str(lt, "sound").ok_or_else(|| {
                            err(file, ml, format!("music.{name}: a layer needs sound"))
                        })?;
                        let from = get_num(lt, "from").unwrap_or(0.0);
                        let to = get_num(lt, "to").unwrap_or(from).max(from);
                        layers.push(LayerDef { sound, from, to });
                    }
                }
                if layers.is_empty() {
                    return Err(err(file, ml, format!("music.{name} has no layers")));
                }
                let bpm = get_num(mt, "bpm");
                if let Some(b) = bpm {
                    if !(20.0..=400.0).contains(&b) {
                        return Err(err(
                            file,
                            ml,
                            format!(
                                "music.{name}.bpm = {b} is not a tempo; 20 to 400 beats a minute"
                            ),
                        ));
                    }
                }
                let beats_per_bar = match get_num(mt, "beats_per_bar") {
                    None => 4,
                    Some(n) if (1.0..=16.0).contains(&n) && n.fract() == 0.0 => n as u32,
                    Some(n) => {
                        return Err(err(
                            file,
                            ml,
                            format!("music.{name}.beats_per_bar = {n}: a whole number, 1 to 16"),
                        ))
                    }
                };
                let change_on = match get_str(mt, "change_on").as_deref() {
                    None if bpm.is_some() => ChangeOn::Bar,
                    None => ChangeOn::Now,
                    Some("bar") => ChangeOn::Bar,
                    Some("beat") => ChangeOn::Beat,
                    Some("now") => ChangeOn::Now,
                    Some(other) => {
                        return Err(err(
                            file,
                            ml,
                            format!(
                                "music.{name}.change_on = {other:?}: \"bar\", \"beat\", or \"now\""
                            ),
                        ))
                    }
                };
                if bpm.is_none() && change_on != ChangeOn::Now {
                    return Err(err(
                        file,
                        ml,
                        format!(
                            "music.{name}.change_on = {:?} needs a bpm to find the {} by",
                            change_on.name(),
                            change_on.name()
                        ),
                    ));
                }
                cfg.tracks.insert(
                    name.to_string(),
                    TrackDef {
                        name: name.to_string(),
                        bus,
                        layers,
                        fade_ms: get_num(mt, "fade_ms").unwrap_or(1000.0).max(1.0),
                        bpm,
                        beats_per_bar,
                        change_on,
                    },
                );
            }
        }
        // Layers must name sounds (defined here, or a file found later: checked at load).
        Ok(cfg)
    }
}
