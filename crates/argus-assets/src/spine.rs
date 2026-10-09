//! Spine skeletons (§13 "Skeletal 2D via a Spine-compatible runtime",
//! phase 5 item 8): a skeleton's JSON export, read into the data the
//! engine's own runtime (argus-skeletal) poses and draws. No Spine code:
//! whoever makes the files uses the Spine editor; the engine reads what it
//! writes (decided with Kevin, 2026-10-04).
//!
//! `assets/<name>.json` is the skeleton, exported from Spine 4.0 or later
//! with its images unpacked: each region attachment is an image the
//! skeleton's `images` path names (by default `images/` beside the JSON),
//! an ordinary sprite asset. What is read: bones and their transform
//! inheritance; slots, their colours and setup attachments; skins and their
//! region attachments; events; one- and two-bone IK constraints; and
//! animations: every bone timeline (rotate, translate, scale, shear, and
//! each axis alone), slot attachments and colours, events, and draw
//! order, with linear, stepped, and bezier curves. What is not (meshes,
//! paths, clipping, transform, path, and physics constraints, deform) is a
//! warning with its line, never silent.

use crate::{codes, Warning};
use argus_ecs::{ArgusError, Result};
use serde_json::Value as Json;
use std::collections::BTreeMap;

/// How a bone takes its parent's transform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inherit {
    /// All of it.
    Normal,
    /// Its position only.
    OnlyTranslation,
    /// Not its rotation or reflection.
    NoRotationOrReflection,
    /// Not its scale.
    NoScale,
    /// Not its scale or reflection.
    NoScaleOrReflection,
}

/// A bone's setup pose.
#[derive(Clone, Debug, PartialEq)]
pub struct BoneData {
    /// Name.
    pub name: String,
    /// Parent bone, by index (a parent comes before its children).
    pub parent: Option<usize>,
    /// Length, for IK.
    pub length: f32,
    /// Position relative to the parent.
    pub x: f32,
    /// Position relative to the parent.
    pub y: f32,
    /// Degrees, counter-clockwise.
    pub rotation: f32,
    /// Scale x.
    pub scale_x: f32,
    /// Scale y.
    pub scale_y: f32,
    /// Shear x, degrees.
    pub shear_x: f32,
    /// Shear y, degrees.
    pub shear_y: f32,
    /// What it takes from its parent.
    pub inherit: Inherit,
}

/// A slot: where an attachment is drawn, on a bone.
#[derive(Clone, Debug, PartialEq)]
pub struct SlotData {
    /// Name.
    pub name: String,
    /// Its bone.
    pub bone: usize,
    /// The attachment it shows in the setup pose.
    pub attachment: Option<String>,
    /// Tint, RGBA 0 to 1.
    pub color: [f32; 4],
}

/// A region attachment: an image on a slot's bone.
#[derive(Clone, Debug, PartialEq)]
pub struct Attachment {
    /// The image's sprite asset name.
    pub image: String,
    /// Centre relative to the bone.
    pub x: f32,
    /// Centre relative to the bone.
    pub y: f32,
    /// Degrees.
    pub rotation: f32,
    /// Scale x.
    pub scale_x: f32,
    /// Scale y.
    pub scale_y: f32,
    /// Width in skeleton units.
    pub width: f32,
    /// Height in skeleton units.
    pub height: f32,
    /// Tint.
    pub color: [f32; 4],
}

/// A skin: attachments by slot and name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Skin {
    /// `(slot, attachment name)` to the attachment.
    pub attachments: BTreeMap<(usize, String), Attachment>,
}

/// An event's defaults.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EventData {
    /// Integer.
    pub int: i64,
    /// Float.
    pub float: f32,
    /// String.
    pub string: String,
    /// An audio file's path, as Spine has it.
    pub audio: String,
}

/// One- or two-bone IK: bend `bones` so the last reaches `target`.
#[derive(Clone, Debug, PartialEq)]
pub struct IkData {
    /// Name.
    pub name: String,
    /// One bone, or a parent and its child.
    pub bones: Vec<usize>,
    /// The target bone.
    pub target: usize,
    /// How much, 0 to 1.
    pub mix: f32,
    /// Which way two bones bend.
    pub bend_positive: bool,
}

/// How a value goes from one key to the next.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    /// Straight.
    Linear,
    /// Holds until the next key.
    Stepped,
    /// A cubic bezier from (time, value) to the next key's, through two
    /// control points in the same coordinates: cx1, cy1, cx2, cy2.
    Bezier([f32; 4]),
}

/// A key of `N` values.
#[derive(Clone, Debug, PartialEq)]
pub struct Key<const N: usize> {
    /// Seconds.
    pub time: f32,
    /// The values.
    pub values: [f32; N],
    /// To the next key, per value.
    pub curves: [Curve; N],
}

/// What a bone timeline moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoneProp {
    /// Rotation, added to the setup's.
    Rotate,
    /// Position x and y, added to the setup's.
    Translate,
    /// Position x.
    TranslateX,
    /// Position y.
    TranslateY,
    /// Scale x and y, times the setup's.
    Scale,
    /// Scale x.
    ScaleX,
    /// Scale y.
    ScaleY,
    /// Shear x and y, added to the setup's.
    Shear,
    /// Shear x.
    ShearX,
    /// Shear y.
    ShearY,
}

/// A bone timeline. One- and two-value keys share a type: a one-value
/// timeline uses `values[0]`.
#[derive(Clone, Debug, PartialEq)]
pub struct BoneTimeline {
    /// The bone.
    pub bone: usize,
    /// What it moves.
    pub prop: BoneProp,
    /// Keys in time order.
    pub keys: Vec<Key<2>>,
}

/// A slot timeline.
#[derive(Clone, Debug, PartialEq)]
pub enum SlotTimeline {
    /// The attachment shown, from each key's time.
    Attachment {
        /// The slot.
        slot: usize,
        /// `(time, attachment)`.
        keys: Vec<(f32, Option<String>)>,
    },
    /// Its colour (RGBA, or RGB or alpha alone: the others are kept).
    Color {
        /// The slot.
        slot: usize,
        /// Which channels the keys hold: 0..4 for RGBA, 0..3 for RGB, 3..4
        /// for alpha.
        channels: (usize, usize),
        /// Keys.
        keys: Vec<Key<4>>,
    },
}

/// An event in an animation.
#[derive(Clone, Debug, PartialEq)]
pub struct EventKey {
    /// Seconds.
    pub time: f32,
    /// The event's name.
    pub name: String,
    /// Integer.
    pub int: i64,
    /// Float.
    pub float: f32,
    /// String.
    pub string: String,
}

/// An animation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Animation {
    /// Seconds: its last key.
    pub duration: f32,
    /// Bone timelines.
    pub bones: Vec<BoneTimeline>,
    /// Slot timelines.
    pub slots: Vec<SlotTimeline>,
    /// Events in time order.
    pub events: Vec<EventKey>,
    /// `(time, slots in draw order)`; `None` is the setup order.
    pub draw_order: Vec<(f32, Option<Vec<usize>>)>,
    /// IK mix and bend by constraint: `(ik, keys of [mix, bend])`.
    pub ik: Vec<(usize, Vec<Key<2>>)>,
}

/// A skeleton, as its JSON export has it.
#[derive(Clone, Debug, PartialEq)]
pub struct SkeletonAsset {
    /// The JSON, relative to `assets/`.
    pub source: String,
    /// The Spine version that wrote it.
    pub spine: String,
    /// The editor's frame rate (Spine's nonessential `fps`, 30 when it is
    /// not exported): the rate an animator plays its animations' frames at.
    pub fps: f32,
    /// Bones, parents first.
    pub bones: Vec<BoneData>,
    /// Slots, in setup draw order.
    pub slots: Vec<SlotData>,
    /// Skins by name; `default` holds what no skin overrides.
    pub skins: BTreeMap<String, Skin>,
    /// Events by name.
    pub events: BTreeMap<String, EventData>,
    /// IK constraints, in order.
    pub ik: Vec<IkData>,
    /// Animations by name.
    pub animations: BTreeMap<String, Animation>,
}

impl SkeletonAsset {
    /// A bone's index.
    pub fn bone(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|b| b.name == name)
    }

    /// A slot's index.
    pub fn slot(&self, name: &str) -> Option<usize> {
        self.slots.iter().position(|s| s.name == name)
    }

    /// The attachment a skin shows on a slot under a name, falling back
    /// to the default skin.
    pub fn attachment(&self, skin: &str, slot: usize, name: &str) -> Option<&Attachment> {
        // A slot's few attachments, without making a key (posing asks for
        // every slot every tick).
        fn find<'s>(s: &'s Skin, slot: usize, name: &str) -> Option<&'s Attachment> {
            s.attachments
                .range((slot, String::new())..(slot + 1, String::new()))
                .find(|((_, n), _)| n == name)
                .map(|(_, a)| a)
        }
        let own = self.skins.get(skin).and_then(|s| find(s, slot, name));
        own.or_else(|| find(self.skins.get("default")?, slot, name))
    }

    /// Every image the skeleton draws, sorted.
    pub fn images(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self
            .skins
            .values()
            .flat_map(|s| s.attachments.values().map(|a| a.image.as_str()))
            .collect();
        v.sort();
        v.dedup();
        v
    }
}

/// Whether a `.json` under `assets/` is a skeleton.
pub fn is_skeleton_file(rel: &str) -> bool {
    rel.to_ascii_lowercase().ends_with(".json")
}

fn line_of(text: &str, needle: &str) -> Option<usize> {
    text.find(needle).map(|i| text[..i].lines().count().max(1))
}

struct Reader<'a> {
    file: &'a str,
    text: &'a str,
    warnings: Vec<Warning>,
}

impl Reader<'_> {
    fn at(&self, needle: &str) -> String {
        match line_of(self.text, needle) {
            Some(l) => format!("{}:{l}", self.file),
            None => self.file.to_string(),
        }
    }

    fn bad(&self, needle: &str, message: String) -> ArgusError {
        ArgusError::new(codes::BAD_SIDECAR, message)
            .hint("Export the skeleton again from Spine 4 as JSON; argus-assets reads Spine's own format.")
            .at(Some(self.at(needle)))
    }

    fn warn(&mut self, needle: &str, message: String) {
        let at = self.at(needle);
        self.warnings.push(Warning { message, at });
    }
}

fn num(v: &Json, key: &str, or: f32) -> f32 {
    v.get(key).and_then(Json::as_f64).map_or(or, |f| f as f32)
}

fn text<'a>(v: &'a Json, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Json::as_str)
}

/// `rrggbbaa` (or `rrggbb`) as RGBA 0 to 1.
pub fn hex_color(s: &str) -> Option<[f32; 4]> {
    let b = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
    let a = if s.len() >= 8 { b(6)? } else { 255 };
    Some([
        b(0)? as f32 / 255.0,
        b(2)? as f32 / 255.0,
        b(4)? as f32 / 255.0,
        a as f32 / 255.0,
    ])
}

/// Read a skeleton. `file` is the JSON's path relative to its `assets/`
/// (to find its images), `shown` the path errors name, `text` its
/// contents.
pub fn parse(file: &str, shown: &str, body: &str) -> Result<(SkeletonAsset, Vec<Warning>)> {
    let shown = shown.to_string();
    let root: Json = serde_json::from_str(body).map_err(|e| {
        ArgusError::new(
            codes::BAD_SIDECAR,
            format!("the skeleton does not read: {e}"),
        )
        .at(Some(format!("{shown}:{}", e.line())))
    })?;
    let mut r = Reader {
        file: &shown,
        text: body,
        warnings: Vec::new(),
    };
    let Some(sk) = root.get("skeleton") else {
        return Err(r.bad(
            "{",
            "this .json is not a Spine skeleton: it has no \"skeleton\" section".into(),
        ));
    };
    let spine = text(sk, "spine").unwrap_or_default().to_string();
    let major = spine.split('.').next().and_then(|m| m.parse::<u32>().ok());
    if major.is_some_and(|m| m < 4) {
        return Err(r.bad(
            "\"spine\"",
            format!(
                "the skeleton was exported by Spine {spine}; the engine reads Spine 4's format"
            ),
        ));
    }
    let fps = num(sk, "fps", 30.0).max(1.0);
    // Where the images are: the export's `images` path, from the JSON's
    // folder.
    let dir = file.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let images = text(sk, "images").unwrap_or("./images/");
    let images = images.trim_start_matches("./").trim_end_matches('/');
    let prefix = [dir, images]
        .iter()
        .filter(|p| !p.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join("/");

    // Bones: a parent before its children.
    let mut bones: Vec<BoneData> = Vec::new();
    for b in root
        .get("bones")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        let name = text(b, "name").unwrap_or_default().to_string();
        let parent = match text(b, "parent") {
            None => None,
            Some(p) => match bones.iter().position(|x| x.name == p) {
                Some(i) => Some(i),
                None => {
                    return Err(r.bad(
                        &format!("\"{name}\""),
                        format!("bone {name}'s parent {p} is not a bone before it"),
                    ))
                }
            },
        };
        let inherit = match text(b, "inherit").or_else(|| text(b, "transform")) {
            None | Some("normal") => Inherit::Normal,
            Some("onlyTranslation") => Inherit::OnlyTranslation,
            Some("noRotationOrReflection") => Inherit::NoRotationOrReflection,
            Some("noScale") => Inherit::NoScale,
            Some("noScaleOrReflection") => Inherit::NoScaleOrReflection,
            Some(other) => {
                r.warn(
                    &format!("\"{other}\""),
                    format!("bone {name}: inherit {other} is not known; it inherits all"),
                );
                Inherit::Normal
            }
        };
        bones.push(BoneData {
            parent,
            length: num(b, "length", 0.0),
            x: num(b, "x", 0.0),
            y: num(b, "y", 0.0),
            rotation: num(b, "rotation", 0.0),
            scale_x: num(b, "scaleX", 1.0),
            scale_y: num(b, "scaleY", 1.0),
            shear_x: num(b, "shearX", 0.0),
            shear_y: num(b, "shearY", 0.0),
            inherit,
            name,
        });
    }
    if bones.is_empty() {
        return Err(r.bad("\"bones\"", "the skeleton has no bones".into()));
    }
    let bone_of = |r: &Reader, name: &str, who: &str| -> Result<usize> {
        bones.iter().position(|b| b.name == name).ok_or_else(|| {
            r.bad(
                &format!("\"{name}\""),
                format!("{who} names the bone {name}, which the skeleton does not have"),
            )
        })
    };

    let mut slots: Vec<SlotData> = Vec::new();
    for s in root
        .get("slots")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        let name = text(s, "name").unwrap_or_default().to_string();
        let bone = bone_of(
            &r,
            text(s, "bone").unwrap_or_default(),
            &format!("slot {name}"),
        )?;
        if let Some(b) = text(s, "blend").filter(|b| *b != "normal") {
            r.warn(
                &format!("\"{b}\""),
                format!("slot {name}: blend {b} is drawn as normal"),
            );
        }
        slots.push(SlotData {
            bone,
            attachment: text(s, "attachment").map(str::to_string),
            color: text(s, "color").and_then(hex_color).unwrap_or([1.0; 4]),
            name,
        });
    }
    let slot_of = |r: &Reader, name: &str, who: &str| -> Result<usize> {
        slots.iter().position(|s| s.name == name).ok_or_else(|| {
            r.bad(
                &format!("\"{name}\""),
                format!("{who} names the slot {name}, which the skeleton does not have"),
            )
        })
    };

    // Skins: Spine 4 writes an array; each holds attachments by slot.
    let mut skins: BTreeMap<String, Skin> = BTreeMap::new();
    for sk in root
        .get("skins")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        let sname = text(sk, "name").unwrap_or("default").to_string();
        let mut skin = Skin::default();
        for (slot_name, atts) in sk
            .get("attachments")
            .and_then(Json::as_object)
            .into_iter()
            .flatten()
        {
            let slot = slot_of(&r, slot_name, &format!("skin {sname}"))?;
            for (aname, a) in atts.as_object().into_iter().flatten() {
                let kind = text(a, "type").unwrap_or("region");
                if kind != "region" {
                    r.warn(
                        &format!("\"{aname}\""),
                        format!("{sname}/{slot_name}/{aname}: a {kind} attachment is not drawn (region attachments are)"),
                    );
                    continue;
                }
                let path = text(a, "path").unwrap_or(aname);
                let image = if prefix.is_empty() {
                    path.to_string()
                } else {
                    format!("{prefix}/{path}")
                };
                skin.attachments.insert(
                    (slot, aname.clone()),
                    Attachment {
                        image,
                        x: num(a, "x", 0.0),
                        y: num(a, "y", 0.0),
                        rotation: num(a, "rotation", 0.0),
                        scale_x: num(a, "scaleX", 1.0),
                        scale_y: num(a, "scaleY", 1.0),
                        width: num(a, "width", 32.0),
                        height: num(a, "height", 32.0),
                        color: text(a, "color").and_then(hex_color).unwrap_or([1.0; 4]),
                    },
                );
            }
        }
        skins.insert(sname, skin);
    }

    let mut events: BTreeMap<String, EventData> = BTreeMap::new();
    for (name, e) in root
        .get("events")
        .and_then(Json::as_object)
        .into_iter()
        .flatten()
    {
        events.insert(
            name.clone(),
            EventData {
                int: e.get("int").and_then(Json::as_i64).unwrap_or(0),
                float: num(e, "float", 0.0),
                string: text(e, "string").unwrap_or_default().to_string(),
                audio: text(e, "audio").unwrap_or_default().to_string(),
            },
        );
    }

    let mut ik: Vec<IkData> = Vec::new();
    for c in root
        .get("ik")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        let name = text(c, "name").unwrap_or_default().to_string();
        let mut bs = Vec::new();
        for b in c
            .get("bones")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
        {
            bs.push(bone_of(
                &r,
                b.as_str().unwrap_or_default(),
                &format!("IK {name}"),
            )?);
        }
        if bs.is_empty() || bs.len() > 2 {
            return Err(r.bad(
                &format!("\"{name}\""),
                format!("IK {name} has {} bones; IK bends one or two", bs.len()),
            ));
        }
        let target = bone_of(
            &r,
            text(c, "target").unwrap_or_default(),
            &format!("IK {name}"),
        )?;
        for opt in ["softness", "compress", "stretch"] {
            if c.get(opt)
                .is_some_and(|v| v.as_bool() == Some(true) || v.as_f64().is_some_and(|f| f != 0.0))
            {
                r.warn(
                    &format!("\"{name}\""),
                    format!("IK {name}: {opt} is not applied"),
                );
            }
        }
        ik.push(IkData {
            bones: bs,
            target,
            mix: num(c, "mix", 1.0),
            bend_positive: c
                .get("bendPositive")
                .and_then(Json::as_bool)
                .unwrap_or(true),
            name,
        });
    }
    for (kind, label) in [
        ("transform", "transform constraints"),
        ("path", "path constraints"),
        ("physics", "physics constraints"),
    ] {
        if root
            .get(kind)
            .and_then(Json::as_array)
            .is_some_and(|a| !a.is_empty())
        {
            r.warn(&format!("\"{kind}\""), format!("{label} are not applied"));
        }
    }

    let mut animations: BTreeMap<String, Animation> = BTreeMap::new();
    let anims = root.get("animations").and_then(Json::as_object);
    for (aname, a) in anims.into_iter().flatten() {
        let mut anim = Animation::default();
        let mut last = 0.0f32;
        for (bname, tls) in a
            .get("bones")
            .and_then(Json::as_object)
            .into_iter()
            .flatten()
        {
            let bone = bone_of(&r, bname, &format!("animation {aname}"))?;
            for (tname, keys) in tls.as_object().into_iter().flatten() {
                let (prop, n, default) = match tname.as_str() {
                    "rotate" => (BoneProp::Rotate, 1, 0.0),
                    "translate" => (BoneProp::Translate, 2, 0.0),
                    "translatex" => (BoneProp::TranslateX, 1, 0.0),
                    "translatey" => (BoneProp::TranslateY, 1, 0.0),
                    "scale" => (BoneProp::Scale, 2, 1.0),
                    "scalex" => (BoneProp::ScaleX, 1, 1.0),
                    "scaley" => (BoneProp::ScaleY, 1, 1.0),
                    "shear" => (BoneProp::Shear, 2, 0.0),
                    "shearx" => (BoneProp::ShearX, 1, 0.0),
                    "sheary" => (BoneProp::ShearY, 1, 0.0),
                    other => {
                        r.warn(
                            &format!("\"{other}\""),
                            format!("animation {aname}, bone {bname}: the {other} timeline is not applied"),
                        );
                        continue;
                    }
                };
                let names: [&str; 2] = if n == 1 { ["value", ""] } else { ["x", "y"] };
                let ks = keys_of(&mut r, aname, keys, &names, default)?;
                last = last.max(ks.last().map_or(0.0, |k| k.time));
                anim.bones.push(BoneTimeline {
                    bone,
                    prop,
                    keys: ks,
                });
            }
        }
        for (sname, tls) in a
            .get("slots")
            .and_then(Json::as_object)
            .into_iter()
            .flatten()
        {
            let slot = slot_of(&r, sname, &format!("animation {aname}"))?;
            for (tname, keys) in tls.as_object().into_iter().flatten() {
                let arr = keys.as_array().cloned().unwrap_or_default();
                match tname.as_str() {
                    "attachment" => {
                        let ks: Vec<(f32, Option<String>)> = arr
                            .iter()
                            .map(|k| (num(k, "time", 0.0), text(k, "name").map(str::to_string)))
                            .collect();
                        last = last.max(ks.last().map_or(0.0, |k| k.0));
                        anim.slots.push(SlotTimeline::Attachment { slot, keys: ks });
                    }
                    "rgba" | "rgb" | "alpha" => {
                        let channels = match tname.as_str() {
                            "rgba" => (0, 4),
                            "rgb" => (0, 3),
                            _ => (3, 4),
                        };
                        let mut ks = Vec::new();
                        for k in &arr {
                            let values = if tname == "alpha" {
                                [0.0, 0.0, 0.0, num(k, "value", 1.0)]
                            } else {
                                text(k, "color").and_then(hex_color).unwrap_or([1.0; 4])
                            };
                            let width = channels.1 - channels.0;
                            let curves = curves_of(k, width);
                            let mut c4 = [Curve::Linear; 4];
                            c4[channels.0..channels.1].copy_from_slice(&curves[..width]);
                            ks.push(Key {
                                time: num(k, "time", 0.0),
                                values,
                                curves: c4,
                            });
                        }
                        last = last.max(ks.last().map_or(0.0, |k| k.time));
                        anim.slots.push(SlotTimeline::Color {
                            slot,
                            channels,
                            keys: ks,
                        });
                    }
                    other => r.warn(
                        &format!("\"{other}\""),
                        format!(
                            "animation {aname}, slot {sname}: the {other} timeline is not applied"
                        ),
                    ),
                }
            }
        }
        for e in a
            .get("events")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
        {
            let name = text(e, "name").unwrap_or_default().to_string();
            let Some(d) = events.get(&name) else {
                return Err(r.bad(
                    &format!("\"{name}\""),
                    format!("animation {aname} fires the event {name}, which the skeleton does not declare"),
                ));
            };
            let time = num(e, "time", 0.0);
            last = last.max(time);
            anim.events.push(EventKey {
                time,
                int: e.get("int").and_then(Json::as_i64).unwrap_or(d.int),
                float: num(e, "float", d.float),
                string: text(e, "string").unwrap_or(&d.string).to_string(),
                name,
            });
        }
        anim.events.sort_by(|a, b| a.time.total_cmp(&b.time));
        for k in a
            .get("drawOrder")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
        {
            let time = num(k, "time", 0.0);
            last = last.max(time);
            let order = match k.get("offsets").and_then(Json::as_array) {
                None => None,
                Some(offs) => {
                    let mut moved = Vec::new();
                    for o in offs {
                        let s = slot_of(
                            &r,
                            text(o, "slot").unwrap_or_default(),
                            &format!("animation {aname}"),
                        )?;
                        moved.push((s, o.get("offset").and_then(Json::as_i64).unwrap_or(0)));
                    }
                    Some(draw_order(slots.len(), &moved))
                }
            };
            anim.draw_order.push((time, order));
        }
        for (cname, keys) in a.get("ik").and_then(Json::as_object).into_iter().flatten() {
            let Some(ci) = ik.iter().position(|c| &c.name == cname) else {
                return Err(r.bad(
                    &format!("\"{cname}\""),
                    format!(
                        "animation {aname} keys the IK {cname}, which the skeleton does not have"
                    ),
                ));
            };
            let mut ks = Vec::new();
            for k in keys.as_array().into_iter().flatten() {
                // Spine reads a key's bend as positive unless it says not.
                let bend = k
                    .get("bendPositive")
                    .and_then(Json::as_bool)
                    .unwrap_or(true);
                let c = curves_of(k, 1);
                ks.push(Key {
                    time: num(k, "time", 0.0),
                    values: [num(k, "mix", 1.0), if bend { 1.0 } else { -1.0 }],
                    curves: [c[0], Curve::Stepped],
                });
            }
            last = last.max(ks.last().map_or(0.0, |k: &Key<2>| k.time));
            anim.ik.push((ci, ks));
        }
        for unread in ["deform", "attachments", "transform", "path", "physics"] {
            if a.get(unread).is_some() {
                r.warn(
                    &format!("\"{unread}\""),
                    format!("animation {aname}: its {unread} timelines are not applied"),
                );
            }
        }
        anim.duration = last;
        animations.insert(aname.clone(), anim);
    }
    let warnings = std::mem::take(&mut r.warnings);
    Ok((
        SkeletonAsset {
            source: file.to_string(),
            spine,
            fps,
            bones,
            slots,
            skins,
            events,
            ik,
            animations,
        },
        warnings,
    ))
}

/// Keys of up to two values named `names`, each `default` when absent.
fn keys_of(
    r: &mut Reader,
    anim: &str,
    keys: &Json,
    names: &[&str; 2],
    default: f32,
) -> Result<Vec<Key<2>>> {
    let Some(arr) = keys.as_array() else {
        return Err(r.bad(
            &format!("\"{anim}\""),
            format!("animation {anim}: a timeline is not a list of keys"),
        ));
    };
    let width = if names[1].is_empty() { 1 } else { 2 };
    let mut out: Vec<Key<2>> = Vec::with_capacity(arr.len());
    for k in arr {
        let time = num(k, "time", 0.0);
        if out.last().is_some_and(|p| time < p.time) {
            return Err(r.bad(
                &format!("\"{anim}\""),
                format!("animation {anim}: keys go back in time at {time}"),
            ));
        }
        let c = curves_of(k, width);
        out.push(Key {
            time,
            values: [
                num(k, names[0], default),
                if width == 2 {
                    num(k, names[1], default)
                } else {
                    0.0
                },
            ],
            curves: [c[0], c[1]],
        });
    }
    Ok(out)
}

/// A key's curves to the next, one per value: `"stepped"`, a list of four
/// numbers per value (a bezier), or nothing (linear).
fn curves_of(k: &Json, width: usize) -> [Curve; 4] {
    let mut out = [Curve::Linear; 4];
    match k.get("curve") {
        Some(Json::String(s)) if s == "stepped" => {
            out = [Curve::Stepped; 4];
        }
        Some(Json::Array(a)) => {
            for (v, slot) in out.iter_mut().enumerate().take(width) {
                let at = |i: usize| a.get(v * 4 + i).and_then(Json::as_f64).map(|f| f as f32);
                if let (Some(a0), Some(a1), Some(a2), Some(a3)) = (at(0), at(1), at(2), at(3)) {
                    *slot = Curve::Bezier([a0, a1, a2, a3]);
                }
            }
        }
        _ => {}
    }
    out
}

/// Spine's draw-order offsets: each moved slot `offset` places from where
/// it is in the setup order; the rest keep their order around them.
fn draw_order(n: usize, moved: &[(usize, i64)]) -> Vec<usize> {
    let mut order: Vec<Option<usize>> = vec![None; n];
    let mut unchanged: Vec<usize> = Vec::new();
    let mut original = 0usize;
    for &(slot, offset) in moved {
        while original < slot {
            unchanged.push(original);
            original += 1;
        }
        let at = (original as i64 + offset).clamp(0, n as i64 - 1) as usize;
        order[at] = Some(slot);
        original += 1;
    }
    while original < n {
        unchanged.push(original);
        original += 1;
    }
    let mut rest = unchanged.into_iter().rev();
    for i in (0..n).rev() {
        if order[i].is_none() {
            order[i] = rest.next();
        }
    }
    order.into_iter().map(|o| o.unwrap_or(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draw_order_offsets_move_the_named_slots_and_keep_the_rest_in_order() {
        // Slot 1 moves two places forward: 0, 2, 3, 1, 4.
        assert_eq!(draw_order(5, &[(1, 2)]), vec![0, 2, 3, 1, 4]);
        // Slot 3 moves to the back.
        assert_eq!(draw_order(4, &[(3, -3)]), vec![3, 0, 1, 2]);
    }

    #[test]
    fn colors_read_with_and_without_alpha() {
        assert_eq!(hex_color("ff000080"), Some([1.0, 0.0, 0.0, 128.0 / 255.0]));
        assert_eq!(hex_color("00ff00"), Some([0.0, 1.0, 0.0, 1.0]));
    }
}
