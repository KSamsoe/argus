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
//! material = "fx/glow"           # optional: a material to draw them with (item 11)
//! velocity = [[0, 1], [1, 0.2]]  # curves over life: [t, value], t from 0 to 1
//! size = [[0, 0.15], [0.2, 0.25], [1, 0]]
//! rotation = [[0, 0], [1, 180]]  # degrees
//! color = [[0, "#FFE08AFF"], [0.6, "#FF7A30C0"], [1, "#FF400000"]]
//! ```
//!
//! A curve may also be one number (a constant). This module checks the file;
//! `argus-particles` bakes the curves and runs the pools.
//!
//! [`KNOBS`] are the numbers a person tunes for feel (§13: "every knob it
//! needs must be a field the tuning panel can show"), and [`knobs`] finds
//! them in a sidecar's text: the daemon's `tune.fields` lists them and
//! `tune.set {asset}` moves them live (phase 5 item 3).

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
    /// A material to draw them with; none for the plain sprite.
    pub material: Option<String>,
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
    "material",
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

/// What two numbers are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pair {
    /// `[min, max]`, 0 or more, min first; one number is a range of one.
    Range,
    /// `[width, height]`, 0 or more; one number is a square.
    Size,
    /// `[x, y]`, always two.
    Vector,
}

/// How a knob's key is written in the sidecar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnobShape {
    /// One number: `drag = 3`.
    Number,
    /// One number, or two named by these words: `speed = [3, 7]` is
    /// `speed[0]` (min) and `speed[1]` (max).
    Pair([&'static str; 2]),
    /// A curve over life: one number, or `[[t, value], ...]`, whose values
    /// are the knobs (`size[0][1]` is the size at the first point's t).
    Curve,
}

/// A number in an emitter's sidecar that a person tunes for feel. What
/// draws (`sprite`, `particle`, `frame`, `material`), where (`shape`,
/// `space`, `z`), and the pool's size (`max`) are what the effect is, not
/// how it feels, and are not knobs; nor are a curve's t, which moved past
/// its neighbour breaks the curve, nor the colours, which are not numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Knob {
    /// The key in the sidecar.
    pub key: &'static str,
    /// How it is written.
    pub shape: KnobShape,
    /// A slider's span. A value in the file outside it widens it, so a
    /// knob is never stuck at an end it is already past.
    pub range: (f64, f64),
    /// What the parser reads when the key is left out.
    pub default: f64,
    /// A count: whole numbers only.
    pub whole: bool,
    /// What it does, for the slider's hover.
    pub doc: &'static str,
}

const fn knob(
    key: &'static str,
    shape: KnobShape,
    range: (f64, f64),
    default: f64,
    doc: &'static str,
) -> Knob {
    Knob {
        key,
        shape,
        range,
        default,
        whole: false,
        doc,
    }
}

const MIN_MAX: KnobShape = KnobShape::Pair(["min", "max"]);

/// Every knob an emitter has, in the order a panel shows them.
pub const KNOBS: &[Knob] = &[
    knob(
        "rate",
        KnobShape::Number,
        (0.0, 2000.0),
        0.0,
        "Particles a second while it plays",
    ),
    Knob {
        whole: true,
        ..knob(
            "burst",
            KnobShape::Number,
            (0.0, 1000.0),
            0.0,
            "Particles at once when it starts",
        )
    },
    knob(
        "duration",
        KnobShape::Number,
        (0.0, 30.0),
        0.0,
        "Seconds of emission; 0 is for as long as it plays",
    ),
    knob(
        "lifetime",
        MIN_MAX,
        (0.01, 10.0),
        1.0,
        "Seconds a particle lives, picked between min and max",
    ),
    knob(
        "speed",
        MIN_MAX,
        (0.0, 50.0),
        1.0,
        "Starting speed in units a second, picked between min and max",
    ),
    knob(
        "direction",
        KnobShape::Number,
        (-360.0, 360.0),
        90.0,
        "Degrees the cone points: 0 is right, 90 is up",
    ),
    knob(
        "spread",
        KnobShape::Number,
        (0.0, 360.0),
        360.0,
        "Degrees across the cone",
    ),
    knob(
        "gravity",
        KnobShape::Pair(["x", "y"]),
        (-50.0, 50.0),
        0.0,
        "Acceleration in units a second, each second",
    ),
    knob(
        "drag",
        KnobShape::Number,
        (0.0, 20.0),
        0.0,
        "Velocity lost a second, as a rate",
    ),
    knob(
        "radius",
        KnobShape::Number,
        (0.0, 20.0),
        0.5,
        "The circle particles start in, in units (shape = \"circle\")",
    ),
    knob(
        "box",
        KnobShape::Pair(["width", "height"]),
        (0.0, 50.0),
        1.0,
        "The box particles start in, in units (shape = \"box\")",
    ),
    knob(
        "velocity",
        KnobShape::Curve,
        (0.0, 5.0),
        1.0,
        "Speed multiplier over life",
    ),
    knob(
        "size",
        KnobShape::Curve,
        (0.0, 5.0),
        0.2,
        "Size over life, in units",
    ),
    knob(
        "rotation",
        KnobShape::Curve,
        (-720.0, 720.0),
        0.0,
        "Rotation over life, in degrees",
    ),
];

/// One knob a sidecar holds.
#[derive(Clone, Debug, PartialEq)]
pub struct KnobAt {
    /// Its path in the sidecar, as `edit.asset` names one: `drag`,
    /// `speed[1]`, `size[0][1]` (the value at the curve's first point).
    pub path: String,
    /// What a slider says: `drag`, `speed max`, `size at 0`.
    pub name: String,
    /// The value in the text.
    pub value: f64,
    /// Which knob.
    pub knob: &'static Knob,
}

impl KnobAt {
    /// The knob's span, widened to hold `v` (a file's value past an end).
    pub fn range_with(&self, v: f64) -> (f64, f64) {
        (self.knob.range.0.min(v), self.knob.range.1.max(v))
    }
}

/// The knobs an emitter sidecar's text holds, in the file's own shape: a
/// number is one (`speed = 4` is `speed`), two numbers two (`speed[0]`,
/// `speed[1]`), a curve one a point. Keys the file leaves out are not
/// knobs until it has them (`edit.asset` adds one), and a value that does
/// not read is left to the importer to report.
pub fn knobs(text: &str) -> Vec<KnobAt> {
    let Ok(doc) = ImDocument::parse(text.to_string()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for k in KNOBS {
        let Some(v) = doc.get(k.key).and_then(Item::as_value) else {
            continue;
        };
        // As written, not as the f32 the parser reads: a slider shows 0.45.
        let num = |v: &TV| v.as_float().or_else(|| v.as_integer().map(|i| i as f64));
        let at = |path: String, name: String, value: f64| KnobAt {
            path,
            name,
            value,
            knob: k,
        };
        if let Some(n) = num(v) {
            out.push(at(k.key.to_string(), k.key.to_string(), n));
            continue;
        }
        let Some(a) = v.as_array() else {
            continue;
        };
        match k.shape {
            KnobShape::Number => {}
            KnobShape::Pair(words) => {
                if a.len() == 2 {
                    for (i, w) in words.iter().enumerate() {
                        if let Some(n) = a.get(i).and_then(num) {
                            out.push(at(format!("{}[{i}]", k.key), format!("{} {w}", k.key), n));
                        }
                    }
                }
            }
            KnobShape::Curve => {
                for (i, p) in a.iter().enumerate() {
                    let tv = p
                        .as_array()
                        .filter(|p| p.len() == 2)
                        .and_then(|p| Some((num(p.get(0)?)?, num(p.get(1)?)?)));
                    if let Some((t, n)) = tv {
                        out.push(at(
                            format!("{}[{i}][1]", k.key),
                            format!("{} at {t}", k.key),
                            n,
                        ));
                    }
                }
            }
        }
    }
    out
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
    let pair = |k: &str, d: [f32; 2], kind: Pair| -> Result<[f32; 2]> {
        match get(k) {
            None => Ok(d),
            Some(i) => {
                let v = i.as_value();
                // A number is a range (or a square) of one; a vector must be two.
                let p = match v.and_then(num).filter(|_| kind != Pair::Vector) {
                    Some(n) => Some([n, n]),
                    None => v.and_then(|v| v.as_array()).and_then(|a| {
                        let xs: Vec<f32> = a.iter().filter_map(num).collect();
                        (xs.len() == 2 && a.len() == 2).then(|| [xs[0], xs[1]])
                    }),
                };
                match (p, kind) {
                    (Some(p), Pair::Vector) => Ok(p),
                    (Some(p), Pair::Range) if p[0] >= 0.0 && p[1] >= p[0] => Ok(p),
                    // A box is as wide and as tall as it likes (it was read
                    // as a range, so a box wider than tall was refused).
                    (Some(p), Pair::Size) if p[0] >= 0.0 && p[1] >= 0.0 => Ok(p),
                    (_, Pair::Range) => Err(bad(
                        file,
                        line(k),
                        format!("{k} is a number or [min, max], 0 or more, min first"),
                    )),
                    (_, Pair::Size) => Err(bad(
                        file,
                        line(k),
                        format!("{k} is a number or [width, height], 0 or more"),
                    )),
                    (_, Pair::Vector) => Err(bad(file, line(k), format!("{k} is [x, y]"))),
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
            size: pair("box", [1.0, 1.0], Pair::Size)?,
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
    let lifetime = pair("lifetime", [1.0, 1.0], Pair::Range)?;
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
        speed: pair("speed", [1.0, 1.0], Pair::Range)?,
        direction: number("direction", 90.0, -1e6, 1e6)?,
        spread: number("spread", 360.0, 0.0, 360.0)?,
        gravity: pair("gravity", [0.0, 0.0], Pair::Vector)?,
        drag: number("drag", 0.0, 0.0, 1e4)?,
        space,
        z: number("z", 0.0, -1e9, 1e9)?,
        material: get("material").and_then(|s| s.as_str()).map(str::to_string),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Swarm's sparks, as `examples/swarm/assets/fx/pop.toml` has them.
    const POP: &str = r##"# Sparks where an enemy pops.
type = "emitter"
particle = "circle"
max = 20000
lifetime = [0.2, 0.45]
speed = [3, 7]
spread = 360
drag = 3
size = [[0, 0.35], [1, 0]]
color = [[0, "#FFF2A0FF"], [0.5, "#FF9040C0"], [1, "#FF402000"]]
z = 4
material = "fx/glow"
"##;

    #[test]
    fn the_knobs_are_the_numbers_a_person_tunes_in_the_files_own_shape() {
        let ks = knobs(POP);
        let paths: Vec<(&str, &str, f64)> = ks
            .iter()
            .map(|k| (k.path.as_str(), k.name.as_str(), k.value))
            .collect();
        assert_eq!(
            paths,
            vec![
                ("lifetime[0]", "lifetime min", 0.2),
                ("lifetime[1]", "lifetime max", 0.45),
                ("speed[0]", "speed min", 3.0),
                ("speed[1]", "speed max", 7.0),
                ("spread", "spread", 360.0),
                ("drag", "drag", 3.0),
                ("size[0][1]", "size at 0", 0.35),
                ("size[1][1]", "size at 1", 0.0),
            ],
            "not max, particle, material, z, the colours, or a curve's t"
        );
        // One number is one knob; gravity is two; a constant curve is one.
        let ks = knobs(
            "type = \"emitter\"\nspeed = 4\ngravity = [0, -9.8]\nrotation = 90\nburst = 30\n",
        );
        let names: Vec<&str> = ks.iter().map(|k| k.path.as_str()).collect();
        assert_eq!(
            names,
            vec!["burst", "speed", "gravity[0]", "gravity[1]", "rotation"]
        );
        assert!(ks[0].knob.whole, "a burst is a count");
        // A value past the span widens it.
        let speed = &ks[1];
        assert_eq!(speed.range_with(4.0), (0.0, 50.0));
        assert_eq!(speed.range_with(250.0), (0.0, 250.0));
        // Text that does not read has none.
        assert!(knobs("speed = [").is_empty());
    }

    #[test]
    fn a_box_is_as_wide_as_it_likes_and_a_range_is_min_first() {
        let (spec, _) = parse_emitter(
            "fx/b.toml",
            "type = \"emitter\"\nshape = \"box\"\nbox = [4, 1]\n",
        )
        .unwrap();
        assert_eq!(spec.area, Area::Box { size: [4.0, 1.0] });
        let (spec, _) = parse_emitter(
            "fx/b.toml",
            "type = \"emitter\"\nshape = \"box\"\nbox = 2\n",
        )
        .unwrap();
        assert_eq!(spec.area, Area::Box { size: [2.0, 2.0] });
        let e = parse_emitter(
            "fx/b.toml",
            "type = \"emitter\"\nshape = \"box\"\nbox = [-1, 1]\n",
        )
        .unwrap_err();
        assert!(e.message.contains("[width, height]"), "{}", e.message);
        let e = parse_emitter("fx/s.toml", "type = \"emitter\"\nspeed = [8, 7]\n").unwrap_err();
        assert!(e.message.contains("min first"), "{}", e.message);
        let e = parse_emitter("fx/g.toml", "type = \"emitter\"\ngravity = 3\n").unwrap_err();
        assert!(e.message.contains("[x, y]"), "{}", e.message);
    }
}
