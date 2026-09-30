//! The editor's window (§11): panels over protocol calls. Everything a
//! person does here is a call the CLI makes the same way (P1), and "show
//! as command" prints each one as the `argus` line that makes it, "so the
//! human learns the agent's language by using the editor and can paste a
//! command into a chat instead of describing a click".

use crate::hierarchy::{Ask, Hierarchy};
use crate::inspector::{Do, Inspector};
use crate::link::{Answer, Link, State, Want};
use crate::tuning::{Tune, Tuning};
use crate::viewport::{Selected, Send, Viewport};
use argus_client::command;
use eframe::egui::{self, Color32, RichText};
use serde_json::{json, Value as Json};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How often the bar asks where the sim is.
const STATUS_EVERY: Duration = Duration::from_millis(200);
/// How often the overview asks for the world's summary.
const SUMMARY_EVERY: Duration = Duration::from_secs(1);
/// Feed rows kept in the panel.
const FEED_KEEP: usize = 500;
/// How often the inspected entity is read again while the game plays, and
/// while it does not.
const GET_PLAYING: Duration = Duration::from_millis(250);
const GET_STILL: Duration = Duration::from_secs(1);
/// How often the locks are read.
const LOCKS_EVERY: Duration = Duration::from_secs(1);
/// How often the tuning panel reads its fields and the session's paths.
const TUNE_EVERY: Duration = Duration::from_millis(500);

/// The right panel's tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    /// The selected entity.
    Inspector,
    /// The tune fields' sliders.
    Tuning,
    /// The change feed.
    Feed,
}

/// A transaction of the person's on its way: ops queued, and whether one
/// was refused.
struct TxRun {
    left: usize,
    failed: Option<Problem>,
    line: String,
}

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
    /// The game, drawn and played in.
    pub viewport: Viewport,
    /// The game's sound plays while the viewport shows it (tests turn it
    /// off).
    pub audio: bool,
    /// The scene's entities, a page at a time.
    pub hierarchy: Hierarchy,
    /// The selected entity's components.
    pub inspector: Inspector,
    /// The entity selected, in the hierarchy or the viewport.
    pub selected: Option<String>,
    /// The right panel's tab.
    pub tab: Tab,
    /// The tune fields, a slider each.
    pub tuning: Tuning,
    txs: HashMap<u64, TxRun>,
    described: bool,
    last_status: Option<Instant>,
    last_summary: Option<Instant>,
    last_get: Option<Instant>,
    last_locks: Option<Instant>,
    last_tune: Option<Instant>,
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
            viewport: Viewport::default(),
            audio: true,
            hierarchy: Hierarchy::default(),
            inspector: Inspector::default(),
            selected: None,
            tab: Tab::Inspector,
            tuning: Tuning::default(),
            txs: HashMap::new(),
            described: false,
            last_status: None,
            last_summary: None,
            last_get: None,
            last_locks: None,
            last_tune: None,
        }
    }

    /// Select an entity (or none), as the hierarchy and the viewport do.
    pub fn select(&mut self, entity: Option<String>) {
        if self.selected != entity {
            self.selected = entity.clone();
            self.inspector.show(entity.clone());
            self.last_get = None;
            let same = self.viewport.selected.as_ref().map(|s| &s.id) == entity.as_ref();
            if !same {
                self.viewport.selected = entity.map(|id| Selected {
                    id,
                    name: String::new(),
                    bounds: [0.0; 4],
                });
            }
        }
    }

    /// A transaction by the person: `edit.begin`, its ops, `edit.commit`,
    /// shown as the one `argus edit` line the change feed will carry.
    pub fn edit(&mut self, note: &str, ops: Vec<Json>) {
        let line = command::line(
            "edit",
            &json!({ "author": self.author, "note": note, "ops": ops }),
        );
        self.commands.push(line.clone());
        self.link.send(
            "edit.begin",
            json!({ "author": self.author, "note": note }),
            Want::EditBegin(line, ops),
        );
    }

    fn tuned(&mut self, t: Tune) {
        match t {
            // Every move goes to the daemon, which logs it with its frame.
            Tune::Set(p) => {
                self.link.send("tune.set", p, Want::TuneSet);
                self.last_tune = None;
            }
            // The value the person let go at is the command they gave.
            Tune::Done(p) => self.commands.push(command::line("tune.set", &p)),
            Tune::End(commit, note) => {
                let mut p = json!({ "commit": commit });
                if !note.is_empty() {
                    p["note"] = json!(note);
                }
                self.act("tune.end", p);
                self.tuning.note.clear();
            }
        }
    }

    fn done(&mut self, d: Do) {
        let author = self.author.clone();
        match d {
            Do::Edit(note, ops) => self.edit(&note, ops),
            Do::Lock(entities, paths, note) => {
                let mut p = json!({ "author": author, "note": note });
                if !entities.is_empty() {
                    p["entities"] = json!(entities);
                }
                if !paths.is_empty() {
                    p["paths"] = json!(paths);
                }
                self.act("edit.lock", p);
                self.last_locks = None;
            }
            Do::Unlock(id) => {
                self.act("edit.unlock", json!({ "author": author, "lock": id }));
                self.last_locks = None;
            }
            Do::Undo => self.act("edit.undo", json!({ "author": author })),
            Do::Redo => self.act("edit.redo", json!({ "author": author })),
        }
    }

    /// Draw the viewport's frames with the window's GPU (eframe's wgpu
    /// render state), straight from the shared file into a texture.
    pub fn with_gpu(mut self, state: Option<eframe::egui_wgpu::RenderState>) -> Editor {
        self.viewport.gpu = state.map(crate::viewport::Gpu::new);
        self
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
            self.viewport.reset();
            self.described = false;
            self.hierarchy.stale();
        }
        let (answers, notes) = self.link.poll();
        for a in answers {
            self.take(a);
        }
        for (method, row) in notes {
            if method == "edit.committed" || method == "edit.command" {
                self.push_feed(row);
                // Something changed: the tree and the entity shown, again.
                self.hierarchy.stale();
                self.last_get = None;
            } else if method == "render.frame" {
                self.viewport.on_frame(row);
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
        if !self.described && !self.link.waiting(&Want::Describe) {
            self.described = true;
            self.link
                .send("world.describe", json!({ "limit": 1000 }), Want::Describe);
        }
        for ask in self.hierarchy.due(now) {
            let want = Want::Hier(ask.clone());
            if self.link.waiting(&want) {
                continue;
            }
            let (method, params) = match &ask {
                Ask::Top(c, n) => ("world.query", self.hierarchy.top_query(c.as_deref(), *n)),
                Ask::Children(p) => ("world.query", self.hierarchy.children_query(p)),
                Ask::Parents => ("world.query", Hierarchy::parents_query()),
                Ask::Rooms => ("scene.rooms", json!({})),
            };
            self.link.send(method, params, want);
        }
        let playing = self.status["playing"] == true && self.status["paused"] != true;
        if let Some(e) = self.selected.clone() {
            let every = if playing { GET_PLAYING } else { GET_STILL };
            let want = Want::Get(e.clone());
            if due(self.last_get, every) && !self.link.waiting(&want) {
                self.last_get = Some(now);
                self.link
                    .send("world.get", json!({ "entity": e, "resolved": true }), want);
                let want = Want::Overrides(e.clone());
                if !self.link.waiting(&want) {
                    self.link.send("world.get", json!({ "entity": e }), want);
                }
            }
            if let Some(p) = self.inspector.prefab() {
                let want = Want::Instances(p.clone());
                if self
                    .inspector
                    .instances
                    .as_ref()
                    .is_none_or(|(q, _)| *q != p)
                    && !self.link.waiting(&want)
                {
                    self.link
                        .send("world.instances", json!({ "prefab": p, "limit": 1 }), want);
                }
            }
        }
        // The tuning panel's fields while it is shown, and the session's
        // search paths while one is open.
        if (self.tab == Tab::Tuning || !self.tuning.session.is_null())
            && due(self.last_tune, TUNE_EVERY)
            && !self.link.waiting(&Want::TuneFields)
        {
            self.last_tune = Some(now);
            self.link.send("tune.fields", json!({}), Want::TuneFields);
            if let Some(id) = self.tuning.session["id"].as_u64() {
                if !self.link.waiting(&Want::TuneLog) {
                    self.link.send(
                        "tune.log",
                        json!({ "session": id, "limit": 1 }),
                        Want::TuneLog,
                    );
                }
            }
        }
        if due(self.last_locks, LOCKS_EVERY) && !self.link.waiting(&Want::Locks) {
            self.last_locks = Some(now);
            self.link.send("edit.locks", json!({}), Want::Locks);
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
                self.last_get = None;
                self.last_locks = None;
                self.last_tune = None;
                self.hierarchy.stale();
            }
            (Want::Action(line), Err(e)) => self.problem = Some(Problem::from_error(&line, &e)),
            (Want::Pick(at), Ok(v)) => {
                self.viewport.pick(&v, at);
                let id = self.viewport.selected.as_ref().map(|s| s.id.clone());
                self.select(id);
            }
            (Want::Hier(ask), Ok(v)) => self.hierarchy.take(&ask, &v),
            // A flat scene has no rooms to list.
            (Want::Hier(Ask::Rooms), Err(_)) => {}
            (Want::Get(e), Ok(v)) => {
                if self.selected.as_deref() == Some(e.as_str()) {
                    self.inspector.take(v);
                }
            }
            (Want::Overrides(e), Ok(v)) => {
                if self.selected.as_deref() == Some(e.as_str()) {
                    self.inspector.overrides = v["overrides"].clone();
                }
            }
            (Want::Overrides(_), Err(_)) => {}
            (Want::Get(e), Err(err)) => {
                if self.selected.as_deref() == Some(e.as_str()) {
                    self.inspector.got = json!({ "entity": e, "error": err["message"] });
                }
            }
            (Want::Describe, Ok(v)) => {
                self.inspector.take_schemas(&v);
                self.hierarchy.named = self.inspector.schemas.contains_key("Describe");
                self.hierarchy.stale();
            }
            (Want::Instances(p), Ok(v)) => {
                self.inspector.instances = Some((p, v["total"].as_u64().unwrap_or(0)));
            }
            (Want::Locks, Ok(v)) => {
                self.inspector.locks = v["rows"].as_array().cloned().unwrap_or_default();
            }
            (Want::EditBegin(line, ops), Ok(v)) => {
                let tx = v["tx"].as_u64().unwrap_or(0);
                self.txs.insert(
                    tx,
                    TxRun {
                        left: ops.len(),
                        failed: None,
                        line: line.clone(),
                    },
                );
                for op in ops {
                    let mut p = op.clone();
                    let verb = p["op"].as_str().unwrap_or("set").to_string();
                    if let Some(m) = p.as_object_mut() {
                        m.remove("op");
                        m.insert("tx".into(), json!(tx));
                    }
                    self.link
                        .send(&format!("edit.{verb}"), p, Want::EditPart(tx));
                }
                if self.txs.get(&tx).is_some_and(|t| t.left == 0) {
                    self.finish_tx(tx);
                }
            }
            (Want::EditBegin(line, _), Err(e)) => {
                self.problem = Some(Problem::from_error(&line, &e));
            }
            (Want::EditPart(tx), r) => {
                if let Some(t) = self.txs.get_mut(&tx) {
                    t.left = t.left.saturating_sub(1);
                    if let Err(e) = r {
                        let line = t.line.clone();
                        t.failed.get_or_insert(Problem::from_error(&line, &e));
                    }
                    if t.left == 0 {
                        self.finish_tx(tx);
                    }
                }
            }
            (Want::EditAbort, _) => {}
            (Want::TuneFields, Ok(v)) => {
                self.tuning.take(&v);
                if self.tuning.session.is_null() {
                    self.tuning.story.clear();
                }
            }
            (Want::TuneLog, Ok(v)) => {
                self.tuning.story = v["story"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|s| s.as_str().map(str::to_string))
                    .collect();
            }
            (Want::TuneSet, Ok(_)) => {}
            (w, Err(e)) => {
                self.problem = Some(Problem::from_error(&format!("{w:?}"), &e));
            }
            _ => {}
        }
    }

    /// Every op of a transaction answered: commit it, or abort it if one was
    /// refused (and say why).
    fn finish_tx(&mut self, tx: u64) {
        let Some(t) = self.txs.remove(&tx) else {
            return;
        };
        match t.failed {
            Some(p) => {
                self.problem = Some(p);
                self.link
                    .send("edit.abort", json!({ "tx": tx }), Want::EditAbort);
            }
            None => self
                .link
                .send("edit.commit", json!({ "tx": tx }), Want::Action(t.line)),
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
        egui::SidePanel::right("right")
            .resizable(true)
            .min_width(300.0)
            .default_width(380.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tab, Tab::Inspector, "Inspector");
                    ui.selectable_value(&mut self.tab, Tab::Tuning, "Tuning");
                    ui.selectable_value(&mut self.tab, Tab::Feed, "Change feed");
                });
                ui.separator();
                match self.tab {
                    Tab::Inspector => {
                        let author = self.author.clone();
                        for d in self.inspector.ui(ui, &author) {
                            self.done(d);
                        }
                    }
                    Tab::Tuning => {
                        let author = self.author.clone();
                        for t in self.tuning.ui(ui, &author) {
                            self.tuned(t);
                        }
                    }
                    Tab::Feed => self.feed_ui(ui),
                }
            });
        egui::SidePanel::left("world")
            .resizable(true)
            .min_width(220.0)
            .default_width(280.0)
            .show(ctx, |ui| self.overview(ui));
        egui::CentralPanel::default().show(ctx, |ui| self.viewport_ui(ui));
        // Ctrl+Z and Ctrl+Shift+Z (or Ctrl+Y): the person's own undo and
        // redo, when neither the game nor a text field has the keys.
        let free = !self.viewport.focused && ctx.memory(|m| m.focused().is_none());
        if free && self.link.is_up() {
            let (redo, undo) = ctx.input_mut(|i| {
                let redo = i.consume_key(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::Z,
                ) || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y);
                let undo = i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z);
                (redo, undo)
            });
            if redo {
                self.done(Do::Redo);
            } else if undo {
                self.done(Do::Undo);
            }
        }
        // A click in the viewport selected something.
        let picked = self.viewport.selected.as_ref().map(|s| s.id.clone());
        if picked != self.selected && picked.is_some() {
            self.select(picked);
        }
    }

    fn viewport_ui(&mut self, ui: &mut egui::Ui) {
        let up = self.link.is_up();
        let playing = self.status["playing"] == true && self.status["paused"] != true;
        for s in self.viewport.ui(ui, up, playing) {
            match s {
                Send::Stream(w, h) => self.link.send(
                    "render.stream",
                    json!({ "width": w, "height": h, "fps": 60, "audio": self.audio }),
                    Want::Stream,
                ),
                Send::Input(events, text) => {
                    let mut p = json!({ "events": events });
                    if !text.is_empty() {
                        p["text"] = json!(text);
                    }
                    self.link.send("input.device", p, Want::Input);
                }
                Send::Pick(at, w, h) => self.link.send(
                    "obs.frame",
                    json!({ "format": "semantic", "width": w, "height": h, "limit": 500 }),
                    Want::Pick(at),
                ),
            }
        }
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

    /// The left panel: the scene in a line, then its hierarchy.
    fn overview(&mut self, ui: &mut egui::Ui) {
        let s = &self.summary;
        if s.is_null() {
            ui.weak("waiting for the world");
            return;
        }
        ui.label(format!(
            "{}: {} entities, {} authored, seed {}",
            s["scene"].as_str().unwrap_or("-"),
            s["entities"],
            s["authored"],
            s["seed"],
        ))
        .on_hover_text(format!("systems: {}", list(&s["systems"])));
        ui.separator();
        let comps: Vec<String> = self.inspector.schemas.keys().cloned().collect();
        let selected = self.selected.clone();
        let (picked, asks) = self.hierarchy.ui(ui, selected.as_deref(), &comps);
        for ask in asks {
            let (method, params) = match &ask {
                Ask::Top(c, n) => ("world.query", self.hierarchy.top_query(c.as_deref(), *n)),
                Ask::Children(p) => ("world.query", self.hierarchy.children_query(p)),
                Ask::Parents => ("world.query", Hierarchy::parents_query()),
                Ask::Rooms => ("scene.rooms", json!({})),
            };
            self.link.send(method, params, Want::Hier(ask));
        }
        if picked.is_some() {
            self.select(picked);
        }
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
