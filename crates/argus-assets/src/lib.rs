//! Argus asset importer, phase 1 item 4 (§14): source assets live in
//! `assets/`, described by sidecar TOML files an agent can write; the
//! importer validates them, writes a cache under `.argus/assets/`, reports
//! every import with its warnings, and watches the tree.
//!
//! An asset's name is its path under `assets/` without the extension
//! (`assets/sprites/hero.png` is `sprites/hero`), and its sidecar is
//! `<name>.toml` beside it. A sprite is a PNG (a sheet of frames), or a
//! sidecar alone describing a shape the importer draws into the cache, so
//! a placeholder and the art that replaces it are the same asset: the
//! human drops `hero.png` in and no reference changes. A sound is a WAV,
//! or a sidecar naming variations in the `audio.toml` sound format.
//!
//! The sidecar already carries what Aseprite and Tiled files hold (frame
//! durations, tags as clips with a direction, per-frame properties), so
//! their native importers (phase 5) fill the same shape.

#![warn(missing_docs)]

pub mod animator;
pub mod effect;
pub mod sidecar;

pub use animator::{
    AnimatorSpec, Condition, FrameEventSpec, Num, Op, Operand, StateSpec, Term, TransitionSpec,
};
use argus_audio::{SoundDef, Source};
use argus_ecs::{ArgusError, Result};
pub use effect::{Area, ColorCurve, Curve, EmitterSpec, Space};
use serde_json::{json, Value as Json};
pub use sidecar::{AutoMode, AutoTileSpec};
use sidecar::{Direction, Shape, SidecarKind};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

/// Error codes this crate adds (52xx).
pub mod codes {
    /// A sidecar is malformed.
    pub const BAD_SIDECAR: u32 = 5201;
    /// An image or sound could not be read.
    pub const BAD_SOURCE: u32 = 5202;
    /// Two sources claim one name, or a sidecar describes nothing.
    pub const AMBIGUOUS: u32 = 5203;
    /// No asset by that name.
    pub const UNKNOWN_ASSET: u32 = 5204;
    /// The cache could not be written.
    pub const CACHE: u32 = 5205;
}

/// Bump when the import result changes, so every cache entry is redone.
pub const IMPORTER_VERSION: &str = "argus-assets 1";

/// One frame of a sprite sheet, in pixels of the source image.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// x, y, w, h.
    pub rect: [u32; 4],
    /// Pivot as a fraction of the frame, y up from the bottom.
    pub pivot: [f32; 2],
    /// How long the frame shows when a clip has no fps.
    pub duration_ms: u32,
    /// Free-form properties (the Tiled shape).
    pub properties: BTreeMap<String, String>,
}

/// An animation clip.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    /// Frame indices in order.
    pub frames: Vec<u32>,
    /// Frames per second, when given.
    pub fps: Option<f32>,
    /// Loops.
    pub looping: bool,
    /// Play order.
    pub direction: Direction,
}

/// An imported sprite.
#[derive(Clone, Debug, PartialEq)]
pub struct SpriteAsset {
    /// The PNG, relative to `assets/`, when there is one.
    pub source: Option<String>,
    /// Source size in pixels.
    pub width: u32,
    /// Source size in pixels.
    pub height: u32,
    /// The cached texture (padded to powers of two).
    pub texture: PathBuf,
    /// Its size.
    pub texture_size: [u32; 2],
    /// Frames.
    pub frames: Vec<Frame>,
    /// Clips by name.
    pub clips: BTreeMap<String, Clip>,
    /// World scale.
    pub pixels_per_unit: f32,
    /// The described shape, when the sprite is a placeholder.
    pub shape: Option<Shape>,
    /// Auto-tiling rules by terrain, when the sprite is a tileset.
    pub autotile: BTreeMap<String, AutoTileSpec>,
}

impl SpriteAsset {
    /// A tile's property (tile n is frame n - 1).
    pub fn tile_property(&self, tile: u32, key: &str) -> Option<&str> {
        let f = self.frames.get((tile as usize).checked_sub(1)?)?;
        f.properties.get(key).map(String::as_str)
    }
}

/// A tile's `slope` property: `[left, right]`, the floor's height at the
/// tile's left and right edges as fractions of the tile, from its bottom
/// (`[0, 1]` rises to the right at 45 degrees, `[0, 0.5]` is the low half of
/// a 22.5-degree pair). Accepts the TOML array or two numbers.
pub fn parse_slope(s: &str) -> Option<(f32, f32)> {
    let t = s.trim().trim_start_matches('[').trim_end_matches(']');
    let v: Vec<f32> = t
        .split([',', ' '])
        .filter(|p| !p.trim().is_empty())
        .map(|p| p.trim().parse::<f32>())
        .collect::<std::result::Result<_, _>>()
        .ok()?;
    match v[..] {
        [l, r] if (0.0..=1.0).contains(&l) && (0.0..=1.0).contains(&r) => Some((l, r)),
        _ => None,
    }
}

/// A WAV a sound plays.
#[derive(Clone, Debug, PartialEq)]
pub struct WavInfo {
    /// Path relative to `assets/`.
    pub path: String,
    /// Sample rate.
    pub rate: u32,
    /// Channels.
    pub channels: u16,
    /// Length.
    pub seconds: f32,
    /// Peak in dBFS.
    pub peak_db: f32,
}

/// An imported sound.
#[derive(Clone, Debug, PartialEq)]
pub struct SoundAsset {
    /// The definition the mixer loads (variation paths relative to `assets/`).
    pub def: SoundDef,
    /// The WAVs it plays.
    pub files: Vec<WavInfo>,
    /// Defined by a sidecar (a bare WAV the mixer finds on its own is not).
    pub from_sidecar: bool,
}

/// What an asset is.
#[derive(Clone, Debug, PartialEq)]
pub enum Asset {
    /// A sprite.
    Sprite(SpriteAsset),
    /// A sound.
    Sound(SoundAsset),
    /// An animator state machine.
    Animator(AnimatorSpec),
    /// A particle effect.
    Emitter(EmitterSpec),
}

/// A warning about an import: a sentence and where.
#[derive(Clone, Debug, PartialEq)]
pub struct Warning {
    /// What is wrong.
    pub message: String,
    /// `file:line` or the file.
    pub at: String,
}

/// One imported asset.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// Name (`sprites/hero`).
    pub name: String,
    /// The sidecar, relative to `assets/`.
    pub sidecar: Option<String>,
    /// Content hash of every input file and the importer version.
    pub hash: u64,
    /// Warnings.
    pub warnings: Vec<Warning>,
    /// The asset.
    pub asset: Asset,
    /// Import time in ms (0 when reused unchanged).
    pub ms: f64,
}

impl Entry {
    /// `sprite`, `sound`, or `animator`.
    pub fn kind(&self) -> &'static str {
        match self.asset {
            Asset::Sprite(_) => "sprite",
            Asset::Sound(_) => "sound",
            Asset::Animator(_) => "animator",
            Asset::Emitter(_) => "emitter",
        }
    }

    /// The entry as `asset.get` reports it.
    pub fn to_json(&self) -> Json {
        let warnings: Vec<Json> = self
            .warnings
            .iter()
            .map(|w| json!({ "message": w.message, "at": w.at }))
            .collect();
        let mut o = json!({
            "name": self.name,
            "kind": self.kind(),
            "sidecar": self.sidecar,
            "hash": format!("{:016x}", self.hash),
            "warnings": warnings,
        });
        match &self.asset {
            Asset::Emitter(f) => {
                o["sprite"] = json!(f.sprite);
                o["particle"] = json!(f.particle);
                o["rate"] = json!(f.rate);
                o["burst"] = json!(f.burst);
                o["max"] = json!(f.max);
                o["lifetime"] = json!(f.lifetime);
                o["space"] = json!(format!("{:?}", f.space).to_lowercase());
            }
            Asset::Animator(a) => {
                o["initial"] = json!(a.initial);
                o["states"] = Json::Array(
                    a.states
                        .iter()
                        .map(|s| {
                            json!({
                                "name": s.name,
                                "clip": s.clip,
                                "events": s.events.iter().map(|e| json!([e.frame, e.name, e.sound])).collect::<Vec<_>>(),
                                "transitions": s.transitions.iter().map(|t| json!([t.to, t.when.text])).collect::<Vec<_>>(),
                            })
                        })
                        .collect(),
                );
                o["any"] = Json::Array(a.any.iter().map(|t| json!([t.to, t.when.text])).collect());
            }
            Asset::Sprite(s) => {
                o["source"] = json!(s.source);
                o["size"] = json!([s.width, s.height]);
                o["texture"] = json!(s.texture.to_string_lossy());
                o["texture_size"] = json!(s.texture_size);
                o["pixels_per_unit"] = json!(s.pixels_per_unit);
                o["frames"] = Json::Array(
                    s.frames
                        .iter()
                        .map(|f| {
                            let mut j = json!({ "rect": f.rect, "pivot": f.pivot, "duration_ms": f.duration_ms });
                            if !f.properties.is_empty() {
                                j["properties"] = json!(f.properties);
                            }
                            j
                        })
                        .collect(),
                );
                o["clips"] = Json::Object(
                    s.clips
                        .iter()
                        .map(|(n, c)| {
                            (
                                n.clone(),
                                json!({ "frames": c.frames, "fps": c.fps, "loop": c.looping, "direction": c.direction.name() }),
                            )
                        })
                        .collect(),
                );
                if !s.autotile.is_empty() {
                    o["autotile"] = Json::Object(
                        s.autotile
                            .iter()
                            .map(|(n, a)| {
                                (
                                    n.clone(),
                                    json!({ "mode": a.mode.name(), "tiles": a.tiles, "edges": if a.connect_edges { "connect" } else { "open" } }),
                                )
                            })
                            .collect(),
                    );
                }
                if let Some(sh) = &s.shape {
                    o["shape"] = json!({ "kind": sh.kind, "size": sh.size, "color": sh.color });
                }
            }
            Asset::Sound(s) => {
                o["bus"] = json!(s.def.bus);
                o["gain"] = json!(s.def.gain);
                o["variations"] = json!(s.def.variations.len());
                o["files"] = Json::Array(
                    s.files
                        .iter()
                        .map(|f| json!({ "path": f.path, "rate": f.rate, "channels": f.channels, "seconds": f.seconds, "peak_db": f.peak_db }))
                        .collect(),
                );
            }
        }
        o
    }
}

/// A failed import.
#[derive(Clone, Debug)]
pub struct Failure {
    /// Asset name.
    pub name: String,
    /// The error, with `file:line`.
    pub error: ArgusError,
}

/// What a scan did.
#[derive(Clone, Debug, Default)]
pub struct Report {
    /// Names imported (new or changed).
    pub imported: Vec<String>,
    /// Names unchanged since the last scan.
    pub unchanged: usize,
    /// Names that went away.
    pub removed: Vec<String>,
    /// Names that failed, with why.
    pub failed: Vec<Failure>,
    /// Wall time, ms.
    pub ms: f64,
}

impl Report {
    /// True when nothing changed.
    pub fn is_empty(&self) -> bool {
        self.imported.is_empty() && self.removed.is_empty() && self.failed.is_empty()
    }
}

/// Decode a PNG to RGBA8.
pub fn decode_png(path: &Path) -> Result<(u32, u32, Vec<u8>)> {
    let e = |m: String| ArgusError::new(codes::BAD_SOURCE, format!("{}: {m}", path.display()));
    let file = std::fs::File::open(path).map_err(|x| e(x.to_string()))?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|x| e(x.to_string()))?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|x| e(x.to_string()))?;
    let px = &buf[..info.buffer_size()];
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => px.to_vec(),
        png::ColorType::Rgb => px.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::Grayscale => px.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => px
            .chunks(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        other => return Err(e(format!("unsupported PNG colour type {other:?}"))),
    };
    Ok((info.width, info.height, rgba))
}

/// Encode RGBA8 as a PNG.
pub fn encode_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<()> {
    let e = |m: String| ArgusError::new(codes::CACHE, format!("{}: {m}", path.display()));
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|x| e(x.to_string()))?;
    }
    let file = std::fs::File::create(path).map_err(|x| e(x.to_string()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().map_err(|x| e(x.to_string()))?;
    wr.write_image_data(rgba).map_err(|x| e(x.to_string()))?;
    Ok(())
}

fn fnv(h: &mut u64, bytes: &[u8]) {
    for b in bytes {
        *h ^= *b as u64;
        *h = h.wrapping_mul(0x100000001b3);
    }
}

/// Every file of one asset: its sources and its sidecar.
#[derive(Clone, Debug, Default, PartialEq)]
struct Group {
    png: Option<String>,
    wav: Option<String>,
    sidecar: Option<String>,
}

type Stamp = BTreeMap<String, (SystemTime, u64)>;

/// The importer: the catalog of `assets/` and its cache.
pub struct Importer {
    root: PathBuf,
    assets: PathBuf,
    cache: PathBuf,
    entries: BTreeMap<String, Entry>,
    failed: BTreeMap<String, Failure>,
    /// Stamps of each asset's input files when last imported.
    inputs: BTreeMap<String, Stamp>,
    /// Stamps of the whole tree at the last poll.
    tree: Stamp,
}

fn walk(dir: &Path, base: &Path, out: &mut Stamp) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Ok(ft) = e.file_type() else { continue };
        let p = e.path();
        if ft.is_dir() {
            walk(&p, base, out);
        } else if let Ok(md) = e.metadata() {
            if let Ok(rel) = p.strip_prefix(base) {
                let rel = rel.to_string_lossy().replace('\\', "/");
                out.insert(
                    rel,
                    (md.modified().unwrap_or(SystemTime::UNIX_EPOCH), md.len()),
                );
            }
        }
    }
}

fn pow2(n: u32) -> u32 {
    n.max(1).next_power_of_two()
}

impl Importer {
    /// An importer for a project; nothing is read until `scan`.
    pub fn new(root: &Path) -> Importer {
        Importer {
            root: root.to_path_buf(),
            assets: root.join("assets"),
            cache: root.join(".argus").join("assets"),
            entries: BTreeMap::new(),
            failed: BTreeMap::new(),
            inputs: BTreeMap::new(),
            tree: BTreeMap::new(),
        }
    }

    /// The project root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Imported assets by name.
    pub fn entries(&self) -> &BTreeMap<String, Entry> {
        &self.entries
    }

    /// One asset.
    pub fn get(&self, name: &str) -> Result<&Entry> {
        self.entries.get(name).ok_or_else(|| {
            let hint = match self.failed.get(name) {
                Some(f) => format!("It failed to import: {}", f.error.message),
                None => "asset.list shows every asset; a name is its path under assets/ without the extension.".to_string(),
            };
            ArgusError::new(codes::UNKNOWN_ASSET, format!("no asset {name:?}")).hint(hint)
        })
    }

    /// Failed imports by name.
    pub fn failures(&self) -> &BTreeMap<String, Failure> {
        &self.failed
    }

    /// Sprites.
    pub fn sprites(&self) -> impl Iterator<Item = (&String, &SpriteAsset)> {
        self.entries.iter().filter_map(|(n, e)| match &e.asset {
            Asset::Sprite(s) => Some((n, s)),
            _ => None,
        })
    }

    /// Animator state machines.
    pub fn animators(&self) -> impl Iterator<Item = (&String, &AnimatorSpec)> {
        self.entries.iter().filter_map(|(n, e)| match &e.asset {
            Asset::Animator(a) => Some((n, a)),
            _ => None,
        })
    }

    /// Particle effects.
    pub fn emitters(&self) -> impl Iterator<Item = (&String, &EmitterSpec)> {
        self.entries.iter().filter_map(|(n, e)| match &e.asset {
            Asset::Emitter(f) => Some((n, f)),
            _ => None,
        })
    }

    /// Sounds defined by sidecars, for the mixer.
    pub fn sidecar_sounds(&self) -> Vec<SoundDef> {
        self.entries
            .values()
            .filter_map(|e| match &e.asset {
                Asset::Sound(s) if s.from_sidecar => Some(s.def.clone()),
                _ => None,
            })
            .collect()
    }

    /// Re-import everything, ignoring what is unchanged.
    pub fn rescan_all(&mut self) -> Report {
        self.inputs.clear();
        self.entries.clear();
        self.failed.clear();
        self.scan()
    }

    /// If any file under `assets/` changed since the last poll, scan.
    /// Cheap when nothing changed: one metadata read per file.
    pub fn poll(&mut self) -> Option<Report> {
        let mut now = BTreeMap::new();
        walk(&self.assets, &self.assets, &mut now);
        if now == self.tree {
            return None;
        }
        Some(self.scan())
    }

    /// Scan `assets/`: import what is new or changed, drop what went away.
    pub fn scan(&mut self) -> Report {
        let started = Instant::now();
        let mut tree = BTreeMap::new();
        walk(&self.assets, &self.assets, &mut tree);
        // Group files into assets by name.
        let mut groups: BTreeMap<String, Group> = BTreeMap::new();
        let mut report = Report::default();
        for rel in tree.keys() {
            let lower = rel.to_ascii_lowercase();
            let (name, slot) = if let Some(n) = lower.strip_suffix(".atlas.toml") {
                (rel[..n.len()].to_string(), 2)
            } else if let Some(n) = lower.strip_suffix(".toml") {
                (rel[..n.len()].to_string(), 2)
            } else if let Some(n) = lower.strip_suffix(".png") {
                (rel[..n.len()].to_string(), 0)
            } else if let Some(n) = lower.strip_suffix(".wav") {
                (rel[..n.len()].to_string(), 1)
            } else {
                continue;
            };
            let g = groups.entry(name).or_default();
            match slot {
                0 => g.png = Some(rel.clone()),
                1 => g.wav = Some(rel.clone()),
                _ => g.sidecar = Some(rel.clone()),
            }
        }
        // What went away.
        let gone: Vec<String> = self
            .entries
            .keys()
            .chain(self.failed.keys())
            .filter(|n| !groups.contains_key(*n))
            .cloned()
            .collect();
        for n in gone {
            if self.entries.remove(&n).is_some() {
                report.removed.push(n.clone());
            }
            self.failed.remove(&n);
            self.inputs.remove(&n);
            let _ = std::fs::remove_file(self.cache.join(format!("{n}.png")));
        }
        report.removed.sort();
        report.removed.dedup();
        for (name, g) in groups {
            // The files this asset reads: its own, plus a sound's variations
            // (added after the sidecar is parsed, below).
            let mut files: Vec<&String> =
                [&g.png, &g.wav, &g.sidecar].into_iter().flatten().collect();
            files.sort();
            let stamp: Stamp = files
                .iter()
                .filter_map(|f| tree.get(*f).map(|s| ((*f).clone(), *s)))
                .collect();
            let prev = self.inputs.get(&name);
            let unchanged = prev.is_some_and(|p| {
                p.iter().all(|(f, s)| tree.get(f) == Some(s))
                    && stamp.keys().all(|k| p.contains_key(k))
            });
            if unchanged && (self.entries.contains_key(&name) || self.failed.contains_key(&name)) {
                report.unchanged += 1;
                continue;
            }
            let t0 = Instant::now();
            match self.import(&name, &g) {
                Ok((mut entry, inputs)) => {
                    entry.ms = t0.elapsed().as_secs_f64() * 1e3;
                    let mut st = stamp.clone();
                    for f in inputs {
                        if let Some(s) = tree.get(&f) {
                            st.insert(f, *s);
                        }
                    }
                    let same = self
                        .entries
                        .get(&name)
                        .is_some_and(|e| e.hash == entry.hash);
                    self.inputs.insert(name.clone(), st);
                    self.failed.remove(&name);
                    if same {
                        report.unchanged += 1;
                    } else {
                        report.imported.push(name.clone());
                    }
                    self.entries.insert(name, entry);
                }
                Err(error) => {
                    self.inputs.insert(name.clone(), stamp);
                    self.entries.remove(&name);
                    let f = Failure {
                        name: name.clone(),
                        error,
                    };
                    self.failed.insert(name, f.clone());
                    report.failed.push(f);
                }
            }
        }
        self.tree = tree;
        let _ = self.write_manifest();
        report.ms = started.elapsed().as_secs_f64() * 1e3;
        report
    }

    fn read(&self, rel: &str) -> Result<Vec<u8>> {
        std::fs::read(self.assets.join(rel)).map_err(|e| {
            ArgusError::new(codes::BAD_SOURCE, format!("cannot read assets/{rel}: {e}"))
                .at(Some(format!("assets/{rel}")))
        })
    }

    /// Import one asset. Returns the entry and any extra input files
    /// (a sound's variations) to watch.
    fn import(&self, name: &str, g: &Group) -> Result<(Entry, Vec<String>)> {
        let mut hash: u64 = 0xcbf29ce484222325;
        fnv(&mut hash, IMPORTER_VERSION.as_bytes());
        let side_text = match &g.sidecar {
            Some(s) => {
                let b = self.read(s)?;
                fnv(&mut hash, &b);
                Some(String::from_utf8_lossy(&b).into_owned())
            }
            None => None,
        };
        let side_file = g.sidecar.as_ref().map(|s| format!("assets/{s}"));
        if g.png.is_some() && g.wav.is_some() {
            return Err(ArgusError::new(
                codes::AMBIGUOUS,
                format!("{name} has both a .png and a .wav; one name is one asset"),
            )
            .hint("Rename one of them (asset.move keeps references right).")
            .at(g.png.as_ref().map(|p| format!("assets/{p}"))));
        }
        let kind = match (&g.png, &g.wav, &side_text) {
            (Some(_), _, _) => SidecarKind::Sprite,
            (_, Some(_), _) => SidecarKind::Sound,
            (None, None, Some(t)) => sidecar::kind_of(side_file.as_deref().unwrap_or(""), t)?,
            (None, None, None) => SidecarKind::Unknown,
        };
        match kind {
            SidecarKind::Sprite => self.import_sprite(name, g, side_text.as_deref(), side_file.as_deref(), hash),
            SidecarKind::Sound => self.import_sound(name, g, side_text.as_deref(), side_file.as_deref(), hash),
            SidecarKind::Emitter => {
                let file = side_file.clone().unwrap_or_default();
                let (spec, notes) = effect::parse_emitter(&file, side_text.as_deref().unwrap_or(""))?;
                let warnings = notes
                    .into_iter()
                    .map(|(message, line)| Warning { message, at: format!("{file}:{line}") })
                    .collect();
                Ok((
                    Entry {
                        name: name.to_string(),
                        sidecar: g.sidecar.clone(),
                        hash,
                        warnings,
                        asset: Asset::Emitter(spec),
                        ms: 0.0,
                    },
                    Vec::new(),
                ))
            }
            SidecarKind::Animator => {
                let file = side_file.clone().unwrap_or_default();
                let (spec, notes) = animator::parse_animator(&file, side_text.as_deref().unwrap_or(""))?;
                let warnings = notes
                    .into_iter()
                    .map(|(message, line)| Warning { message, at: format!("{file}:{line}") })
                    .collect();
                Ok((
                    Entry {
                        name: name.to_string(),
                        sidecar: g.sidecar.clone(),
                        hash,
                        warnings,
                        asset: Asset::Animator(spec),
                        ms: 0.0,
                    },
                    Vec::new(),
                ))
            }
            SidecarKind::Unknown => Err(ArgusError::new(
                codes::AMBIGUOUS,
                format!("{name}: the sidecar describes no sprite, sound, animator, or emitter"),
            )
            .hint("Give it type = \"sprite\" (with grid, [[frames]], or kind/size/color), type = \"sound\" (with variations, tone, or file), type = \"animator\" (with [states.<name>]), or type = \"emitter\".")
            .at(side_file.clone())),
        }
    }

    fn import_sprite(
        &self,
        name: &str,
        g: &Group,
        side: Option<&str>,
        side_file: Option<&str>,
        mut hash: u64,
    ) -> Result<(Entry, Vec<String>)> {
        let sf = side_file.unwrap_or("");
        let sc = match side {
            Some(t) => sidecar::parse_sprite(sf, t)?,
            None => sidecar::SpriteSidecar::default(),
        };
        let mut warnings = Vec::new();
        let warn = |w: &mut Vec<Warning>, m: String, at: &str| {
            w.push(Warning {
                message: m,
                at: at.to_string(),
            })
        };
        for k in &sc.unknown {
            warn(&mut warnings, format!("unknown key {k:?}, ignored"), sf);
        }
        let (w, h, mut rgba, source) = match &g.png {
            Some(p) => {
                let bytes = self.read(p)?;
                fnv(&mut hash, &bytes);
                let (w, h, rgba) = decode_png(&self.assets.join(p))
                    .map_err(|e| e.at(Some(format!("assets/{p}"))))?;
                if sc.shape.is_some() {
                    warn(
                        &mut warnings,
                        format!("kind, size, and color are ignored: assets/{p} is the art now"),
                        sf,
                    );
                }
                (w, h, rgba, Some(p.clone()))
            }
            None => {
                let shape = sc.shape.clone().ok_or_else(|| {
                    ArgusError::new(
                        codes::AMBIGUOUS,
                        format!("{name}: a sprite sidecar with no image needs kind, size, and color"),
                    )
                    .hint("A placeholder is kind = \"rect\", size = [8, 8], color = \"#E8703F\"; or add the PNG.")
                    .at(side_file.map(str::to_string))
                })?;
                let [w, h] = shape.size;
                (w, h, rasterise(&shape), None)
            }
        };
        let at_src = source
            .as_ref()
            .map(|s| format!("assets/{s}"))
            .unwrap_or_else(|| sf.to_string());
        // Frames: explicit rects, a grid, or the whole image.
        let default_pivot = sc.pivot.unwrap_or([0.5, 0.5]);
        let default_ms = sc.duration_ms.unwrap_or(100);
        let mut frames: Vec<Frame> = Vec::new();
        let explicit: Vec<&sidecar::FrameSpec> =
            sc.frames.iter().filter(|f| f.rect.is_some()).collect();
        if !explicit.is_empty() {
            for (i, f) in explicit.iter().enumerate() {
                let r = f.rect.unwrap();
                if r[0] < 0
                    || r[1] < 0
                    || r[2] < 1
                    || r[3] < 1
                    || (r[0] + r[2]) as u32 > w
                    || (r[1] + r[3]) as u32 > h
                {
                    return Err(ArgusError::new(
                        codes::BAD_SIDECAR,
                        format!("frame {i} rect {r:?} is outside the {w}x{h} image"),
                    )
                    .at(Some(f.at.clone())));
                }
                frames.push(Frame {
                    rect: [r[0] as u32, r[1] as u32, r[2] as u32, r[3] as u32],
                    pivot: default_pivot,
                    duration_ms: default_ms,
                    properties: BTreeMap::new(),
                });
            }
        } else if let Some([cols, rows]) = sc.grid {
            let (fw, fh) = (w / cols, h / rows);
            if fw == 0 || fh == 0 {
                return Err(ArgusError::new(
                    codes::BAD_SIDECAR,
                    format!("grid {cols}x{rows} does not fit the {w}x{h} image"),
                )
                .at(Some(sf.to_string())));
            }
            if w % cols != 0 {
                warn(
                    &mut warnings,
                    format!(
                        "{w} px is not a multiple of {cols} columns; the last {} px are unused",
                        w % cols
                    ),
                    sf,
                );
            }
            if h % rows != 0 {
                warn(
                    &mut warnings,
                    format!(
                        "{h} px is not a multiple of {rows} rows; the last {} px are unused",
                        h % rows
                    ),
                    sf,
                );
            }
            for r in 0..rows {
                for c in 0..cols {
                    frames.push(Frame {
                        rect: [c * fw, r * fh, fw, fh],
                        pivot: default_pivot,
                        duration_ms: default_ms,
                        properties: BTreeMap::new(),
                    });
                }
            }
        } else {
            frames.push(Frame {
                rect: [0, 0, w, h],
                pivot: default_pivot,
                duration_ms: default_ms,
                properties: BTreeMap::new(),
            });
        }
        // Overrides by index (and explicit frames' own fields, in order).
        let mut explicit_i = 0u32;
        for f in &sc.frames {
            let i = match (f.index, f.rect) {
                (Some(i), _) => i,
                (None, Some(_)) => {
                    explicit_i += 1;
                    explicit_i - 1
                }
                (None, None) => {
                    warn(
                        &mut warnings,
                        "a [[frames]] entry with neither rect nor index describes nothing"
                            .to_string(),
                        &f.at,
                    );
                    continue;
                }
            };
            let Some(fr) = frames.get_mut(i as usize) else {
                warn(
                    &mut warnings,
                    format!("frame {i} does not exist; the sheet has {}", frames.len()),
                    &f.at,
                );
                continue;
            };
            if let Some(p) = f.pivot {
                fr.pivot = p;
            }
            if let Some(d) = f.duration_ms {
                fr.duration_ms = d;
            }
            fr.properties.extend(f.properties.clone());
            if let Some(s) = fr.properties.get("slope") {
                if parse_slope(s).is_none() {
                    warn(
                        &mut warnings,
                        format!("slope = {s} is not [left, right] with heights from 0 to 1; the tile is solid"),
                        &f.at,
                    );
                }
            }
        }
        for (i, fr) in frames.iter_mut().enumerate() {
            if !(0.0..=1.0).contains(&fr.pivot[0]) || !(0.0..=1.0).contains(&fr.pivot[1]) {
                warn(
                    &mut warnings,
                    format!(
                        "frame {i} pivot {:?} is outside the frame; clamped",
                        fr.pivot
                    ),
                    sf,
                );
                fr.pivot = [fr.pivot[0].clamp(0.0, 1.0), fr.pivot[1].clamp(0.0, 1.0)];
            }
            let [x, y, fw, fh] = fr.rect;
            let empty = (y..y + fh)
                .all(|yy| (x..x + fw).all(|xx| rgba[((yy * w + xx) * 4 + 3) as usize] == 0));
            if empty {
                warn(&mut warnings, format!("frame {i} is empty"), &at_src);
            }
        }
        // Clips.
        let mut clips = BTreeMap::new();
        for c in &sc.clips {
            let mut keep = Vec::new();
            for &f in &c.frames {
                if (f as usize) < frames.len() {
                    keep.push(f);
                } else {
                    warn(
                        &mut warnings,
                        format!(
                            "clip {} names frame {f}; the sheet has {} frames",
                            c.name,
                            frames.len()
                        ),
                        &c.at,
                    );
                }
            }
            if keep.is_empty() {
                warn(
                    &mut warnings,
                    format!("clip {} has no frames left; dropped", c.name),
                    &c.at,
                );
                continue;
            }
            clips.insert(
                c.name.clone(),
                Clip {
                    frames: keep,
                    fps: c.fps,
                    looping: c.looping,
                    direction: c.direction,
                },
            );
        }
        // Auto-tiling rules must name tiles the sheet has.
        let mut autotile = BTreeMap::new();
        for a in &sc.autotile {
            if let Some(bad) = a.tiles.iter().find(|&&t| t as usize > frames.len()) {
                return Err(ArgusError::new(
                    codes::BAD_SIDECAR,
                    format!("autotile.{} names tile {bad}; the sheet has {} frames (tile n is frame n - 1)", a.terrain, frames.len()),
                )
                .at(Some(a.at.clone())));
            }
            autotile.insert(a.terrain.clone(), a.clone());
        }
        // Pad to powers of two for the GPU, and say so.
        let (tw, th) = (pow2(w), pow2(h));
        if (tw, th) != (w, h) {
            warn(
                &mut warnings,
                format!("texture {w}x{h} is not power-of-two, padding to {tw}x{th}"),
                &at_src,
            );
            let mut padded = vec![0u8; (tw * th * 4) as usize];
            for y in 0..h {
                let src = (y * w * 4) as usize;
                let dst = (y * tw * 4) as usize;
                padded[dst..dst + (w * 4) as usize]
                    .copy_from_slice(&rgba[src..src + (w * 4) as usize]);
            }
            rgba = padded;
        }
        let texture = self.cache.join(format!("{name}.png"));
        encode_png(&texture, tw, th, &rgba)?;
        Ok((
            Entry {
                name: name.to_string(),
                sidecar: g.sidecar.clone(),
                hash,
                warnings,
                asset: Asset::Sprite(SpriteAsset {
                    source,
                    width: w,
                    height: h,
                    texture,
                    texture_size: [tw, th],
                    frames,
                    clips,
                    pixels_per_unit: sc.pixels_per_unit.unwrap_or(16.0),
                    autotile,
                    shape: if g.png.is_none() {
                        sc.shape.clone()
                    } else {
                        None
                    },
                }),
                ms: 0.0,
            },
            Vec::new(),
        ))
    }

    fn import_sound(
        &self,
        name: &str,
        g: &Group,
        side: Option<&str>,
        side_file: Option<&str>,
        mut hash: u64,
    ) -> Result<(Entry, Vec<String>)> {
        let mut warnings = Vec::new();
        let dir = name
            .rsplit_once('/')
            .map(|(d, _)| format!("{d}/"))
            .unwrap_or_default();
        let (mut def, from_sidecar) = match side {
            Some(t) => (
                argus_audio::parse_sound_sidecar(name, side_file.unwrap_or(""), t)?,
                true,
            ),
            None => (
                SoundDef {
                    name: name.to_string(),
                    bus: if name.starts_with("music/") {
                        "music"
                    } else {
                        "sfx"
                    }
                    .to_string(),
                    gain: 1.0,
                    gain_jitter: 0.0,
                    pitch_jitter: 0.0,
                    variations: Vec::new(),
                    spatial: true,
                    looping: false,
                    max_voices: 8,
                    at: format!("assets/{}", g.wav.clone().unwrap_or_default()),
                },
                false,
            ),
        };
        // A sidecar beside a WAV plays that WAV unless it names others.
        if let Some(wav) = &g.wav {
            if !def.variations.iter().any(|v| matches!(v, Source::File(_))) {
                def.variations.push(Source::File(wav.clone()));
            }
        }
        // Variation paths: relative to the sidecar's folder, else to assets/.
        let mut extra = Vec::new();
        let mut files = Vec::new();
        for v in &mut def.variations {
            if let Source::File(f) = v {
                let local = format!("{dir}{f}");
                let rel = if self.assets.join(&local).is_file() && !f.starts_with(&dir) {
                    local.clone()
                } else {
                    f.clone()
                };
                let path = self.assets.join(&rel);
                if !path.is_file() {
                    return Err(ArgusError::new(
                        codes::BAD_SOURCE,
                        format!("{name}: variation {f:?} is not a file under assets/ (looked for assets/{local} and assets/{f})"),
                    )
                    .at(side_file.map(str::to_string)));
                }
                let bytes = self.read(&rel)?;
                fnv(&mut hash, &bytes);
                let info =
                    probe_wav(&path, &rel).map_err(|e| e.at(Some(format!("assets/{rel}"))))?;
                let at = format!("assets/{rel}");
                if info.channels > 1 {
                    warnings.push(Warning {
                        message: format!(
                            "{} channels, mixed to mono for spatial playback",
                            info.channels
                        ),
                        at: at.clone(),
                    });
                }
                if info.rate != argus_audio::RATE {
                    warnings.push(Warning {
                        message: format!(
                            "{} Hz, resampled to {} Hz at load",
                            info.rate,
                            argus_audio::RATE
                        ),
                        at: at.clone(),
                    });
                }
                if info.peak_db <= -90.0 {
                    warnings.push(Warning {
                        message: "the file is silent".to_string(),
                        at: at.clone(),
                    });
                } else if info.peak_db > -0.1 {
                    warnings.push(Warning {
                        message: format!(
                            "peaks at {:.1} dBFS: it clips; leave headroom",
                            info.peak_db
                        ),
                        at: at.clone(),
                    });
                }
                if info.seconds > 30.0 && !name.starts_with("music/") {
                    warnings.push(Warning {
                        message: format!(
                            "{:.0} s long for a sound effect; music belongs under music/",
                            info.seconds
                        ),
                        at,
                    });
                }
                extra.push(rel.clone());
                files.push(info);
                *f = rel;
            }
        }
        Ok((
            Entry {
                name: name.to_string(),
                sidecar: g.sidecar.clone(),
                hash,
                warnings,
                asset: Asset::Sound(SoundAsset {
                    def,
                    files,
                    from_sidecar,
                }),
                ms: 0.0,
            },
            extra,
        ))
    }

    fn write_manifest(&self) -> Result<()> {
        let entries: Vec<Json> = self.entries.values().map(Entry::to_json).collect();
        let failed: Vec<Json> = self
            .failed
            .values()
            .map(|f| json!({ "name": f.name, "message": f.error.message, "at": f.error.r#ref }))
            .collect();
        let text = serde_json::to_string_pretty(&json!({
            "importer": IMPORTER_VERSION,
            "assets": entries,
            "failed": failed,
        }))
        .unwrap_or_default();
        std::fs::create_dir_all(&self.cache)
            .map_err(|e| ArgusError::new(codes::CACHE, e.to_string()))?;
        std::fs::write(self.cache.join("manifest.json"), text)
            .map_err(|e| ArgusError::new(codes::CACHE, e.to_string()))
    }
}

/// Draw a described shape into RGBA pixels.
pub fn rasterise(shape: &Shape) -> Vec<u8> {
    let [w, h] = shape.size;
    let c = sidecar::parse_color(&shape.color).unwrap_or([255, 0, 255, 255]);
    let mut out = vec![0u8; (w * h * 4) as usize];
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    for y in 0..h {
        for x in 0..w {
            let inside = match shape.kind.as_str() {
                "circle" => {
                    let dx = (x as f32 + 0.5 - cx) / cx;
                    let dy = (y as f32 + 0.5 - cy) / cy;
                    dx * dx + dy * dy <= 1.0
                }
                _ => true,
            };
            if inside {
                let i = ((y * w + x) * 4) as usize;
                out[i..i + 4].copy_from_slice(&c);
            }
        }
    }
    out
}

fn probe_wav(path: &Path, rel: &str) -> Result<WavInfo> {
    let e = |m: String| ArgusError::new(codes::BAD_SOURCE, format!("assets/{rel}: {m}"));
    let mut r = hound::WavReader::open(path).map_err(|x| e(x.to_string()))?;
    let spec = r.spec();
    let frames = r.duration();
    let mut peak = 0.0f32;
    match spec.sample_format {
        hound::SampleFormat::Float => {
            for s in r.samples::<f32>() {
                peak = peak.max(s.map_err(|x| e(x.to_string()))?.abs());
            }
        }
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample.max(1) - 1)) as f32;
            for s in r.samples::<i32>() {
                peak = peak.max((s.map_err(|x| e(x.to_string()))? as f32 * scale).abs());
            }
        }
    }
    Ok(WavInfo {
        path: rel.to_string(),
        rate: spec.sample_rate,
        channels: spec.channels,
        seconds: frames as f32 / spec.sample_rate.max(1) as f32,
        peak_db: argus_audio::lin_to_db(peak),
    })
}
