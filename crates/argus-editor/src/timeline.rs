//! The timeline editor (§13 "Tweens and sequencing"; phase 4 item 9): a
//! timeline is "a def table of tracks (tweens, animator states, camera
//! cuts, dialogue lines, events) against time, playable from Luau,
//! scrubbable in the editor". Its tracks are lanes under a time ruler, its
//! cues bars from when they fire to when they end. A bar dragged moves its
//! cue, its right edge its duration; the selected cue's fields are edited
//! below the lanes; cues and timelines are added and taken out. Every
//! change is the person's transaction over the `timeline` def table
//! (`edit.def` for a field, `edit.def_row` for a whole row), the file
//! written with its comments kept and read again, so the next play uses
//! it; a row the timelines cannot read is refused with why, and Ctrl+Z
//! puts the file back. Play plays it now (`timeline.play`, with an actor
//! and an other); the ruler scrubs the latest play (`timeline.seek`), one
//! seek in flight and the latest next.

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use serde_json::{json, Value as Json};
use std::collections::HashMap;

/// What the tab asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum TlDo {
    /// A transaction: its note and ops.
    Edit(String, Vec<Json>),
    /// `timeline.play` with these params.
    Play(Json),
    /// Pause the game: a drag along the ruler began while it played.
    Pause,
    /// Scrub the latest play to this time.
    Seek(f64),
    /// The ruler let go at this time; play on if the game played.
    Let(f64, bool),
}

/// One cue, from `timeline.list`'s rows.
#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    /// Its track (the lane).
    pub track: String,
    /// Its row's key (`outro.call`).
    pub key: String,
    pub at: f64,
    pub end: f64,
    pub kind: String,
    pub who: String,
    pub what: String,
    pub wait: bool,
    /// Its row's file:line.
    pub at_ref: String,
    /// Its row as the file writes it.
    pub fields: Json,
}

/// A new cue: its name, kind, track, and the one field its kind needs.
#[derive(Clone, Debug, Default)]
pub struct NewCue {
    pub name: String,
    pub kind: String,
    pub track: String,
    pub need: String,
}

/// A bar being dragged: by its body (its time) or its right edge (its
/// duration), from where it was, by how far.
#[derive(Clone, Debug)]
struct Drag {
    cue: String,
    end: bool,
    at: f64,
    duration: f64,
    dt: f64,
}

/// The tab's state.
#[derive(Default)]
pub struct Timelines {
    /// `timeline.list {}`.
    pub list: Json,
    /// Whether it has answered (what shows does not depend on timing).
    pub loaded: bool,
    /// The timeline shown.
    pub chosen: Option<String>,
    /// The cue selected: its row's key.
    pub cue: Option<String>,
    /// Who Play plays it with: entities, as typed.
    pub actor: String,
    pub other: String,
    /// A new timeline's name.
    pub new_line: String,
    /// A new cue.
    pub new_cue: NewCue,
    /// The time scrubbed to, while a seek is on its way.
    pub at: Option<f64>,
    /// Why the last seek stopped short of where it was asked to go.
    pub note: Option<String>,
    drag: Option<Drag>,
    /// The ruler held: whether the game played when it was pressed.
    ruler: Option<bool>,
    in_flight: bool,
    pending: Option<f64>,
    last_sent: Option<f64>,
    /// Values being edited, by field, until let go.
    nums: HashMap<String, f64>,
    texts: HashMap<String, String>,
}

/// A track's lane as laid out: its name, its cues each with the row of
/// the lane it is drawn in, and how many rows.
type Lane<'a> = (&'a String, Vec<(&'a Cue, usize)>, usize);

/// The kinds of cue, when the daemon has not said.
const KINDS: [&str; 5] = ["tween", "anim", "camera", "line", "event"];

/// An `edit.def` op: one field of a row.
pub fn op(def: &str, path: &str, value: Json) -> Json {
    json!({ "op": "def", "def": def, "path": path, "value": value })
}

/// An `edit.def_row` op: a whole row, or with `None` its removal.
pub fn row_op(def: &str, fields: Option<Json>) -> Json {
    match fields {
        Some(f) => json!({ "op": "def_row", "def": def, "fields": f }),
        None => json!({ "op": "def_row", "def": def, "remove": true }),
    }
}

/// A time on the ruler, to a twentieth of a second.
pub fn snap(t: f64) -> f64 {
    ((t.max(0.0)) * 20.0).round() / 20.0
}

/// A number from the table (an f32) as it reads: 1.2, not 1.2000000476.
fn short(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// Where a cue's bar ends: where it ends, or for a cut with no duration,
/// the timeline's end (it draws until then).
fn bar_end(c: &Cue, length: f64) -> f64 {
    let timed = c
        .fields
        .get("duration")
        .and_then(Json::as_f64)
        .unwrap_or(0.0)
        > 0.0;
    if c.kind == "camera" && !timed {
        c.end.max(length)
    } else {
        c.end
    }
}

/// The field a kind of cue needs to read, and its hint.
pub fn need_of(kind: &str) -> (&'static str, &'static str) {
    match kind {
        "tween" => ("path", "Component.field"),
        "anim" => ("state", "an animator state"),
        "line" => ("line", "dialogue:node"),
        "camera" => ("who", "@camera"),
        _ => ("effects", "path = value; emit name"),
    }
}

impl Timelines {
    /// `timeline.list {}`'s answer.
    pub fn take(&mut self, v: Json) {
        self.list = v;
        self.loaded = true;
        let names = self.names();
        if self.chosen.as_ref().is_none_or(|c| !names.contains(c)) {
            self.chosen = names.first().cloned();
        }
        if let Some(c) = self.cue.clone() {
            if !self.cues().iter().any(|x| x.key == c) {
                self.cue = None;
            }
        }
    }

    /// The timelines there are.
    pub fn names(&self) -> Vec<String> {
        self.list["timelines"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t["name"].as_str().map(str::to_string))
            .collect()
    }

    fn line(&self) -> Option<&Json> {
        let c = self.chosen.as_deref()?;
        self.list["timelines"]
            .as_array()?
            .iter()
            .find(|t| t["name"] == c)
    }

    /// The chosen timeline's tracks, in order.
    pub fn tracks(&self) -> Vec<String> {
        self.line()
            .and_then(|l| l["tracks"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|t| t["track"].as_str().map(str::to_string))
            .collect()
    }

    /// The chosen timeline's cues, track by track, each in time order.
    pub fn cues(&self) -> Vec<Cue> {
        let mut out = Vec::new();
        for t in self
            .line()
            .and_then(|l| l["tracks"].as_array())
            .into_iter()
            .flatten()
        {
            let track = t["track"].as_str().unwrap_or("").to_string();
            for c in t["cues"].as_array().into_iter().flatten() {
                let s = |i: usize| c[i].as_str().unwrap_or("").to_string();
                out.push(Cue {
                    track: track.clone(),
                    key: s(1),
                    at: short(c[2].as_f64().unwrap_or(0.0)),
                    end: short(c[3].as_f64().unwrap_or(0.0)),
                    kind: s(4),
                    who: s(5),
                    what: s(6),
                    wait: c[7] == true,
                    at_ref: s(8),
                    fields: c[9].clone(),
                });
            }
        }
        out
    }

    /// The chosen timeline's length: its own, or where its last cue ends.
    pub fn length(&self) -> f64 {
        let own = self
            .line()
            .and_then(|l| l["length"].as_f64())
            .unwrap_or(0.0);
        let last = self.cues().iter().map(|c| c.end).fold(0.0, f64::max);
        if own > 0.0 {
            short(own)
        } else {
            last
        }
    }

    /// The latest play of the chosen timeline, while it plays.
    pub fn playing(&self) -> Option<&Json> {
        let c = self.chosen.as_deref()?;
        self.list["playing"]
            .as_array()?
            .iter()
            .filter(|p| p["name"] == c)
            .max_by_key(|p| p["began"].as_u64().unwrap_or(0))
    }

    /// Where the playhead is: the time scrubbed to while a seek is on its
    /// way, else the latest play's.
    pub fn head(&self) -> Option<f64> {
        if self.ruler.is_some() || self.in_flight || self.pending.is_some() {
            return self.at;
        }
        self.playing()
            .and_then(|p| p["time"].as_f64())
            .map(short)
            .or(self.at)
    }

    /// A seek to send now, or `None` while one is on its way (the latest
    /// goes next).
    pub fn want_seek(&mut self, t: f64) -> Option<f64> {
        if self.last_sent == Some(t) && self.pending.is_none() && !self.in_flight {
            return None;
        }
        if self.in_flight {
            self.pending = Some(t);
            return None;
        }
        self.in_flight = true;
        self.last_sent = Some(t);
        Some(t)
    }

    /// A seek answered: the one to send next, if the drag moved on.
    pub fn seek_answered(&mut self, v: Option<&Json>) -> Option<f64> {
        self.in_flight = false;
        if let Some(v) = v {
            let t = v["time"].as_f64().map(short);
            if self.pending.is_none() && self.ruler.is_none() {
                self.at = t.or(self.at);
            }
            // Past what was played, or ended before it got there.
            self.note = if v["ended"] == true {
                Some("It had ended by then: the frame it ended.".into())
            } else if v["reached"] == false {
                Some(format!(
                    "Played only to {} s: a scrub goes over what was played.",
                    t.unwrap_or(0.0)
                ))
            } else {
                None
            };
        }
        let t = self.pending.take()?;
        self.want_seek(t)
    }

    /// `timeline.seek`'s params for a time.
    pub fn seek_params(&self, t: f64) -> Json {
        json!({ "name": self.chosen.clone().unwrap_or_default(), "time": short(t) })
    }

    /// The row as the file writes it, with `timeline` (a cue's own).
    fn row_fields(c: &Cue) -> serde_json::Map<String, Json> {
        c.fields.as_object().cloned().unwrap_or_default()
    }

    /// A number field, a change when let go.
    #[allow(clippy::too_many_arguments)]
    fn num(
        &mut self,
        ui: &mut egui::Ui,
        def: &str,
        path: &str,
        now: f64,
        label: &str,
        hint: &str,
        out: &mut Vec<TlDo>,
    ) {
        let named = ui.label(label).id;
        let id = format!("{def}.{path}");
        let mut v = *self.nums.get(&id).unwrap_or(&now);
        let r = ui
            .add(
                egui::DragValue::new(&mut v)
                    .speed(0.01)
                    .range(0.0..=3600.0)
                    .max_decimals(3),
            )
            .labelled_by(named);
        if r.changed() {
            self.nums.insert(id.clone(), v);
        }
        if r.drag_stopped() || r.lost_focus() {
            self.nums.remove(&id);
            let v = short(v);
            if (v - now).abs() > 1e-9 {
                out.push(TlDo::Edit(
                    format!("{def}: {path} {now} to {v}"),
                    vec![op(def, path, json!(v))],
                ));
            }
        }
        if !hint.is_empty() {
            r.on_hover_text(hint);
        }
        ui.end_row();
    }

    /// A text field, a change on Enter or when it loses focus.
    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        ui: &mut egui::Ui,
        def: &str,
        path: &str,
        now: &str,
        label: &str,
        hint: &str,
        many: bool,
        out: &mut Vec<TlDo>,
    ) {
        let named = ui.label(label).id;
        let id = format!("{def}.{path}");
        let t = self
            .texts
            .entry(id.clone())
            .or_insert_with(|| now.to_string());
        let edit = if many {
            egui::TextEdit::multiline(t).desired_rows(2)
        } else {
            egui::TextEdit::singleline(t)
        };
        let r = ui
            .add(edit.desired_width(220.0).hint_text(hint))
            .labelled_by(named);
        let t = t.clone();
        if r.lost_focus() {
            self.texts.remove(&id);
            if t != now {
                out.push(TlDo::Edit(
                    format!("{def}: {path} = {t:?}"),
                    vec![op(def, path, json!(t))],
                ));
            }
        } else if !r.has_focus() && t != now {
            // Changed by someone else while not being typed in.
            self.texts.insert(id, now.to_string());
        }
        ui.end_row();
    }

    /// A vec2 field (a tween's from or to), a change when let go.
    fn vec2(
        &mut self,
        ui: &mut egui::Ui,
        def: &str,
        path: &str,
        now: [f64; 2],
        label: &str,
        out: &mut Vec<TlDo>,
    ) {
        let named = ui.label(label).id;
        let mut done = false;
        let mut v = now;
        ui.horizontal(|ui| {
            for (i, axis) in ["x", "y"].iter().enumerate() {
                let id = format!("{def}.{path}.{axis}");
                let mut n = *self.nums.get(&id).unwrap_or(&now[i]);
                let r = ui
                    .add(egui::DragValue::new(&mut n).speed(0.05).max_decimals(3))
                    .labelled_by(named);
                if r.changed() {
                    self.nums.insert(id.clone(), n);
                }
                if r.drag_stopped() || r.lost_focus() {
                    self.nums.remove(&id);
                    done = true;
                }
                v[i] = short(n);
            }
        });
        if done && (v[0] - now[0]).abs() + (v[1] - now[1]).abs() > 1e-9 {
            out.push(TlDo::Edit(
                format!("{def}: {path} {now:?} to {v:?}"),
                vec![op(def, path, json!(v))],
            ));
        }
        ui.end_row();
    }

    /// A choice among names (a kind, an easing), a change when picked.
    #[allow(clippy::too_many_arguments)]
    fn choice(
        &mut self,
        ui: &mut egui::Ui,
        def: &str,
        path: &str,
        now: &str,
        shown: &str,
        names: &[String],
        label: &str,
        out: &mut Vec<TlDo>,
    ) {
        let named = ui.label(label);
        let mut picked = None;
        egui::ComboBox::from_id_salt(format!("tl-{def}-{path}"))
            .selected_text(shown)
            .show_ui(ui, |ui| {
                for n in names {
                    if ui.selectable_label(n == now, n).clicked() && n != now {
                        picked = Some(n.clone());
                    }
                }
            })
            .response
            .labelled_by(named.id);
        if let Some(n) = picked {
            out.push(TlDo::Edit(
                format!("{def}: {path} = {n}"),
                vec![op(def, path, json!(n))],
            ));
        }
        ui.end_row();
    }

    fn names_of(&self, key: &str, fallback: &[&str]) -> Vec<String> {
        match self.list[key].as_array() {
            Some(a) if !a.is_empty() => a
                .iter()
                .filter_map(|n| n.as_str().map(str::to_string))
                .collect(),
            _ => fallback.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Draw it; what the person asked for. `selected` is the entity
    /// selected in the hierarchy or the game; `playing`, whether the game
    /// plays.
    pub fn ui(&mut self, ui: &mut egui::Ui, selected: Option<&str>, playing: bool) -> Vec<TlDo> {
        let mut out = Vec::new();
        if !self.loaded {
            ui.weak("Reading the timelines.");
            return out;
        }
        let names = self.names();
        ui.horizontal(|ui| {
            egui::ComboBox::new("tl-chosen", "Timeline")
                .selected_text(self.chosen.clone().unwrap_or_else(|| "none yet".into()))
                .show_ui(ui, |ui| {
                    for n in &names {
                        if ui
                            .selectable_label(self.chosen.as_ref() == Some(n), n)
                            .clicked()
                        {
                            self.chosen = Some(n.clone());
                            self.cue = None;
                            self.at = None;
                        }
                    }
                });
        });
        // A new timeline: a row with a length.
        ui.horizontal(|ui| {
            let named = ui.label("New timeline").id;
            ui.add(
                egui::TextEdit::singleline(&mut self.new_line)
                    .desired_width(140.0)
                    .hint_text("boss.intro"),
            )
            .labelled_by(named);
            let name = self.new_line.trim().to_string();
            let fine = !name.is_empty() && !names.contains(&name) && !name.contains(':');
            if ui
                .add_enabled(fine, egui::Button::new("Add timeline"))
                .clicked()
            {
                out.push(TlDo::Edit(
                    format!("new timeline {name}"),
                    vec![row_op(
                        &format!("timeline:{name}"),
                        Some(json!({ "length": 2.0 })),
                    )],
                ));
                self.chosen = Some(name);
                self.cue = None;
                self.new_line.clear();
            }
        });
        let Some(name) = self.chosen.clone() else {
            ui.weak("No timelines yet: add one.");
            return out;
        };
        let def = format!("timeline:{name}");
        let cues = self.cues();
        let length = self.length();
        let at_ref = self.line().and_then(|l| l["ref"].as_str()).unwrap_or("");
        ui.horizontal(|ui| {
            ui.weak(at_ref.to_string());
            if ui
                .button("Remove timeline")
                .on_hover_text("Takes out the timeline and its cues: one change, undone by Ctrl+Z")
                .clicked()
            {
                // Its cues first: a cue left on no timeline would be refused.
                let mut ops: Vec<Json> = cues
                    .iter()
                    .map(|c| row_op(&format!("timeline:{}", c.key), None))
                    .collect();
                ops.push(row_op(&def, None));
                out.push(TlDo::Edit(
                    format!("take out timeline {name} and its {} cues", cues.len()),
                    ops,
                ));
            }
        });
        egui::Grid::new("tl-line").num_columns(2).show(ui, |ui| {
            let own = self
                .line()
                .and_then(|l| l["fields"]["length"].as_f64())
                .map(short)
                .unwrap_or(0.0);
            self.num(
                ui,
                &def,
                "length",
                own,
                "length",
                "Seconds; 0: when its last cue ends",
                &mut out,
            );
        });

        // Play, and where the latest play is.
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            for (label, who) in [("Actor", 0), ("Other", 1)] {
                let named = ui.label(label).id;
                let field = if who == 0 {
                    &mut self.actor
                } else {
                    &mut self.other
                };
                ui.add(
                    egui::TextEdit::singleline(&mut *field)
                        .desired_width(90.0)
                        .hint_text("@id"),
                )
                .labelled_by(named);
                if let Some(s) = selected {
                    if ui
                        .small_button("selected")
                        .on_hover_text(format!("{label}: {s}"))
                        .clicked()
                    {
                        *field = s.to_string();
                    }
                }
            }
            if ui
                .button("Play timeline")
                .on_hover_text("Play it now, as a script does")
                .clicked()
            {
                let mut p = json!({ "name": name });
                if !self.actor.trim().is_empty() {
                    p["actor"] = json!(self.actor.trim());
                }
                if !self.other.trim().is_empty() {
                    p["other"] = json!(self.other.trim());
                }
                out.push(TlDo::Play(p));
            }
        });
        match self.playing() {
            Some(p) => {
                let mut s = format!(
                    "{} plays: {:.2} s of {} s",
                    p["timeline"].as_str().unwrap_or("?"),
                    p["time"].as_f64().unwrap_or(0.0),
                    short(p["length"].as_f64().unwrap_or(length))
                );
                if p["held"] == true {
                    s.push_str(", held");
                }
                if let Some(w) = p["waiting"].as_str() {
                    s.push_str(&format!(", waiting for {w}"));
                }
                ui.label(RichText::new(s).strong());
            }
            None => {
                ui.weak(
                    "Not playing. Play it, then drag along the ruler to scrub its latest play.",
                );
            }
        }

        if let Some(n) = &self.note {
            ui.weak(n);
        }
        self.lanes(ui, &cues, length, playing, &mut out);
        self.cue_ui(ui, &cues, &mut out);
        self.new_cue_ui(ui, &name, &cues, &mut out);

        let problems = self.list["problems"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if !problems.is_empty() {
            ui.separator();
            ui.strong("Rows that do not read");
            for p in problems {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!(
                        "{} ({})",
                        p["message"].as_str().unwrap_or(""),
                        p["ref"].as_str().unwrap_or("")
                    ),
                );
            }
        }
        out
    }

    /// The ruler and the lanes, with the playhead.
    fn lanes(
        &mut self,
        ui: &mut egui::Ui,
        cues: &[Cue],
        length: f64,
        playing: bool,
        out: &mut Vec<TlDo>,
    ) {
        let tracks = self.tracks();
        let span = length
            .max(cues.iter().map(|c| c.end).fold(0.0, f64::max))
            .max(0.5);
        let (gutter, ruler_h, lane_h) = (64.0_f32, 18.0_f32, 22.0_f32);
        let width = ui.available_width().max(200.0);
        // Cues that overlap on a track stack in rows of its lane, each a
        // bar of its own to click and drag.
        let least = 10.0 / ((width - gutter - 8.0) as f64 / span).max(1.0);
        let layout: Vec<Lane> = tracks
            .iter()
            .map(|track| {
                let mut ends: Vec<f64> = Vec::new();
                let mut placed = Vec::new();
                for c in cues.iter().filter(|c| &c.track == track) {
                    let end = bar_end(c, length).max(c.at + least);
                    let row = match ends.iter().position(|e| *e <= c.at + 1e-9) {
                        Some(r) => r,
                        None => {
                            ends.push(0.0);
                            ends.len() - 1
                        }
                    };
                    ends[row] = end;
                    placed.push((c, row));
                }
                (track, placed, ends.len().max(1))
            })
            .collect();
        let rows: usize = layout.iter().map(|l| l.2).sum();
        let height = ruler_h + lane_h * rows.max(1) as f32;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
        let (x0, x1) = (rect.left() + gutter, rect.right() - 8.0);
        let pps = ((x1 - x0) as f64 / span).max(1.0);
        let x_of = |t: f64| x0 + (t * pps) as f32;
        let t_of = |x: f32| ((x - x0) as f64 / pps).clamp(0.0, span);
        let p = ui.painter_at(rect);
        let text = ui.visuals().text_color();
        let weak = ui.visuals().weak_text_color();
        p.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);

        // The ruler: a tick a step that leaves room for its label.
        let ruler = Rect::from_min_max(
            Pos2::new(x0, rect.top()),
            Pos2::new(x1, rect.top() + ruler_h),
        );
        let step = [0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0]
            .into_iter()
            .find(|s| s * pps >= 44.0)
            .unwrap_or(120.0);
        let mut t = 0.0;
        while t <= span + 1e-9 {
            let x = x_of(t);
            p.line_segment(
                [
                    Pos2::new(x, ruler.bottom() - 5.0),
                    Pos2::new(x, rect.bottom()),
                ],
                Stroke::new(1.0_f32, weak.gamma_multiply(0.3)),
            );
            p.text(
                Pos2::new(x + 2.0, ruler.top() + 1.0),
                egui::Align2::LEFT_TOP,
                format!("{}s", short(t)),
                egui::FontId::proportional(10.0),
                weak,
            );
            t += step;
        }
        if length > 0.0 {
            p.line_segment(
                [
                    Pos2::new(x_of(length), rect.top()),
                    Pos2::new(x_of(length), rect.bottom()),
                ],
                Stroke::new(1.5_f32, weak),
            );
        }
        let rr = ui.interact(ruler, ui.id().with("tl-ruler"), Sense::click_and_drag());
        let shown = self.head().unwrap_or(0.0);
        rr.widget_info(|| egui::WidgetInfo::slider(true, shown, "Timeline time"));
        let pointer = rr.interact_pointer_pos().or(rr.hover_pos());
        if let Some(pos) = rr.hover_pos() {
            rr.clone()
                .on_hover_text_at_pointer(format!("{:.2} s: drag to scrub", t_of(pos.x)));
        }
        if rr.drag_started() {
            self.ruler = Some(playing);
            if playing {
                out.push(TlDo::Pause);
            }
        }
        if let (Some(_), Some(pos)) = (self.ruler, pointer) {
            if rr.dragged() || rr.drag_started() {
                let t = short(t_of(pos.x));
                self.at = Some(t);
                out.push(TlDo::Seek(t));
            }
        }
        if rr.drag_stopped() {
            let was = self.ruler.take().unwrap_or(false);
            if let Some(t) = self.at {
                out.push(TlDo::Let(t, was));
            }
        } else if rr.clicked() && self.ruler.is_none() {
            if let Some(pos) = pointer {
                let t = short(t_of(pos.x));
                self.at = Some(t);
                out.push(TlDo::Seek(t));
                out.push(TlDo::Let(t, false));
            }
        }

        // The lanes, a track each, and the cues on them as bars.
        let mut top = rect.top() + ruler_h;
        for (i, (track, placed, n)) in layout.iter().enumerate() {
            let lane = Rect::from_min_max(
                Pos2::new(rect.left(), top),
                Pos2::new(rect.right(), top + lane_h * *n as f32),
            );
            top = lane.bottom();
            if i % 2 == 1 {
                p.rect_filled(lane, 0.0, ui.visuals().faint_bg_color);
            }
            p.with_clip_rect(Rect::from_min_max(
                lane.min,
                Pos2::new(x0 - 4.0, lane.bottom()),
            ))
            .text(
                Pos2::new(lane.left() + 4.0, lane.top() + lane_h / 2.0),
                egui::Align2::LEFT_CENTER,
                track,
                egui::FontId::proportional(11.0),
                text,
            );
            for (c, row) in placed {
                let y = lane.top() + *row as f32 * lane_h;
                let sub = Rect::from_min_max(
                    Pos2::new(lane.left(), y),
                    Pos2::new(lane.right(), y + lane_h),
                );
                self.bar(ui, &p, c, sub, length, pps, &x_of, out);
            }
        }

        if let Some(h) = self.head() {
            let x = x_of(h.min(span));
            let red = Color32::from_rgb(230, 70, 70);
            p.line_segment(
                [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                Stroke::new(2.0_f32, red),
            );
            p.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(x - 5.0, rect.top()),
                    Pos2::new(x + 5.0, rect.top()),
                    Pos2::new(x, rect.top() + 7.0),
                ],
                red,
                Stroke::NONE,
            ));
        }
    }

    /// One cue's bar: clicked selects it, dragged moves it, its right edge
    /// dragged makes it longer or shorter.
    #[allow(clippy::too_many_arguments)]
    fn bar(
        &mut self,
        ui: &mut egui::Ui,
        p: &egui::Painter,
        c: &Cue,
        lane: Rect,
        length: f64,
        pps: f64,
        x_of: &dyn Fn(f64) -> f32,
        out: &mut Vec<TlDo>,
    ) {
        let timed = c.kind == "tween" || c.kind == "camera";
        let duration = c
            .fields
            .get("duration")
            .and_then(Json::as_f64)
            .map(short)
            .unwrap_or(0.0);
        let (mut at, mut end) = (c.at, bar_end(c, length));
        if let Some(d) = self.drag.as_ref().filter(|d| d.cue == c.key) {
            if d.end {
                end = d.at + snap(d.duration + d.dt).max(0.0);
            } else {
                at = snap(d.at + d.dt);
                end = at + (c.end - c.at).max(0.0);
            }
        }
        let left = x_of(at);
        let right = x_of(end).max(left + 8.0);
        let r = Rect::from_min_max(
            Pos2::new(left, lane.top() + 3.0),
            Pos2::new(right, lane.bottom() - 3.0),
        );
        let colour = match c.kind.as_str() {
            "tween" => Color32::from_rgb(80, 140, 220),
            "anim" => Color32::from_rgb(100, 180, 110),
            "camera" => Color32::from_rgb(160, 110, 210),
            "line" => Color32::from_rgb(225, 155, 60),
            _ => Color32::from_rgb(210, 85, 85),
        };
        p.rect_filled(r, 3.0, colour.gamma_multiply(0.85));
        if c.wait {
            p.rect_filled(
                Rect::from_min_max(Pos2::new(r.right() - 3.0, r.top()), r.right_bottom()),
                0.0,
                Color32::from_gray(30),
            );
        }
        if self.cue.as_deref() == Some(c.key.as_str()) {
            p.rect_stroke(
                r,
                3.0,
                Stroke::new(2.0_f32, Color32::WHITE),
                egui::StrokeKind::Outside,
            );
        }
        let name = c
            .key
            .strip_prefix(&format!("{}.", self.chosen.as_deref().unwrap_or("")))
            .unwrap_or(&c.key)
            .to_string();
        p.with_clip_rect(r.shrink(1.0)).text(
            Pos2::new(r.left() + 3.0, r.center().y),
            egui::Align2::LEFT_CENTER,
            &name,
            egui::FontId::proportional(10.5),
            Color32::WHITE,
        );
        let id = ui.id().with(("tl-cue", &c.key));
        let resp = ui.interact(r, id, Sense::click_and_drag());
        let key = c.key.clone();
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &key));
        let mut tip = format!("{}: {} {}\nat {} s", c.key, c.kind, c.what, short(at));
        if end > at {
            tip.push_str(&format!(", ends {} s", short(end)));
        }
        if c.wait {
            tip.push_str("\nwaits until it is done");
        }
        tip.push_str(&format!("\n{}\ndrag to move it", c.at_ref));
        let resp = resp.on_hover_text(tip);
        if resp.clicked() {
            self.cue = Some(c.key.clone());
        }
        // The right edge: its duration.
        let edge = if timed {
            let e = Rect::from_min_max(Pos2::new(r.right() - 5.0, r.top()), r.right_bottom());
            let er = ui.interact(e, id.with("end"), Sense::drag());
            let label = format!("{} end", c.key);
            er.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
            Some(er.on_hover_cursor(egui::CursorIcon::ResizeHorizontal))
        } else {
            None
        };
        for (r, by_end) in [(Some(resp), false), (edge, true)] {
            let Some(r) = r else {
                continue;
            };
            if r.drag_started() {
                self.cue = Some(c.key.clone());
                self.drag = Some(Drag {
                    cue: c.key.clone(),
                    end: by_end,
                    at: c.at,
                    duration,
                    dt: 0.0,
                });
            }
            if r.dragged() {
                if let Some(d) = self.drag.as_mut().filter(|d| d.cue == c.key) {
                    d.dt += r.drag_delta().x as f64 / pps;
                }
            }
            if r.drag_stopped() {
                if let Some(d) = self.drag.take().filter(|d| d.cue == c.key) {
                    let def = format!("timeline:{}", c.key);
                    if d.end {
                        let v = snap(d.duration + d.dt);
                        if (v - d.duration).abs() > 1e-9 {
                            out.push(TlDo::Edit(
                                format!("{} lasts {v} s", c.key),
                                vec![op(&def, "duration", json!(v))],
                            ));
                        }
                    } else {
                        let v = snap(d.at + d.dt);
                        if (v - d.at).abs() > 1e-9 {
                            out.push(TlDo::Edit(
                                format!("{} at {v} s", c.key),
                                vec![op(&def, "at", json!(v))],
                            ));
                        }
                    }
                }
            }
        }
    }

    /// The selected cue's fields.
    fn cue_ui(&mut self, ui: &mut egui::Ui, cues: &[Cue], out: &mut Vec<TlDo>) {
        ui.separator();
        let Some(c) = self
            .cue
            .as_ref()
            .and_then(|k| cues.iter().find(|c| &c.key == k))
            .cloned()
        else {
            ui.weak("Click a cue to edit it; drag a bar to move it, its right edge to stretch it.");
            return;
        };
        let def = format!("timeline:{}", c.key);
        ui.horizontal(|ui| {
            ui.strong(&c.key);
            ui.weak(format!("{}  {}", c.kind, c.at_ref));
            if ui.button("Remove cue").clicked() {
                out.push(TlDo::Edit(
                    format!("take out {}", c.key),
                    vec![row_op(&def, None)],
                ));
                self.cue = None;
            }
        });
        let f = c.fields.clone();
        let s = |k: &str| f[k].as_str().unwrap_or("").to_string();
        let n = |k: &str| f[k].as_f64().map(short).unwrap_or(0.0);
        let v2 = |k: &str| {
            [
                f[k][0].as_f64().map(short).unwrap_or(0.0),
                f[k][1].as_f64().map(short).unwrap_or(0.0),
            ]
        };
        let kinds = self.names_of("kinds", &KINDS);
        let mut easings = self.names_of("easings", &["linear"]);
        easings.retain(|e| e != "linear");
        easings.insert(0, "linear".into());
        egui::Grid::new("tl-cue")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                self.text(ui, &def, "track", &s("track"), "track", &c.kind, false, out);
                self.num(ui, &def, "at", c.at, "at", "Seconds from the start", out);
                let kind = s("kind");
                self.choice(ui, &def, "kind", &kind, &kind, &kinds, "kind", out);
                self.text(
                    ui,
                    &def,
                    "who",
                    &s("who"),
                    "who",
                    "actor, other, camera, or @id",
                    false,
                    out,
                );
                match kind.as_str() {
                    "tween" => {
                        self.text(
                            ui,
                            &def,
                            "path",
                            &s("path"),
                            "path",
                            "Component.field",
                            false,
                            out,
                        );
                        // A tween with no from starts where the field is.
                        ui.label("from");
                        let mut here = f.get("from").is_none();
                        let r = ui.checkbox(&mut here, "where the field is");
                        if r.changed() {
                            if here {
                                let mut row = Self::row_fields(&c);
                                row.remove("from");
                                out.push(TlDo::Edit(
                                    format!("{}: from where the field is", c.key),
                                    vec![row_op(&def, Some(Json::Object(row)))],
                                ));
                            } else {
                                out.push(TlDo::Edit(
                                    format!("{}: from {:?}", c.key, v2("to")),
                                    vec![op(&def, "from", json!(v2("to")))],
                                ));
                            }
                        }
                        ui.end_row();
                        if f.get("from").is_some() {
                            self.vec2(ui, &def, "from", v2("from"), "from value", out);
                        }
                        self.vec2(ui, &def, "to", v2("to"), "to", out);
                        self.num(
                            ui,
                            &def,
                            "duration",
                            n("duration"),
                            "duration",
                            "Seconds",
                            out,
                        );
                        let e = s("easing");
                        let shown = if e.is_empty() {
                            "linear".to_string()
                        } else {
                            e.clone()
                        };
                        self.choice(ui, &def, "easing", &e, &shown, &easings, "easing", out);
                    }
                    "anim" => self.text(
                        ui,
                        &def,
                        "state",
                        &s("state"),
                        "state",
                        "an animator state",
                        false,
                        out,
                    ),
                    "camera" => self.num(
                        ui,
                        &def,
                        "duration",
                        n("duration"),
                        "duration",
                        "Seconds; 0: it draws until the timeline ends",
                        out,
                    ),
                    "line" => self.text(
                        ui,
                        &def,
                        "line",
                        &s("line"),
                        "line",
                        "dialogue:node",
                        false,
                        out,
                    ),
                    _ => self.text(
                        ui,
                        &def,
                        "effects",
                        &s("effects"),
                        "effects",
                        "path = value; emit name",
                        true,
                        out,
                    ),
                }
                self.text(
                    ui,
                    &def,
                    "when",
                    &s("when"),
                    "when",
                    "a condition: passed over unless it holds",
                    false,
                    out,
                );
                self.text(
                    ui,
                    &def,
                    "sound",
                    &s("sound"),
                    "sound",
                    "a sound, played from who",
                    false,
                    out,
                );
                ui.label("wait");
                let mut wait = f["wait"] == true;
                if ui
                    .checkbox(&mut wait, "hold the timeline until it is done")
                    .changed()
                {
                    out.push(TlDo::Edit(
                        format!("{}: wait = {wait}", c.key),
                        vec![op(&def, "wait", json!(wait))],
                    ));
                }
                ui.end_row();
            });
    }

    /// A new cue, at the playhead.
    fn new_cue_ui(&mut self, ui: &mut egui::Ui, name: &str, cues: &[Cue], out: &mut Vec<TlDo>) {
        ui.separator();
        let kinds = self.names_of("kinds", &KINDS);
        if self.new_cue.kind.is_empty() {
            self.new_cue.kind = "event".into();
        }
        let at = snap(self.head().unwrap_or(0.0));
        ui.strong(format!("New cue at {at} s"));
        egui::Grid::new("tl-new").num_columns(2).show(ui, |ui| {
            let named = ui.label("name").id;
            ui.add(
                egui::TextEdit::singleline(&mut self.new_cue.name)
                    .desired_width(140.0)
                    .hint_text("its kind and a number"),
            )
            .labelled_by(named);
            ui.end_row();
            let named = ui.label("of kind").id;
            egui::ComboBox::from_id_salt("tl-new-kind")
                .selected_text(self.new_cue.kind.clone())
                .show_ui(ui, |ui| {
                    for k in &kinds {
                        ui.selectable_value(&mut self.new_cue.kind, k.clone(), k);
                    }
                })
                .response
                .labelled_by(named);
            ui.end_row();
            let named = ui.label("on track").id;
            let mut tracks = self.tracks();
            tracks.dedup();
            ui.add(
                egui::TextEdit::singleline(&mut self.new_cue.track)
                    .desired_width(140.0)
                    .hint_text(tracks.first().cloned().unwrap_or_else(|| "its kind".into())),
            )
            .labelled_by(named);
            ui.end_row();
            let (field, hint) = need_of(&self.new_cue.kind);
            let named = ui.label(field).id;
            ui.add(
                egui::TextEdit::singleline(&mut self.new_cue.need)
                    .desired_width(220.0)
                    .hint_text(hint),
            )
            .labelled_by(named);
            ui.end_row();
        });
        let kind = self.new_cue.kind.clone();
        let short_name = if self.new_cue.name.trim().is_empty() {
            (1..)
                .map(|i| format!("{kind}{i}"))
                .find(|n| !cues.iter().any(|c| c.key == format!("{name}.{n}")))
                .unwrap_or_default()
        } else {
            self.new_cue.name.trim().to_string()
        };
        let key = format!("{name}.{short_name}");
        let taken = cues.iter().any(|c| c.key == key) || short_name.contains(':');
        let need = self.new_cue.need.trim().to_string();
        if ui
            .add_enabled(!taken && !need.is_empty(), egui::Button::new("Add cue"))
            .on_hover_text(format!("timeline:{key}"))
            .clicked()
        {
            let (field, _) = need_of(&kind);
            let mut f = json!({ "timeline": format!("timeline:{name}"), "kind": kind, "at": at });
            f[field] = json!(need);
            if !self.new_cue.track.trim().is_empty() {
                f["track"] = json!(self.new_cue.track.trim());
            }
            if kind == "tween" {
                f["duration"] = json!(1.0);
            }
            out.push(TlDo::Edit(
                format!("new cue {key} at {at} s"),
                vec![row_op(&format!("timeline:{key}"), Some(f))],
            ));
            self.cue = Some(key);
            self.new_cue.name.clear();
            self.new_cue.need.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed() -> Json {
        json!({
            "timelines": [{
                "name": "outro", "length": 5.0, "ref": "defs/timeline.toml:7",
                "fields": { "length": 5.0 },
                "tracks": [
                    { "track": "wide", "cues": [
                        [0, "outro.pull", 0.0, 2.5, "tween", "@wide", "Transform.pos", false, "defs/timeline.toml:20",
                         { "timeline": "timeline:outro", "track": "wide", "kind": "tween", "who": "@wide",
                           "path": "Transform.pos", "from": [61.3, 7.0], "to": [58.0, 7.0], "duration": 2.5 }],
                    ] },
                    { "track": "hermit", "cues": [
                        [1, "outro.call", 3.0000001, 3.0000001, "line", "@hermit", "dialogue:outro.start", true, "defs/timeline.toml:40",
                         { "timeline": "timeline:outro", "track": "hermit", "at": 3.0, "kind": "line", "who": "@hermit",
                           "line": "dialogue:outro.start", "wait": true }],
                    ] },
                ],
            }],
            "playing": [
                { "timeline": "e:7", "name": "outro", "time": 1.0, "began": 10 },
                { "timeline": "e:9", "name": "outro", "time": 0.25, "began": 300 },
            ],
            "problems": [],
        })
    }

    #[test]
    fn the_rows_read_into_cues_and_the_latest_play_is_the_head() {
        let mut t = Timelines::default();
        t.take(listed());
        assert_eq!(t.chosen.as_deref(), Some("outro"), "the first is shown");
        assert_eq!(t.tracks(), ["wide", "hermit"]);
        let cues = t.cues();
        assert_eq!(cues[1].key, "outro.call");
        assert_eq!(cues[1].at, 3.0, "read as written, not as an f32");
        assert!(cues[1].wait);
        assert_eq!(t.length(), 5.0);
        assert_eq!(t.head(), Some(0.25), "the latest play's time");
        // While a seek is on its way, the time scrubbed to.
        t.at = Some(2.0);
        assert_eq!(t.want_seek(2.0), Some(2.0));
        assert_eq!(t.head(), Some(2.0));
    }

    #[test]
    fn one_seek_in_flight_and_the_latest_next() {
        let mut t = Timelines::default();
        assert_eq!(t.want_seek(1.0), Some(1.0));
        assert_eq!(t.want_seek(1.5), None);
        assert_eq!(t.want_seek(2.0), None);
        assert_eq!(
            t.seek_answered(None),
            Some(2.0),
            "the latest, not every one"
        );
        assert_eq!(t.seek_answered(Some(&json!({ "time": 2.0 }))), None);
        assert_eq!(t.at, Some(2.0));
        assert_eq!(t.want_seek(2.0), None, "already there");
    }

    #[test]
    fn ops_name_a_field_or_a_row() {
        assert_eq!(
            op("timeline:outro.call", "at", json!(2.5)),
            json!({ "op": "def", "def": "timeline:outro.call", "path": "at", "value": 2.5 })
        );
        assert_eq!(
            row_op("timeline:outro.call", None),
            json!({ "op": "def_row", "def": "timeline:outro.call", "remove": true })
        );
        assert_eq!(snap(1.2345), 1.25);
        assert_eq!(snap(-1.0), 0.0);
        assert_eq!(need_of("tween").0, "path");
    }
}
