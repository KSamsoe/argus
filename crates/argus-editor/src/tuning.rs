//! The tuning panel (§10: "any field with tune = true appears in the
//! editor's tuning panel as a slider while the game runs"; phase 4 item 3).
//! Its rows are `tune.fields`: named entities' fields, the runtime entities
//! of a kind there are few of (Outpost's job kinds), the def tables' rows
//! (a meal's food), and the particle effects' knobs (phase 5 item 3: Swarm's
//! sparks, their speed's min and max, their size over life). Every movement
//! of a slider is a `tune.set`, logged with its frame, so the agent sees
//! the search path and not only the value the person settled on; the
//! session's end keeps the final values as one change by the person, an
//! effect's in its sidecar, or puts them back. A field that is not a number
//! has the control its value needs (phase 5 item 3: a light's colour): a
//! colour as its swatch and its hex, sent when Enter is pressed; words as
//! a text box; two numbers as two to drag; a switch as a box to tick.

use eframe::egui::{self, RichText};
use serde_json::{json, Value as Json};
use std::collections::{BTreeMap, HashMap};

/// What the panel asks the editor to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Tune {
    /// A slider moved: `tune.set` of the target (`{entity}`, `{def}`, or
    /// `{asset}`).
    Set(Json),
    /// The person let go of a slider: the last `tune.set` is the command
    /// they gave (Show as command lists it once, not every step).
    Done(Json),
    /// Keep the session's values (`tune.end {commit: true}`), or put them
    /// back, with a note.
    End(bool, String),
}

/// The panel's state.
#[derive(Default)]
pub struct Tuning {
    /// `tune.fields`' rows.
    pub rows: Vec<Json>,
    /// The open session (`tune.fields`' `session`), or null.
    pub session: Json,
    /// The session's search paths (`tune.log`'s `story`).
    pub story: Vec<String>,
    /// Only rows whose label or path has this in it.
    pub filter: String,
    /// The note the session is kept with.
    pub note: String,
    /// Values a slider holds while it is dragged, by row key.
    held: HashMap<String, f64>,
    /// Words being typed, and two numbers being dragged, by row key.
    typed: HashMap<String, String>,
    held2: HashMap<String, [f64; 2]>,
    /// The last `tune.set` sent for each row: a slider held still sends none.
    sent: HashMap<String, Json>,
}

/// The place of the many of a kind among the groups: last, and folded.
const MANY: u8 = 4;

/// A row's key: its target and path.
pub fn key(row: &Json) -> String {
    let target = row["entity"]
        .as_str()
        .or_else(|| row["def"].as_str())
        .or_else(|| row["asset"].as_str())
        .or_else(|| row["component"].as_str())
        .unwrap_or("?");
    format!("{target} {}", row["path"].as_str().unwrap_or("?"))
}

impl Tuning {
    /// `tune.fields`' answer.
    pub fn take(&mut self, answer: &Json) {
        self.rows = answer["rows"].as_array().cloned().unwrap_or_default();
        self.session = answer["session"].clone();
    }

    /// The `tune.set` params for a row at a value.
    pub fn set_params(row: &Json, value: f64, author: &str) -> Json {
        let v = if row["value"].is_i64() || row["value"].is_u64() {
            json!(value.round() as i64)
        } else {
            json!((value * 10_000.0).round() / 10_000.0)
        };
        Tuning::set_value(row, v, author)
    }

    /// The `tune.set` params for a row at any value: words, two numbers, a
    /// switch.
    pub fn set_value(row: &Json, v: Json, author: &str) -> Json {
        let mut p = json!({ "path": row["path"], "value": v, "author": author });
        if let Some(d) = row["def"].as_str() {
            p["def"] = json!(d);
        } else if let Some(a) = row["asset"].as_str() {
            p["asset"] = json!(a);
        } else {
            p["entity"] = row["entity"].clone();
        }
        p
    }

    /// Draw it; what the person did.
    pub fn ui(&mut self, ui: &mut egui::Ui, author: &str) -> Vec<Tune> {
        let mut out = Vec::new();
        // The session: its fields, and keeping them or putting them back.
        // The same height open or not, so the sliders below never move under
        // a hand dragging one (a session opening at a drag's first step
        // pushed the list down and the slider dropped the drag).
        let open = !self.session.is_null();
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), 112.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_min_height(112.0);
                if open {
                    let n = self.session["fields"].as_array().map_or(0, Vec::len);
                    ui.label(format!(
                        "Tuning session {} by {}: {n} field{} moved",
                        self.session["id"],
                        self.session["author"].as_str().unwrap_or("?"),
                        if n == 1 { "" } else { "s" }
                    ));
                } else {
                    ui.weak("No tuning session: move a slider to start one.");
                }
                ui.horizontal(|ui| {
                    ui.add_enabled(
                        open,
                        egui::TextEdit::singleline(&mut self.note)
                            .hint_text("why (for the change feed)")
                            .desired_width(170.0),
                    );
                    if ui
                        .add_enabled(open, egui::Button::new("Keep"))
                        .on_hover_text("Save the values as one change by you (undo takes it back)")
                        .clicked()
                    {
                        out.push(Tune::End(true, self.note.clone()));
                    }
                    if ui
                        .add_enabled(open, egui::Button::new("Put back"))
                        .on_hover_text("Every field back to where the session found it")
                        .clicked()
                    {
                        out.push(Tune::End(false, String::new()));
                    }
                });
                // The search path of each field moved: what the agent reads.
                egui::ScrollArea::vertical()
                    .id_salt("tuning-story")
                    .max_height(54.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        if self.story.is_empty() {
                            ui.weak("Every move is logged with its frame, for the agent to read.");
                        }
                        for l in &self.story {
                            ui.add(
                                egui::Label::new(RichText::new(l).monospace().size(11.0)).wrap(),
                            );
                        }
                    });
            },
        );
        ui.horizontal(|ui| {
            ui.label("Find");
            ui.add(egui::TextEdit::singleline(&mut self.filter).desired_width(160.0));
            ui.weak(format!("{} fields", self.rows.len()));
        });
        ui.separator();
        // Rows grouped by what they tune: an entity, a kind, a table's row.
        let f = self.filter.to_lowercase();
        let mut groups: BTreeMap<(u8, String), Vec<Json>> = BTreeMap::new();
        for r in &self.rows {
            let label = r["label"].as_str().unwrap_or("?").to_string();
            let path = r["path"].as_str().unwrap_or("");
            if !f.is_empty()
                && !label.to_lowercase().contains(&f)
                && !path.to_lowercase().contains(&f)
            {
                continue;
            }
            // The named first, then the data tables, the effects, the few
            // of a kind made at run time (Outpost's job kinds); the many of
            // a kind, which only what makes them tunes, last and folded.
            let order = if r["runtime"].is_u64() {
                MANY
            } else if r["def"].is_string() {
                1
            } else if r["asset"].is_string() {
                2
            } else if r["runtime"] == true {
                3
            } else {
                0
            };
            groups.entry((order, label)).or_default().push(r.clone());
        }
        egui::ScrollArea::vertical()
            .id_salt("tuning")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let many: Vec<Json> = groups
                    .iter()
                    .filter(|((o, _), _)| *o == MANY)
                    .flat_map(|(_, rows)| rows.iter().cloned())
                    .collect();
                for ((order, label), rows) in groups.iter().filter(|((o, _), _)| *o < MANY) {
                    let head = match order {
                        1 => format!("{label} (data table)"),
                        2 => format!("{label} (effect)"),
                        _ => label.clone(),
                    };
                    egui::CollapsingHeader::new(head)
                        .id_salt(("tune", label))
                        .default_open(true)
                        .show(ui, |ui| {
                            for r in rows {
                                // Each slider's id is its row's, not its place:
                                // a session's line appearing above the list
                                // moved every place, and the slider being
                                // dragged lost its grip at the drag's first
                                // step (found by this item's test).
                                ui.push_id(key(r), |ui| self.row(ui, r, author, &mut out));
                            }
                        });
                }
                if !many.is_empty() {
                    egui::CollapsingHeader::new(format!("Many of a kind ({})", many.len()))
                        .id_salt("tune-many")
                        .default_open(false)
                        .show(ui, |ui| {
                            ui.weak("Fields on many entities made while the game runs: tune what makes them (a prefab, a data table).");
                            for r in &many {
                                ui.push_id(key(r), |ui| self.row(ui, r, author, &mut out));
                            }
                        });
                }
            });
        out
    }

    fn row(&mut self, ui: &mut egui::Ui, r: &Json, author: &str, out: &mut Vec<Tune>) {
        let path = r["path"].as_str().unwrap_or("?");
        // An effect's knob says what it is ("speed max"); a field, its name.
        let name = r["name"]
            .as_str()
            .unwrap_or_else(|| path.rsplit('.').next().unwrap_or(path));
        let mut hover = r["doc"].as_str().unwrap_or("").to_string();
        if let Some(t) = r["table"].as_str() {
            hover.push_str(&format!(
                "\nrow {} of defs/{t}.toml",
                r["def"].as_str().unwrap_or("?")
            ));
        }
        if let Some(f) = r["file"].as_str() {
            hover.push_str(&format!("\n{path} in {f}"));
        }
        if let Some(d) = r.get("default").filter(|d| !d.is_null()) {
            hover.push_str(&format!("\ndefault {d}"));
        }
        if r["runtime"].is_u64() {
            // Many of a kind: tuned through what they are made from.
            ui.horizontal(|ui| {
                ui.label(path).on_hover_text(&hover);
                ui.weak(format!("on {} entities", r["runtime"]));
            });
            return;
        }
        let k = key(r);
        // Not a number: words (a colour), two numbers, a switch.
        if !r["value"].is_number() && !r["value"].is_null() {
            self.other(ui, r, name, &hover, author, out);
            return;
        }
        let now = r["value"].as_f64().unwrap_or(0.0);
        let mut v = *self.held.get(&k).unwrap_or(&now);
        let range = r["range"]
            .as_array()
            .and_then(|a| Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?)));
        let resp = match range {
            Some((lo, hi)) => ui.add(
                egui::Slider::new(&mut v, lo..=hi)
                    .text(name)
                    .clamping(egui::SliderClamping::Always),
            ),
            None => {
                ui.horizontal(|ui| {
                    let resp = ui.add(egui::DragValue::new(&mut v).speed(0.01));
                    ui.label(name);
                    resp
                })
                .inner
            }
        }
        .on_hover_text(hover);
        if resp.changed() {
            self.held.insert(k.clone(), v);
            let p = Tuning::set_params(r, v, author);
            if self.sent.get(&k) != Some(&p) {
                self.sent.insert(k.clone(), p.clone());
                out.push(Tune::Set(p));
            }
        }
        if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
            // Let go (or a click on the track): the value the person gave.
            self.held.remove(&k);
            if let Some(p) = self.sent.get(&k) {
                out.push(Tune::Done(p.clone()));
            }
        }
    }

    /// A row's new value: a `tune.set` when it moved, and the command given
    /// when `done`.
    fn send(&mut self, r: &Json, author: &str, v: Json, done: bool, out: &mut Vec<Tune>) {
        let k = key(r);
        let p = Tuning::set_value(r, v, author);
        if self.sent.get(&k) != Some(&p) {
            self.sent.insert(k, p.clone());
            out.push(Tune::Set(p.clone()));
        }
        if done {
            out.push(Tune::Done(p));
        }
    }

    /// A row whose value is not a number.
    fn other(
        &mut self,
        ui: &mut egui::Ui,
        r: &Json,
        name: &str,
        hover: &str,
        author: &str,
        out: &mut Vec<Tune>,
    ) {
        let k = key(r);
        match &r["value"] {
            Json::String(now) => {
                ui.horizontal(|ui| {
                    let lab = ui.label(name);
                    // A colour: its swatch beside the words.
                    let shown = self.typed.get(&k).unwrap_or(now).clone();
                    if let Some(c) = hex(&shown) {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 2.0, c);
                    }
                    let mut text = shown;
                    let resp = ui
                        .add(
                            egui::TextEdit::singleline(&mut text)
                                .desired_width(90.0)
                                .hint_text("empty: the default"),
                        )
                        .labelled_by(lab.id)
                        .on_hover_text(hover);
                    if resp.changed() {
                        self.typed.insert(k.clone(), text.clone());
                    }
                    // Sent as a whole, when Enter is pressed or the box is
                    // left: a half-typed colour is no colour.
                    if resp.lost_focus() {
                        if let Some(t) = self.typed.remove(&k) {
                            if &t != now {
                                self.send(r, author, json!(t), true, out);
                            }
                        }
                    }
                });
            }
            Json::Array(a) if a.len() == 2 && a.iter().all(Json::is_number) => {
                let now = [a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0)];
                let mut v = *self.held2.get(&k).unwrap_or(&now);
                ui.horizontal(|ui| {
                    let rx = ui.add(egui::DragValue::new(&mut v[0]).speed(0.01));
                    let ry = ui.add(egui::DragValue::new(&mut v[1]).speed(0.01));
                    ui.label(name).on_hover_text(hover);
                    let round = |x: f64| (x * 10_000.0).round() / 10_000.0;
                    if rx.changed() || ry.changed() {
                        self.held2.insert(k.clone(), v);
                        let let_go = !(rx.dragged() || ry.dragged());
                        self.send(r, author, json!([round(v[0]), round(v[1])]), let_go, out);
                        if let_go {
                            self.held2.remove(&k);
                        }
                    } else if rx.drag_stopped() || ry.drag_stopped() {
                        self.held2.remove(&k);
                        if let Some(p) = self.sent.get(&k) {
                            out.push(Tune::Done(p.clone()));
                        }
                    }
                });
            }
            Json::Bool(now) => {
                let mut on = *now;
                if ui.checkbox(&mut on, name).on_hover_text(hover).changed() {
                    self.send(r, author, json!(on), true, out);
                }
            }
            v => {
                ui.horizontal(|ui| {
                    ui.label(name).on_hover_text(hover);
                    ui.weak(v.to_string());
                });
            }
        }
    }
}

/// A `#RRGGBB` or `#RRGGBBAA` colour, for its swatch.
fn hex(s: &str) -> Option<egui::Color32> {
    let h = s.trim().strip_prefix('#')?;
    let v = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    match h.len() {
        6 => Some(egui::Color32::from_rgb(v(0)?, v(2)?, v(4)?)),
        8 => Some(egui::Color32::from_rgba_unmultiplied(
            v(0)?,
            v(2)?,
            v(4)?,
            v(6)?,
        )),
        _ => None,
    }
}
