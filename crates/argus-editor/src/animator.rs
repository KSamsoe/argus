//! The animator editor (§13 "Sprite animation"; phase 4 item 8). The
//! selected entity's animator, live: its state, its clip's frame, the
//! events it fires; and its machine, edited: each state's clip, loop, rate,
//! wind-up, anticipation, and hold, its events by frame ("hitbox enabled at
//! frame 4 of slash"), its transitions with their conditions and blend
//! windows, the transitions from any state, and new states. Every change is
//! an `edit.asset` op by the person: one transaction, the sidecar written
//! with its comments kept and imported again, so the running game plays it
//! at once, and Ctrl+Z puts back its exact text. A rate, a wind-up, or a
//! hold may be a field instead of a number (a tune field: the Tuning tab's
//! slider moves it while the game plays).

use eframe::egui::{self, Color32, Rect, RichText, Sense, Stroke, Vec2};
use serde_json::{json, Value as Json};
use std::collections::HashMap;

/// A change the panel asks for: a note and its `edit.asset` ops.
pub type Change = (String, Vec<Json>);

/// The panel's state.
#[derive(Default)]
pub struct Animator {
    /// The machine edited (its asset's name).
    pub machine: Option<String>,
    /// `asset.sidecar` of it.
    pub sidecar: Json,
    /// The sprite whose clips it plays, and `asset.get` of it.
    pub sprite: Option<String>,
    pub clips: Json,
    /// Animator assets there are (`asset.list`).
    pub machines: Vec<String>,
    /// The entity shown live, and its Animator's fields.
    pub entity: Option<String>,
    pub live: Json,
    /// The events it fired, newest last: (tick, name, fields).
    pub fired: Vec<(u64, String, Json)>,
    /// Where the event log was read to.
    pub since: u64,
    /// A new state's name and clip.
    pub new_state: (String, String),
    /// Values being edited, by path, until the edit is let go.
    nums: HashMap<String, f64>,
    texts: HashMap<String, String>,
}

/// One asset op.
pub fn op(machine: &str, path: &str, value: Option<Json>) -> Json {
    match value {
        Some(v) => json!({ "op": "asset", "asset": machine, "path": path, "value": v }),
        None => json!({ "op": "asset", "asset": machine, "path": path, "remove": true }),
    }
}

impl Animator {
    /// The selected entity's components (`world.get`): its animator, if it
    /// has one, becomes the one shown.
    pub fn take_entity(&mut self, entity: Option<&str>, components: &Json) {
        let a = &components["Animator"];
        if a.is_object() {
            let machine = a["machine"]
                .as_str()
                .filter(|m| !m.is_empty())
                .map(str::to_string);
            if entity.map(str::to_string) != self.entity || machine != self.machine {
                self.fired.clear();
                self.since = 0;
                self.sidecar = Json::Null;
            }
            self.entity = entity.map(str::to_string);
            if machine.is_some() {
                self.machine = machine;
            }
            let sprite = a["sprite"]
                .as_str()
                .filter(|s| !s.is_empty())
                .or_else(|| components["Sprite"]["atlas"].as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            if sprite != self.sprite {
                self.sprite = sprite;
                self.clips = Json::Null;
            }
            self.live = a.clone();
        } else if self.entity.is_some() && entity.map(str::to_string) != self.entity {
            self.entity = None;
            self.live = Json::Null;
            self.fired.clear();
        }
    }

    /// `obs.events {entity, since}`'s answer: the events it fired.
    pub fn take_events(&mut self, v: &Json) {
        if let Some(s) = v["last_seq"].as_u64() {
            self.since = s;
        }
        for r in v["rows"].as_array().into_iter().flatten() {
            let name = r["name"].as_str().unwrap_or("").to_string();
            // What the animator fired: its states entered and its events
            // by frame, which name the state they came from.
            if name == "anim.entered" || r["fields"]["state"].is_string() {
                self.fired
                    .push((r["frame"].as_u64().unwrap_or(0), name, r["fields"].clone()));
            }
        }
        let over = self.fired.len().saturating_sub(12);
        self.fired.drain(..over);
    }

    fn states(&self) -> Vec<(String, Json)> {
        self.sidecar["doc"]["states"]
            .as_object()
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default()
    }

    fn clip_frames(&self, clip: &str) -> Option<usize> {
        self.clips["clips"][clip]["frames"].as_array().map(Vec::len)
    }

    /// A number (or a field path) at `path`, edited in place: a change when
    /// let go.
    #[allow(clippy::too_many_arguments)]
    fn num(
        &mut self,
        ui: &mut egui::Ui,
        machine: &str,
        path: &str,
        current: &Json,
        default: f64,
        whole: bool,
        label: &str,
        out: &mut Vec<Change>,
    ) {
        let named = ui.label(label).id;
        if let Some(field) = current.as_str() {
            // A field: the game's own (a tune field for the Tuning tab).
            let text = self
                .texts
                .entry(path.to_string())
                .or_insert_with(|| field.to_string());
            let r = ui
                .add(egui::TextEdit::singleline(text).desired_width(120.0))
                .labelled_by(named);
            let text = text.clone();
            if r.lost_focus() && text != field && !text.is_empty() {
                let v = text.parse::<f64>().map(|n| json!(n)).unwrap_or(json!(text));
                out.push((
                    format!("{machine}: {path} = {text}"),
                    vec![op(machine, path, Some(v))],
                ));
                self.texts.remove(path);
            }
            r.on_hover_text(
                "A field of the entity, read each tick; a number instead makes it constant.",
            );
            return;
        }
        let now = current.as_f64().unwrap_or(default);
        let mut v = *self.nums.get(path).unwrap_or(&now);
        let integer = whole || current.is_i64() || current.is_u64();
        let r = ui
            .add(
                egui::DragValue::new(&mut v)
                    .speed(if integer { 0.1 } else { 0.05 })
                    .range(0.0..=1000.0)
                    .max_decimals(if integer { 0 } else { 2 }),
            )
            .labelled_by(named);
        if r.changed() {
            self.nums.insert(path.to_string(), v);
        }
        if (r.drag_stopped() || r.lost_focus()) && (v - now).abs() > 1e-9 {
            let value = if integer {
                json!(v.round() as i64)
            } else {
                json!((v * 1000.0).round() / 1000.0)
            };
            out.push((
                format!("{machine}: {path} {now} to {value}"),
                vec![op(machine, path, Some(value))],
            ));
            self.nums.remove(path);
        } else if r.drag_stopped() || r.lost_focus() {
            self.nums.remove(path);
        }
    }

    /// A text at `path`, a change on Enter or when it loses focus.
    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        ui: &mut egui::Ui,
        machine: &str,
        path: &str,
        current: &str,
        width: f32,
        label: &str,
        hint: &str,
        out: &mut Vec<Change>,
    ) {
        let named = ui.label(label).id;
        let t = self
            .texts
            .entry(path.to_string())
            .or_insert_with(|| current.to_string());
        let r = ui
            .add(
                egui::TextEdit::singleline(t)
                    .desired_width(width)
                    .hint_text(hint),
            )
            .labelled_by(named);
        let t = t.clone();
        if r.lost_focus() {
            if t != current {
                let v = if t.is_empty() { None } else { Some(json!(t)) };
                out.push((
                    format!("{machine}: {path} = {t:?}"),
                    vec![op(machine, path, v)],
                ));
            }
            self.texts.remove(path);
        }
    }

    /// Transitions under `base` (`states.run.transitions`, `any`).
    #[allow(clippy::too_many_arguments)]
    fn transitions(
        &mut self,
        ui: &mut egui::Ui,
        machine: &str,
        base: &str,
        list: &[Json],
        states: &[String],
        out: &mut Vec<Change>,
    ) {
        for (i, t) in list.iter().enumerate() {
            let path = format!("{base}[{i}]");
            ui.push_id(&path, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let to = t["to"].as_str().unwrap_or("?").to_string();
                    ui.label("to");
                    egui::ComboBox::from_id_salt("to")
                        .selected_text(&to)
                        .width(80.0)
                        .show_ui(ui, |ui| {
                            for s in states {
                                if ui.selectable_label(*s == to, s).clicked() && *s != to {
                                    out.push((
                                        format!("{machine}: {path} goes to {s}"),
                                        vec![op(machine, &format!("{path}.to"), Some(json!(s)))],
                                    ));
                                }
                            }
                        });
                    self.text(
                        ui,
                        machine,
                        &format!("{path}.when"),
                        t["when"].as_str().unwrap_or(""),
                        230.0,
                        "when",
                        "Body.speed > 0.5, end",
                        out,
                    );
                    self.num(
                        ui,
                        machine,
                        &format!("{path}.blend"),
                        &t["blend"],
                        0.0,
                        false,
                        "blend",
                        out,
                    );
                    if ui
                        .small_button("x")
                        .on_hover_text("Take this transition out")
                        .clicked()
                    {
                        out.push((
                            format!("{machine}: without {path}"),
                            vec![op(machine, &path, None)],
                        ));
                    }
                });
                if let Some(w) = t["window"].as_array() {
                    ui.weak(format!(
                        "  only while the clip is on frames {} to {}",
                        w.first().unwrap_or(&Json::Null),
                        w.get(1).unwrap_or(&Json::Null)
                    ));
                }
            });
        }
        if ui.small_button("+ transition").clicked() {
            let to = states.first().cloned().unwrap_or_default();
            out.push((
                format!("{machine}: a transition at {base}"),
                vec![op(
                    machine,
                    &format!("{base}[{}]", list.len()),
                    Some(json!({ "to": to, "when": "end" })),
                )],
            ));
        }
    }

    /// The frames of a state's clip, its events on them, and the live frame.
    fn strip(&self, ui: &mut egui::Ui, frames: usize, events: &[Json], live: Option<u64>) {
        let n = frames.max(1);
        let w = 22.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(w * n as f32, 18.0), Sense::hover());
        let p = ui.painter_at(rect);
        for i in 0..n {
            let r = Rect::from_min_size(
                rect.min + Vec2::new(i as f32 * w, 0.0),
                Vec2::new(w - 2.0, 18.0),
            );
            let on = live == Some(i as u64);
            p.rect_filled(
                r,
                2.0,
                if on {
                    ui.visuals().selection.bg_fill
                } else {
                    ui.visuals().extreme_bg_color
                },
            );
            p.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                i.to_string(),
                egui::FontId::proportional(10.0),
                ui.visuals().text_color(),
            );
            if events.iter().any(|e| e["frame"].as_u64() == Some(i as u64)) {
                p.line_segment(
                    [r.left_top(), r.right_top()],
                    Stroke::new(3.0_f32, Color32::from_rgb(240, 170, 60)),
                );
            }
        }
    }

    /// Draw it; the changes the person made.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Vec<Change> {
        let mut out = Vec::new();
        ui.horizontal(|ui| {
            egui::ComboBox::new("anim-machine", "Machine")
                .selected_text(self.machine.clone().unwrap_or_else(|| "choose one".into()))
                .show_ui(ui, |ui| {
                    for m in self.machines.clone() {
                        if ui
                            .selectable_label(self.machine.as_deref() == Some(m.as_str()), &m)
                            .clicked()
                        {
                            self.machine = Some(m);
                            self.sidecar = Json::Null;
                        }
                    }
                });
        });
        let Some(machine) = self.machine.clone() else {
            ui.weak("Select an entity with an Animator (in the hierarchy or the game), or choose a machine.");
            return out;
        };
        // Live: the entity's state, frame, and time.
        match (&self.entity, self.live.is_object()) {
            (Some(e), true) => {
                let l = &self.live;
                let state = l["state"].as_str().unwrap_or("?");
                let frames = self
                    .states()
                    .iter()
                    .find(|(n, _)| n == state)
                    .and_then(|(_, s)| s["clip"].as_str().and_then(|c| self.clip_frames(c)));
                ui.label(
                    RichText::new(format!(
                        "{e}: {state}, frame {}{}",
                        l["frame"],
                        frames.map(|n| format!(" of {n}")).unwrap_or_default()
                    ))
                    .strong(),
                );
                let mut more = format!(
                    "{:.2} s in it, looped {}",
                    l["time"].as_f64().unwrap_or(0.0),
                    l["loops"]
                );
                if l["blend_left"].as_f64().unwrap_or(0.0) > 0.0 {
                    more.push_str(", blending");
                }
                if l["speed"].as_f64().is_some_and(|s| (s - 1.0).abs() > 1e-6) {
                    more.push_str(&format!(", speed {}", l["speed"]));
                }
                ui.weak(more);
            }
            _ => {
                ui.weak("No entity plays it now: the machine alone.");
            }
        }
        if self.sidecar.is_null() {
            ui.weak("Reading the machine...");
            return out;
        }
        ui.weak(format!(
            "{}, its file",
            self.sidecar["file"].as_str().unwrap_or("?")
        ));
        let doc = self.sidecar["doc"].clone();
        let states = self.states();
        let names: Vec<String> = states.iter().map(|(n, _)| n.clone()).collect();
        let live_state = self.live["state"].as_str().map(str::to_string);
        ui.horizontal(|ui| {
            let initial = doc["initial"].as_str().unwrap_or("").to_string();
            ui.label("Starts in");
            egui::ComboBox::from_id_salt("anim-initial")
                .selected_text(&initial)
                .show_ui(ui, |ui| {
                    for s in &names {
                        if ui.selectable_label(*s == initial, s).clicked() && *s != initial {
                            out.push((
                                format!("{machine}: starts in {s}"),
                                vec![op(&machine, "initial", Some(json!(s)))],
                            ));
                        }
                    }
                });
        });
        egui::ScrollArea::vertical().id_salt("animator").auto_shrink([false, false]).show(ui, |ui| {
            for (name, st) in &states {
                let on = live_state.as_deref() == Some(name.as_str());
                let clip = st["clip"].as_str().unwrap_or("").to_string();
                let frames = self.clip_frames(&clip);
                let head = RichText::new(format!(
                    "{name}  (clip {clip}{})",
                    frames
                        .map(|n| format!(", {n} frame{}", if n == 1 { "" } else { "s" }))
                        .unwrap_or_default()
                ));
                let head = if on { head.strong().color(ui.visuals().selection.stroke.color) } else { head };
                let base = format!("states.{name}");
                egui::CollapsingHeader::new(head)
                    .id_salt(("state", name))
                    .default_open(on)
                    .show(ui, |ui| {
                        let events: Vec<Json> = st["events"].as_array().cloned().unwrap_or_default();
                        let live_frame = if on { self.live["frame"].as_u64() } else { None };
                        self.strip(ui, frames.unwrap_or(1), &events, live_frame);
                        ui.horizontal_wrapped(|ui| {
                            self.text(ui, &machine, &format!("{base}.clip"), &clip, 80.0, "clip", "a clip of the sprite", &mut out);
                            let mut looped = st["loop"].as_bool().unwrap_or(true);
                            if ui.checkbox(&mut looped, "loops").changed() {
                                out.push((format!("{machine}: {name} loops {looped}"), vec![op(&machine, &format!("{base}.loop"), Some(json!(looped)))]));
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            let clip_fps = self.clips["clips"][clip.as_str()]["fps"].as_f64().unwrap_or(10.0);
                            self.num(ui, &machine, &format!("{base}.fps"), &st["fps"], clip_fps, false, "rate", &mut out);
                            self.num(ui, &machine, &format!("{base}.windup"), &st["windup"], 0.0, true, "wind-up frames", &mut out);
                        });
                        ui.horizontal_wrapped(|ui| {
                            self.num(ui, &machine, &format!("{base}.anticipation"), &st["anticipation"], 0.0, true, "anticipation", &mut out);
                            self.num(ui, &machine, &format!("{base}.hold"), &st["hold"], 0.0, true, "hold", &mut out);
                        });
                        ui.label("Events by frame");
                        for (i, e) in events.iter().enumerate() {
                            let path = format!("{base}.events[{i}]");
                            ui.push_id(&path, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    self.num(ui, &machine, &format!("{path}.frame"), &e["frame"], 0.0, true, "frame", &mut out);
                                    self.text(ui, &machine, &format!("{path}.name"), e["name"].as_str().unwrap_or(""), 90.0, "name", "hitbox.on", &mut out);
                                    self.text(ui, &machine, &format!("{path}.sound"), e["sound"].as_str().unwrap_or(""), 90.0, "sound", "none", &mut out);
                                    if ui.small_button("x").on_hover_text("Take this event out").clicked() {
                                        out.push((format!("{machine}: {name} without its event at frame {}", e["frame"]), vec![op(&machine, &path, None)]));
                                    }
                                });
                            });
                        }
                        if ui.small_button("+ event").clicked() {
                            out.push((
                                format!("{machine}: an event in {name}"),
                                vec![op(&machine, &format!("{base}.events[{}]", events.len()), Some(json!({ "frame": 0, "name": format!("{name}.event") })))],
                            ));
                        }
                        ui.label("Transitions, checked in order");
                        let list: Vec<Json> = st["transitions"].as_array().cloned().unwrap_or_default();
                        self.transitions(ui, &machine, &format!("{base}.transitions"), &list, &names, &mut out);
                        if ui.small_button("Take this state out").clicked() {
                            out.push((format!("{machine}: without {name}"), vec![op(&machine, &base, None)]));
                        }
                    });
            }
            ui.separator();
            ui.label("From any state, checked first");
            let any: Vec<Json> = doc["any"].as_array().cloned().unwrap_or_default();
            self.transitions(ui, &machine, "any", &any, &names, &mut out);
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label("New state");
                ui.add(egui::TextEdit::singleline(&mut self.new_state.0).desired_width(80.0).hint_text("name"));
                ui.add(egui::TextEdit::singleline(&mut self.new_state.1).desired_width(80.0).hint_text("clip"));
                let (n, c) = (self.new_state.0.trim().to_string(), self.new_state.1.trim().to_string());
                let ok = !n.is_empty() && !c.is_empty() && !names.contains(&n) && !n.contains(['.', '[', ' ']);
                if ui.add_enabled(ok, egui::Button::new("Add")).clicked() {
                    out.push((format!("{machine}: a state {n}"), vec![op(&machine, &format!("states.{n}"), Some(json!({ "clip": c })))]));
                    self.new_state = (String::new(), String::new());
                }
            });
            ui.separator();
            ui.label("What it fired");
            if self.fired.is_empty() {
                ui.weak(if self.entity.is_some() { "Nothing yet: play the game." } else { "No entity plays it." });
            }
            for (tick, name, f) in self.fired.iter().rev() {
                let what = if name == "anim.entered" {
                    format!("entered {}", f["state"].as_str().unwrap_or("?"))
                } else {
                    format!("{name} at frame {} of {}", f["frame"], f["state"].as_str().unwrap_or("?"))
                };
                ui.weak(format!("tick {tick}: {what}"));
            }
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_op_sets_or_takes_out() {
        assert_eq!(
            op("anim/hero", "states.run.fps", Some(json!(10))),
            json!({ "op": "asset", "asset": "anim/hero", "path": "states.run.fps", "value": 10 })
        );
        assert_eq!(
            op("anim/hero", "any[0]", None),
            json!({ "op": "asset", "asset": "anim/hero", "path": "any[0]", "remove": true })
        );
    }

    #[test]
    fn the_selected_entitys_animator_is_the_one_shown() {
        let mut a = Animator::default();
        a.take_entity(
            Some("@player"),
            &json!({ "Animator": { "machine": "anim/hero", "state": "run", "sprite": "" }, "Sprite": { "atlas": "hero" } }),
        );
        assert_eq!(a.machine.as_deref(), Some("anim/hero"));
        assert_eq!(
            a.sprite.as_deref(),
            Some("hero"),
            "the Sprite's atlas when the Animator names none"
        );
        a.take_events(&json!({ "last_seq": 7, "rows": [
            { "frame": 10, "name": "footstep", "fields": { "entity": "@player", "state": "run", "frame": 0 } },
            { "frame": 11, "name": "shot", "fields": { "entity": "@player" } },
        ] }));
        assert_eq!(a.since, 7);
        assert_eq!(a.fired.len(), 1, "only what the animator fired");
        a.take_entity(Some("@bat"), &json!({ "Health": {} }));
        assert!(
            a.entity.is_none() && a.fired.is_empty(),
            "no animator: nothing live"
        );
        assert_eq!(
            a.machine.as_deref(),
            Some("anim/hero"),
            "the machine stays open"
        );
    }
}
