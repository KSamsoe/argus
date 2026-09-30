//! The tuning panel (§10: "any field with tune = true appears in the
//! editor's tuning panel as a slider while the game runs"; phase 4 item 3).
//! Its rows are `tune.fields`: named entities' fields, the runtime entities
//! of a kind there are few of (Outpost's job kinds), and the def tables'
//! rows (a meal's food). Every movement of a slider is a `tune.set`, logged
//! with its frame, so the agent sees the search path and not only the value
//! the person settled on; the session's end keeps the final values as one
//! change by the person, or puts them back.

use eframe::egui::{self, RichText};
use serde_json::{json, Value as Json};
use std::collections::{BTreeMap, HashMap};

/// What the panel asks the editor to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Tune {
    /// A slider moved: `tune.set` of the target (`{entity}` or `{def}`).
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
    /// The last `tune.set` sent for each row: a slider held still sends none.
    sent: HashMap<String, Json>,
}

/// A row's key: its target and path.
pub fn key(row: &Json) -> String {
    let target = row["entity"]
        .as_str()
        .or_else(|| row["def"].as_str())
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
        let mut p = json!({ "path": row["path"], "value": v, "author": author });
        if let Some(d) = row["def"].as_str() {
            p["def"] = json!(d);
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
            // The named first, then the data tables, then the few of a kind
            // made at run time (Outpost's job kinds); the many of a kind,
            // which only what makes them tunes, last and folded.
            let order = if r["runtime"].is_u64() {
                3
            } else if r["def"].is_string() {
                1
            } else if r["runtime"] == true {
                2
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
                    .filter(|((o, _), _)| *o == 3)
                    .flat_map(|(_, rows)| rows.iter().cloned())
                    .collect();
                for ((order, label), rows) in groups.iter().filter(|((o, _), _)| *o < 3) {
                    let head = match order {
                        1 => format!("{label} (data table)"),
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
        let name = path.rsplit('.').next().unwrap_or(path);
        let mut hover = r["doc"].as_str().unwrap_or("").to_string();
        if let Some(t) = r["table"].as_str() {
            hover.push_str(&format!(
                "\nrow {} of defs/{t}.toml",
                r["def"].as_str().unwrap_or("?")
            ));
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
}
