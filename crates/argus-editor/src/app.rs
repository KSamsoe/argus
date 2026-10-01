//! The editor's window (§11): panels over protocol calls. Everything a
//! person does here is a call the CLI makes the same way (P1), and "show
//! as command" prints each one as the `argus` line that makes it, "so the
//! human learns the agent's language by using the editor and can paste a
//! command into a chat instead of describing a click".

use crate::agent::Agent;
use crate::animator::Animator;
use crate::hierarchy::{Ask, Hierarchy};
use crate::inspector::{Do, Inspector};
use crate::link::{Answer, Link, State, Want};
use crate::profile::Profile;
use crate::scrubber::{Scrub, Scrubber};
use crate::tiles::{TileDo, Tiles};
use crate::timeline::{Timelines, TlDo};
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
/// How often the agent panel reads the questions, proposals, and marks
/// (the bar counts them, so also while it is not shown).
const AGENT_EVERY: Duration = Duration::from_secs(1);
/// How often the scrubber reads the ring, and the events it marks.
const RING_EVERY: Duration = Duration::from_millis(250);
const MARKERS_EVERY: Duration = Duration::from_secs(1);
/// How often the scrubber lists the saved replays.
const REPLAYS_EVERY: Duration = Duration::from_secs(3);
/// How often the profiler view reads the ticks, and the rest, while shown.
const PROF_TICKS_EVERY: Duration = Duration::from_millis(500);
const PROF_EVERY: Duration = Duration::from_secs(2);
/// How often the Tiles tab reads the maps placed, and what bodies hit.
const TILES_EVERY: Duration = Duration::from_secs(1);
const COLLISION_EVERY: Duration = Duration::from_millis(400);
/// How often the Animator tab reads its machine, and what the entity fired.
const SIDECAR_EVERY: Duration = Duration::from_secs(1);
const FIRED_EVERY: Duration = Duration::from_millis(300);
/// How often the Timeline tab reads the timelines and where they play.
const TIMELINES_EVERY: Duration = Duration::from_millis(250);

/// The right panel's tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    /// The selected entity.
    Inspector,
    /// The tune fields' sliders.
    Tuning,
    /// What the agent waits on the person for, and what it did.
    Agent,
    /// Where the time goes, beside §17's targets.
    Profile,
    /// Painting tilemaps.
    Tiles,
    /// The selected entity's animator.
    Animator,
    /// Timelines: tracks against time.
    Timeline,
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
    /// Review requests, proposals, marks.
    pub agent: Agent,
    /// The change feed shows only this author's rows.
    pub feed_author: Option<String>,
    /// The replay scrubber under the viewport.
    pub scrubber: Scrubber,
    /// The profiler view.
    pub profile: Profile,
    /// Tilemap painting.
    pub tiles: Tiles,
    /// The animator editor.
    pub animator: Animator,
    /// The timeline editor.
    pub timelines: Timelines,
    txs: HashMap<u64, TxRun>,
    described: bool,
    last_status: Option<Instant>,
    last_summary: Option<Instant>,
    last_get: Option<Instant>,
    last_locks: Option<Instant>,
    last_tune: Option<Instant>,
    last_agent: Option<Instant>,
    last_ring: Option<Instant>,
    last_markers: Option<Instant>,
    last_replays: Option<Instant>,
    last_prof_ticks: Option<Instant>,
    last_prof: Option<Instant>,
    last_capture: Option<Instant>,
    last_tiles: Option<Instant>,
    last_collision: Option<Instant>,
    last_sidecar: Option<Instant>,
    last_fired: Option<Instant>,
    last_timelines: Option<Instant>,
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
            agent: Agent::default(),
            feed_author: None,
            scrubber: Scrubber::default(),
            profile: Profile::default(),
            tiles: Tiles::default(),
            animator: Animator::default(),
            timelines: Timelines::default(),
            txs: HashMap::new(),
            described: false,
            last_status: None,
            last_summary: None,
            last_get: None,
            last_locks: None,
            last_tune: None,
            last_agent: None,
            last_ring: None,
            last_markers: None,
            last_replays: None,
            last_prof_ticks: None,
            last_prof: None,
            last_capture: None,
            last_tiles: None,
            last_collision: None,
            last_sidecar: None,
            last_fired: None,
            last_timelines: None,
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
    pub fn edit(&mut self, note: &str, ops: Vec<Json>) -> String {
        let line = command::line(
            "edit",
            &json!({ "author": self.author, "note": note, "ops": ops }),
        );
        self.commands.push(line.clone());
        self.link.send(
            "edit.begin",
            json!({ "author": self.author, "note": note }),
            Want::EditBegin(line.clone(), ops),
        );
        line
    }

    /// Where the frame on screen is, and what it shows.
    pub fn mapping(&self) -> Option<crate::tiles::Mapping> {
        crate::tiles::Mapping::new(
            self.viewport.image_rect?,
            self.viewport.info?,
            self.viewport.view.as_ref()?,
        )
    }

    fn tile_do(&mut self, d: TileDo) {
        match d {
            TileDo::Edit(note, ops) => {
                let line = self.edit(&note, ops);
                self.tiles.sent(line);
            }
            TileDo::Pick(p) => self.link.send("scene.tiles.get", p, Want::TilePick),
            TileDo::Create(create, note, ops) => {
                self.act("scene.tiles.create", create);
                self.edit(&note, ops);
                self.last_tiles = None;
            }
        }
    }

    fn timeline_do(&mut self, d: TlDo) {
        match d {
            TlDo::Edit(note, ops) => {
                self.edit(&note, ops);
            }
            TlDo::Play(p) => {
                self.act("timeline.play", p);
                self.last_timelines = None;
            }
            TlDo::Pause => self.act("sim.pause", json!({})),
            TlDo::Seek(t) => {
                if let Some(t) = self.timelines.want_seek(t) {
                    let p = self.timelines.seek_params(t);
                    self.link.send("timeline.seek", p, Want::TimelineSeek);
                }
            }
            TlDo::Let(t, resume) => {
                // The time let go at is the command the person gave (the
                // daemon's feed keeps one row for the drag too).
                self.commands.push(command::line(
                    "timeline.seek",
                    &self.timelines.seek_params(t),
                ));
                if resume {
                    self.act("sim.play", json!({}));
                }
            }
        }
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
            Do::Edit(note, ops) => {
                self.edit(&note, ops);
            }
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
            } else if method.starts_with("review.") {
                // Asked or answered: the queue, again.
                self.last_agent = None;
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
        if due(self.last_ring, RING_EVERY) && !self.link.waiting(&Want::Ring) {
            self.last_ring = Some(now);
            self.link.send("sim.ring", json!({}), Want::Ring);
        }
        if due(self.last_markers, MARKERS_EVERY) {
            self.last_markers = Some(now);
            for name in self.scrubber.event_names() {
                let want = Want::Events(name.clone());
                if !self.link.waiting(&want) {
                    let since = self.scrubber.since(&name);
                    self.link.send(
                        "obs.events",
                        json!({ "name": name, "since": since, "limit": 500 }),
                        want,
                    );
                }
            }
        }
        if due(self.last_replays, REPLAYS_EVERY) && !self.link.waiting(&Want::Replays) {
            self.last_replays = Some(now);
            self.link.send("replay.list", json!({}), Want::Replays);
        }
        // The profiler view's answers while it is shown (and a capture's
        // progress until it is written).
        if self.tab == Tab::Profile && due(self.last_prof_ticks, PROF_TICKS_EVERY) {
            self.last_prof_ticks = Some(now);
            for (method, params, want) in [
                (
                    "prof.systems",
                    json!({ "last": 600, "series": true }),
                    Want::ProfSystems,
                ),
                ("prof.targets", json!({ "last": 600 }), Want::ProfTargets),
            ] {
                if !self.link.waiting(&want) {
                    self.link.send(method, params, want);
                }
            }
        }
        if self.tab == Tab::Profile && due(self.last_prof, PROF_EVERY) {
            self.last_prof = Some(now);
            for (method, params, want) in [
                ("prof.alloc", json!({ "last": 600 }), Want::ProfAlloc),
                ("prof.worst", json!({ "n": 5 }), Want::ProfWorst),
                ("prof.calls", json!({}), Want::ProfCalls),
                ("prof.archetypes", json!({}), Want::ProfArchetypes),
            ] {
                if !self.link.waiting(&want) {
                    self.link.send(method, params, want);
                }
            }
        }
        if self.profile.capturing()
            && due(self.last_capture, PROF_TICKS_EVERY)
            && !self.link.waiting(&Want::ProfCapture)
        {
            self.last_capture = Some(now);
            self.link.send("prof.capture", json!({}), Want::ProfCapture);
        }
        // The Tiles tab: the maps placed, their tilesets, what bodies hit on
        // screen, and the cell under the pointer.
        if self.tab == Tab::Tiles {
            if due(self.last_tiles, TILES_EVERY) && !self.link.waiting(&Want::TileMaps) {
                self.last_tiles = Some(now);
                self.link
                    .send("scene.tiles.list", json!({}), Want::TileMaps);
                if !self.link.waiting(&Want::Sprites) {
                    self.link.send(
                        "asset.list",
                        json!({ "kind": "sprite", "limit": 500 }),
                        Want::Sprites,
                    );
                }
            }
            for name in self.tiles.wanted_tilesets() {
                self.link.send(
                    "asset.get",
                    json!({ "name": name }),
                    Want::TileAsset(name.clone()),
                );
            }
            let visible = self.mapping().and_then(|m| self.tiles.visible_rect(&m));
            if let (true, Some(r), Some(file)) =
                (self.tiles.show_collision, visible, self.tiles.map.clone())
            {
                if due(self.last_collision, COLLISION_EVERY)
                    && !self.link.waiting(&Want::TileCollision)
                {
                    self.last_collision = Some(now);
                    self.link.send(
                        "scene.tiles.collision",
                        json!({ "file": file, "rect": { "x": r[0], "y": r[1], "w": r[2], "h": r[3] } }),
                        Want::TileCollision,
                    );
                }
            }
            if let (Some(c), None, Some(file), Some(layer)) = (
                self.tiles.hover,
                self.tiles.hover_tile,
                self.tiles.map.clone(),
                self.tiles.layer.clone(),
            ) {
                let want = Want::TileAt(c);
                if !self.link.waiting(&want) {
                    self.link.send(
                        "scene.tiles.get",
                        json!({ "file": file, "layer": layer, "rect": { "x": c[0], "y": c[1], "w": 1, "h": 1 } }),
                        want,
                    );
                }
            }
        }
        // The Animator tab: the machine, its sprite's clips, and what the
        // entity fired.
        if self.tab == Tab::Animator {
            if due(self.last_sidecar, SIDECAR_EVERY) {
                self.last_sidecar = Some(now);
                if let Some(m) = self.animator.machine.clone() {
                    let want = Want::Sidecar(m.clone());
                    if !self.link.waiting(&want) {
                        self.link.send("asset.sidecar", json!({ "name": m }), want);
                    }
                }
                if !self.link.waiting(&Want::Animators) {
                    self.link.send(
                        "asset.list",
                        json!({ "kind": "animator", "limit": 500 }),
                        Want::Animators,
                    );
                }
                if let (Some(s), true) =
                    (self.animator.sprite.clone(), self.animator.clips.is_null())
                {
                    let want = Want::AnimClips(s.clone());
                    if !self.link.waiting(&want) {
                        self.link.send("asset.get", json!({ "name": s }), want);
                    }
                }
            }
            if let Some(e) = self.animator.entity.clone() {
                let want = Want::AnimEvents(e.clone());
                if due(self.last_fired, FIRED_EVERY) && !self.link.waiting(&want) {
                    self.last_fired = Some(now);
                    self.link.send(
                        "obs.events",
                        json!({ "entity": e, "since": self.animator.since, "limit": 200 }),
                        want,
                    );
                }
            }
        }
        if self.tab == Tab::Timeline
            && due(self.last_timelines, TIMELINES_EVERY)
            && !self.link.waiting(&Want::TimelineList)
        {
            self.last_timelines = Some(now);
            self.link
                .send("timeline.list", json!({}), Want::TimelineList);
        }
        if due(self.last_agent, AGENT_EVERY) {
            self.last_agent = Some(now);
            for (method, want) in [
                ("review.list", Want::Reviews),
                ("proposal.list", Want::Proposals),
                ("mark.list", Want::Marks),
            ] {
                if !self.link.waiting(&want) {
                    self.link.send(method, json!({}), want);
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
                // A stroke landed: the game draws it now.
                self.tiles.landed(&line);
                // An animator's edit: its file again; a timeline's, its rows.
                self.last_sidecar = None;
                self.last_timelines = None;
                self.tiles.hover_tile = None;
                // What changed shows at once, not at the next poll.
                self.last_status = None;
                self.last_summary = None;
                self.last_get = None;
                self.last_locks = None;
                self.last_tune = None;
                self.last_agent = None;
                self.last_ring = None;
                self.last_markers = None;
                self.hierarchy.stale();
            }
            (Want::Action(line), Err(e)) => {
                self.tiles.landed(&line);
                self.problem = Some(Problem::from_error(&line, &e));
            }
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
                self.tiles.landed(&line);
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
            (Want::Reviews, Ok(v)) => self.agent.take_reviews(&v),
            (Want::Proposals, Ok(v)) => self.agent.proposals = v,
            (Want::Marks, Ok(v)) => self.agent.take_marks(&v),
            (Want::Ring, Ok(v)) => self.scrubber.take_ring(v),
            (Want::ProfTargets, Ok(v)) => self.profile.targets = v,
            (Want::ProfSystems, Ok(v)) => self.profile.systems = v,
            (Want::ProfAlloc, Ok(v)) => self.profile.alloc = v,
            (Want::ProfWorst, Ok(v)) => self.profile.worst = v,
            (Want::ProfCalls, Ok(v)) => self.profile.calls = v,
            (Want::ProfArchetypes, Ok(v)) => self.profile.archetypes = v,
            (Want::ProfCapture, Ok(v)) => self.profile.take_capture(v),
            (Want::TileMaps, Ok(v)) => self.tiles.take_maps(&v),
            (Want::TileAsset(name), Ok(v)) => self.tiles.take_tileset(&name, &v),
            (Want::TileCollision, Ok(v)) => self.tiles.collision = v,
            (Want::TileAt(c), Ok(v)) => {
                if self.tiles.hover == Some(c) {
                    self.tiles.hover_tile = v["cells"][0].as_u64();
                }
            }
            (Want::TilePick, Ok(v)) => self.tiles.take_pick(&v),
            (Want::Sidecar(m), Ok(v)) => {
                if self.animator.machine.as_deref() == Some(m.as_str()) {
                    self.animator.sidecar = v;
                }
            }
            (Want::AnimClips(s), Ok(v)) => {
                if self.animator.sprite.as_deref() == Some(s.as_str()) {
                    self.animator.clips = v;
                }
            }
            (Want::AnimEvents(e), Ok(v)) => {
                if self.animator.entity.as_deref() == Some(e.as_str()) {
                    self.animator.take_events(&v);
                }
            }
            (Want::Animators, Ok(v)) => {
                self.animator.machines = v["rows"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|r| r[0].as_str().map(str::to_string))
                    .collect();
            }
            (
                Want::Sidecar(_) | Want::AnimClips(_) | Want::AnimEvents(_) | Want::Animators,
                Err(_),
            ) => {}
            (Want::Sprites, Ok(v)) => {
                self.tiles.sprites = v["rows"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|r| {
                        r[0].as_str()
                            .or_else(|| r["name"].as_str())
                            .map(str::to_string)
                    })
                    .collect();
            }
            (Want::TileAt(_) | Want::TileCollision | Want::TileMaps | Want::Sprites, Err(_)) => {}
            (Want::TimelineList, Ok(v)) => self.timelines.take(v),
            (Want::TimelineList, Err(_)) => {}
            (Want::TimelineSeek, r) => {
                match &r {
                    Ok(_) => {
                        self.last_status = None;
                        self.last_ring = None;
                        self.last_timelines = None;
                        self.hierarchy.stale();
                        self.last_get = None;
                    }
                    Err(e) => {
                        let line = command::line("timeline.seek", &json!({}));
                        self.problem = Some(Problem::from_error(&line, e));
                    }
                }
                if let Some(t) = self.timelines.seek_answered(r.as_ref().ok()) {
                    let p = self.timelines.seek_params(t);
                    self.link.send("timeline.seek", p, Want::TimelineSeek);
                }
            }
            (Want::Replays, Ok(v)) => self.scrubber.replays = v,
            (Want::Events(name), Ok(v)) => self.scrubber.take_events(&name, &v),
            (Want::Seek, r) => {
                match r {
                    Ok(v) => {
                        self.status = v;
                        self.last_ring = None;
                        self.hierarchy.stale();
                        self.last_get = None;
                    }
                    Err(e) => {
                        let line = command::line("sim.seek", &json!({}));
                        self.problem = Some(Problem::from_error(&line, &e));
                    }
                }
                if let Some(t) = self.scrubber.seek_answered() {
                    let p = self.scrubber.seek_params(t);
                    self.link.send("sim.seek", p, Want::Seek);
                }
            }
            // No project open yet: nothing to list.
            (
                Want::Reviews
                | Want::Proposals
                | Want::Marks
                | Want::Ring
                | Want::Replays
                | Want::Events(_)
                | Want::ProfTargets
                | Want::ProfSystems
                | Want::ProfAlloc
                | Want::ProfWorst
                | Want::ProfCalls
                | Want::ProfArchetypes,
                Err(_),
            ) => {}
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
                self.tiles.landed(&t.line);
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
        // A row announced again (a drag's seeks are one row, its latest).
        if let Some(r) = self
            .feed
            .iter_mut()
            .find(|r| id.is_some() && r["id"].as_u64() == id)
        {
            *r = row;
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
                    let n = self.agent.waiting();
                    let agent = if n == 0 {
                        "Agent".to_string()
                    } else {
                        format!("Agent ({n})")
                    };
                    ui.selectable_value(&mut self.tab, Tab::Agent, agent);
                    ui.selectable_value(&mut self.tab, Tab::Profile, "Profile");
                    ui.selectable_value(&mut self.tab, Tab::Tiles, "Tiles");
                    ui.selectable_value(&mut self.tab, Tab::Animator, "Animator");
                    ui.selectable_value(&mut self.tab, Tab::Timeline, "Timeline");
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
                    Tab::Agent => {
                        let author = self.author.clone();
                        for (method, params) in self.agent.ui(ui, &author, &self.feed) {
                            self.act(&method, params);
                        }
                    }
                    Tab::Profile => {
                        let discrete = self.status["driver"] == "discrete";
                        for (method, params) in self.profile.ui(ui, discrete) {
                            if method == "prof.capture" {
                                // Its answer says where the file will be.
                                self.commands.push(command::line(&method, &params));
                                self.link.send(&method, params, Want::ProfCapture);
                            } else {
                                self.act(&method, params);
                                self.last_ring = None;
                            }
                        }
                    }
                    Tab::Tiles => {
                        let scene = self.status["scene"].as_str().unwrap_or("scene").to_string();
                        let rooms = self.hierarchy.rooms.clone();
                        let top_left = self.mapping().map(|m| m.world_of_px([0.0, 0.0]));
                        for d in self.tiles.ui(ui, &scene, &rooms, top_left) {
                            self.tile_do(d);
                        }
                    }
                    Tab::Animator => {
                        let comps = self.inspector.got["components"].clone();
                        self.animator.take_entity(self.selected.as_deref(), &comps);
                        for (note, ops) in self.animator.ui(ui) {
                            self.edit(&note, ops);
                        }
                    }
                    Tab::Timeline => {
                        let playing =
                            self.status["playing"] == true && self.status["paused"] != true;
                        let selected = self.selected.clone();
                        let asks = egui::ScrollArea::vertical()
                            .id_salt("timeline")
                            .auto_shrink([false, false])
                            .show(ui, |ui| self.timelines.ui(ui, selected.as_deref(), playing))
                            .inner;
                        for d in asks {
                            self.timeline_do(d);
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
        egui::TopBottomPanel::bottom("scrubber").show(ctx, |ui| self.scrubber_ui(ui));
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
            // The painter's tools, a key each, while the Tiles tab shows.
            if self.tab == Tab::Tiles {
                use crate::tiles::{Brush, Tool};
                let keys = ctx.input_mut(|i| {
                    [egui::Key::B, egui::Key::R, egui::Key::I, egui::Key::E]
                        .map(|k| i.consume_key(egui::Modifiers::NONE, k))
                });
                if keys[0] {
                    self.tiles.tool = Tool::Pencil;
                }
                if keys[1] {
                    self.tiles.tool = Tool::Rect;
                }
                if keys[2] {
                    self.tiles.tool = Tool::Pick;
                }
                if keys[3] {
                    self.tiles.brush = Brush::Erase;
                }
            }
            // A review's choices, a key each, while the question shows.
            if self.tab == Tab::Agent {
                const KEYS: [egui::Key; 9] = [
                    egui::Key::Num1,
                    egui::Key::Num2,
                    egui::Key::Num3,
                    egui::Key::Num4,
                    egui::Key::Num5,
                    egui::Key::Num6,
                    egui::Key::Num7,
                    egui::Key::Num8,
                    egui::Key::Num9,
                ];
                let n = ctx.input_mut(|i| {
                    KEYS.iter()
                        .position(|k| i.consume_key(egui::Modifiers::NONE, *k))
                });
                if let Some((method, params)) = n.and_then(|n| self.agent.choice(n + 1)) {
                    self.act(&method, params);
                }
            }
        }
        // A click in the viewport selected something.
        let picked = self.viewport.selected.as_ref().map(|s| s.id.clone());
        if picked != self.selected && picked.is_some() {
            self.select(picked);
        }
    }

    fn scrubber_ui(&mut self, ui: &mut egui::Ui) {
        if !self.link.is_up() {
            return;
        }
        // The tick on screen: the viewport's latest frame, else the clock's.
        let now = self
            .viewport
            .info
            .as_ref()
            .map(|i| i.tick)
            .or(self.status["tick"].as_u64())
            .unwrap_or(0);
        self.scrubber
            .settle(self.status["tick"].as_u64().unwrap_or(now));
        let playing = self.status["playing"] == true && self.status["paused"] != true;
        let free = !self.viewport.focused && ui.ctx().memory(|m| m.focused().is_none());
        for s in self.scrubber.ui(ui, now, playing, free) {
            match s {
                Scrub::Pause => self.act("sim.pause", json!({})),
                Scrub::Seek(t) => {
                    if let Some(t) = self.scrubber.want_seek(t) {
                        let p = self.scrubber.seek_params(t);
                        self.link.send("sim.seek", p, Want::Seek);
                    }
                }
                Scrub::Let(t, resume) => {
                    // The position let go at is the command the person gave
                    // (the daemon's feed keeps one row for the drag too).
                    self.commands
                        .push(command::line("sim.seek", &self.scrubber.seek_params(t)));
                    if resume {
                        self.act("sim.play", json!({}));
                    }
                }
                Scrub::Act(method, params) => {
                    self.act(&method, params);
                    self.last_ring = None;
                    self.last_markers = None;
                }
            }
        }
    }

    fn viewport_ui(&mut self, ui: &mut egui::Ui) {
        let up = self.link.is_up();
        let playing = self.status["playing"] == true && self.status["paused"] != true;
        // The Tiles tab with a map and a layer: the pointer paints.
        self.viewport.paint = up && self.tab == Tab::Tiles && self.tiles.ready();
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
                Send::Paint(p) => {
                    if let Some(m) = self.mapping() {
                        for d in self.tiles.on_paint(p, &m) {
                            self.tile_do(d);
                        }
                    }
                }
            }
        }
        if self.tab == Tab::Tiles {
            if let Some(m) = self.mapping() {
                self.tiles.overlay(ui, &m);
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
            // What the agent waits on the person for, one click away.
            if let Some(line) = self.agent.waiting_line() {
                ui.separator();
                if ui
                    .button(
                        RichText::new(format!("The agent asks: {line}"))
                            .color(Color32::from_rgb(240, 170, 60)),
                    )
                    .on_hover_text("Show the agent panel")
                    .clicked()
                {
                    self.tab = Tab::Agent;
                }
            }
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
        // Only one author's rows: what the agent did while you were away.
        let mut authors: Vec<String> = self
            .feed
            .iter()
            .filter_map(|r| r["author"].as_str().map(str::to_string))
            .collect();
        authors.sort();
        authors.dedup();
        ui.horizontal(|ui| {
            ui.label("By");
            egui::ComboBox::from_id_salt("feed-author")
                .selected_text(self.feed_author.as_deref().unwrap_or("everyone"))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.feed_author, None, "everyone");
                    for a in authors {
                        let label = a.clone();
                        ui.selectable_value(&mut self.feed_author, Some(a), label);
                    }
                });
        });
        ui.separator();
        let only = self.feed_author.clone();
        egui::ScrollArea::vertical()
            .id_salt("feed")
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for r in self
                    .feed
                    .iter()
                    .filter(|r| only.as_deref().is_none_or(|a| r["author"] == a))
                {
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
