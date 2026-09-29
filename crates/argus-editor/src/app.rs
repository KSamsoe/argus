//! The editor's window (§11): panels over protocol calls. Everything a
//! person does here is a call the CLI makes the same way (P1), and "show
//! as command" prints each one as the `argus` line that makes it, "so the
//! human learns the agent's language by using the editor and can paste a
//! command into a chat instead of describing a click".

use crate::link::{Answer, Link, State, Want};
use argus_client::command;
use eframe::egui::{self, Color32, RichText};
use serde_json::{json, Value as Json};
use std::time::{Duration, Instant};

/// How often the bar asks where the sim is.
const STATUS_EVERY: Duration = Duration::from_millis(200);
/// How often the overview asks for the world's summary.
const SUMMARY_EVERY: Duration = Duration::from_secs(1);
/// Feed rows kept in the panel.
const FEED_KEEP: usize = 500;

/// A call that failed: the protocol's sentence, hint, and file:line (§5:
/// "errors are documentation").
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    /// The `argus` line of what was tried.
    pub what: String,
    /// The sentence.
    pub message: String,
    /// The hint, "written as if to a capable colleague".
    pub hint: String,
    /// Where, as file:line.
    pub reference: String,
}

impl Problem {
    fn from_error(what: &str, e: &Json) -> Problem {
        let s = |v: &Json| v.as_str().unwrap_or_default().to_string();
        Problem {
            what: what.to_string(),
            message: s(&e["message"]),
            hint: s(&e["data"]["hint"]),
            reference: s(&e["data"]["ref"]),
        }
    }
}

/// The editor: its link to the daemon and what it has learned from it.
pub struct Editor {
    /// Who the person is in the change feed.
    pub author: String,
    link: Link,
    /// `session.status`: project, scene, clock, play state.
    pub status: Json,
    /// `world.summary`.
    pub summary: Json,
    /// Change feed rows, oldest first.
    pub feed: Vec<Json>,
    /// Everything the person did, as `argus` lines, oldest first.
    pub commands: Vec<String>,
    /// Show the commands panel.
    pub show_commands: bool,
    /// The last call that failed.
    pub problem: Option<Problem>,
    last_status: Option<Instant>,
    last_summary: Option<Instant>,
}

impl Editor {
    /// An editor for the daemon on this socket, as this author. It
    /// connects on its first frame.
    pub fn new(socket: impl Into<String>, author: impl Into<String>) -> Editor {
        Editor {
            author: author.into(),
            link: Link::new(socket),
            status: Json::Null,
            summary: Json::Null,
            feed: Vec::new(),
            commands: Vec::new(),
            show_commands: false,
            problem: None,
            last_status: None,
            last_summary: None,
        }
    }

    /// The connection's state.
    pub fn link_state(&self) -> &State {
        &self.link.state
    }

    /// Do something to the world as the person: the call, recorded as its
    /// `argus` line for "show as command".
    pub fn act(&mut self, method: &str, params: Json) {
        let line = command::line(method, &params);
        self.commands.push(line.clone());
        self.link.send(method, params, Want::Action(line));
    }

    /// Take in what arrived, and ask again for what is due.
    fn pump(&mut self, ctx: &egui::Context) {
        if self.link.state == State::Idle {
            let c = ctx.clone();
            self.link.connect(&self.author, move || c.request_repaint());
        }
        let (answers, notes) = self.link.poll();
        for a in answers {
            self.take(a);
        }
        for (method, row) in notes {
            if method == "edit.committed" || method == "edit.command" {
                self.push_feed(row);
            }
        }
        if !self.link.is_up() {
            return;
        }
        let now = Instant::now();
        let due = |t: Option<Instant>, every| t.is_none_or(|t| now.duration_since(t) >= every);
        if due(self.last_status, STATUS_EVERY) && !self.link.waiting(&Want::Status) {
            self.last_status = Some(now);
            self.link.send("session.status", json!({}), Want::Status);
        }
        if due(self.last_summary, SUMMARY_EVERY) && !self.link.waiting(&Want::Summary) {
            self.last_summary = Some(now);
            self.link.send("world.summary", json!({}), Want::Summary);
        }
        ctx.request_repaint_after(STATUS_EVERY);
    }

    fn take(&mut self, a: Answer) {
        match (a.want, a.result) {
            (Want::Status, Ok(v)) => self.status = v,
            (Want::Summary, Ok(v)) => self.summary = v,
            (Want::Feed, Ok(v)) => {
                // Rows from before this editor connected, then the live ones.
                let mut rows: Vec<Json> = v["rows"].as_array().cloned().unwrap_or_default();
                let seen: Vec<u64> = self.feed.iter().filter_map(|r| r["id"].as_u64()).collect();
                rows.retain(|r| !r["id"].as_u64().is_some_and(|i| seen.contains(&i)));
                rows.append(&mut self.feed);
                rows.sort_by_key(|r| r["id"].as_u64().unwrap_or(0));
                self.feed = rows;
            }
            (Want::Action(line), Ok(_)) => {
                if self.problem.as_ref().is_some_and(|p| p.what == line) {
                    self.problem = None;
                }
                // What changed shows at once, not at the next poll.
                self.last_status = None;
                self.last_summary = None;
            }
            (Want::Action(line), Err(e)) => self.problem = Some(Problem::from_error(&line, &e)),
            (w, Err(e)) => {
                self.problem = Some(Problem::from_error(&format!("{w:?}"), &e));
            }
            _ => {}
        }
    }

    fn push_feed(&mut self, row: Json) {
        let id = row["id"].as_u64();
        if id.is_some() && self.feed.iter().any(|r| r["id"].as_u64() == id) {
            return;
        }
        self.feed.push(row);
        if self.feed.len() > FEED_KEEP {
            let over = self.feed.len() - FEED_KEEP;
            self.feed.drain(..over);
        }
    }

    /// Draw a frame, and do what the person asked in it.
    pub fn ui(&mut self, ctx: &egui::Context) {
        self.pump(ctx);
        egui::TopBottomPanel::top("bar").show(ctx, |ui| self.bar(ui));
        if let Some(p) = self.problem.clone() {
            egui::TopBottomPanel::bottom("problem").show(ctx, |ui| self.problem_ui(ui, &p));
        }
        if self.show_commands {
            egui::TopBottomPanel::bottom("commands")
                .resizable(true)
                .default_height(140.0)
                .show(ctx, |ui| self.commands_ui(ui));
        }
        egui::SidePanel::right("feed")
            .resizable(true)
            .default_width(380.0)
            .show(ctx, |ui| self.feed_ui(ui));
        egui::CentralPanel::default().show(ctx, |ui| self.overview(ui));
    }

    fn bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let st = self.status.clone();
            match &self.link.state {
                State::Up => {
                    let name = st["project"].as_str().unwrap_or("no project");
                    let scene = st["scene"].as_str().unwrap_or("-");
                    ui.strong(format!("{name} / {scene}"));
                }
                State::Idle => {
                    ui.label("connecting");
                }
                State::Down(why) => {
                    ui.colored_label(ui.visuals().error_fg_color, format!("disconnected: {why}"));
                    if ui.button("Reconnect").clicked() {
                        self.link.state = State::Idle;
                    }
                    return;
                }
            }
            ui.separator();
            let playing = st["playing"].as_bool().unwrap_or(false);
            let paused = st["paused"].as_bool().unwrap_or(false);
            if playing && !paused {
                if ui.button("Pause").clicked() {
                    self.act("sim.pause", json!({}));
                }
            } else if ui.button("Play").clicked() {
                self.act("sim.play", json!({}));
            }
            if ui
                .add_enabled(playing, egui::Button::new("Stop"))
                .on_hover_text("Leave play and go back to where it started")
                .clicked()
            {
                self.act("sim.stop", json!({}));
            }
            if ui.button("Step").on_hover_text("One tick").clicked() {
                self.act("sim.step", json!({ "n": 1 }));
            }
            let speed = st["speed"].as_f64().unwrap_or(1.0);
            for s in [1u32, 3, 10] {
                let label = format!("{s}x");
                if ui
                    .selectable_label((speed - s as f64).abs() < 1e-9, label)
                    .clicked()
                {
                    self.act("sim.speed", json!({ "x": s }));
                }
            }
            ui.separator();
            let clock = st["clock"].as_str().unwrap_or("");
            ui.label(format!(
                "tick {}  frame {}  {:.2} s  {clock}",
                st["tick"],
                st["frame"],
                st["time"].as_f64().unwrap_or(0.0)
            ));
            ui.separator();
            ui.checkbox(&mut self.show_commands, "Show as command");
            ui.weak(format!("as {}", self.author));
        });
    }

    fn problem_ui(&mut self, ui: &mut egui::Ui, p: &Problem) {
        ui.horizontal(|ui| {
            ui.colored_label(ui.visuals().error_fg_color, &p.message);
            if ui.small_button("Dismiss").clicked() {
                self.problem = None;
            }
        });
        if !p.hint.is_empty() {
            ui.label(format!("hint: {}", p.hint));
        }
        if !p.reference.is_empty() {
            ui.label(format!("at: {}", p.reference));
        }
        ui.label(RichText::new(&p.what).monospace().weak());
    }

    fn commands_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("Commands");
            ui.weak("what you did, as the agent would type it");
            if ui.small_button("Copy all").clicked() {
                ui.ctx().copy_text(self.commands.join("\n"));
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("commands")
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (i, l) in self.commands.iter().enumerate() {
                    ui.push_id(i, |ui| {
                        ui.horizontal(|ui| {
                            if ui.small_button("Copy").clicked() {
                                ui.ctx().copy_text(l.clone());
                            }
                            ui.add(egui::Label::new(RichText::new(l).monospace()).wrap());
                        });
                    });
                }
            });
    }

    fn feed_ui(&mut self, ui: &mut egui::Ui) {
        ui.strong("Change feed");
        ui.weak("every change, by whom, as a command");
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("feed")
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for r in &self.feed {
                    let id = r["id"].as_u64().unwrap_or(0);
                    ui.push_id(id, |ui| {
                        let author = r["author"].as_str().unwrap_or("?");
                        let mine = author == self.author;
                        let who = RichText::new(author).strong().color(if mine {
                            Color32::from_rgb(90, 170, 255)
                        } else {
                            Color32::from_rgb(240, 170, 60)
                        });
                        ui.horizontal(|ui| {
                            ui.label(who);
                            ui.weak(format!("#{id}  frame {}", r["frame"]));
                            if let Some(k) = kind_word(&r["kind"]) {
                                ui.weak(k);
                            }
                        });
                        let note = r["note"].as_str().unwrap_or("");
                        if !note.is_empty() {
                            ui.label(note);
                        }
                        let cmd = r["command"].as_str().unwrap_or("");
                        if !cmd.is_empty() {
                            ui.add(
                                egui::Label::new(RichText::new(cmd).monospace().size(11.5)).wrap(),
                            );
                        }
                        ui.separator();
                    });
                }
            });
    }

    fn overview(&mut self, ui: &mut egui::Ui) {
        let s = &self.summary;
        if s.is_null() {
            ui.weak("waiting for the world");
            return;
        }
        ui.heading(format!(
            "{}  ({} entities, {} authored)",
            s["scene"].as_str().unwrap_or("-"),
            s["entities"],
            s["authored"]
        ));
        ui.label(format!(
            "seed {}  {}",
            s["seed"],
            s["clock"].as_str().unwrap_or("")
        ));
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("overview")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.strong("Named entities");
                for n in s["named"].as_array().into_iter().flatten().take(200) {
                    ui.label(format!("{}  {}", n[0].as_str().unwrap_or("?"), list(&n[1])));
                }
                ui.add_space(8.0);
                ui.strong("Archetypes");
                for a in s["archetypes"].as_array().into_iter().flatten().take(100) {
                    ui.label(format!("{} x {}", a[1], list(&a[0])));
                }
                ui.add_space(8.0);
                ui.strong("Systems");
                ui.label(list(&s["systems"]));
            });
    }
}

impl eframe::App for Editor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ui(ctx);
    }
}

fn list(v: &Json) -> String {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|x| {
                    x.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| x.to_string())
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

/// A feed row's kind in a word, where it is not a plain commit.
fn kind_word(k: &Json) -> Option<String> {
    match k {
        Json::String(s) if s == "commit" => None,
        Json::String(s) => Some(s.clone()),
        Json::Object(m) => m.iter().next().map(|(k, v)| format!("{k} #{v}")),
        _ => None,
    }
}
