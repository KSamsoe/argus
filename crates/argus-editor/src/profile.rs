//! The profiler view (§5 `prof.*`; phase 4 item 6). "A perf regression is
//! a query result with a system name on it", and "the view shows the §17
//! targets beside what is measured": the tick's p95 and its recent ticks
//! against the budget, §17's rows for this game with what is measured now
//! (a row over its bound names the system), every system's time and
//! allocations, the slowest recent ticks (one click seeks there, to see
//! what the game was doing), a capture for Perfetto or Tracy, the
//! protocol's call times, and the archetypes.

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use serde_json::{json, Value as Json};

/// The view's state: the answers it draws.
pub struct Profile {
    /// `prof.targets`.
    pub targets: Json,
    /// `prof.systems {series: true}`.
    pub systems: Json,
    /// `prof.alloc`.
    pub alloc: Json,
    /// `prof.worst`.
    pub worst: Json,
    /// `prof.calls`.
    pub calls: Json,
    /// `prof.archetypes`.
    pub archetypes: Json,
    /// `prof.capture`'s last answer.
    pub capture: Json,
    /// Ticks the next capture records.
    pub capture_ticks: u32,
    /// The file the last capture wrote.
    pub captured: Option<String>,
    /// Systems slowest first (else in stage order).
    pub slowest_first: bool,
    capturing: Option<String>,
}

impl Default for Profile {
    fn default() -> Profile {
        Profile {
            targets: Json::Null,
            systems: Json::Null,
            alloc: Json::Null,
            worst: Json::Null,
            calls: Json::Null,
            archetypes: Json::Null,
            capture: Json::Null,
            capture_ticks: 300,
            captured: None,
            slowest_first: true,
            capturing: None,
        }
    }
}

const GREEN: Color32 = Color32::from_rgb(90, 190, 110);
const RED: Color32 = Color32::from_rgb(235, 80, 70);
const AMBER: Color32 = Color32::from_rgb(240, 170, 60);

fn ms(v: &Json) -> String {
    match v.as_f64() {
        Some(x) if x >= 100.0 => format!("{x:.0}"),
        Some(x) if x >= 1.0 => format!("{x:.2}"),
        Some(x) => format!("{x:.3}"),
        None => "-".into(),
    }
}

impl Profile {
    /// Is a capture being recorded?
    pub fn capturing(&self) -> bool {
        self.capturing.is_some()
    }

    /// `prof.capture`'s answer: armed with its path, or done.
    pub fn take_capture(&mut self, v: Json) {
        if v["armed"] == true {
            if let Some(p) = v["path"].as_str() {
                self.capturing = Some(p.to_string());
            }
        } else if let Some(p) = self.capturing.take() {
            self.captured = Some(p);
        }
        self.capture = v;
    }

    /// The tick budget to draw: the game's tick row, else a frame at this
    /// speed.
    pub fn budget_ms(&self) -> f64 {
        let row = self.targets["rows"]
            .as_array()
            .and_then(|rows| rows.iter().find(|r| r["live"] == true && r["tick"] == true));
        match row.and_then(|r| r["bound"]["below"].as_f64()) {
            Some(b) => b,
            None => 1000.0 / 60.0 / self.targets["speed"].as_f64().unwrap_or(1.0).max(1.0),
        }
    }

    /// Draw it; the calls the person made (a capture, a seek to a slow tick).
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        discrete: bool,
        run: (u64, u64),
    ) -> Vec<(String, Json)> {
        let mut out = Vec::new();
        if self.targets.is_null() {
            ui.weak("Reading the profile...");
            return out;
        }
        egui::ScrollArea::vertical()
            .id_salt("profile")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.header_ui(ui);
                self.sparkline_ui(ui);
                ui.add_space(4.0);
                self.targets_ui(ui);
                ui.add_space(4.0);
                self.systems_ui(ui);
                ui.add_space(4.0);
                self.worst_ui(ui, discrete, run, &mut out);
                ui.add_space(4.0);
                self.capture_ui(ui, &mut out);
                ui.add_space(4.0);
                self.calls_ui(ui);
                self.archetypes_ui(ui);
            });
        out
    }

    fn header_ui(&self, ui: &mut egui::Ui) {
        let t = &self.targets;
        ui.horizontal_wrapped(|ui| {
            match t["tick_p95_ms"].as_f64() {
                Some(_) => {
                    ui.label(
                        RichText::new(format!("Tick p95 {} ms", ms(&t["tick_p95_ms"]))).strong(),
                    );
                    ui.weak(format!(
                        "over the last {} ticks",
                        self.systems["ticks"].as_array().map_or(0, Vec::len)
                    ));
                }
                None => {
                    ui.label(RichText::new("No ticks yet").strong());
                    ui.weak("play or step to measure");
                }
            }
            ui.weak(format!("{} entities, {}x", t["entities"], t["speed"]));
        });
        if let Some(h) = t["heaviest"].as_object() {
            ui.weak(format!(
                "Heaviest: {} at {} ms p95",
                h.get("name").and_then(Json::as_str).unwrap_or("?"),
                ms(h.get("p95_ms").unwrap_or(&Json::Null))
            ));
        }
        if t["build"] == "debug" {
            ui.colored_label(
                AMBER,
                "A debug build: §17's numbers are for release, where this runs several times faster.",
            );
        }
    }

    fn sparkline_ui(&self, ui: &mut egui::Ui) {
        let ticks: Vec<(u64, f64)> = self.systems["ticks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| Some((r[0].as_u64()?, r[1].as_f64()?)))
            .collect();
        let (rect, resp) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 54.0), Sense::hover());
        let p = ui.painter_at(rect);
        let v = ui.visuals();
        p.rect_filled(rect, 3.0, v.extreme_bg_color);
        if ticks.is_empty() {
            return;
        }
        let budget = self.budget_ms();
        let top = ticks.iter().map(|t| t.1).fold(budget * 1.25, f64::max);
        let y_of = |ms: f64| rect.bottom() - (ms / top) as f32 * rect.height();
        let w = rect.width() / ticks.len().max(1) as f32;
        for (i, (_, t)) in ticks.iter().enumerate() {
            let x = rect.left() + i as f32 * w;
            let c = if *t > budget {
                RED
            } else {
                v.selection.bg_fill
            };
            p.rect_filled(
                Rect::from_min_max(
                    Pos2::new(x, y_of(*t)),
                    Pos2::new(x + w.max(1.0), rect.bottom()),
                ),
                0.0,
                c,
            );
        }
        let by = y_of(budget);
        p.line_segment(
            [Pos2::new(rect.left(), by), Pos2::new(rect.right(), by)],
            Stroke::new(1.0_f32, AMBER),
        );
        p.text(
            Pos2::new(rect.left() + 3.0, by - 2.0),
            egui::Align2::LEFT_BOTTOM,
            format!("budget {budget:.2} ms"),
            egui::FontId::proportional(10.0),
            AMBER,
        );
        if let Some(pos) = resp.hover_pos() {
            let i = (((pos.x - rect.left()) / w) as usize).min(ticks.len() - 1);
            resp.on_hover_text_at_pointer(format!("tick {}: {:.3} ms", ticks[i].0, ticks[i].1));
        }
    }

    fn targets_ui(&self, ui: &mut egui::Ui) {
        let rows: Vec<&Json> = self.targets["rows"]
            .as_array()
            .into_iter()
            .flatten()
            .collect();
        let game = self.targets["game"].as_str();
        let (here, others): (Vec<&Json>, Vec<&Json>) = rows
            .into_iter()
            .partition(|r| r["game"] == "all" || Some(r["game"].as_str().unwrap_or("")) == game);
        let over = self.targets["over"].as_u64().unwrap_or(0);
        let title = match game {
            Some(g) => format!("§17 targets for {g}"),
            None => "§17 targets".to_string(),
        };
        let head = if over > 0 {
            format!("{title}: {over} over")
        } else {
            title
        };
        egui::CollapsingHeader::new(head)
            .id_salt("targets")
            .default_open(true)
            .show(ui, |ui| {
                target_grid(ui, "targets-here", &here);
                if !others.is_empty() {
                    egui::CollapsingHeader::new(format!("Other games' rows ({})", others.len()))
                        .id_salt("targets-others")
                        .default_open(false)
                        .show(ui, |ui| target_grid(ui, "targets-others-grid", &others));
                }
            });
    }

    fn systems_ui(&mut self, ui: &mut egui::Ui) {
        let mut rows: Vec<Json> = self.systems["rows"].as_array().cloned().unwrap_or_default();
        let alloc: Vec<Json> = self.alloc["rows"].as_array().cloned().unwrap_or_default();
        if self.slowest_first {
            rows.sort_by(|a, b| {
                b[3].as_f64()
                    .partial_cmp(&a[3].as_f64())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        let top = rows
            .iter()
            .filter_map(|r| r[3].as_f64())
            .fold(0.0, f64::max)
            .max(1e-9);
        egui::CollapsingHeader::new(format!("Systems ({})", rows.len()))
            .id_salt("systems")
            .default_open(true)
            .show(ui, |ui| {
                ui.checkbox(
                    &mut self.slowest_first,
                    "Slowest first (else in stage order)",
                );
                egui::Grid::new("systems-grid")
                    .striped(true)
                    .num_columns(5)
                    .show(ui, |ui| {
                        ui.weak("system");
                        ui.weak("p95 ms");
                        ui.weak("mean");
                        ui.weak("max");
                        ui.weak("allocs/tick");
                        ui.end_row();
                        for r in &rows {
                            let name = r[0].as_str().unwrap_or("?");
                            let p95 = r[3].as_f64().unwrap_or(0.0);
                            ui.horizontal(|ui| {
                                let (bar, _) =
                                    ui.allocate_exact_size(Vec2::new(36.0, 8.0), Sense::hover());
                                ui.painter()
                                    .rect_filled(bar, 1.0, ui.visuals().extreme_bg_color);
                                let f = (p95 / top) as f32;
                                ui.painter().rect_filled(
                                    Rect::from_min_size(
                                        bar.min,
                                        Vec2::new(bar.width() * f, bar.height()),
                                    ),
                                    1.0,
                                    ui.visuals().selection.bg_fill,
                                );
                                ui.label(name).on_hover_text(format!(
                                    "{} stage, {} ticks sampled",
                                    r[1].as_str().unwrap_or("?"),
                                    r[5]
                                ));
                            });
                            ui.label(ms(&r[3]));
                            ui.label(ms(&r[2]));
                            ui.label(ms(&r[4]));
                            let a = alloc.iter().find(|a| a[0] == r[0]);
                            ui.label(a.map_or("-".to_string(), |a| {
                                format!("{:.0}", a[2].as_f64().unwrap_or(0.0))
                            }))
                            .on_hover_text(
                                a.map_or(String::new(), |a| {
                                    format!("{:.0} bytes a tick", a[3].as_f64().unwrap_or(0.0))
                                }),
                            );
                            ui.end_row();
                        }
                    });
                if self.alloc["counted"] == false {
                    ui.weak("Allocations are not counted in this build.");
                }
            });
    }

    /// `run` is the ticks a seek can reach, floor to end: a slow tick from
    /// a run that went (play stopped, or a branch) is not one to go to.
    fn worst_ui(
        &self,
        ui: &mut egui::Ui,
        discrete: bool,
        run: (u64, u64),
        out: &mut Vec<(String, Json)>,
    ) {
        let rows: Vec<&Json> = self.worst["rows"]
            .as_array()
            .into_iter()
            .flatten()
            .collect();
        egui::CollapsingHeader::new("Slowest recent ticks")
            .id_salt("worst")
            .default_open(true)
            .show(ui, |ui| {
                if rows.is_empty() {
                    ui.weak("None yet.");
                }
                for r in rows {
                    let tick = r["tick"].as_u64().unwrap_or(0);
                    let mut systems: Vec<(String, f64)> = r["systems"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|s| Some((s[0].as_str()?.to_string(), s[1].as_f64()?)))
                        .collect();
                    systems
                        .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                    let top: Vec<String> = systems
                        .iter()
                        .take(3)
                        .map(|(n, t)| format!("{n} {t:.2}"))
                        .collect();
                    ui.push_id(("worst", tick), |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(format!("tick {tick}")).strong());
                            ui.label(format!("{} ms", ms(&r["total_ms"])));
                            let here = (run.0..=run.1).contains(&tick);
                            if ui
                                .add_enabled(here, egui::Button::new("Go there").small())
                                .on_hover_text("Seek to this tick, to see what the game was doing")
                                .on_disabled_hover_text(
                                    "This tick was in a run that is gone (play was stopped, or the run branched)",
                                )
                                .clicked()
                            {
                                let p = if discrete {
                                    json!({ "turn": tick })
                                } else {
                                    json!({ "frame": tick })
                                };
                                out.push(("sim.seek".into(), p));
                            }
                        });
                        ui.weak(top.join(", "));
                    });
                }
            });
    }

    fn capture_ui(&mut self, ui: &mut egui::Ui, out: &mut Vec<(String, Json)>) {
        egui::CollapsingHeader::new("Capture for Perfetto or Tracy")
            .id_salt("capture")
            .default_open(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut self.capture_ticks)
                            .range(1..=100_000)
                            .suffix(" ticks"),
                    );
                    if ui
                        .add_enabled(!self.capturing(), egui::Button::new("Capture"))
                        .on_hover_text("Every system's span over the next ticks, one file")
                        .clicked()
                    {
                        out.push((
                            "prof.capture".into(),
                            json!({ "ticks": self.capture_ticks }),
                        ));
                    }
                });
                if self.capturing() {
                    ui.label(format!(
                        "Recording: {} of {} ticks (the game has to be running)",
                        self.capture["captured"], self.capture["ticks"]
                    ));
                }
                if let Some(p) = self.captured.clone() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Written:");
                        ui.add(egui::Label::new(RichText::new(&p).monospace().size(11.0)).wrap());
                        if ui.small_button("Copy path").clicked() {
                            ui.ctx().copy_text(p.clone());
                        }
                    });
                    ui.weak("Open it at ui.perfetto.dev, or in Tracy with tracy-import-chrome.");
                }
            });
    }

    fn calls_ui(&self, ui: &mut egui::Ui) {
        let mut rows: Vec<Json> = self.calls["rows"].as_array().cloned().unwrap_or_default();
        rows.sort_by(|a, b| {
            b[3].as_f64()
                .partial_cmp(&a[3].as_f64())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        egui::CollapsingHeader::new(format!("Protocol calls ({})", rows.len()))
            .id_salt("calls")
            .default_open(false)
            .show(ui, |ui| {
                ui.weak(
                    "How long each method took, the slowest on average first: the agent's loop.",
                );
                egui::Grid::new("calls-grid")
                    .striped(true)
                    .num_columns(5)
                    .show(ui, |ui| {
                        for h in ["method", "calls", "mean ms", "last", "max"] {
                            ui.weak(h);
                        }
                        ui.end_row();
                        for r in &rows {
                            ui.label(r[0].as_str().unwrap_or("?"));
                            ui.label(r[1].to_string());
                            ui.label(ms(&r[3]));
                            ui.label(ms(&r[2]));
                            ui.label(ms(&r[4]));
                            ui.end_row();
                        }
                    });
            });
    }

    fn archetypes_ui(&self, ui: &mut egui::Ui) {
        let rows: Vec<&Json> = self.archetypes["rows"]
            .as_array()
            .into_iter()
            .flatten()
            .collect();
        egui::CollapsingHeader::new(format!("Archetypes ({})", rows.len()))
            .id_salt("archetypes")
            .default_open(false)
            .show(ui, |ui| {
                egui::Grid::new("arch-grid")
                    .striped(true)
                    .num_columns(3)
                    .show(ui, |ui| {
                        for h in ["entities", "KB", "components"] {
                            ui.weak(h);
                        }
                        ui.end_row();
                        for r in rows.iter().take(40) {
                            ui.label(r[1].to_string());
                            ui.label(format!("{:.0}", r[2].as_f64().unwrap_or(0.0) / 1024.0));
                            let comps: Vec<&str> = r[0]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(Json::as_str)
                                .collect();
                            ui.add(
                                egui::Label::new(RichText::new(comps.join(", ")).size(11.0)).wrap(),
                            );
                            ui.end_row();
                        }
                    });
            });
    }
}

/// §17's rows, two lines each: the measure and its target, then what is
/// measured now (or the test that gates it), with a dot for over or under.
fn target_grid(ui: &mut egui::Ui, id: &str, rows: &[&Json]) {
    ui.push_id(id, |ui| {
        for (i, r) in rows.iter().enumerate() {
            ui.push_id(i, |ui| {
                let c = match r["ok"].as_bool() {
                    Some(true) => Some(GREEN),
                    Some(false) => Some(RED),
                    None => None,
                };
                let mut tip = format!(
                    "{}\nGated by: {}",
                    r["why"].as_str().unwrap_or(""),
                    r["gate"].as_str().unwrap_or("")
                );
                if let Some(cx) = r["context"].as_str() {
                    tip.push_str(&format!("\nHere: {cx}"));
                }
                if let Some(s) = r["system"].as_str() {
                    tip.push_str(&format!("\nSystem: {s}"));
                }
                ui.horizontal(|ui| {
                    let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                    match c {
                        Some(c) => {
                            ui.painter().circle_filled(dot.center(), 4.0, c);
                        }
                        None => {
                            ui.painter().circle_stroke(
                                dot.center(),
                                3.5,
                                Stroke::new(1.0_f32, ui.visuals().weak_text_color()),
                            );
                        }
                    }
                    ui.label(RichText::new(r["measure"].as_str().unwrap_or("?")))
                        .on_hover_text(&tip);
                    ui.weak(r["target"].as_str().unwrap_or(""));
                });
                let now = match (r["live"].as_bool(), r["text"].as_str()) {
                    (Some(true), Some(t)) => {
                        let mut t = t.to_string();
                        if r["ok"] == false {
                            if let Some(s) = r["system"].as_str() {
                                t.push_str(&format!(": look at {s}"));
                            }
                        }
                        let t = RichText::new(t).size(11.5);
                        if r["ok"] == false {
                            t.color(RED)
                        } else {
                            t
                        }
                    }
                    _ => RichText::new(r["gate"].as_str().unwrap_or(""))
                        .size(11.0)
                        .weak(),
                };
                ui.horizontal(|ui| {
                    ui.add_space(18.0);
                    ui.add(egui::Label::new(now).wrap());
                });
                ui.add_space(3.0);
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capture_is_armed_then_written() {
        let mut p = Profile::default();
        p.take_capture(
            json!({ "armed": true, "captured": 0, "ticks": 300, "path": "C:/x/1-tick0.json" }),
        );
        assert!(p.capturing());
        p.take_capture(
            json!({ "armed": true, "captured": 120, "ticks": 300, "path": "C:/x/1-tick0.json" }),
        );
        p.take_capture(json!({ "armed": false, "captured": 0, "ticks": 0, "path": null }));
        assert!(!p.capturing());
        assert_eq!(p.captured.as_deref(), Some("C:/x/1-tick0.json"));
    }

    #[test]
    fn the_budget_is_the_games_tick_row_else_a_frame() {
        let mut p = Profile {
            targets: json!({ "speed": 10.0, "rows": [] }),
            ..Profile::default()
        };
        assert!((p.budget_ms() - 1.6667).abs() < 1e-3, "a frame at 10x");
        p.targets = json!({ "speed": 1.0, "rows": [{
            "measure": "Sim tick at 10× speed, 200 colonists, 20k items", "live": true, "tick": true,
            "bound": { "below": 1.6 },
        }] });
        assert_eq!(p.budget_ms(), 1.6);
    }
}
