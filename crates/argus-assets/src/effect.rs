//! Particle effects as assets (§13 "Particles and VFX", phase 1 item 10): a
//! TOML file with `type = "emitter"`, everything as data and the curves as
//! control-point lists.
//!
//! ```toml
//! # assets/fx/sparks.toml
//! type = "emitter"
//! sprite = "fx/spark"            # an atlas; or particle = "circle" / "rect"
//! shape = "circle"               # where particles start: point, circle, box
//! radius = 0.2                   # (box = [w, h] for a box)
//! rate = 120                     # per second while playing
//! burst = 30                     # at once when it starts
//! duration = 0                   # seconds of emission; 0 is for as long as it plays
//! max = 2000                     # the pool's size
//! lifetime = [0.4, 0.9]          # seconds, a number or a [min, max] range
//! speed = [3, 6]
//! direction = 90                 # degrees, 0 is right, 90 is up
//! spread = 60                    # degrees across the cone
//! gravity = [0, -12]
//! drag = 1.5                     # velocity lost per second, as a rate
//! space = "world"                # or "local": particles follow the emitter
//! z = 2
//! velocity = [[0, 1], [1, 0.2]]  # curves over life: [t, value], t from 0 to 1
//! size = [[0, 0.15], [0.2, 0.25], [1, 0]]
//! rotation = [[0, 0], [1, 180]]  # degrees
//! color = [[0, "#FFE08AFF"], [0.6, "#FF7A30C0"], [1, "#FF400000"]]
//! ```
//!
//! A curve may also be one number (a constant). This module checks the file;
//! `argus-particles` bakes the curves and runs the pools.

use crate::codes;
use crate::sidecar::parse_color;
use argus_ecs::{ArgusError, Result};
use toml_edit::{ImDocument, Item, Value as TV};

/// Where particles start, relative to the emitter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Area {
    /// At the emitter.
    Point,
    /// Anywhere in a circle.
    Circle {
        /// Radius.
        radius: f32,
    },
    /// Anywhere in a box centred on the emitter.
    Box {
        /// Width and height.
        size: [f32; 2],
    },
}

/// A scalar over a particle's life: points of (t from 0 to 1, value),
/// ascending in t, linear between them.
#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    /// The control points.
    pub points: Vec<(f32, f32)>,
}

impl Curve {
    /// A constant.
    pub fn constant(v: f32) -> Curve {
        Curve {
            points: vec![(0.0, v)],
        }
    }
    /// The value at `t`.
    pub fn at(&self, t: f32) -> f32 {
        let p = &self.points;
        if t <= p[0].0 {
            return p[0].1;
        }
        for w in p.windows(2) {
            if t <= w[1].0 {
                let span = (w[1].0 - w[0].0).max(1e-6);
                return w[0].1 + (w[1].1 - w[0].1) * (t - w[0].0) / span;
            }
        }
        p[p.len() - 1].1
    }
}

/// A colour over life: (t, sRGBA bytes) points.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorCurve {
    /// The control points.
    pub points: Vec<(f32, [u8; 4])>,
}

/// World-space particles stay where they were born; local ones move with
/// the emitter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    /// Where they were born.
    World,
    /// Relative to the emitter.
    Local,
}

/// An imported particle effect.
#[derive(Clone, Debug, PartialEq)]
pub struct EmitterSpec {
    /// The sidecar, `assets/...`.
    pub file: String,
    /// An atlas to draw particles from.
    pub sprite: Option<String>,
    /// Its frame.
    pub frame: u32,
    /// A described shape when there is no sprite: rect or circle.
    pub particle: String,
    /// Where particles start.
    pub area: Area,
    /// Per second while playing.
    pub rate: f32,
    /// At once when it starts.
    pub burst: u32,
    /// Seconds of emission; 0 is endless.
    pub duration: f32,
    /// Pool size.
    pub max: u32,
    /// Seconds, min and max.
    pub lifetime: [f32; 2],
    /// Starting speed, min and max.
    pub speed: [f32; 2],
    /// Degrees; 0 is right, 90 is up.
    pub direction: f32,
    /// Degrees across the cone.
    pub spread: f32,
    /// Acceleration.
    pub gravity: [f32; 2],
    /// Velocity lost per second (a rate).
    pub drag: f32,
    /// World or local.
    pub space: Space,
    /// Draw order.
    pub z: f32,
    /// Speed multiplier over life.
    pub velocity: Curve,
    /// Size over life, world units.
    pub size: Curve,
    /// Rotation over life, degrees.
    pub rotation: Curve,
    /// Colour over life.
    pub color: ColorCurve,
}

const KEYS: &[&str] = &[
    "type",
    "sprite",
    "frame",
    "particle",
    "shape",
    "radius",
    "box",
    "rate",
    "burst",
    "duration",
    "max",
    "lifetime",
    "speed",
    "direction",
    "spread",
    "gravity",
    "drag",
    "space",
    "z",
    "velocity",
    "size",
    "rotation",
    "color",
];

const HINT: &str = "An emitter is type = \"emitter\" with sprite or particle, shape (point, circle with radius, box), rate, burst, duration, max, lifetime, speed, direction, spread, gravity, drag, space, z, and curves velocity, size, rotation, color as [[t, value], ...] with t from 0 to 1.";

fn bad(file: &str, line: u32, msg: impl Into<String>) -> ArgusError {
    ArgusError::new(codes::BAD_SIDECAR, msg.into())
        .hint(HINT)
        .at(Some(format!("{file}:{line}")))
}

fn line_of(text: &str, item: &Item) -> u32 {
    let at = item.span().map(|s| s.start).unwrap_or(0).min(text.len());
    text[..at].matches('\n').count() as u32 + 1
}

fn num(v: &TV) -> Option<f32> {
    v.as_float()
        .map(|f| f as f32)
        .or_else(|| v.as_integer().map(|i| i as f32))
}

/// Parse an emitter sidecar. Mistakes are errors with `file:line`; unknown
/// keys come back as warnings.
pub fn parse_emitter(file: &str, text: &str) -> Result<(EmitterSpec, Vec<(String, u32)>)> {
    let doc = ImDocument::parse(text.to_string()).map_err(|e| {
        let line = e
            .span()
            .map(|s| text[..s.start.min(text.len())].matches('\n').count() as u32 + 1)
            .unwrap_or(1);
        bad(file, line, format!("not valid TOML: {}", e.message()))
    })?;
    let mut warnings = Vec::new();
    for (k, i) in doc.as_table().iter() {
        if !KEYS.contains(&k) {
            warnings.push((format!("unknown key {k:?}, ignored"), line_of(text, i)));
        }
    }
    let get = |k: &str| doc.get(k);
    let line = |k: &str| get(k).map(|i| line_of(text, i)).unwrap_or(1);
    let number = |k: &str, d: f32, lo: f32, hi: f32| -> Result<f32> {
        match get(k) {
            None => Ok(d),
            Some(i) => i
                .as_value()
                .and_then(num)
                .filter(|v| (lo..=hi).contains(v))
                .ok_or_else(|| bad(file, line(k), format!("{k} is a number from {lo} to {hi}"))),
        }
    };
    let pair = |k: &str, d: [f32; 2], positive: bool| -> Result<[f32; 2]> {
        match get(k) {
            None => Ok(d),
            Some(i) => {
                let v = i.as_value();
                // A number is a range of one; a vector must be two.
                let p = match v.and_then(num).filter(|_| positive) {
                    Some(n) => Some([n, n]),
                    None => v.and_then(|v| v.as_array()).and_then(|a| {
                        let xs: Vec<f32> = a.iter().filter_map(num).collect();
                        (xs.len() == 2 && a.len() == 2).then(|| [xs[0], xs[1]])
                    }),
                };
                match p {
                    Some(p) if !positive || (p[0] >= 0.0 && p[1] >= p[0]) => Ok(p),
                    _ if positive => Err(bad(
                        file,
                        line(k),
                        format!("{k} is a number or [min, max], 0 or more, min first"),
                    )),
                    _ => Err(bad(file, line(k), format!("{k} is [x, y]"))),
                }
            }
        }
    };
    let curve = |k: &str, d: f32| -> Result<Curve> {
        let Some(i) = get(k) else {
            return Ok(Curve::constant(d));
        };
        let v = i.as_value();
        if let Some(n) = v.and_then(num) {
            return Ok(Curve::constant(n));
        }
        let arr = v.and_then(|v| v.as_array()).ok_or_else(|| {
            bad(
                file,
                line(k),
                format!("{k} is a number or [[t, value], ...]"),
            )
        })?;
        let mut points = Vec::new();
        for p in arr.iter() {
            let pa = p.as_array().filter(|a| a.len() == 2);
            let tv = pa.and_then(|a| Some((num(a.get(0)?)?, num(a.get(1)?)?)));
            let (t, v) =
                tv.ok_or_else(|| bad(file, line(k), format!("each point of {k} is [t, value]")))?;
            points.push((t, v));
        }
        check_ts(file, line(k), k, points.iter().map(|p| p.0))?;
        Ok(Curve { points })
    };
    let color = match get("color") {
        None => ColorCurve {
            points: vec![(0.0, [255, 255, 255, 255])],
        },
        Some(i) => {
            let v = i.as_value();
            if let Some(c) = v.and_then(|v| v.as_str()) {
                let rgba = parse_color(c).ok_or_else(|| {
                    bad(
                        file,
                        line("color"),
                        format!("color {c:?} is not #RRGGBB or #RRGGBBAA"),
                    )
                })?;
                ColorCurve {
                    points: vec![(0.0, rgba)],
                }
            } else {
                let arr = v.and_then(|v| v.as_array()).ok_or_else(|| {
                    bad(
                        file,
                        line("color"),
                        "color is \"#RRGGBBAA\" or [[t, \"#RRGGBBAA\"], ...]",
                    )
                })?;
                let mut points = Vec::new();
                for p in arr.iter() {
                    let pa = p.as_array().filter(|a| a.len() == 2);
                    let tc =
                        pa.and_then(|a| Some((num(a.get(0)?)?, a.get(1)?.as_str()?.to_string())));
                    let (t, c) = tc.ok_or_else(|| {
                        bad(
                            file,
                            line("color"),
                            "each point of color is [t, \"#RRGGBBAA\"]",
                        )
                    })?;
                    let rgba = parse_color(&c).ok_or_else(|| {
                        bad(
                            file,
                            line("color"),
                            format!("color {c:?} is not #RRGGBB or #RRGGBBAA"),
                        )
                    })?;
                    points.push((t, rgba));
                }
                check_ts(file, line("color"), "color", points.iter().map(|p| p.0))?;
                ColorCurve { points }
            }
        }
    };
    let area = match get("shape").and_then(|s| s.as_str()).unwrap_or("point") {
        "point" => Area::Point,
        "circle" => Area::Circle {
            radius: number("radius", 0.5, 0.0, 1e6)?,
        },
        "box" => Area::Box {
            size: pair("box", [1.0, 1.0], true)?,
        },
        other => {
            return Err(bad(
                file,
                line("shape"),
                format!("shape = {other:?}; shapes are point, circle, box"),
            ))
        }
    };
    let space = match get("space").and_then(|s| s.as_str()).unwrap_or("world") {
        "world" => Space::World,
        "local" => Space::Local,
        other => {
            return Err(bad(
                file,
                line("space"),
                format!("space = {other:?}; spaces are world and local"),
            ))
        }
    };
    let particle = get("particle")
        .and_then(|p| p.as_str())
        .unwrap_or("rect")
        .to_string();
    if particle != "rect" && particle != "circle" {
        return Err(bad(
            file,
            line("particle"),
            format!("particle = {particle:?}; described particles are rect and circle"),
        ));
    }
    let lifetime = pair("lifetime", [1.0, 1.0], true)?;
    if lifetime[0] <= 0.0 {
        return Err(bad(file, line("lifetime"), "lifetime must be above 0"));
    }
    let spec = EmitterSpec {
        file: file.to_string(),
        sprite: get("sprite").and_then(|s| s.as_str()).map(str::to_string),
        frame: number("frame", 0.0, 0.0, 1e6)? as u32,
        particle,
        area,
        rate: number("rate", 0.0, 0.0, 1e7)?,
        burst: number("burst", 0.0, 0.0, 1e7)? as u32,
        duration: number("duration", 0.0, 0.0, 1e7)?,
        max: number("max", 1000.0, 1.0, 1e7)? as u32,
        lifetime,
        speed: pair("speed", [1.0, 1.0], true)?,
        direction: number("direction", 90.0, -1e6, 1e6)?,
        spread: number("spread", 360.0, 0.0, 360.0)?,
        gravity: pair("gravity", [0.0, 0.0], false)?,
        drag: number("drag", 0.0, 0.0, 1e4)?,
        space,
        z: number("z", 0.0, -1e9, 1e9)?,
        velocity: curve("velocity", 1.0)?,
        size: curve("size", 0.2)?,
        rotation: curve("rotation", 0.0)?,
        color,
    };
    Ok((spec, warnings))
}

fn check_ts(file: &str, line: u32, k: &str, ts: impl Iterator<Item = f32>) -> Result<()> {
    let ts: Vec<f32> = ts.collect();
    if ts.is_empty() {
        return Err(bad(file, line, format!("{k} needs at least one point")));
    }
    if ts.iter().any(|t| !(0.0..=1.0).contains(t)) || ts.windows(2).any(|w| w[1] < w[0]) {
        return Err(bad(
            file,
            line,
            format!("{k}: t goes from 0 to 1 and never back"),
        ));
    }
    Ok(())
}
