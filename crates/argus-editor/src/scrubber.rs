//! The replay scrubber (§11: "the timeline scrubber is `sim.seek` over the
//! keyframe ring"; phase 4 item 5). A strip under the viewport: the run
//! from the oldest frame a seek reaches to the furthest it went, the
//! stretch where a seek is instant (every keyframe kept) and the older one
//! where it replays up to a minute, the changes made between ticks (edits,
//! room changes), the person's marks and the agent's questions, and any
//! game event the person names. Drag it and the game follows: one seek in
//! flight, the latest position next. From a frame of the run, Try from here
//! forks a world to play in without losing the run (§6), and the saved
//! replays, a failing test's among them, open here (§9).

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use serde_json::{json, Value as Json};
use std::collections::{BTreeMap, HashMap};

/// What the strip asks the editor to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Scrub {
    /// Go to this tick while the pointer drags (or on a click).
    Seek(u64),
    /// The pointer let go here: the command the person gave, and play goes
    /// on if it was playing before the drag.
    Let(u64, bool),
    /// Pause for a drag.
    Pause,
    /// A call the person made (a step, a fork, a replay opened).
    Act(String, Json),
}

/// A marker on the strip.
#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    /// The tick.
    pub tick: u64,
    /// The event's name.
    pub name: String,
    /// What it says.
    pub text: String,
}

/// The strip's state.
#[derive(Default)]
pub struct Scrubber {
    /// `sim.ring`.
    pub ring: Json,
    /// `replay.list`.
    pub replays: Json,
    /// Markers by tick and what they are, from the event log. They stay when
    /// a seek back cuts the log, since the run goes on the same until it
    /// branches; a branch shortens the run and drops those past its end.
    pub markers: BTreeMap<(u64, String), Marker>,
    /// Game events to mark, comma-separated ("death, shot").
    pub names: String,
    /// The pointer is dragging: whether the game played before.
    pub drag: Option<bool>,
    /// The tick the drag shows.
    pub at: Option<u64>,
    since: HashMap<String, u64>,
    run: Option<u64>,
    in_flight: bool,
    pending: Option<u64>,
    last_sent: Option<u64>,
}

/// The events always marked: the person's marks, the agent's questions.
pub const ALWAYS: [&str; 2] = ["mark.dropped", "review.asked"];

impl Scrubber {
    /// The event names to read: the two always marked, and the person's.
    pub fn event_names(&self) -> Vec<String> {
        let mut out: Vec<String> = ALWAYS.iter().map(|s| s.to_string()).collect();
        for n in self.names.split(',') {
            let n = n.trim();
            if !n.is_empty() && !out.iter().any(|o| o == n) {
                out.push(n.to_string());
            }
        }
        out
    }

    /// Where to read a name's events from: after the last seen.
    pub fn since(&self, name: &str) -> u64 {
        self.since.get(name).copied().unwrap_or(0)
    }

    /// Discrete clocks count turns.
    pub fn discrete(&self) -> bool {
        self.ring["driver"] == "discrete"
    }

    /// The run's range: the oldest tick a seek reaches, and the furthest.
    pub fn range(&self) -> (u64, u64) {
        let lo = self.ring["floor"].as_u64().unwrap_or(0);
        let hi = self.ring["end"]
            .as_u64()
            .unwrap_or(0)
            .max(self.ring["tick"].as_u64().unwrap_or(0));
        (lo, hi.max(lo))
    }

    /// `sim.ring`'s answer: a new run forgets the old one's markers; a
    /// shorter run the ones past its end.
    pub fn take_ring(&mut self, v: Json) {
        let run = v["run"].as_u64();
        if run != self.run {
            self.markers.clear();
            self.since.clear();
            self.run = run;
        }
        self.ring = v;
        let (lo, hi) = self.range();
        self.markers.retain(|(t, _), _| *t >= lo && *t <= hi);
    }

    /// `obs.events {name, since}`'s answer.
    pub fn take_events(&mut self, name: &str, v: &Json) {
        if let Some(s) = v["last_seq"].as_u64() {
            self.since.insert(name.to_string(), s);
        }
        let (lo, _) = self.range();
        for r in v["rows"].as_array().into_iter().flatten() {
            let Some(tick) = r["frame"].as_u64() else {
                continue;
            };
            if tick < lo {
                continue;
            }
            let f = &r["fields"];
            let text = match name {
                "mark.dropped" => {
                    let note = f["note"].as_str().unwrap_or("");
                    format!(
                        "mark #{}{}",
                        f["id"],
                        if note.is_empty() {
                            String::new()
                        } else {
                            format!(": {note}")
                        }
                    )
                }
                "review.asked" => format!(
                    "question #{}: {}",
                    f["id"],
                    f["text"].as_str().unwrap_or("")
                ),
                _ => {
                    let fields = f.to_string();
                    if fields == "{}" || fields == "null" {
                        name.to_string()
                    } else {
                        format!("{name} {fields}")
                    }
                }
            };
            let key = format!("{name} {}", f);
            self.markers.insert(
                (tick, key),
                Marker {
                    tick,
                    name: name.to_string(),
                    text,
                },
            );
        }
    }

    /// A seek the drag wants: the tick to send now, or none while one is on
    /// its way (the latest goes next).
    pub fn want_seek(&mut self, t: u64) -> Option<u64> {
        if self.last_sent == Some(t) && self.pending.is_none() {
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
    pub fn seek_answered(&mut self) -> Option<u64> {
        self.in_flight = false;
        let t = self.pending.take()?;
        self.want_seek(t)
    }

    /// The seek params for a tick.
    pub fn seek_params(&self, t: u64) -> Json {
        if self.discrete() {
            json!({ "turn": t })
        } else {
            json!({ "frame": t })
        }
    }

    /// Ticks a seek to `t` replays: from the keyframe at or before it.
    pub fn replays_for(&self, t: u64) -> Option<u64> {
        let ticks = self.ring["ticks"].as_array()?;
        let k = ticks
            .iter()
            .filter_map(Json::as_u64)
            .filter(|k| *k <= t)
            .max()?;
        Some(t - k)
    }

    fn when(&self, t: u64) -> String {
        match self.ring["hz"].as_f64() {
            Some(hz) if hz > 0.0 && !self.discrete() => {
                format!("tick {t} ({:.2} s)", t as f64 / hz)
            }
            _ => format!("turn {t}"),
        }
    }

    /// Draw it. `now` is the tick on screen; `free` is whether the keys are
    /// the editor's (for the arrows over the strip).
    pub fn ui(&mut self, ui: &mut egui::Ui, now: u64, playing: bool, free: bool) -> Vec<Scrub> {
        let mut out = Vec::new();
        if self.ring.is_null() {
            ui.weak("The run's frames show here once a scene is open.");
            return out;
        }
        let (lo, hi) = self.range();
        let end = self.ring["end"].as_u64().unwrap_or(hi);
        let discrete = self.discrete();
        let unit = if discrete { "Turn" } else { "Frame" };
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(now > lo, egui::Button::new(format!("{unit} -1")))
                .on_hover_text("One back, rebuilt from the keyframe before it")
                .clicked()
            {
                out.push(Scrub::Act("sim.seek".into(), self.seek_params(now - 1)));
            }
            if ui
                .button(format!("{unit} +1"))
                .on_hover_text(if now < end {
                    "One on, as the run went"
                } else {
                    "One on"
                })
                .clicked()
            {
                let (m, p) = if discrete {
                    ("sim.advance", json!({ "n": 1 }))
                } else {
                    ("sim.step", json!({ "n": 1 }))
                };
                out.push(Scrub::Act(m.into(), p));
            }
            if ui
                .add_enabled(now < end, egui::Button::new("To the end"))
                .on_hover_text("The furthest this run went")
                .clicked()
            {
                out.push(Scrub::Act("sim.seek".into(), self.seek_params(end)));
            }
            ui.separator();
            let worlds: Vec<String> = self.ring["worlds"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|w| w["name"].as_str().map(str::to_string))
                .collect();
            if ui
                .button("Try from here")
                .on_hover_text(
                    "A world of its own from this frame, to play in; this run stays as it is",
                )
                .clicked()
            {
                let name = (1..)
                    .map(|i| format!("try-{i}"))
                    .find(|n| !worlds.contains(n))
                    .expect("a free name");
                out.push(Scrub::Act(
                    "sim.fork".into(),
                    json!({ "name": name, "switch": true }),
                ));
            }
            let active = self.ring["fork"].as_str().unwrap_or("main").to_string();
            egui::ComboBox::new("world", "World")
                .selected_text(&active)
                .show_ui(ui, |ui| {
                    for w in &worlds {
                        ui.horizontal(|ui| {
                            if ui.selectable_label(*w == active, w).clicked() && *w != active {
                                out.push(Scrub::Act(
                                    "sim.fork".into(),
                                    json!({ "name": w, "switch": true }),
                                ));
                            }
                            if *w != active && ui.small_button("Drop").clicked() {
                                out.push(Scrub::Act(
                                    "sim.fork".into(),
                                    json!({ "name": w, "drop": true }),
                                ));
                            }
                        });
                    }
                });
            ui.separator();
            let total = self.replays["total"].as_u64().unwrap_or(0);
            ui.menu_button(format!("Replays ({total})"), |ui| {
                ui.set_max_width(520.0);
                if total == 0 {
                    ui.weak("None saved: a failing test saves one, and so does replay.save.");
                }
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        for r in self.replays["rows"].as_array().into_iter().flatten() {
                            let path = r["path"].as_str().unwrap_or("").to_string();
                            ui.push_id(&path, |ui| {
                                ui.horizontal(|ui| {
                                    if ui.button("Open").clicked() {
                                        out.push(Scrub::Act(
                                            "replay.open".into(),
                                            json!({ "path": path }),
                                        ));
                                        ui.close_menu();
                                    }
                                    let what = r["about"]["scenario"]
                                        .as_str()
                                        .map(str::to_string)
                                        .unwrap_or_else(|| path.clone());
                                    ui.label(RichText::new(what).strong());
                                    ui.weak(format!(
                                        "{} seed {} frame {}",
                                        r["scene"].as_str().unwrap_or(""),
                                        r["seed"],
                                        r["frame"]
                                    ));
                                });
                                if let Some(n) = r["note"].as_str().filter(|n| !n.is_empty()) {
                                    ui.add(egui::Label::new(RichText::new(n).size(11.5)).wrap());
                                }
                                ui.separator();
                            });
                        }
                    });
            });
            ui.separator();
            ui.label("Mark events");
            ui.add(
                egui::TextEdit::singleline(&mut self.names)
                    .hint_text("death, shot")
                    .desired_width(120.0),
            );
        });

        // The track.
        let (rect, resp) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), 28.0),
            Sense::click_and_drag(),
        );
        // A slider to assistive technology (and to the editor's tests): the
        // run's position.
        let shown_value = self.at.unwrap_or(now) as f64;
        resp.widget_info(|| egui::WidgetInfo::slider(true, shown_value, "Run"));
        let span = (hi - lo).max(1) as f32;
        let x_of = |t: u64| rect.left() + (t.saturating_sub(lo)) as f32 / span * rect.width();
        let t_of = |x: f32| {
            let f = ((x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
            lo + (f * span).round() as u64
        };
        let v = ui.visuals().clone();
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 3.0, v.extreme_bg_color);
        // Where a seek is instant: every keyframe kept.
        if let Some(d) = self.ring["dense_from"].as_u64() {
            let r = Rect::from_min_max(Pos2::new(x_of(d.max(lo)), rect.top()), rect.max);
            p.rect_filled(r, 3.0, v.widgets.inactive.bg_fill);
        }
        // Played up to the tick on screen.
        let shown = self.at.unwrap_or(now).clamp(lo, hi);
        let played = Rect::from_min_max(rect.min, Pos2::new(x_of(shown), rect.bottom()));
        p.rect_filled(played, 3.0, v.selection.bg_fill.gamma_multiply(0.35));
        // Changes made between ticks.
        let amber = Color32::from_rgb(240, 170, 60);
        for t in self.ring["barrier_ticks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Json::as_u64)
        {
            let x = x_of(t);
            p.line_segment(
                [
                    Pos2::new(x, rect.bottom() - 9.0),
                    Pos2::new(x, rect.bottom()),
                ],
                Stroke::new(1.0_f32, amber),
            );
        }
        // Marks, questions, and the person's events.
        for m in self.markers.values() {
            let x = x_of(m.tick);
            let (c, y) = match m.name.as_str() {
                "mark.dropped" => (Color32::from_rgb(235, 80, 70), rect.top() + 5.0),
                "review.asked" => (amber, rect.top() + 5.0),
                _ => (Color32::from_rgb(120, 190, 255), rect.center().y + 3.0),
            };
            if m.name == "mark.dropped" || m.name == "review.asked" {
                p.add(egui::Shape::convex_polygon(
                    vec![
                        Pos2::new(x - 4.0, rect.top()),
                        Pos2::new(x + 4.0, rect.top()),
                        Pos2::new(x, y + 4.0),
                    ],
                    c,
                    Stroke::NONE,
                ));
            } else {
                p.circle_filled(Pos2::new(x, y), 2.5, c);
            }
        }
        // The tick on screen.
        let x = x_of(shown);
        p.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(2.0_f32, v.strong_text_color()),
        );

        // The pointer.
        let pointer = resp.interact_pointer_pos().or(resp.hover_pos());
        if let Some(pos) = resp.hover_pos() {
            let t = t_of(pos.x);
            let mut tip = self.when(t);
            if let Some(n) = self.replays_for(t) {
                tip.push_str(&format!(", replays {n} ticks"));
            }
            for m in self
                .markers
                .values()
                .filter(|m| (x_of(m.tick) - pos.x).abs() <= 4.0)
            {
                tip.push_str(&format!("\n{} at {}", m.text, m.tick));
            }
            resp.clone().on_hover_text_at_pointer(tip);
        }
        if resp.drag_started() {
            let was = playing;
            self.drag = Some(was);
            if was {
                out.push(Scrub::Pause);
            }
        }
        if let (Some(_), Some(pos)) = (self.drag, pointer) {
            if resp.dragged() || resp.drag_started() {
                let t = t_of(pos.x);
                self.at = Some(t);
                out.push(Scrub::Seek(t));
            }
        }
        if resp.drag_stopped() {
            let was = self.drag.take().unwrap_or(false);
            if let Some(t) = self.at.or(pointer.map(|p| t_of(p.x))) {
                out.push(Scrub::Let(t, was));
            }
        } else if resp.clicked() && self.drag.is_none() {
            if let Some(pos) = pointer {
                let t = t_of(pos.x);
                self.at = Some(t);
                out.push(Scrub::Seek(t));
                out.push(Scrub::Let(t, false));
            }
        }
        // The arrows over the strip, a frame at a time.
        if resp.hovered() && free {
            let (left, right) = ui.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight),
                )
            });
            if left && now > lo {
                out.push(Scrub::Act("sim.seek".into(), self.seek_params(now - 1)));
            }
            if right {
                let (m, p) = if discrete {
                    ("sim.advance", json!({ "n": 1 }))
                } else {
                    ("sim.step", json!({ "n": 1 }))
                };
                out.push(Scrub::Act(m.into(), p));
            }
        }

        ui.horizontal(|ui| {
            ui.weak(lo.to_string());
            ui.label(format!("{} of {hi}", self.when(shown)));
            if shown < end {
                ui.weak("Back in the run: Play shows what happened next; new input changes it from here (Try from here keeps this run).");
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak(hi.to_string());
            });
        });
        out
    }

    /// The drag's seek landed and the tick on screen is it: stop showing
    /// the drag's own position.
    pub fn settle(&mut self, now: u64) {
        if self.drag.is_none() && !self.in_flight && self.pending.is_none() && self.at == Some(now)
        {
            self.at = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_seek_in_flight_and_the_latest_next() {
        let mut s = Scrubber::default();
        assert_eq!(s.want_seek(100), Some(100));
        assert_eq!(s.want_seek(90), None);
        assert_eq!(s.want_seek(80), None);
        assert_eq!(s.seek_answered(), Some(80), "the latest, not every one");
        assert_eq!(s.seek_answered(), None);
        assert_eq!(s.want_seek(80), None, "already there");
        assert_eq!(s.want_seek(70), Some(70));
    }

    #[test]
    fn markers_stay_across_a_seek_and_go_with_a_branch_or_a_new_run() {
        let mut s = Scrubber::default();
        s.take_ring(json!({ "run": 1, "floor": 0, "end": 600, "tick": 600 }));
        s.take_events(
            "mark.dropped",
            &json!({ "last_seq": 9, "rows": [{ "frame": 300, "fields": { "id": 1, "note": "here" } }] }),
        );
        assert_eq!(s.since("mark.dropped"), 9);
        // Back at 150: the log is cut there, the mark stays.
        s.take_ring(json!({ "run": 1, "floor": 0, "end": 600, "tick": 150 }));
        assert_eq!(s.markers.len(), 1);
        assert_eq!(s.markers.values().next().unwrap().text, "mark #1: here");
        // The run branched at 200: the mark at 300 never happened.
        s.take_ring(json!({ "run": 1, "floor": 0, "end": 200, "tick": 200 }));
        assert!(s.markers.is_empty());
        s.take_events(
            "mark.dropped",
            &json!({ "last_seq": 12, "rows": [{ "frame": 180, "fields": { "id": 2, "note": "" } }] }),
        );
        s.take_ring(json!({ "run": 2, "floor": 0, "end": 50, "tick": 50 }));
        assert!(s.markers.is_empty(), "another run");
        assert_eq!(s.since("mark.dropped"), 0);
    }

    #[test]
    fn a_seek_replays_from_the_keyframe_before() {
        let mut s = Scrubber::default();
        s.take_ring(
            json!({ "run": 1, "floor": 0, "end": 200, "tick": 200, "ticks": [0, 60, 120, 180] }),
        );
        assert_eq!(s.replays_for(150), Some(30));
        assert_eq!(s.replays_for(60), Some(0));
        assert_eq!(s.seek_params(5), json!({ "frame": 5 }));
        s.ring["driver"] = json!("discrete");
        assert_eq!(s.seek_params(5), json!({ "turn": 5 }));
        s.names = "death, shot,death".into();
        assert_eq!(
            s.event_names(),
            vec!["mark.dropped", "review.asked", "death", "shot"]
        );
    }
}
