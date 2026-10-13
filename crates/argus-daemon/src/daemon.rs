//! The daemon core: one world, one scheduler, one script host, one scene,
//! and the dispatch of every protocol method. Transport-agnostic: the
//! server feeds it request lines and drains its outbox of notifications.

use crate::codes;
use crate::docs;
use crate::edit::{Commit, CommitKind, Editing, Op};
use crate::json::{self, bad_params};
use crate::query;
use crate::rpc;
use argus_ecs::{ArgusError, Entity, Event, EventRecord, Result, Value, World};
use argus_index::shapes::{overrides_json, resolved_json, Summary};
use argus_index::{Index, SyncReport};
use argus_input::InputState;
use argus_luau::ScriptHost;
use argus_scene::{Bulk, DefSchema, EntityDoc, Project, ProjectConfig, Rect, Scene};
use argus_sim::{ClockKind, Scheduler, SchedulerConfig};
use serde_json::{json, Map, Value as Json};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[path = "jobs.rs"]
mod jobs;
#[path = "marks.rs"]
mod marks;
#[path = "nav.rs"]
mod nav;
#[path = "perf.rs"]
mod perf;
#[path = "proposals.rs"]
mod proposals;
#[path = "reviews.rs"]
mod reviews;
#[path = "saves.rs"]
mod saves;
#[path = "sidecar.rs"]
mod sidecar;
#[path = "strings.rs"]
mod strings;
#[path = "timeline.rs"]
mod timeline;
#[path = "tuning.rs"]
mod tuning;

/// Identifies a client connection for subscriptions.
pub type ConnId = u64;

/// The most a file edit writes or `project.read` gives: a script or a
/// scene file, not an asset.
const FILE_MAX: usize = 2 * 1024 * 1024;

/// The engine's version: the workspace's, one for the CLI, the daemon, the
/// editor, and the game runtimes (distribution item 0).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The source this was built from: the commit (with `-dirty` for
/// uncommitted changes) for a staged release, which
/// `tools/release/stage.sh` sets as `ARGUS_REVISION`, else "dev".
pub const REVISION: &str = match option_env!("ARGUS_REVISION") {
    Some(r) => r,
    None => "dev",
};

/// The version, the revision, and the platform, as `argus version --json`
/// says them.
pub fn version_json() -> Json {
    json!({
        "version": VERSION,
        "revision": REVISION,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
    })
}

struct Session {
    root: PathBuf,
    project: Project,
    /// The scene, shared with `rooms.stream` (phase 3 item 6).
    scene: Rc<RefCell<Scene>>,
    scene_name: String,
    host: ScriptHost,
    /// The project's Rust plugins (phase 5 item 6), and the names
    /// `argus.toml` lists, in its order.
    plugins: argus_plugins::Plugins,
    plugin_names: Vec<String>,
    seed: u64,
    /// The world was instantiated (no issues).
    loaded: bool,
    /// `.argus/index.db`, when it could be opened.
    index: Option<Index>,
    index_error: Option<String>,
    /// Injected input and the log; `None` when the project has no input.toml.
    input: Option<Rc<RefCell<InputState>>>,
    /// Where `input.record` writes, if on.
    run_dir: Option<PathBuf>,
    /// The asset importer and its catalog (phase 1, item 4).
    assets: argus_assets::Importer,
    /// Open tilemaps (phase 1, item 5), shared with the physics step so a
    /// tile edit is ground on the next tick (item 6).
    tiles: Rc<RefCell<argus_tiles::MapStore>>,
    /// Navigation grids and the searches made on them (phase 3 item 2).
    nav: Rc<RefCell<argus_nav::NavStore>>,
    /// Animator machines and sprite clips (item 8), refreshed on import.
    anims: Rc<RefCell<argus_anim::AnimStore>>,
    /// Particle effects and every emitter's pool (item 10), refreshed on
    /// import and drawn by the renderer.
    particles: Rc<RefCell<argus_particles::ParticleStore>>,
    /// Baselines for obs.diff and traces for obs.trace (item 13), fed by
    /// a scheduler observer at the end of every tick.
    history: Rc<RefCell<crate::history::History>>,
    /// The mods laid over the game, those left out and why, and what they
    /// changed (phase 3 item 8).
    mods: argus_player::mods::Report,
    /// Keyframes for `sim.seek` (phase 2 item 0), fed at the end of ticks.
    ring: Rc<RefCell<crate::ring::Ring>>,
    /// What a keyframe captures besides the world and the scheduler.
    sources: crate::ring::Sources,
    /// The tick `sim.play` started from, which `sim.stop` goes back to.
    play_from: Option<u64>,
    /// What was committed while playing, in order, by whom: `sim.stop`
    /// puts the world back to where play started and then makes these
    /// again, world only, since their files already have them.
    kept_in_play: Vec<(String, Vec<Op>)>,
    /// The key that drops a mark (`[marks] key`, F8 by default).
    mark_key: String,
    /// Keyframes of the ticks that asked for a save (`q:save`), by the
    /// save's name, written once the step is over (phase 3 item 0).
    saves: Rc<RefCell<Vec<(String, crate::ring::Keyframe)>>>,
    /// The UI's theme and fonts (phase 3 item 1), and the string tables
    /// (item 4).
    ui: Rc<RefCell<argus_ui::Shared>>,
    /// The dialogue graph (phase 3 item 4).
    dialogue: Rc<RefCell<argus_dialogue::Graph>>,
    /// The timelines (phase 3 item 5).
    timelines: Rc<RefCell<argus_timeline::Timelines>>,
}

#[derive(Default)]
struct EventLog {
    next_seq: u64,
    records: VecDeque<(u64, EventRecord)>,
    /// Beside each record: whether a tick emitted it (a seek to frame T
    /// drops what ticks from T on emitted, and keeps a change made at T
    /// between ticks).
    ticked: VecDeque<bool>,
    /// Ticks before this frame are in the log on this branch of the run (a
    /// seek back cuts it; ticks run from there extend it again).
    through: u64,
    cap: usize,
}

/// One world and everything that runs it: the main one, or a fork
/// (`sim.fork`, phase 2 item 0), parked until a call names it.
struct Slot {
    world: World,
    sched: Scheduler,
    session: Option<Session>,
    events: EventLog,
}

impl Slot {
    fn empty() -> Slot {
        Slot {
            world: World::new(),
            sched: Scheduler::new(SchedulerConfig::default()),
            session: None,
            events: EventLog {
                next_seq: 1,
                cap: 200_000,
                ..Default::default()
            },
        }
    }
}

/// The daemon.
pub struct Daemon {
    /// The one world.
    pub world: World,
    /// Its scheduler.
    pub sched: Scheduler,
    session: Option<Session>,
    edits: Editing,
    events: EventLog,
    subs: BTreeMap<ConnId, BTreeSet<String>>,
    /// Who each connection said it is (`session.identify`, phase 4): the
    /// author of its calls in the change feed.
    identities: BTreeMap<ConnId, String>,
    /// The connections open now (the server's), so a client can tell
    /// whether it is the last before it stops a daemon it started.
    conns: std::collections::BTreeSet<ConnId>,
    /// The call a client is making, while it runs: its `argus` line and
    /// author, for the feed row it makes (phase 4).
    calling: Option<(String, String)>,
    /// Feed rows made so far: a call that made none gets a command row.
    feed_rows: u64,
    /// Where the armed `prof.capture` writes when its last tick is in.
    capture_path: Option<PathBuf>,
    /// `session.shutdown` was called: the server should stop after replying.
    pub shutdown: bool,
    /// The events being pumped came from ticks.
    from_ticks: bool,
    /// Worlds parked beside this one, by name (`sim.fork`).
    forks: BTreeMap<String, Slot>,
    /// The name of the world in `world`: "main" until a switch.
    active: String,
    /// `open_scene` is building a fork: no audio engine of its own, no index
    /// sync, no window catalog.
    building_fork: bool,
    /// Edits change the live world and never the project's files: set
    /// while `test.run` runs scenarios, since a test that places the hero
    /// must not move the hero in the scene the game ships (found by the web
    /// spike: Hollow's planks scenario had rewritten player.toml).
    live_edits: bool,
    /// While `sim.stop` makes again what was kept in play: with
    /// `live_edits`, an authored entity spawned in play comes from its
    /// scene document, which already has it.
    replaying_kept: bool,
    /// The window was opened from the command line (`--window`): it is the
    /// person's, and closing it ends the daemon. A window an agent opens
    /// with `render.window` only closes.
    pub exit_on_window_close: bool,
    /// Bumped by every change to the world made outside a tick (commits,
    /// undo, scene load); `ensure_broadphase` rebuilds when it moved.
    mutations: u64,
    broadphase_epoch: u64,
    /// The renderer's targets (phase 1, item 1).
    #[cfg(feature = "render")]
    render: render::RenderState,
    /// Raw device state resolved through the action map (phase 1, item 2).
    devices: argus_input::Devices,
    /// The mixer (phase 1, item 3); rebuilt with every scene.
    #[cfg(feature = "audio")]
    audio: Option<argus_audio::AudioEngine>,
    /// Why the mixer could not load, when it could not.
    audio_error: Option<Json>,
    /// Turns taken by device commands on a discrete scene.
    device_turns: u64,
    /// The note being typed in the window after F8 (phase 2 item 2).
    mark_prompt: Option<marks::Prompt>,
    /// A line the window shows for a moment, and until when.
    toast: Option<(String, std::time::Instant)>,
    /// The proposal being tried, A/B by feel (phase 2 item 4).
    trial: Option<proposals::Trial>,
    /// A save a tick asked to load (`q:load`), done after the step.
    load_request: Option<String>,
    /// A trial's parked world is catching up: no sound, no notifications.
    backstage: bool,
    /// The tuning session open, if any (phase 2 item 5).
    tuning: Option<tuning::TuneSession>,
    /// A tuning session's own commit is running: its rows are the session's.
    tune_committing: bool,
    /// Notifications waiting for the transport, per connection.
    pub outbox: Vec<(ConnId, Json)>,
    /// Clients' calls by method: how long they took (`prof.calls`, and the
    /// §17 rows `prof.targets` measures from them; phase 4 item 6).
    calls: BTreeMap<String, perf::CallStat>,
    call_seq: u64,
    /// Calls in progress: only the outermost is timed (the harness's
    /// calls inside `test.run` are the test's).
    call_depth: u32,
    /// The last `sim.run_until`: ticks, and the ms it took.
    fast_forward: Option<(u64, f64)>,
    /// The mods the session asked for, in load order (phase 3 item 8):
    /// `session.open {mods}`, kept by `scene.load` and forks, set by a
    /// replay, a mark, or a save from the mods it recorded.
    mods_asked: Vec<String>,
    /// The platform the game's requests go to (phase 5 item 9): the null
    /// one, its cloud a folder (`.argus/platform/cloud`), kept across scene
    /// loads as a player's account is.
    platform: argus_platform::NullPlatform,
    /// `platform.toml`.
    platform_cfg: argus_platform::PlatformConfig,
    /// The project the platform is for.
    platform_root: Option<PathBuf>,
    /// Where the web client is served, when it is (phase 5 item 10).
    web: Option<std::net::SocketAddr>,
}

impl Default for Daemon {
    fn default() -> Self {
        Self::new()
    }
}

impl Daemon {
    /// An empty daemon: no project, an empty world.
    pub fn new() -> Daemon {
        Daemon {
            world: World::new(),
            sched: Scheduler::new(SchedulerConfig::default()),
            session: None,
            edits: Editing::default(),
            identities: BTreeMap::new(),
            conns: std::collections::BTreeSet::new(),
            calling: None,
            feed_rows: 0,
            events: EventLog {
                next_seq: 1,
                records: VecDeque::new(),
                ticked: VecDeque::new(),
                through: 0,
                cap: 200_000,
            },
            from_ticks: false,
            forks: BTreeMap::new(),
            active: "main".to_string(),
            building_fork: false,
            live_edits: false,
            replaying_kept: false,
            subs: BTreeMap::new(),
            capture_path: None,
            shutdown: false,
            exit_on_window_close: false,
            mutations: 0,
            broadphase_epoch: 0,
            #[cfg(feature = "render")]
            render: render::RenderState::default(),
            devices: argus_input::Devices::default(),
            #[cfg(feature = "audio")]
            audio: None,
            audio_error: None,
            device_turns: 0,
            mark_prompt: None,
            toast: None,
            trial: None,
            load_request: None,
            backstage: false,
            tuning: None,
            tune_committing: false,
            outbox: Vec::new(),
            calls: BTreeMap::new(),
            call_seq: 0,
            call_depth: 0,
            fast_forward: None,
            mods_asked: Vec::new(),
            platform: argus_platform::NullPlatform::new(),
            platform_cfg: argus_platform::PlatformConfig::default(),
            platform_root: None,
            web: None,
        }
    }

    // ----- transport surface ------------------------------------------------

    /// Handle one request line from a connection. Returns the response to
    /// send back, or `None` for a client notification (no id).
    pub fn handle_line(&mut self, conn: ConnId, line: &str) -> Option<Json> {
        let req = match rpc::parse_request(line) {
            Ok(r) => r,
            Err(resp) => return Some(resp),
        };
        let result = self.call(conn, &req.method, &req.params);
        let id = req.id?;
        Some(match result {
            Ok(v) => rpc::ok_response(id, v),
            Err(e) if e.code == codes::UNKNOWN_METHOD => rpc::err_response(
                id,
                rpc::METHOD_NOT_FOUND,
                &e.message,
                json!({ "hint": e.hint }),
            ),
            Err(e) => rpc::err_response_from(id, json::error_json(&e)),
        })
    }

    /// Call a method for a client. Every call that changes something is a
    /// row in the change feed, with its author and the `argus` line that
    /// makes it (phase 4, `feed.rs`): a commit's row where the call commits,
    /// else a command row. Connection 0 is the daemon's own (the launcher,
    /// the harness), and calls made inside a client's call belong to it.
    pub fn call(&mut self, conn: ConnId, method: &str, params: &Json) -> Result<Json> {
        self.call_depth += 1;
        let started = std::time::Instant::now();
        let r = self.call_fed(conn, method, params);
        self.call_depth -= 1;
        if self.call_depth == 0 {
            self.time_call(method, &r, started.elapsed());
        }
        r
    }

    fn call_fed(&mut self, conn: ConnId, method: &str, params: &Json) -> Result<Json> {
        if conn == 0 || self.calling.is_some() {
            return self.dispatch(conn, method, params);
        }
        let author = crate::feed::author_of(params, self.identities.get(&conn).map(String::as_str));
        self.calling = Some((argus_client::command::line(method, params), author));
        let rows = self.feed_rows;
        let r = self.dispatch(conn, method, params);
        let (line, author) = self.calling.take().unwrap_or_default();
        if r.is_ok() && self.feed_rows == rows && crate::feed::changes(method, params) {
            self.feed_command(&author, line, method, params);
        }
        r
    }

    /// A command row: a call that changed something outside a transaction.
    fn feed_command(&mut self, author: &str, line: String, method: &str, params: &Json) {
        // A seek right after a seek by the same author, in the same world,
        // is the same move: a drag along the editor's scrubber is one row,
        // the frame it let go at (phase 4 item 5), and a drag along a
        // timeline's ruler is one row, the time it let go at (item 9). The
        // row keeps its id and is announced again.
        if method == "sim.seek" || method == "timeline.seek" {
            if let Some(last) = self.edits.feed.last_mut() {
                let same = matches!(last.kind, CommitKind::Command)
                    && last.author == author
                    && last.call.as_ref().is_some_and(|(m, p)| {
                        m == method
                            && ["fork", "name", "timeline"]
                                .iter()
                                .all(|k| p.get(k) == params.get(k))
                    });
                if same {
                    last.frame = self.world.frame();
                    last.command = line;
                    last.call = Some((method.to_string(), params.clone()));
                    let row = last.to_json();
                    self.feed_rows += 1;
                    self.notify("edits", "edit.command", row);
                    return;
                }
            }
        }
        let id = self.edits.next_commit_id();
        let c = Commit {
            id,
            author: author.to_string(),
            note: String::new(),
            frame: self.world.frame(),
            kind: CommitKind::Command,
            ops: Vec::new(),
            inverse: Vec::new(),
            touched: Vec::new(),
            files: Vec::new(),
            command: line,
            call: Some((method.to_string(), params.clone())),
        };
        let row = c.to_json();
        self.edits.feed.push(c);
        self.feed_rows += 1;
        self.notify("edits", "edit.command", row);
    }

    /// The `argus` line of the call being made, for a feed row; empty for
    /// the daemon's own calls.
    pub(crate) fn call_line(&self) -> String {
        self.calling
            .as_ref()
            .map(|(l, _)| l.clone())
            .unwrap_or_default()
    }

    /// Dispatch a method. This is the whole protocol.
    fn dispatch(&mut self, conn: ConnId, method: &str, params: &Json) -> Result<Json> {
        // `fork: "name"` on any method runs it against that world, which is
        // swapped in for the call (phase 2 item 0).
        if method != "sim.fork" {
            if let Some(name) = params.get("fork").and_then(Json::as_str) {
                if name != self.active {
                    let mut slot = self
                        .forks
                        .remove(name)
                        .ok_or_else(|| self.unknown_fork(name))?;
                    self.swap_slot(&mut slot);
                    let was = std::mem::replace(&mut self.active, name.to_string());
                    let r = self.dispatch(conn, method, params);
                    self.active = was;
                    self.swap_slot(&mut slot);
                    self.forks.insert(name.to_string(), slot);
                    return r;
                }
            }
        }
        check_params(method, params)?;
        let r = match method {
            "session.open" => self.session_open(params),
            "session.status" => Ok(self.session_status()),
            "session.identify" => {
                let author = json::str_param(params, "author")?.to_string();
                self.identities.insert(conn, author.clone());
                Ok(json!({ "author": author }))
            }
            "session.capabilities" => Ok(self.capabilities()),
            "session.docs" => Ok(json!({ "text": docs::text(json::opt_str(params, "method")?) })),
            "session.close" => {
                self.close();
                Ok(json!({ "closed": true }))
            }
            "session.shutdown" => {
                self.close();
                self.shutdown = true;
                Ok(json!({ "shutdown": true }))
            }
            "world.summary" => match self.index_target(params)? {
                Some(s) => self.index()?.summary(&s),
                None => self.world_summary(),
            },
            "world.query" => match self.index_target(params)? {
                Some(s) => self.index()?.query(&s, params),
                None => self.world_query(params),
            },
            "world.get" => match self.index_target(params)? {
                Some(s) => {
                    let entity = json::str_param(params, "entity")?;
                    let resolved = json::bool_param(params, "resolved", false)?;
                    self.index()?.get(&s, entity, resolved)
                }
                None => {
                    self.require_session()?;
                    self.world_get(params)
                }
            },
            "world.describe" => {
                if self.index_forced(params)? {
                    self.index()?.describe(params)
                } else {
                    self.world_describe(params)
                }
            }
            "world.who_writes" => {
                if self.index_forced(params)? {
                    self.index()?.who_writes(json::str_param(params, "component")?)
                } else {
                    self.world_who_writes(params)
                }
            }
            "world.defs" => {
                if self.index_forced(params)? {
                    self.index()?.defs(params)
                } else {
                    self.world_defs(params)
                }
            }
            "world.refs" => {
                let target = json::str_param(params, "target")?;
                self.index()?.references_to(target, params)
            }
            "world.search" => {
                let text = json::str_param(params, "text")?;
                self.index()?.search(text, params)
            }
            "world.prefabs" => self.index()?.prefabs(params),
            "world.instances" => {
                let prefab = json::str_param(params, "prefab")?;
                let rows: Vec<Json> = self
                    .index()?
                    .instances_of(prefab)?
                    .into_iter()
                    .map(|(s, id)| json!([s, format!("@{id}")]))
                    .collect();
                let (limit, offset) = query::page(params)?;
                let mut out = query::paged(rows, limit, offset);
                out["prefab"] = json!(prefab);
                out["columns"] = json!(["scene", "entity"]);
                Ok(out)
            }
            "scene.index" => self.scene_index(params),
            "input.inject" => {
                let r = self.input_inject(params);
                self.input_branched();
                r
            }
            "input.script" => {
                let r = self.input_script(params);
                self.input_branched();
                r
            }
            "input.message" => {
                let r = self.input_message(params);
                self.input_branched();
                r
            }
            "input.record" => self.input_record(params),
            "input.replay" => {
                let r = self.input_replay(params);
                self.input_branched();
                r
            }
            "input.log.get" => self.input_log_get(params),
            "input.bindings" => Ok(self.input()?.borrow().map().to_json()),
            "input.bind" => self.input_bind(params),
            "input.devices" => Ok(self.input_devices()),
            "platform.status" => self.platform_status(),
            "token.issue" | "token.list" | "token.revoke" => self.token_call(conn, method, params),
            "platform.overlay" => self.platform_overlay(params),
            "platform.reset" => self.platform_reset(params),
            "input.state" => {
                let slot = json::u64_param(params, "slot", 1)? as u8;
                let st = self.input()?;
                let st = st.borrow();
                st.map().slot(slot)?;
                Ok(st.state_json(slot))
            }
            "asset.move" => self.asset_move(params),
            "asset.generate" => self.asset_generate(params),
            "asset.preview" => self.asset_preview(params),
            "asset.list" => self.asset_list(params),
            "asset.sidecar" => self.asset_sidecar(params),
            "asset.get" => {
                let name = json::str_param(params, "name")?;
                match self.require_session()?.assets.get(name) {
                    Ok(e) => Ok(e.to_json()),
                    // The material library is there without a file.
                    Err(e) => match argus_material::builtin(name) {
                        Some(b) => {
                            let mut o = json!({ "name": name, "kind": "material", "builtin": true, "warnings": [] });
                            argus_assets::material_json(b, &mut o);
                            Ok(o)
                        }
                        None => Err(e),
                    },
                }
            }
            "asset.import" => {
                self.require_session()?;
                let force = json::bool_param(params, "force", false)?;
                let ses = self.session.as_mut().expect("session");
                let report = if force { ses.assets.rescan_all() } else { ses.assets.scan() };
                Ok(self.apply_asset_report(report))
            }
            "scene.list" => self.scene_list(),
            "scene.load" => self.scene_load(params),
            "mods.list" => self.mods_list(),
            "scene.issues" => self.scene_issues(),
            "scene.rooms" => match self.index_target(params)? {
                Some(s) => self.index()?.rooms(&s),
                None => self.scene_rooms(params),
            },
            "scene.transition" => {
                self.mutations += 1;
                self.scene_transition(params)
            }
            "scene.tiles.get" => self.scene_tiles_get(params),
            "edit.begin" => self.edit_begin(params),
            "edit.tiles" => self.edit_tiles(params),
            "edit.text" => self.edit_text(params),
            "scene.tiles.create" => self.tiles_create(params),
            "scene.tiles.info" => {
                let file = json::str_param(params, "file")?.to_string();
                self.tiles_info(&file)
            }
            "scene.tiles.collision" => self.tiles_collision(params),
            "scene.tiles.list" => self.tiles_list(),
            "edit.set" | "edit.add" | "edit.remove" | "edit.spawn" | "edit.despawn"
            | "edit.def" | "edit.asset" | "edit.def_row" | "edit.prefab" | "edit.file" => {
                self.edit_queue(method, params)
            }
            "project.read" => self.project_read(params),
            "edit.commit" => {
                self.mutations += 1;
                self.edit_commit(params)
            }
            "edit.abort" => self.edit_abort(params),
            "edit.undo" => {
                self.mutations += 1;
                self.edit_undo(params, false)
            }
            "edit.redo" => {
                self.mutations += 1;
                self.edit_undo(params, true)
            }
            "edit.feed" => self.edit_feed(params),
            "edit.lock" => self.edit_lock(params),
            "edit.unlock" => self.edit_unlock(params),
            "edit.locks" => Ok(json!({ "rows": self.edits.locks.iter().map(|l| l.to_json()).collect::<Vec<_>>() })),
            "sim.play" => {
                self.require_session()?;
                if !self.sched.is_playing() {
                    // A keyframe where play starts: `sim.stop` comes back to it.
                    self.before_ticks();
                    if let Some(ses) = &mut self.session {
                        let k = ses.sources.capture(&self.world, &self.sched, true);
                        ses.ring.borrow_mut().push(k);
                        ses.play_from = Some(self.sched.tick());
                        ses.kept_in_play.clear();
                    }
                }
                self.sched.play(&self.world);
                Ok(self.sim_status())
            }
            "sim.pause" => {
                self.sched.pause();
                Ok(self.sim_status())
            }
            "sim.stop" => {
                let from = self.session.as_mut().and_then(|s| s.play_from.take());
                let kept = self
                    .session
                    .as_mut()
                    .map(|s| std::mem::take(&mut s.kept_in_play))
                    .unwrap_or_default();
                self.sched.stop(&mut self.world);
                // The world went back to before play: its history is of a
                // run that no longer happened, and so are its keyframes. The
                // input state, the pools, and the rooms go back with it.
                if let (Some(s), Some(t)) = (&mut self.session, from) {
                    s.history.borrow_mut().clear();
                    let ring = s.ring.clone();
                    let r = ring.borrow();
                    if let Some(k) = r.at_or_before(t) {
                        s.sources.restore(k, &mut self.world, &mut self.sched);
                    }
                    drop(r);
                    let _ = self.world.drain_events();
                    if let Some(i) = &s.input {
                        i.borrow_mut().branch(t);
                    }
                    ring.borrow_mut().prune_after(t);
                    self.cut_log(t);
                    self.make_kept_again(&kept);
                } else if let Some(s) = &self.session {
                    s.history.borrow_mut().clear();
                }
                self.pump_events();
                Ok(self.sim_status())
            }
            "sim.seek" => self.sim_seek(params),
            "sim.fork" => self.sim_fork(params),
            "sim.snapshot" => self.sim_snapshot(params),
            "sim.ring" => self.sim_ring(params),
            "nav.grids" => self.nav_grids(),
            "nav.grid" => self.nav_grid(params),
            "nav.path" => self.nav_path(params),
            "nav.field" => self.nav_field(params),
            "nav.bake" => self.nav_bake(params),
            "jobs.why" => self.jobs_why(params),
            "strings.list" => self.strings_list(params),
            "timeline.list" => self.timeline_list(params),
            "timeline.seek" => self.timeline_seek(params),
            "timeline.play" => self.timeline_play(params),
            "sim.save" => self.sim_save(params),
            "sim.load" => self.sim_load(params),
            "mark.drop" => self.mark_drop(params),
            "mark.list" => self.mark_list(params),
            "mark.get" => self.mark_get(params),
            "mark.note" => self.mark_note(params),
            "mark.close" => self.mark_close(params),
            "mark.cancel" => self.mark_cancel(params),
            "mark.open" => self.mark_open(params),
            "mark.import" => self.mark_import(params),
            "mark.pull" => self.mark_pull(params),
            "replay.save" => self.replay_save(params),
            "obs.review" => self.obs_review(params),
            "review.list" => self.review_list(params),
            "review.get" => self.review_get(params),
            "review.answer" => self.review_answer(params),
            "edit.propose" => self.edit_propose(params),
            "proposal.list" => self.proposal_list(params),
            "proposal.get" => self.proposal_get(params),
            "proposal.try" => self.proposal_try(params),
            "proposal.show" => self.proposal_show(params),
            "proposal.pick" => self.proposal_pick(params),
            "proposal.reject" => self.proposal_reject(params),
            "proposal.stop" => self.proposal_stop(params),
            "tune.fields" => self.tune_fields(params),
            "tune.begin" => self.tune_begin(params),
            "tune.set" => self.tune_set(params),
            "tune.end" => self.tune_end(params),
            "tune.log" => self.tune_log(params),
            "replay.open" => self.replay_open(params),
            "replay.list" => self.replay_list(params),
            "obs.diff" => self.obs_diff(params),
            "obs.trace" => self.obs_trace(params),
            "obs.why" => self.obs_why(params),
            "sim.step" => {
                let n = json::u64_param(params, "n", 1)?;
                self.before_ticks();
                self.sched.step(&mut self.world, n)?;
                self.pump_ticks();
                Ok(self.sim_status())
            }
            "sim.advance" => {
                let n = json::u64_param(params, "n", 1)?;
                if let Some(by) = json::opt_str(params, "by")? {
                    if by != "turn" {
                        return Err(bad_params(format!("by must be \"turn\"; got {by:?}")));
                    }
                }
                self.before_ticks();
                self.sched.advance(&mut self.world, n)?;
                self.pump_ticks();
                Ok(self.sim_status())
            }
            "sim.speed" => {
                let x = json::f64_param(params, "x", 1.0)?;
                let before = self.sched.speed();
                self.sched.set_speed(x)?;
                // A game reads its speed (q:speed()), so a speed set from
                // outside is a change between ticks: a keyframe before the
                // next tick, which no replay crosses (found with Outpost's
                // HUD, which said 1x while the editor ran it at 10x).
                if x != before {
                    self.mutations += 1;
                }
                Ok(self.sim_status())
            }
            "sim.run_until" => self.sim_run_until(params),
            "sim.threads" => self.sim_threads(params),
            "sim.status" => {
                let mut s = self.sim_status();
                // The world's digest: two worlds with the same one are the
                // same world (the phase 4 gate compares a feed's replay).
                if json::bool_param(params, "digest", false)? {
                    s["digest"] = json!(format!("{:016x}", self.run_digest()));
                }
                Ok(s)
            }
            "sim.set" => self.sim_set(params),
            "script.reload" => {
                self.mutations += 1;
                self.script_reload()
            }
            "script.systems" => self.script_systems(params),
            "script.errors" => self.script_errors(),
            "obs.events" => self.obs_events(params),
            "obs.subscribe" => self.obs_subscribe(conn, params),
            "prof.frame" => Ok(self.prof_frame()),
            "prof.worst" => {
                let n = json::u64_param(params, "n", 5)? as usize;
                let rows: Vec<Json> = self.sched.prof_worst(n).iter().map(tick_json).collect();
                Ok(json!({ "rows": rows }))
            }
            "prof.systems" => self.prof_systems(params),
            "prof.archetypes" => Ok(self.prof_archetypes()),
            "prof.alloc" => self.prof_alloc(params),
            "prof.capture" => self.prof_capture(params),
            "prof.targets" => self.prof_targets(params),
            "prof.calls" => Ok(self.prof_calls()),
            "obs.frame" => self.obs_frame(params),
            "obs.annotate" => {
                let mut p = if params.is_object() { params.clone() } else { json!({}) };
                p["format"] = json!("png");
                p["annotate"] = json!(true);
                self.obs_frame(&p)
            }
            "audio.status" => self.audio_status(),
            "audio.play" => self.audio_play(params),
            "audio.stop" => self.audio_stop(params),
            "audio.bus" => self.audio_bus(params),
            "audio.clip" => self.audio_clip(params),
            "audio.describe" => self.audio_describe(params),
            "audio.device" => self.audio_device(params),
            "render.window" => self.render_window(params),
            "render.status" => Ok(self.render_status()),
            "render.stream" => self.render_stream(conn, params),
            "input.device" => self.input_device(conn, params),
            "test.list" => {
                let root = self.require_session()?.root.clone();
                let day = ProjectConfig::load(&root)?.day_seconds;
                argus_test::list(&root, Some(day))
                    .map_err(|e| ArgusError::new(codes::TEST_RUNNER, e))
            }
            "test.run" => self.test_run(params),
            other => Err(ArgusError::new(
                codes::UNKNOWN_METHOD,
                format!("no method {other:?}"),
            )
            .hint("session.capabilities lists every method; sim.seek and sim.fork arrive in phase 2.")),
        };
        self.pump_events();
        // Whatever the agent looked at is a baseline for obs.diff.
        if r.is_ok() && (method.starts_with("world.") || method.starts_with("obs.")) {
            if let Some(s) = &self.session {
                s.history.borrow_mut().looked(&self.world, self.mutations);
            }
        }
        r
    }

    /// One real-time frame while playing on the continuous clock. The
    /// server calls this at the clock rate; `present` runs every frame.
    pub fn frame(&mut self) -> Result<()> {
        if self.session.is_some() {
            self.before_ticks();
            self.sched.frame(&mut self.world)?;
            self.pump_ticks();
        }
        Ok(())
    }

    /// True while a window is open and wants presenting every frame.
    pub fn has_window(&self) -> bool {
        #[cfg(feature = "render")]
        {
            self.render.window.is_some()
        }
        #[cfg(not(feature = "render"))]
        {
            false
        }
    }

    /// Real device input (phase 1, item 2): apply raw events, resolve them
    /// through the action map, and schedule what changed for the next
    /// tick. On a discrete scene that is playing, each new command takes
    /// one turn, so a human plays Delve by pressing keys. Returns the
    /// turns taken. Tests call this directly; the window and gamepads
    /// reach it through `poll_devices`.
    pub fn device_input(&mut self, events: &[argus_input::DeviceInput]) -> u64 {
        let st = self.session.as_ref().and_then(|s| s.input.clone());
        let next = self.sched.tick();
        let mut caught = false;
        for ev in events {
            // A controls menu waiting for a key takes the press (phase 3
            // item 1); the game does not hear it. It is journaled.
            if st
                .as_ref()
                .is_some_and(|s| s.borrow_mut().capture(next, ev))
            {
                caught = true;
                continue;
            }
            self.devices.apply(ev);
        }
        let Some(st) = st else {
            return 0;
        };
        let now = self.sched.tick();
        let commands = st.borrow_mut().apply_devices(now, &self.devices);
        self.input_branched();
        let mut turns = 0;
        // A caught key is a command too: the controls page shows it on the
        // next tick, and a discrete game ticks only on a command.
        if (commands > 0 || caught)
            && self.sched.config().driving == ClockKind::Discrete
            && self.sched.is_playing()
            && !self.sched.is_paused()
        {
            // One command, one turn: several keys in one poll are one turn.
            self.before_ticks();
            if self.sched.advance(&mut self.world, 1).is_ok() {
                turns = 1;
                self.device_turns += 1;
            }
            self.pump_ticks();
        }
        turns
    }

    /// Pump the window and the gamepads and feed what they report into
    /// the action map. The server calls this every frame a window is open.
    pub fn poll_devices(&mut self) {
        #[cfg(feature = "render")]
        {
            let evs = self.render.poll();
            let typed = std::mem::take(&mut self.render.typed);
            if self.render.closed_now {
                // A note half typed when the window closed is kept, and so
                // is a tuning session: closing the game ends it.
                self.finish_prompt(true);
                if self.tuning.is_some() {
                    let _ = self.end_tuning(true, None);
                }
                self.render.closed_now = false;
                #[cfg(feature = "audio")]
                if let Some(a) = self.audio.as_mut() {
                    a.close_device();
                }
                self.world.emit(Event::Game {
                    name: "window.closed".to_string(),
                    fields: Vec::new(),
                });
                self.pump_events();
                if self.exit_on_window_close {
                    self.shutdown = true;
                }
            }
            let evs = self.mark_keys(evs, typed);
            let evs = self.trial_keys(evs);
            if !evs.is_empty() {
                self.device_input(&evs);
            }
        }
    }

    /// The UI canvas, for semantic frames (phase 3 item 1).
    pub(crate) fn ui_canvas(&self) -> Option<[f32; 2]> {
        let s = self.session.as_ref()?;
        let t = &s.ui.borrow().theme;
        Some([t.width, t.height])
    }

    fn input_devices(&self) -> Json {
        let (keys, mouse, pads) = self.devices.summary();
        json!({
            "window": self.has_window(),
            "keys": keys,
            "mouse": mouse,
            "gamepads": pads.iter().map(|(i, n)| json!([i, n])).collect::<Vec<_>>(),
            "events_seen": self.devices.events_seen,
            "turns_taken": self.device_turns,
        })
    }

    /// Present one frame into the window, if open: run the `present`
    /// stage and draw. `poll_devices` pumps its events.
    pub fn present(&mut self) {
        #[cfg(feature = "render")]
        {
            self.render_present();
        }
    }

    /// True while the continuous clock should be driven in real time.
    pub fn wants_frames(&self) -> bool {
        self.session.is_some()
            && self.sched.is_playing()
            && !self.sched.is_paused()
            && self.sched.config().driving == ClockKind::Continuous
    }

    /// Seconds per real-time frame.
    pub fn frame_seconds(&self) -> f64 {
        1.0 / self.sched.config().hz.max(1.0)
    }

    /// A connection opened (the server's).
    pub fn open_conn(&mut self, conn: ConnId) {
        self.conns.insert(conn);
    }

    /// Forget a connection: its subscriptions, who it said it is, and its
    /// count among those open.
    pub fn drop_conn(&mut self, conn: ConnId) {
        self.conns.remove(&conn);
        self.subs.remove(&conn);
        self.identities.remove(&conn);
        self.close_stream(conn);
    }

    /// The sound goes with play: held while the clock is stopped or paused
    /// (before Play in the editor, at F8's prompt), so the music waits
    /// where it was. The server calls this every frame a window or a stream
    /// is open, which is when a device is.
    pub fn sync_audio(&mut self) {
        #[cfg(feature = "audio")]
        {
            let running = self.sched.is_playing() && !self.sched.is_paused();
            if let Some(a) = &self.audio {
                if a.held() == running {
                    a.set_hold(!running);
                }
            }
        }
    }

    /// True while a client's frame stream is open.
    pub fn has_stream(&self) -> bool {
        #[cfg(feature = "render")]
        {
            self.render.stream.is_some()
        }
        #[cfg(not(feature = "render"))]
        {
            false
        }
    }

    /// Close this connection's frame stream, if it has one.
    fn close_stream(&mut self, conn: ConnId) {
        #[cfg(feature = "render")]
        {
            if !self.render.stream.as_ref().is_some_and(|s| s.conn == conn) {
                return;
            }
            let s = self.render.stream.take();
            #[cfg(feature = "audio")]
            if s.as_ref().is_some_and(|s| s.audio) && !self.has_window() {
                if let Some(a) = self.audio.as_mut() {
                    a.close_device();
                }
            }
            drop(s);
        }
        #[cfg(not(feature = "render"))]
        let _ = conn;
    }

    /// Do something a key or a click stands for (F8, F9, F10, Escape at a
    /// mark's prompt) as the protocol call that does it, so the change feed
    /// has it as a command by whoever pressed it: the connection whose
    /// `input.device` it came in, or the person at the window (phase 4).
    pub(crate) fn as_command(
        &mut self,
        method: &str,
        params: Json,
        f: impl FnOnce(&mut Self, &Json) -> Result<Json>,
    ) -> Result<Json> {
        let author = self.key_author();
        let line = argus_client::command::line(method, &params);
        let saved = self.calling.replace((line.clone(), author.clone()));
        let rows = self.feed_rows;
        let r = f(self, &params);
        let made = self.feed_rows != rows;
        self.calling = saved;
        if r.is_ok() && !made {
            self.feed_command(&author, line, method, &params);
        }
        r
    }

    /// Who pressed the keys being handled: the calling client, or the
    /// person at the window.
    pub(crate) fn key_author(&self) -> String {
        self.calling
            .as_ref()
            .map(|c| c.1.clone())
            .unwrap_or_else(|| "human".to_string())
    }

    /// `input.device` (phase 4 item 1): a client's viewport's keys, mouse,
    /// and typed text, down the window's own path: a mark's key and its
    /// prompt, a trial's keys, then the action map, so gameplay still sees
    /// actions and the input log records them as a window's.
    fn input_device(&mut self, conn: ConnId, params: &Json) -> Result<Json> {
        self.require_session()?;
        let mut evs = Vec::new();
        for e in params
            .get("events")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
        {
            evs.push(self.device_event(conn, e)?);
        }
        let typed = json::str_list(params, "text")?;
        let seen = evs.len();
        let evs = self.mark_keys(evs, typed);
        let evs = self.trial_keys(evs);
        let turns = if evs.is_empty() {
            0
        } else {
            self.device_input(&evs)
        };
        Ok(json!({ "events": seen, "turns": turns }))
    }

    fn device_event(&self, conn: ConnId, e: &Json) -> Result<argus_input::DeviceInput> {
        use argus_input::DeviceInput;
        let pressed = || {
            e.get("pressed").and_then(Json::as_bool).ok_or_else(|| {
                bad_params(format!(
                    "{e}: pressed (true or false) goes with a key or a button"
                ))
            })
        };
        if let Some(k) = e.get("key").and_then(Json::as_str) {
            return Ok(DeviceInput::Key {
                name: k.to_string(),
                pressed: pressed()?,
            });
        }
        if let Some(b) = e.get("mouse").and_then(Json::as_str) {
            return Ok(DeviceInput::Mouse {
                button: b.to_ascii_lowercase(),
                pressed: pressed()?,
            });
        }
        if e.get("release_all").and_then(Json::as_bool) == Some(true) {
            return Ok(DeviceInput::ReleaseAll);
        }
        if let Some(p) = e.get("pointer").and_then(Json::as_array) {
            let (x, y) = match (
                p.first().and_then(Json::as_f64),
                p.get(1).and_then(Json::as_f64),
            ) {
                (Some(x), Some(y)) => (x as f32, y as f32),
                _ => return Err(bad_params(format!("{e}: pointer is [x, y]"))),
            };
            #[cfg(feature = "render")]
            if let Some(s) = self.render.stream.as_ref().filter(|s| s.conn == conn) {
                // The stream's pixels onto the UI canvas, as the window's are.
                if let Some(f) = s.renderer.ui_fit() {
                    let [x, y] = f.to_canvas([x, y]);
                    return Ok(DeviceInput::Pointer { x, y });
                }
                return Ok(DeviceInput::Pointer { x, y });
            }
            let _ = (conn, x, y);
            return Err(bad_params("pointer is a place in this connection's frames")
                .hint("render.stream opens them; the pointer is in their pixels, as a window's is in its own."));
        }
        Err(bad_params(format!(
            "{e} is not a device event: {{key, pressed}}, {{mouse, pressed}}, {{pointer: [x, y]}}, or {{release_all: true}}"
        )))
    }

    /// Draw the stream's next frame, when one is due: every frame while the
    /// clock plays; paused or stopped, when something changed, and four
    /// times a second besides.
    pub fn stream_frame(&mut self) {
        #[cfg(feature = "render")]
        {
            let Some(s) = &self.render.stream else {
                return;
            };
            if self.session.is_none() {
                return;
            }
            let now = std::time::Instant::now();
            // Due a little early rather than a loop's jitter late: the
            // server's loop runs at the sim's rate, and a due time a hair
            // after its next turn skipped every other frame (37 frames a
            // second of 60, found when Kevin first played in the editor).
            if now + STREAM_SLACK < s.next_due {
                return;
            }
            let key = (self.world.frame(), self.mutations, self.devices.events_seen);
            let quiet = s.last.is_some_and(|(t, k)| {
                k == key && now.duration_since(t) < std::time::Duration::from_millis(250)
            });
            if quiet && !self.wants_frames() {
                return;
            }
            if !self.has_window() {
                // With no window the stream's frame is the one: a trial's
                // worlds keep step and the present stage runs, as a window's
                // frame does.
                if let Err(e) = self.sync_trial() {
                    self.end_trial(true);
                    self.toast = Some((
                        format!("TRIAL ENDED: {}", e.message),
                        now + std::time::Duration::from_secs(5),
                    ));
                }
                let _ = self.sched.present(&mut self.world);
            }
            let bar = self.bar_text();
            let (tick, frame, alpha) = (self.sched.tick(), self.world.frame(), self.sched.alpha());
            let (w, h) = self
                .render
                .stream
                .as_ref()
                .map(|s| s.writer.size())
                .unwrap_or((1, 1));
            let view = argus_render::view_for(&self.world, w, h);
            let extras = self.tile_extras(view);
            let edges = self.tile_occluders(view);
            let t = shader_time(&self.sched, alpha);
            let (Some(s), Some(ses)) = (&mut self.render.stream, &self.session) else {
                return;
            };
            let started = std::time::Instant::now();
            let bar_key = bar.as_ref().map(|(t, _)| t.clone());
            if s.bar.as_ref() != Some(&bar_key) {
                s.renderer
                    .set_bar(bar.as_ref().map(|(t, c)| (t.as_str(), *c)));
                s.bar = Some(bar_key);
            }
            s.renderer.set_time(t);
            s.renderer.set_ui(Some(ses.ui.clone()));
            s.renderer.set_occluders(edges);
            let store = ses.particles.borrow();
            let drawn = s
                .renderer
                .render_frame(&self.world, tick, alpha, &extras, Some(&store));
            drop(store);
            let renderer = &s.renderer;
            let wrote = drawn.and_then(|_| {
                s.writer
                    .write(tick, frame, |buf| renderer.capture_into(buf))
            });
            s.last = Some((now, key));
            // A steady schedule, not one counted from whenever this frame
            // happened to be drawn; one that fell behind starts again.
            let interval = std::time::Duration::from_secs_f64(1.0 / s.fps);
            let next = s.next_due + interval;
            s.next_due = if next + interval < now {
                now + interval
            } else {
                next
            };
            s.last_ms = started.elapsed().as_secs_f64() * 1000.0;
            let conn = s.conn;
            match wrote {
                Ok((slot, seq)) => {
                    s.frames += 1;
                    s.error = None;
                    let n = json!({
                        "path": shown_path(s.writer.path()),
                        "slot": slot, "seq": seq, "tick": tick, "frame": frame,
                        "width": w, "height": h,
                        // What the frame shows, in world units: for a
                        // client to turn a point on it into a place (a
                        // painter's cell, phase 4 item 7).
                        "view": {
                            "center": s.renderer.last().view_center,
                            "half": s.renderer.last().view_half,
                            "angle": s.renderer.last().view_angle,
                        },
                    });
                    self.outbox
                        .push((conn, rpc::notification("render.frame", n)));
                }
                Err(e) => s.error = Some(e.message),
            }
            self.pump_events();
        }
    }

    // ----- session ----------------------------------------------------------

    fn require_session(&self) -> Result<&Session> {
        self.session.as_ref().ok_or_else(|| {
            ArgusError::new(codes::NO_PROJECT, "no project is open")
                .hint("session.open {project: \"path/to/dir\"} first; argusd --project opens one at start.")
        })
    }

    fn close(&mut self) {
        self.session = None;
        self.world = World::new();
        self.sched = Scheduler::new(SchedulerConfig::default());
        self.edits = Editing::default();
    }

    fn session_open(&mut self, params: &Json) -> Result<Json> {
        let root = PathBuf::from(json::str_param(params, "project")?);
        let root = root.canonicalize().map_err(|e| {
            ArgusError::new(
                argus_scene::codes::MISSING_FILE,
                format!("{}: {e}", root.display()),
            )
        })?;
        let cfg = ProjectConfig::load(&root)?;
        let scene_name = match json::opt_str(params, "scene")? {
            Some(s) => s.to_string(),
            None => cfg
                .default_scene
                .clone()
                .ok_or_else(|| bad_params("no scene given and argus.toml has no default_scene"))?,
        };
        let seed = match params.get("seed") {
            Some(Json::Number(n)) => Some(n.as_u64().unwrap_or(0)),
            _ => None,
        };
        self.mods_asked = json::str_list(params, "mods")?;
        self.forks.clear();
        self.active = "main".to_string();
        // Another project, another feed.
        if self.session.as_ref().is_some_and(|s| s.root != root) {
            self.edits = Default::default();
        }
        self.open_scene(root, &cfg, &scene_name, seed)
    }

    /// `mods.list` (phase 3 item 8): the mods laid over the game, in load
    /// order, with what each added; those left out and why; what the mods
    /// changed of the game's or each other's; and every mod in the
    /// project's `mods/`.
    fn mods_list(&self) -> Result<Json> {
        let ses = self.require_session()?;
        let mut out = ses.mods.to_json();
        let (found, problems) =
            argus_player::mods::find(&argus_player::mods::project_dir(&ses.root));
        out["available"] = json!(found
            .iter()
            .map(|f| json!({
                "id": f.manifest.id,
                "name": f.manifest.name,
                "version": f.manifest.version,
                "game": f.manifest.game,
                "requires": f.manifest.requires,
                "doc": f.manifest.doc,
                "hash": f.hash,
            }))
            .collect::<Vec<_>>());
        if !problems.is_empty() {
            out["unreadable"] = json!(problems
                .iter()
                .map(|(id, e)| json!({ "id": id, "why": json::error_json(e) }))
                .collect::<Vec<_>>());
        }
        Ok(out)
    }

    /// The mods a run recorded (a replay's, a mark's, or a save's
    /// identity: `[{id, version, hash}]`) asked for again; after the load,
    /// what differs from them, if anything.
    pub(crate) fn ask_mods_of(&mut self, recorded: &Json) {
        self.mods_asked = recorded
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|m| m["id"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
    }

    /// How the mods loaded now differ from those a run recorded: `None`
    /// when they are the same.
    pub(crate) fn mods_differ(&self, recorded: &Json) -> Option<Json> {
        let now = self.session.as_ref()?.mods.identity();
        let then = if recorded.is_array() {
            recorded.clone()
        } else {
            json!([])
        };
        (now != then).then(|| {
            json!({
                "recorded": then,
                "now": now,
                "said": "the mods loaded are not the ones the run was recorded with; it may not replay the same",
            })
        })
    }

    /// The session's mods as a run's identity, when there are any.
    pub(crate) fn mods_identity(&self) -> Option<Json> {
        let s = self.session.as_ref()?;
        (!s.mods.loaded.is_empty()).then(|| s.mods.identity())
    }

    fn open_scene(
        &mut self,
        root: PathBuf,
        cfg: &ProjectConfig,
        scene_name: &str,
        seed: Option<u64>,
    ) -> Result<Json> {
        // Section 17: a cold scene open, the index warm, under 300 ms.
        let started = std::time::Instant::now();
        // A new world: the log's records refer to the old one's component ids.
        // Sequence numbers keep counting so a client's `since` cursor stays valid.
        self.events.records.clear();
        self.events.ticked.clear();
        self.events.through = 0;
        self.mutations += 1;
        // Fresh world and scheduler, assembled as every player assembles
        // them (argus-player, phase 2 item 7); the daemon's own parts go
        // around it. A random seed policy takes the clock's. The change
        // feed is the session's record of every change, a load among them,
        // so it stays (found by the phase 4 gate: a replay opened from the
        // scrubber emptied it, and the feed no longer made the same world).
        let kept = self.edits.feed_only();
        self.close();
        self.edits = kept;
        let random_seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        // The mods asked for, from the project's mods/ (phase 3 item 8).
        let (mods, _) = argus_player::mods::choose(
            cfg,
            &argus_player::mods::project_dir(&root),
            argus_player::mods::Order::Given(self.mods_asked.clone()),
        )?;
        let a = argus_player::assemble(
            &root,
            cfg,
            scene_name,
            seed,
            &argus_player::Options {
                random_seed,
                audio: !self.building_fork,
                mods,
                plugins: argus_plugins::Source::default(),
            },
        )?;
        let argus_player::Assembly {
            world,
            sched,
            clock,
            seed,
            input,
            tiles,
            nav,
            anims,
            particles,
            contacts,
            rigid,
            host,
            plugins,
            project,
            scene,
            issues,
            load,
            assets,
            asset_report,
            ui,
            dialogue,
            timelines,
            #[cfg(feature = "audio")]
            audio,
            mods,
            platform,
            ..
        } = a;
        // The null platform, one a project (phase 5 item 9).
        if self.platform_root.as_deref() != Some(root.as_path()) {
            self.platform = argus_platform::NullPlatform::with_cloud(
                root.join(".argus").join("platform").join("cloud"),
            );
            self.platform_root = Some(root.clone());
        }
        self.platform_cfg = platform;
        self.world = world;
        self.sched = sched;
        let history = Rc::new(RefCell::new(crate::history::History::default()));
        let h = history.clone();
        self.sched
            .observe(Box::new(move |w, frame| h.borrow_mut().record(w, frame)));
        let (index, index_error) = match Index::open(&root) {
            Ok(i) => (Some(i), None),
            Err(e) => (None, Some(e.message)),
        };
        #[cfg(feature = "render")]
        if !self.building_fork {
            self.render.offscreen = None;
            if let Some((_, r)) = &mut self.render.window {
                let _ = r.load_catalog(&assets);
            }
            if let Some(s) = &mut self.render.stream {
                let _ = s.renderer.load_catalog(&assets);
                s.renderer.forget_motion();
            }
        }
        #[cfg(feature = "audio")]
        match audio {
            Some(Ok(mut a)) => {
                if self.has_window() || self.stream_has_audio() {
                    let _ = a.open_device();
                }
                self.audio = Some(a);
                self.audio_error = None;
            }
            Some(Err(e)) => {
                self.audio = None;
                self.audio_error = Some(json::error_json(&e));
            }
            // A fork keeps the mixer it has.
            None => {}
        }
        let ring_every = cfg.ring_every.unwrap_or(match clock.kind {
            ClockKind::Continuous => ((1.0 / clock.timestep).round() as u64).max(1),
            ClockKind::Discrete => 10,
        });
        let ring_budget = (cfg.ring_budget_mb.unwrap_or(1024) as usize) << 20;
        let sources = crate::ring::Sources {
            input: input.clone(),
            particles: particles.clone(),
            scene: Some(scene.clone()),
            contacts: contacts.clone(),
            tiles: Some(tiles.clone()),
            rigid,
        };
        self.session = Some(Session {
            ring: Rc::new(RefCell::new(crate::ring::Ring::new(
                ring_every,
                ring_budget,
            ))),
            sources,
            play_from: None,
            kept_in_play: Vec::new(),
            mark_key: cfg.mark_key.clone().unwrap_or_else(|| "F8".to_string()),
            saves: Rc::new(RefCell::new(Vec::new())),
            ui,
            dialogue,
            timelines,
            root,
            project,
            scene,
            scene_name: scene_name.to_string(),
            host,
            plugins,
            plugin_names: cfg.plugins.clone(),
            seed,
            loaded: issues.is_empty(),
            index,
            index_error,
            input,
            run_dir: None,
            assets,
            tiles,
            nav,
            anims,
            particles,
            history,
            mods,
        });
        self.asset_events(&asset_report, true);
        self.pump_events();
        // Systems declared `on_load` run now, with everything they could
        // read in place, so the first frame shows what they build. What they
        // emit is the run's, not the load's: `run_from` is where a test's
        // events begin.
        let run_from = self.events.next_seq;
        self.sched.run_load(&mut self.world)?;
        self.pump_events();
        self.start_ring();
        let index_started = std::time::Instant::now();
        if !self.building_fork {
            self.sync_index(false);
        }
        let index_ms = index_started.elapsed().as_secs_f64() * 1000.0;
        let mut out = self.world_summary()?;
        out["run_from"] = json!(run_from);
        out["ms"] = json!((started.elapsed().as_secs_f64() * 1e4).round() / 10.0);
        out["index_ms"] = json!((index_ms * 10.0).round() / 10.0);
        if !issues.is_empty() {
            out["issues"] = Json::Array(issues.iter().map(json::error_json).collect());
        }
        if let Some(r) = load {
            out["load"] = json!({
                "entities": r.entities,
                "rooms": r.rooms,
                "tags_registered": r.tags_registered,
                "pending_refs": r.pending.len(),
                "ms": r.ms,
            });
        }
        out["script_errors"] = json!(self.script_error_count());
        if let Some(s) = &self.session {
            if !self.mods_asked.is_empty() || !s.mods.loaded.is_empty() {
                out["mods"] = json!({
                    "loaded": s.mods.ids(),
                    "left_out": s.mods.left_out.iter().map(|(id, why)| json!({
                        "id": id,
                        "why": why.iter().map(|e| e.message.clone()).collect::<Vec<_>>(),
                    })).collect::<Vec<_>>(),
                    "changes": s.mods.changes.len(),
                });
            }
        }
        if let Some(e) = &self.audio_error {
            out["audio_error"] = e.clone();
        }
        out["assets"] = json!({
            "total": self.session.as_ref().map(|s| s.assets.entries().len()).unwrap_or(0),
            "failed": asset_report.failed.len(),
            "warnings": self.session.as_ref().map(|s| s.assets.entries().values().map(|e| e.warnings.len()).sum::<usize>()).unwrap_or(0),
        });
        Ok(out)
    }

    fn session_status(&self) -> Json {
        match &self.session {
            None => {
                json!({ "project": null, "scene": null, "playing": false, "engine": version_json(), "clients": self.conns.len() })
            }
            Some(s) => {
                let mut st = self.sim_status();
                st["project"] = json!(s.project.config.name);
                st["root"] = json!(s.root.to_string_lossy().trim_start_matches(r"\\?\"));
                st["scene"] = json!(s.scene_name);
                st["script_errors"] = json!(self.script_error_count());
                st["loaded"] = json!(s.loaded);
                st["index"] = json!(s
                    .index
                    .as_ref()
                    .map(|_| Index::path(&s.root).to_string_lossy().into_owned()));
                st["index_error"] = json!(s.index_error);
                // The queue an agent works through (section 10).
                st["marks_open"] = json!(marks::open_count(&s.root));
                st["engine"] = version_json();
                // How many are connected, this one included.
                st["clients"] = json!(self.conns.len());
                st
            }
        }
    }

    fn capabilities(&self) -> Json {
        json!({
            "version": VERSION,
            "revision": REVISION,
            "phase": 0,
            "transports": ["stdio", "local-socket"],
            "methods": docs::METHODS.iter().map(|m| json!({
                "name": m.name,
                "doc": m.doc,
                "params": m.params.iter().map(docs::Param::to_json).collect::<Vec<_>>(),
                "returns": m.returns,
            })).collect::<Vec<_>>(),
            "tools": docs::TOOLS.iter().map(|t| json!({
                "name": t.name,
                "doc": t.doc,
                "params": t.params.iter().map(docs::Param::to_json).collect::<Vec<_>>(),
                "calls": t.calls,
            })).collect::<Vec<_>>(),
        })
    }

    // ----- world ------------------------------------------------------------

    fn world_summary(&mut self) -> Result<Json> {
        let s = self.world.summary();
        let (scene, rooms, seed) = match &self.session {
            Some(ses) => (
                Some(ses.scene_name.clone()),
                ses.scene
                    .borrow()
                    .loaded_rooms()
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                ses.seed,
            ),
            None => (None, Vec::new(), self.sched.seed()),
        };
        let systems = self.sched.systems(self.world.registry())?;
        let enabled: Vec<String> = systems
            .iter()
            .filter(|x| x.enabled)
            .map(|x| x.name.clone())
            .collect();
        let disabled: Vec<String> = systems
            .iter()
            .filter(|x| !x.enabled)
            .map(|x| x.name.clone())
            .collect();
        let named: Vec<(String, Vec<String>)> = s
            .named
            .iter()
            .take(50)
            .map(|(id, e)| {
                let comps: Vec<String> = self
                    .world
                    .components_of(*e)
                    .map(|cs| {
                        cs.iter()
                            .map(|c| self.world.registry().schema(*c).name.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                (id.clone(), comps)
            })
            .collect();
        Ok(Summary {
            frame: s.frame,
            tick: self.sched.tick(),
            time: self.sched.time(),
            seed,
            clock: self.sched.driving().to_string(),
            playing: self.sched.is_playing(),
            paused: self.sched.is_paused(),
            scene,
            rooms,
            entities: s.entities,
            authored: s.authored,
            components: s.components,
            archetypes: s
                .archetypes
                .iter()
                .map(|a| (a.components.clone(), a.entities))
                .collect(),
            named,
            named_total: s.named.len(),
            systems: enabled,
            disabled,
            pending_events: s.pending_events,
            source: "live",
        }
        .to_json())
    }

    /// Which scene a read should be served from the index for: an explicit
    /// `source: "index"`, another scene than the loaded one, or the current
    /// scene when it could not be instantiated. `None` means the live world.
    fn index_target(&self, params: &Json) -> Result<Option<String>> {
        let Some(ses) = self.session.as_ref() else {
            return Ok(None);
        };
        let scene = json::opt_str(params, "scene")?;
        let forced = json::opt_str(params, "source")? == Some("index");
        if forced {
            return Ok(Some(scene.unwrap_or(&ses.scene_name).to_string()));
        }
        if let Some(sc) = scene {
            if sc != ses.scene_name {
                return Ok(Some(sc.to_string()));
            }
        }
        if !ses.loaded {
            return Ok(Some(ses.scene_name.clone()));
        }
        Ok(None)
    }

    fn index_forced(&self, params: &Json) -> Result<bool> {
        Ok(json::opt_str(params, "source")? == Some("index")
            || self.session.as_ref().is_some_and(|s| !s.loaded))
    }

    fn index(&self) -> Result<&Index> {
        let ses = self.require_session()?;
        ses.index.as_ref().ok_or_else(|| {
            ArgusError::new(
                argus_index::codes::INDEX,
                format!(
                    "the index is unavailable: {}",
                    ses.index_error.as_deref().unwrap_or("not opened")
                ),
            )
            .hint("Delete .argus/index.db and reopen the project; the index is a cache.")
        })
    }

    /// Bring the index up to date. `force` rewrites every scene; otherwise
    /// only scenes whose files changed (and the loaded scene when stale).
    pub fn sync_index(&mut self, force: bool) -> Option<SyncReport> {
        let systems = self
            .sched
            .systems(self.world.registry())
            .unwrap_or_default();
        let registry = self.world.registry().clone();
        let ses = self.session.as_mut()?;
        let idx = ses.index.as_mut()?;
        let scene = ses.scene.borrow();
        match idx.sync_project(&ses.project, &registry, &systems, Some(&scene), force) {
            Ok(r) => Some(r),
            Err(e) => {
                ses.index_error = Some(e.message);
                None
            }
        }
    }

    /// Poll `assets/` and import what changed (the watcher). The server
    /// calls this twice a second; `obs.frame` calls it first so an edit
    /// to a sidecar shows in the next picture.
    pub fn refresh_assets(&mut self) {
        let Some(ses) = self.session.as_mut() else {
            return;
        };
        if let Some(report) = ses.assets.poll() {
            self.apply_asset_report(report);
        }
    }

    /// Reload what an import changed, log it, and report it.
    fn apply_asset_report(&mut self, report: argus_assets::Report) -> Json {
        let changed: Vec<String> = report
            .imported
            .iter()
            .chain(report.removed.iter())
            .cloned()
            .collect();
        if !changed.is_empty() {
            // Machines, effects, and tilesets steer ticks: a change between
            // ticks, like an edit.
            self.mutations += 1;
            let ses = self.session.as_ref().expect("session");
            ses.tiles.borrow_mut().set_tilesets(&ses.assets);
            ses.anims.borrow_mut().set_assets(&ses.assets);
            ses.particles.borrow_mut().set_assets(&ses.assets);
            // Fonts again only when one changed: the layout measures anew.
            if changed.iter().any(|n| {
                ses.assets
                    .entries()
                    .get(n)
                    .is_none_or(|e| e.kind() == "font")
            }) {
                argus_player::set_fonts(&mut ses.ui.borrow_mut().fonts, &ses.assets);
            }
            #[cfg(feature = "render")]
            {
                // Sprites first, so a material naming one binds the new texture.
                if let Some((_, r)) = &mut self.render.window {
                    let _ = r.reload_sprites(&ses.assets, &changed);
                    let _ = r.load_materials(&ses.assets);
                }
                if let Some((_, _, r)) = &mut self.render.offscreen {
                    let _ = r.reload_sprites(&ses.assets, &changed);
                    let _ = r.load_materials(&ses.assets);
                }
                if let Some(s) = &mut self.render.stream {
                    let _ = s.renderer.reload_sprites(&ses.assets, &changed);
                    let _ = s.renderer.load_materials(&ses.assets);
                }
            }
            #[cfg(feature = "audio")]
            {
                let sounds_changed = changed.iter().any(|n| {
                    ses.assets
                        .entries()
                        .get(n)
                        .is_none_or(|e| e.kind() == "sound")
                });
                if sounds_changed {
                    if let Some(a) = self.audio.as_mut() {
                        a.set_asset_sounds(ses.assets.sidecar_sounds());
                    }
                }
            }
        }
        self.asset_events(&report, false);
        self.pump_events();
        json!({
            "imported": report.imported,
            "unchanged": report.unchanged,
            "removed": report.removed,
            "failed": report.failed.iter().map(|f| json!({
                "name": f.name, "message": f.error.message, "at": f.error.r#ref,
            })).collect::<Vec<_>>(),
            "ms": report.ms,
        })
    }

    /// Every import as events (section 14 "reports every import as an
    /// event with warnings"). On a scene open the imports are one summary
    /// event; warnings and failures are always one event each.
    fn asset_events(&mut self, report: &argus_assets::Report, opening: bool) {
        let Some(ses) = self.session.as_ref() else {
            return;
        };
        let mut evs: Vec<(String, Vec<(String, Value)>)> = Vec::new();
        let s = |x: &str| Value::Str(x.to_string());
        if opening {
            evs.push((
                "assets.scanned".into(),
                vec![
                    (
                        "assets".into(),
                        Value::Int(ses.assets.entries().len() as i64),
                    ),
                    ("failed".into(), Value::Int(report.failed.len() as i64)),
                    ("ms".into(), Value::Float((report.ms * 10.0).round() / 10.0)),
                ],
            ));
        }
        for name in &report.imported {
            let Some(e) = ses.assets.entries().get(name) else {
                continue;
            };
            if !opening {
                evs.push((
                    "asset.imported".into(),
                    vec![
                        ("asset".into(), s(name)),
                        ("kind".into(), s(e.kind())),
                        ("warnings".into(), Value::Int(e.warnings.len() as i64)),
                    ],
                ));
            }
            for w in &e.warnings {
                evs.push((
                    "asset.warning".into(),
                    vec![
                        ("asset".into(), s(name)),
                        ("at".into(), s(&w.at)),
                        ("message".into(), s(&w.message)),
                    ],
                ));
            }
        }
        for f in &report.failed {
            evs.push((
                "asset.failed".into(),
                vec![
                    ("asset".into(), s(&f.name)),
                    ("at".into(), s(f.error.r#ref.as_deref().unwrap_or(""))),
                    ("message".into(), s(&f.error.message)),
                ],
            ));
        }
        for name in &report.removed {
            evs.push(("asset.removed".into(), vec![("asset".into(), s(name))]));
        }
        for (name, fields) in evs {
            self.world.emit(Event::Game { name, fields });
        }
    }

    fn asset_list(&self, params: &Json) -> Result<Json> {
        let ses = self.require_session()?;
        let kind = json::opt_str(params, "kind")?;
        let mut rows: Vec<Json> = ses
            .assets
            .entries()
            .values()
            .filter(|e| kind.is_none_or(|k| k == e.kind()))
            .map(|e| match &e.asset {
                argus_assets::Asset::Sprite(sp) => json!([
                    e.name,
                    "sprite",
                    sp.source.clone().unwrap_or_else(|| "shape".to_string()),
                    sp.frames.len(),
                    sp.clips.keys().collect::<Vec<_>>(),
                    e.warnings.len()
                ]),
                argus_assets::Asset::Animator(an) => json!([
                    e.name,
                    "animator",
                    e.sidecar.clone().unwrap_or_default(),
                    an.states.len(),
                    an.states.iter().map(|s| s.name.clone()).collect::<Vec<_>>(),
                    e.warnings.len()
                ]),
                argus_assets::Asset::Material(m) => json!([
                    e.name,
                    "material",
                    m.shader_file,
                    m.params.len(),
                    m.params.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
                    e.warnings.len()
                ]),
                argus_assets::Asset::Font(f) => json!([
                    e.name,
                    "font",
                    f.source,
                    f.glyphs.len(),
                    Json::Array(Vec::new()),
                    e.warnings.len()
                ]),
                argus_assets::Asset::Emitter(fx) => json!([
                    e.name,
                    "emitter",
                    e.sidecar.clone().unwrap_or_default(),
                    fx.max,
                    fx.sprite.iter().collect::<Vec<_>>(),
                    e.warnings.len()
                ]),
                argus_assets::Asset::Map(m) => json!([
                    e.name,
                    "tilemap",
                    m.file,
                    m.layers.len(),
                    m.layers,
                    e.warnings.len()
                ]),
                argus_assets::Asset::Skeleton(s) => json!([
                    e.name,
                    "skeleton",
                    format!("assets/{}", s.source),
                    s.bones.len(),
                    s.animations.keys().collect::<Vec<_>>(),
                    e.warnings.len()
                ]),
                argus_assets::Asset::Sound(so) => json!([
                    e.name,
                    "sound",
                    if so.from_sidecar {
                        e.sidecar.clone().unwrap_or_default()
                    } else {
                        so.files.first().map(|f| f.path.clone()).unwrap_or_default()
                    },
                    so.def.variations.len(),
                    Json::Array(Vec::new()),
                    e.warnings.len()
                ]),
            })
            .collect();
        // Asked for materials, the library too, beside the project's (a
        // project asset of the same name shadows one).
        if kind == Some("material") {
            for b in argus_material::builtins() {
                if !ses.assets.entries().contains_key(&b.name) {
                    rows.push(json!([
                        b.name,
                        "material",
                        b.shader_file,
                        b.params.len(),
                        b.params.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
                        0
                    ]));
                }
            }
        }
        let failed: Vec<Json> = ses
            .assets
            .failures()
            .values()
            .map(|f| json!({ "name": f.name, "message": f.error.message, "at": f.error.r#ref }))
            .collect();
        let (limit, offset) = query::page(params)?;
        let mut out = query::paged(rows, limit, offset);
        out["columns"] = json!(["name", "kind", "source", "frames", "clips", "warnings"]);
        out["failed"] = Json::Array(failed);
        Ok(out)
    }

    /// Idle-time refresh: pick up files edited outside the daemon.
    pub fn refresh_index(&mut self) {
        self.sync_index(false);
    }

    fn index_touched(&mut self, touched: &[String]) {
        let Some(ses) = self.session.as_mut() else {
            return;
        };
        let Some(idx) = ses.index.as_mut() else {
            return;
        };
        for t in touched {
            let Some(id) = t.strip_prefix('@') else {
                continue;
            };
            let scene = ses.scene.borrow();
            let (fid, _) = split_child(&scene, id);
            if scene.resolved(&fid).is_some() {
                let _ = idx.sync_entities(&scene, &[&fid]);
            } else {
                let _ = idx.remove_entity(&scene.name, &fid, None);
            }
        }
    }

    fn scene_index(&mut self, params: &Json) -> Result<Json> {
        self.require_session()?;
        let rebuild = json::bool_param(params, "rebuild", false)?;
        let mut stale = serde_json::Map::new();
        if let Some(idx) = self.session.as_ref().and_then(|s| s.index.as_ref()) {
            for name in self
                .session
                .as_ref()
                .map(|s| s.project.scenes())
                .unwrap_or_default()
            {
                if let Ok(Some(why)) = idx.stale_reason(&name) {
                    stale.insert(name, json!(why));
                }
            }
        }
        let report = self.sync_index(rebuild);
        let stats = self.index()?.stats()?;
        let mut out = json!({ "stats": stats, "stale": stale });
        if let Some(r) = report {
            out["scenes"] = json!(r.scenes);
            out["entities"] = json!(r.entities);
            out["failed"] = json!(r.failed);
            out["ms"] = json!(r.ms);
        }
        if let Some(e) = self.session.as_ref().and_then(|s| s.index_error.clone()) {
            out["error"] = json!(e);
        }
        Ok(out)
    }

    fn asset_move(&mut self, params: &Json) -> Result<Json> {
        let from = json::str_param(params, "from")?.to_string();
        let to = json::str_param(params, "to")?.to_string();
        let registry = self.world.registry().clone();
        let ses = self
            .session
            .as_mut()
            .ok_or_else(|| ArgusError::new(codes::NO_PROJECT, "no project is open"))?;
        let report = ses.project.move_asset(&from, &to)?;
        // A moved def row is read again (moves rewrite references in tables),
        // and a moved prefab spawns by its new name.
        ses.host
            .set_defs(Rc::new(argus_player::ProjectDefs(ses.project.defs.clone())));
        ses.host
            .set_prefabs(Rc::new(argus_player::ProjectPrefabs::new(&ses.project)));
        *ses.dialogue.borrow_mut() = argus_dialogue::Graph::from_defs(&ses.project.defs);
        *ses.timelines.borrow_mut() = argus_player::read_timelines(&ses.project.defs, &self.world);
        ses.scene
            .borrow_mut()
            .reload_files(&ses.project.ctx(&registry));
        let entity_move = report.record.kind == argus_scene::MoveKind::Entity;
        self.sync_index(true);
        self.refresh_assets();
        let mut out = json!({
            "kind": report.record.kind.name(),
            "from": report.record.from,
            "to": report.record.to,
            "refs": report.record.refs,
            "rewritten": report.rewritten,
        });
        if entity_move {
            out["note"] = json!("the loaded world keeps the old id until scene.load");
        }
        Ok(out)
    }

    fn world_get(&self, params: &Json) -> Result<Json> {
        let r = json::str_param(params, "entity")?;
        let resolved = json::bool_param(params, "resolved", false)?;
        let e = self.world.entity(r)?;
        let mut comps = Map::new();
        for c in self.world.components_of(e)?.to_vec() {
            let name = self.world.registry().schema(c).name.clone();
            comps.insert(
                name,
                query::project(&self.world, e, &self.world.registry().schema(c).name),
            );
        }
        let id = self.world.string_id(e).map(str::to_string);
        let mut out = json!({
            "entity": e.to_string(),
            "id": id,
            "authored": self.world.is_authored(e),
            "components": comps,
            "source": "live",
        });
        if let (Some(id), Some(ses)) = (&id, &self.session) {
            let scene = ses.scene.borrow();
            if let Some(room) = scene.room_of(id) {
                out["room"] = json!(room);
            }
            if resolved {
                if let Some(re) = scene.resolved(id) {
                    out["resolved"] = resolved_json(re);
                }
            } else if let Some(doc) = scene.overrides(id) {
                out["overrides"] = overrides_json(doc);
            }
        }
        Ok(out)
    }

    fn world_describe(&mut self, params: &Json) -> Result<Json> {
        let only = json::opt_str(params, "component")?.map(str::to_string);
        if let Some(n) = &only {
            self.world.registry().require(n)?;
        }
        let registry = self.world.registry().clone();
        let mut rows = Vec::new();
        for schema in registry.schemas() {
            if only.as_deref().is_some_and(|n| n != schema.name) {
                continue;
            }
            let cid = registry.id(&schema.name).expect("registered");
            let fields: Vec<Json> = schema
                .fields
                .iter()
                .map(|f| {
                    json!({
                        "name": f.name,
                        "type": f.ty.name(),
                        "default": json::to_json(&self.world, &f.default),
                        "range": f.range.map(|(lo, hi)| json!([lo, hi])),
                        "doc": f.doc,
                        "tune": f.tune,
                    })
                })
                .collect();
            rows.push(json!({
                "name": schema.name,
                "doc": schema.doc,
                "version": schema.version,
                "source": schema.source,
                "kind": if schema.fields.is_empty() { "tag" } else { "data" },
                "fields": fields,
                "written_by": self.sched.who_writes(cid).unwrap_or_default(),
                "read_by": self.sched.who_reads(cid).unwrap_or_default(),
            }));
        }
        let (limit, offset) = query::page(params)?;
        Ok(query::paged(rows, limit, offset))
    }

    fn world_who_writes(&mut self, params: &Json) -> Result<Json> {
        let name = json::str_param(params, "component")?;
        let cid = self.world.registry().require(name)?;
        Ok(json!({
            "component": name,
            "systems": self.sched.who_writes(cid)?,
            "readers": self.sched.who_reads(cid)?,
        }))
    }

    fn world_defs(&self, params: &Json) -> Result<Json> {
        let ses = self.require_session()?;
        let defs = &ses.project.defs;
        let Some(table) = json::opt_str(params, "table")? else {
            let tables: Vec<Json> = defs
                .tables()
                .map(|t| json!({ "name": t.name, "prefix": t.prefix, "key": t.key, "rows": t.rows.len(), "columns": t.columns(), "file": t.file }))
                .collect();
            return Ok(json!({ "tables": tables }));
        };
        let t = defs.table(table).ok_or_else(|| {
            ArgusError::new(
                argus_scene::codes::BAD_DEF,
                format!("no def table {table:?}"),
            )
            .hint(format!(
                "Tables: {}",
                defs.tables()
                    .map(|t| t.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?;
        let mut where_ = Vec::new();
        if let Some(Json::Object(o)) = params.get("where") {
            for (k, v) in o {
                let f = t.schema.field(k).ok_or_else(|| {
                    ArgusError::new(
                        argus_ecs::codes::UNKNOWN_FIELD,
                        format!("{table} has no field {k:?}"),
                    )
                    .hint(format!("Fields: {}", t.columns().join(", ")))
                })?;
                where_.push((k.clone(), json::from_json(&self.world, v, f.ty)?));
            }
        }
        let rows: Vec<Json> = t
            .query(&where_)?
            .into_iter()
            .map(|r| {
                Json::Array(
                    r.values
                        .iter()
                        .map(|v| json::to_json(&self.world, v))
                        .collect(),
                )
            })
            .collect();
        let (limit, offset) = query::page(params)?;
        let mut out = query::paged(rows, limit, offset);
        out["table"] = json!(t.name);
        out["columns"] = json!(t.columns());
        Ok(out)
    }

    // ----- scene ------------------------------------------------------------

    fn scene_list(&self) -> Result<Json> {
        let ses = self.require_session()?;
        Ok(json!({ "scenes": ses.project.scenes(), "current": ses.scene_name }))
    }

    fn scene_load(&mut self, params: &Json) -> Result<Json> {
        // A tuning session's values are committed before the world goes.
        if self.tuning.is_some() {
            self.end_tuning(true, None)?;
        }
        self.trial = None;
        self.forks.clear();
        self.active = "main".to_string();
        let (root, current) = {
            let ses = self.require_session()?;
            (ses.root.clone(), ses.scene_name.clone())
        };
        let name = json::opt_str(params, "scene")?
            .unwrap_or(&current)
            .to_string();
        let seed = match params.get("seed") {
            Some(Json::Number(n)) => Some(n.as_u64().unwrap_or(0)),
            _ => None,
        };
        if params.get("mods").is_some() {
            self.mods_asked = json::str_list(params, "mods")?;
        }
        let cfg = ProjectConfig::load(&root)?;
        // A scene loaded while playing plays on, as a save does: a fresh
        // scene's clock starts stopped, and a person's window froze on
        // `argus scene load mods=[...]` (found by Kevin, 2026-09-28). A
        // scenario's loads start stopped, as they always have.
        let play = self.play_state();
        let r = self.open_scene(root, &cfg, &name, seed)?;
        if play.0 && !self.live_edits {
            self.resume(play)?;
        }
        Ok(r)
    }

    /// Whether the sim is playing, paused, and at what speed.
    fn play_state(&self) -> (bool, bool, f64) {
        (
            self.sched.is_playing(),
            self.sched.is_paused(),
            self.sched.speed(),
        )
    }

    /// Put a play state back on a scene just loaded, whose clock starts
    /// stopped.
    fn resume(&mut self, play: (bool, bool, f64)) -> Result<()> {
        if !play.0 {
            return Ok(());
        }
        if let Some(ses) = &mut self.session {
            ses.play_from = Some(self.sched.tick());
        }
        self.sched.play(&self.world);
        if play.1 {
            self.sched.pause();
        }
        if self.sched.driving().kind == ClockKind::Continuous {
            self.sched.set_speed(play.2)?;
        }
        Ok(())
    }

    fn scene_issues(&self) -> Result<Json> {
        let ses = self.require_session()?;
        let scene = ses.scene.borrow();
        let rows: Vec<Json> = ses
            .project
            .issues()
            .iter()
            .chain(scene.issues())
            .map(json::error_json)
            .collect();
        Ok(json!({ "total": rows.len(), "rows": rows }))
    }

    /// `world.query` over the live world. In a scene with rooms the answer
    /// knows each entity's room (`where.room`, the `room` column); a query
    /// about a room that is not loaded is answered by the index, from the
    /// files (§4: "the index still answers queries about rooms that aren't
    /// loaded"), and a live answer says how many rooms it could not see.
    fn world_query(&mut self, params: &Json) -> Result<Json> {
        let ses = self.require_session()?;
        let (names, loaded) = {
            let sc = ses.scene.borrow();
            if sc.has_rooms() {
                let names: BTreeSet<String> = sc.rooms().map(|r| r.name.clone()).collect();
                let loaded: BTreeSet<String> =
                    sc.loaded_rooms().into_iter().map(str::to_string).collect();
                (Some(names), loaded)
            } else {
                (None, BTreeSet::new())
            }
        };
        let scene_name = ses.scene_name.clone();
        if let Some(names) = &names {
            let asked: Vec<String> = match params.get("where").and_then(|w| w.get("room")) {
                Some(Json::String(s)) => vec![s.clone()],
                Some(Json::Array(a)) => a
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect(),
                _ => Vec::new(),
            };
            if asked
                .iter()
                .any(|r| names.contains(r) && !loaded.contains(r))
            {
                let mut out = self.index()?.query(&scene_name, params)?;
                out["source"] = json!("index");
                return Ok(out);
            }
        }
        if params.get("where").and_then(|w| w.get("within")).is_some() {
            self.ensure_broadphase();
        }
        let mut out = query::run(&self.world, names.as_ref(), params)?;
        if let Some(names) = &names {
            let unloaded = names.difference(&loaded).count();
            if unloaded > 0 {
                out["unloaded_rooms"] = json!(unloaded);
            }
        }
        Ok(out)
    }

    fn scene_rooms(&self, params: &Json) -> Result<Json> {
        let ses = self.require_session()?;
        let s = ses.scene.borrow();
        // One room: what it remembers of its entities while it is out.
        if let Some(room) = json::opt_str(params, "room")? {
            let r = s.room(room).ok_or_else(|| {
                ArgusError::new(
                    argus_scene::codes::UNKNOWN_ROOM,
                    format!("{room:?} is not a room of this scene"),
                )
            })?;
            let mut rows = Vec::new();
            for id in &r.entities {
                let ids = s
                    .resolved(id)
                    .map(|re| {
                        re.flatten()
                            .into_iter()
                            .map(|(e, _)| e.id.clone())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                for id in ids {
                    if let Some(m) = s.memory_of(&id) {
                        let set: Vec<Json> = m
                            .set
                            .iter()
                            .map(|(p, v)| json!([p, json::to_json(&self.world, v)]))
                            .collect();
                        rows.push(json!({
                            "entity": format!("@{id}"),
                            "gone": m.gone,
                            "added": m.added,
                            "removed": m.removed,
                            "set": set,
                        }));
                    }
                }
            }
            return Ok(json!({
                "room": room,
                "loaded": s.loaded_rooms().contains(&room),
                "entities": r.entities.len(),
                "remembered": rows,
            }));
        }
        let loaded: BTreeSet<&str> = s.loaded_rooms().into_iter().collect();
        let graph = s.doc.rooms.clone().unwrap_or_default();
        let rows: Vec<Json> = s
            .rooms()
            .map(|r| {
                let neighbors: Vec<&str> = graph
                    .edges
                    .iter()
                    .filter_map(|(a, b)| {
                        if a == &r.name {
                            Some(b.as_str())
                        } else if b == &r.name {
                            Some(a.as_str())
                        } else {
                            None
                        }
                    })
                    .collect();
                json!({
                    "name": r.name,
                    "loaded": loaded.contains(r.name.as_str()),
                    "always": graph.always.contains(&r.name),
                    "remembered": s.remembered_in(&r.name),
                    "entities": r.entities.len(),
                    "neighbors": neighbors,
                    "bounds": r.doc.bounds,
                    "tileset": r.doc.tileset,
                    "bulk": r.bulk,
                })
            })
            .collect();
        Ok(json!({
            "rooms": rows,
            "current": s.current_room(),
            "radius": graph.radius,
            "start": graph.start,
            "follow": graph.follow,
            "live": loaded.len(),
        }))
    }

    fn scene_transition(&mut self, params: &Json) -> Result<Json> {
        let room = json::str_param(params, "room")?.to_string();
        let ses = self
            .session
            .as_mut()
            .ok_or_else(|| ArgusError::new(codes::NO_PROJECT, "no project is open"))?;
        let mut scene = ses.scene.borrow_mut();
        let before: BTreeSet<String> = scene
            .loaded_rooms()
            .into_iter()
            .map(str::to_string)
            .collect();
        scene.transition(&mut self.world, &room)?;
        let after: BTreeSet<String> = scene
            .loaded_rooms()
            .into_iter()
            .map(str::to_string)
            .collect();
        Ok(json!({
            "loaded": after.difference(&before).collect::<Vec<_>>(),
            "unloaded": before.difference(&after).collect::<Vec<_>>(),
            "current": scene.current_room(),
        }))
    }

    fn scene_tiles_get(&mut self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        let file = json::str_param(params, "file")?.to_string();
        if argus_tiles::MapFile::is_tilemap(&root, &file) {
            let rect = rect_param(params)?;
            let layer = json::opt_str(params, "layer")?.map(str::to_string);
            return self.with_map(&file, |m| {
                let layer = layer.unwrap_or_else(|| m.index.layers[0].name.clone());
                let li = m.layer(&layer)?;
                let cells = m.get(li, rect)?;
                Ok(json!({
                    "file": file,
                    "layer": layer,
                    "rect": { "x": rect.x, "y": rect.y, "w": rect.w, "h": rect.h },
                    "width": m.index.width,
                    "height": m.index.height,
                    "layers": m.index.layers.iter().map(|l| l.name.clone()).collect::<Vec<_>>(),
                    "cells": cells,
                }))
            });
        }
        let ses = self.require_session()?;
        let file = file.as_str();
        let layer = json::opt_str(params, "layer")?;
        let rect = params
            .get("rect")
            .ok_or_else(|| bad_params("rect {x, y, w, h} is required"))?;
        let rect = Rect::new(
            json::u64_param(rect, "x", 0)? as u32,
            json::u64_param(rect, "y", 0)? as u32,
            json::u64_param(rect, "w", 1)? as u32,
            json::u64_param(rect, "h", 1)? as u32,
        );
        let b = Bulk::open(&ses.root, file)?;
        let layer = layer.unwrap_or(
            b.index
                .layers
                .first()
                .map(String::as_str)
                .unwrap_or("default"),
        );
        let cells = b.get(layer, rect)?;
        Ok(json!({
            "file": file,
            "layer": layer,
            "rect": { "x": rect.x, "y": rect.y, "w": rect.w, "h": rect.h },
            "width": b.index.width,
            "height": b.index.height,
            "layers": b.index.layers,
            "cells": cells,
        }))
    }

    // ----- tilemaps ----------------------------------------------------------

    /// Run `f` on an open tilemap, reopened first if its index changed on
    /// disk since it was read.
    fn with_map<R>(
        &mut self,
        file: &str,
        f: impl FnOnce(&mut argus_tiles::MapFile) -> Result<R>,
    ) -> Result<R> {
        let tiles = self.require_session()?.tiles.clone();
        let mut st = tiles.borrow_mut();
        f(st.open(file)?)
    }

    /// Save a map through the store (which remembers our own write).
    fn map_save(&mut self, file: &str) -> Result<argus_tiles::SaveReport> {
        let tiles = self.require_session()?.tiles.clone();
        let rep = tiles.borrow_mut().save(file);
        rep
    }

    fn tiles_create(&mut self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        let file = json::str_param(params, "file")?.to_string();
        if !file.ends_with(".tilemap.toml") {
            return Err(bad_params(format!(
                "file must end in .tilemap.toml; got {file:?}"
            )));
        }
        if root.join(&file).exists() {
            return Err(ArgusError::new(
                argus_tiles::codes::BAD_MAP,
                format!("{file} already exists"),
            )
            .hint("Edit it with edit.tiles, or asset.move it out of the way first."));
        }
        let mut layers = Vec::new();
        for l in params
            .get("layers")
            .and_then(Json::as_array)
            .cloned()
            .unwrap_or_default()
        {
            layers.push(argus_tiles::LayerDef {
                name: l["name"]
                    .as_str()
                    .ok_or_else(|| bad_params("every layer needs a name"))?
                    .to_string(),
                tileset: l["tileset"].as_str().map(str::to_string),
                z: l["z"].as_f64().unwrap_or(0.0) as f32,
                collision: l["collision"].as_bool().unwrap_or(false),
            });
        }
        let map = argus_tiles::MapFile::create(
            &root,
            &file,
            json::u64_param(params, "width", 0)? as u32,
            json::u64_param(params, "height", 0)? as u32,
            json::f64_param(params, "tile_size", 1.0)? as f32,
            json::u64_param(params, "chunk", 64)? as u32,
            json::u64_param(params, "cell", 2)? as u8,
            layers,
        )?;
        self.require_session()?
            .tiles
            .borrow_mut()
            .insert(&file, map);
        self.tiles_info(&file)
    }

    /// `scene.tiles.list`: the tilemaps placed in the world (their Tilemap
    /// entities), where each one's tile (0, 0) is, and its index, for a
    /// painter to turn a point on screen into a cell (phase 4 item 7).
    fn tiles_list(&mut self) -> Result<Json> {
        self.require_session()?;
        let mut placed = Vec::new();
        if let Some(tm) = self.world.component_id_of::<argus_tiles::Tilemap>() {
            for e in self.world.query().has(tm).entities() {
                let Ok(t) = self.world.get::<argus_tiles::Tilemap>(e) else {
                    continue;
                };
                let origin = self
                    .world
                    .get::<argus_ecs::GlobalTransform>(e)
                    .map(|g| [g.pos.x, g.pos.y])
                    .or_else(|_| {
                        self.world
                            .get::<argus_ecs::Transform>(e)
                            .map(|t| [t.pos.x, t.pos.y])
                    })
                    .unwrap_or([0.0, 0.0]);
                placed.push((
                    crate::history::id_of(&self.world, e),
                    self.world.strings().resolve(t.map),
                    origin,
                    t.visible,
                ));
            }
        }
        let mut rows = Vec::new();
        for (entity, file, origin, visible) in placed {
            let mut row = match self.tiles_info(&file) {
                Ok(info) => info,
                Err(e) => json!({ "file": file, "error": e.message }),
            };
            row["entity"] = json!(entity);
            row["origin"] = json!(origin);
            row["visible"] = json!(visible);
            rows.push(row);
        }
        rows.sort_by(|a, b| a["file"].as_str().cmp(&b["file"].as_str()));
        Ok(json!({ "rows": rows }))
    }

    fn tiles_info(&mut self, file: &str) -> Result<Json> {
        let (index, chunks, loaded, tiled) = self.with_map(file, |m| {
            Ok((
                m.index.clone(),
                m.chunk_count(),
                m.loaded(),
                m.tiled().cloned(),
            ))
        })?;
        let ses = self.session.as_ref().expect("session");
        let layers: Vec<Json> = index
            .layers
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let ts = l.tileset.as_deref().and_then(|n| ses.assets.sprites().find(|(k, _)| *k == n).map(|(_, s)| s));
                json!({
                    "name": l.name,
                    "tileset": l.tileset,
                    "z": l.z,
                    "collision": l.collision,
                    "visible": tiled.as_ref().and_then(|t| t.visible.get(i).copied()).unwrap_or(true),
                    "tileset_found": l.tileset.is_none() || ts.is_some(),
                    "tiles": ts.map(|s| s.frames.len()),
                    "terrains": ts.map(|s| s.autotile.keys().cloned().collect::<Vec<_>>()).unwrap_or_default(),
                })
            })
            .collect();
        // A Tiled map (phase 5 item 4): what its file holds, and that it is
        // Tiled's to edit.
        let tiled = tiled.map(|t| {
            json!({
                "map": t.info,
                "read_only": "This map is Tiled's: edit it in Tiled; the engine reads it again when it is saved.",
                "warnings": t.warnings.iter().map(|(m, at)| json!({ "message": m, "at": at })).collect::<Vec<_>>(),
            })
        });
        let mut out = json!({
            "file": file,
            "width": index.width,
            "height": index.height,
            "tile_size": index.tile_size,
            "chunk": index.chunk,
            "cell": index.cell,
            "hash": index.hash,
            "layers": layers,
            "chunks": chunks,
            "loaded": loaded,
        });
        if let Some(t) = tiled {
            out["tiled"] = t;
        }
        Ok(out)
    }

    fn tiles_collision(&mut self, params: &Json) -> Result<Json> {
        let file = json::str_param(params, "file")?.to_string();
        let rect = rect_param(params)?;
        let tiles = self.require_session()?.tiles.clone();
        let mut st = tiles.borrow_mut();
        let (w, h) = {
            let m = st.open(&file)?;
            (m.index.width, m.index.height)
        };
        if rect.w == 0 || rect.h == 0 || rect.x + rect.w > w || rect.y + rect.h > h {
            return Err(ArgusError::new(
                argus_ecs::codes::OUT_OF_RANGE,
                format!(
                    "rect {}x{} at ({}, {}) is outside the {w}x{h} map",
                    rect.w, rect.h, rect.x, rect.y
                ),
            ));
        }
        let g = st.grid(&file, [0.0, 0.0]).expect("opened");
        let rows: Vec<String> = (rect.y..rect.y + rect.h)
            .map(|y| {
                (rect.x..rect.x + rect.w)
                    .map(|x| st.collide(&g, x as i64, y as i64).ch())
                    .collect()
            })
            .collect();
        Ok(
            json!({ "file": file, "rect": { "x": rect.x, "y": rect.y, "w": rect.w, "h": rect.h }, "rows": rows }),
        )
    }

    fn edit_tiles(&mut self, params: &Json) -> Result<Json> {
        let tx = json::u64_param(params, "tx", 0)? as u32;
        let file = json::str_param(params, "file")?.to_string();
        if argus_tiles::tiled::is_tiled(&file) {
            return Err(argus_tiles::tiled::read_only(&file));
        }
        let layer = json::str_param(params, "layer")?.to_string();
        let r = rect_param(params)?;
        let rect = [r.x, r.y, r.w, r.h];
        let op = if let Some(cells) = params.get("cells").and_then(Json::as_array) {
            let cells: Vec<u32> = cells
                .iter()
                .map(|c| c.as_u64().unwrap_or(0) as u32)
                .collect();
            if cells.len() != r.area() {
                return Err(bad_params(format!(
                    "cells has {} ids for a {}x{} rect of {}",
                    cells.len(),
                    r.w,
                    r.h,
                    r.area()
                )));
            }
            Op::Tiles {
                file,
                layer,
                rect,
                cells,
            }
        } else if let Some(t) = params.get("tile").and_then(Json::as_u64) {
            Op::Tiles {
                file,
                layer,
                rect,
                cells: vec![t as u32; r.area()],
            }
        } else if let Some(terrain) = json::opt_str(params, "terrain")? {
            Op::Terrain {
                file,
                layer,
                rect,
                terrain: Some(terrain.to_string()),
            }
        } else if json::bool_param(params, "erase", false)? {
            Op::Terrain {
                file,
                layer,
                rect,
                terrain: None,
            }
        } else {
            return Err(bad_params(
                "edit.tiles needs cells, tile, terrain, or erase",
            ));
        };
        let t = self.edits.tx_mut(tx)?;
        t.ops.push(op);
        t.calls.push(crate::edit::tool_op("edit.tiles", params));
        Ok(json!({ "queued": t.ops.len() }))
    }

    /// Apply a tile op; returns its inverse (the old cells).
    fn apply_tiles(
        &mut self,
        op: &Op,
        touched: &mut BTreeSet<String>,
        files: &mut BTreeSet<String>,
    ) -> Result<Option<Op>> {
        let (file, layer, rect) = match op {
            Op::Tiles {
                file, layer, rect, ..
            }
            | Op::Terrain {
                file, layer, rect, ..
            } => (file.clone(), layer.clone(), *rect),
            _ => return Ok(None),
        };
        let r = argus_tiles::Rect::new(rect[0], rect[1], rect[2], rect[3]);
        let (li, w, h, tileset) = self.with_map(&file, |m| {
            // A runtime map's tiles are the run's (phase 5 item 4): refused
            // before a cell is written, not when the save would fail.
            if m.index.runtime {
                return Err(ArgusError::new(
                    codes::BAD_PARAMS,
                    format!("{file}: a runtime map's tiles are the run's, written by the game's scripts; an edit cannot keep them"),
                )
                .hint("Its layers, tileset, and size are its index's to change; its cells are made each run (q:tiles)."));
            }
            // A Tiled map is Tiled's (phase 5 item 4): refused the same way.
            if m.tiled().is_some() {
                return Err(argus_tiles::tiled::read_only(&file));
            }
            let li = m.layer(&layer)?;
            Ok((
                li,
                m.index.width,
                m.index.height,
                m.index.layers[li].tileset.clone(),
            ))
        })?;
        let inverse = match op {
            Op::Tiles { cells, .. } => {
                let old = self.with_map(&file, |m| {
                    let old = m.get(li, r)?;
                    m.set(li, r, cells)?;
                    Ok(old)
                })?;
                Op::Tiles {
                    file: file.clone(),
                    layer: layer.clone(),
                    rect,
                    cells: old,
                }
            }
            Op::Terrain { terrain, .. } => {
                let tileset = tileset.ok_or_else(|| {
                    ArgusError::new(argus_tiles::codes::NO_TILESET, format!("layer {layer} of {file} has no tileset to auto-tile with"))
                        .hint("Give the layer a tileset (a sprite asset with [autotile.<terrain>] rules), or write tiles with cells or tile.")
                })?;
                let ses = self.session.as_mut().expect("session");
                let ts = ses.assets.sprites().find(|(k, _)| **k == tileset).map(|(_, s)| s.clone()).ok_or_else(|| {
                    ArgusError::new(argus_tiles::codes::NO_TILESET, format!("tileset {tileset:?} is not a sprite asset"))
                        .hint("asset.list kind=sprite shows the sprites; a tileset is a grid sheet with an [autotile] sidecar.")
                })?;
                let rules = argus_tiles::Rules::new(&ts);
                let area = r.grow(1, w, h);
                let old = self.with_map(&file, |m| {
                    let old = m.get(li, area)?;
                    argus_tiles::paint(m, li, r, terrain.as_deref(), &rules)?;
                    Ok(old)
                })?;
                Op::Tiles {
                    file: file.clone(),
                    layer: layer.clone(),
                    rect: [area.x, area.y, area.w, area.h],
                    cells: old,
                }
            }
            _ => unreachable!(),
        };
        let rep = self.map_save(&file)?;
        for f in rep.files {
            files.insert(f);
        }
        touched.insert(format!("{file}#{layer}"));
        Ok(Some(inverse))
    }

    /// The solid tiles' edges where they can shadow a view, for the
    /// renderer's 2D lighting (none with lighting off).
    #[cfg(feature = "render")]
    fn tile_occluders(&mut self, view: ([f32; 2], [f32; 2])) -> Vec<argus_light::Edge> {
        let Some(rect) = argus_light::reach(&self.world, view.0, view.1) else {
            return Vec::new();
        };
        let Some(tiles) = self.session.as_ref().map(|s| s.tiles.clone()) else {
            return Vec::new();
        };
        let mut st = tiles.borrow_mut();
        argus_light::tile_edges(&self.world, &mut st, rect)
    }

    /// Tiles every visible Tilemap entity puts in a view.
    #[cfg(feature = "render")]
    fn tile_extras(&mut self, view: ([f32; 2], [f32; 2])) -> Vec<argus_render::Extra> {
        let Some(tm) = self.world.component_id_of::<argus_tiles::Tilemap>() else {
            return Vec::new();
        };
        let mut maps = Vec::new();
        for e in self.world.query().has(tm).entities() {
            let Ok(t) = self.world.get::<argus_tiles::Tilemap>(e) else {
                continue;
            };
            if !t.visible {
                continue;
            }
            let origin = self
                .world
                .get::<argus_ecs::GlobalTransform>(e)
                .map(|g| [g.pos.x, g.pos.y])
                .or_else(|_| {
                    self.world
                        .get::<argus_ecs::Transform>(e)
                        .map(|t| [t.pos.x, t.pos.y])
                })
                .unwrap_or([0.0, 0.0]);
            maps.push((self.world.strings().resolve(t.map), origin));
        }
        let mut out = Vec::new();
        for (file, origin) in maps {
            let Ok(tiles) =
                self.with_map(&file, |m| argus_tiles::visible(m, origin, view.0, view.1))
            else {
                continue;
            };
            out.extend(tiles.into_iter().map(|t| argus_render::Extra {
                atlas: t.tileset,
                frame: t.frame,
                pos: t.pos,
                size: [t.size, t.size],
                z: t.z,
            }));
        }
        out
    }

    // ----- edit -------------------------------------------------------------

    /// A project file named by an agent: its project-relative path and
    /// where it is. Inside the project, and not the engine's own folders
    /// (`.argus/`, `.git/`) or a build (`dist/`).
    fn project_file(&self, rel: &str) -> Result<(String, PathBuf)> {
        let s = self.require_session()?;
        let bad = |why: &str| {
            ArgusError::new(codes::BAD_PARAMS, format!("{rel:?}: {why}"))
                .hint("A path in the project, relative to its folder: systems/move.luau, scenes/main/player.toml, tests/move.luau.")
        };
        let clean = rel.replace('\\', "/");
        let clean = clean.trim_start_matches("./");
        if clean.is_empty() || clean.starts_with('/') || clean.contains(':') {
            return Err(bad("not a path relative to the project"));
        }
        let parts: Vec<&str> = clean.split('/').filter(|p| !p.is_empty()).collect();
        if parts.iter().any(|p| *p == ".." || *p == ".") {
            return Err(bad("a path in the project does not leave it"));
        }
        // As the disk would read the name: a Mac's and Windows' disks ignore
        // case (`.GIT` is `.git`), Windows drops a name's trailing dots and
        // spaces (`.git.`), and its short names alias these (`GIT~1`).
        let first = parts
            .first()
            .map(|p| p.trim_end_matches(['.', ' ']).to_ascii_lowercase())
            .unwrap_or_default();
        if matches!(first.as_str(), ".argus" | ".git" | "dist")
            || first.starts_with("git~")
            || first.starts_with("argus~")
        {
            return Err(bad(
                "the engine's own folders and builds are not the project's files",
            ));
        }
        let rel = parts.join("/");
        Ok((rel.clone(), s.root.join(&rel)))
    }

    /// `project.read`: a project file's text, or a folder's listing.
    fn project_read(&self, params: &Json) -> Result<Json> {
        let path = json::opt_str(params, "path")?.unwrap_or("");
        let s = self.require_session()?;
        let (rel, full) = if path.is_empty() || path == "." {
            (String::new(), s.root.clone())
        } else {
            self.project_file(path)?
        };
        if full.is_dir() {
            let mut entries: Vec<Json> = std::fs::read_dir(&full)
                .map_err(|e| ArgusError::new(codes::BAD_PARAMS, format!("{rel}: {e}")))?
                .flatten()
                .filter(|e| {
                    let n = e.file_name();
                    let n = n.to_string_lossy();
                    !(rel.is_empty() && matches!(n.as_ref(), ".argus" | ".git" | "dist"))
                })
                .map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let meta = e.metadata().ok();
                    if meta.as_ref().is_some_and(|m| m.is_dir()) {
                        json!({ "name": format!("{name}/") })
                    } else {
                        json!({ "name": name, "bytes": meta.map_or(0, |m| m.len()) })
                    }
                })
                .collect();
            entries.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
            return Ok(
                json!({ "path": if rel.is_empty() { "." } else { rel.as_str() }, "entries": entries }),
            );
        }
        let bytes = std::fs::read(&full).map_err(|e| {
            ArgusError::new(codes::BAD_PARAMS, format!("{rel}: {e}"))
                .hint("project.read with a folder (or none) lists what is there.")
        })?;
        if bytes.len() > FILE_MAX {
            return Err(ArgusError::new(
                codes::BAD_PARAMS,
                format!(
                    "{rel} is {} bytes, more than a read gives ({FILE_MAX})",
                    bytes.len()
                ),
            ));
        }
        match String::from_utf8(bytes) {
            Ok(text) => Ok(json!({ "path": rel, "text": text })),
            Err(e) => Ok(json!({ "path": rel, "bytes": e.as_bytes().len(), "binary": true })),
        }
    }

    fn edit_begin(&mut self, params: &Json) -> Result<Json> {
        self.require_session()?;
        let author = json::str_param(params, "author")?;
        let note = json::opt_str(params, "note")?.unwrap_or("");
        Ok(json!({ "tx": self.edits.begin(author, note) }))
    }

    fn edit_queue(&mut self, method: &str, params: &Json) -> Result<Json> {
        let tx = json::u64_param(params, "tx", 0)? as u32;
        let op = match method {
            "edit.file" => {
                let file = json::str_param(params, "file")?;
                let (rel, _) = self.project_file(file)?;
                let text = match params.get("text") {
                    None | Some(Json::Null) => None,
                    Some(Json::String(t)) => Some(t.clone()),
                    Some(other) => {
                        return Err(ArgusError::new(
                            codes::BAD_PARAMS,
                            format!(
                            "edit.file text is a string (or null to remove the file), not {other}"
                        ),
                        ))
                    }
                };
                if text.as_ref().is_some_and(|t| t.len() > FILE_MAX) {
                    return Err(ArgusError::new(
                        codes::BAD_PARAMS,
                        format!(
                            "{rel}: {} bytes is more than a file edit takes ({FILE_MAX})",
                            text.as_ref().map_or(0, |t| t.len())
                        ),
                    ));
                }
                Op::File { file: rel, text }
            }
            "edit.set" => Op::Set {
                entity: json::str_param(params, "entity")?.to_string(),
                path: json::str_param(params, "path")?.to_string(),
                value: params.get("value").cloned().unwrap_or(Json::Null),
            },
            "edit.add" => Op::Add {
                entity: json::str_param(params, "entity")?.to_string(),
                component: json::str_param(params, "component")?.to_string(),
            },
            "edit.remove" => Op::Remove {
                entity: json::str_param(params, "entity")?.to_string(),
                component: json::str_param(params, "component")?.to_string(),
            },
            "edit.spawn" => Op::Spawn {
                id: json::opt_str(params, "id")?.map(str::to_string),
                room: json::opt_str(params, "room")?.map(str::to_string),
                prefab: json::opt_str(params, "prefab")?.map(str::to_string),
                tags: json::str_list(params, "tags")?,
                components: components_param(params)?,
            },
            "edit.def" => Op::Def {
                def: json::str_param(params, "def")?.to_string(),
                path: json::str_param(params, "path")?.to_string(),
                value: params.get("value").cloned().unwrap_or(Json::Null),
            },
            "edit.def_row" => {
                let def = json::str_param(params, "def")?.to_string();
                let fields = if json::bool_param(params, "remove", false)? {
                    None
                } else {
                    match params.get("fields") {
                        Some(f @ Json::Object(_)) => Some(f.clone()),
                        _ => {
                            return Err(bad_params("give fields: {field: value}, or remove: true")
                                .hint("edit.def_row {tx, def: \"timeline:outro.flash\", fields: {timeline: \"timeline:outro\", kind: \"event\", at: 2}}"))
                        }
                    }
                };
                // The table must be one there is, now.
                let prefix = def.split_once(':').map(|(p, _)| p).unwrap_or("");
                let ses = self.require_session()?;
                if ses.project.defs.table_for_prefix(prefix).is_none() || !def.contains(':') {
                    return Err(
                        bad_params(format!("{def:?} is not prefix:key of a def table"))
                            .hint("A row is prefix:key: timeline:outro.flash, item:meal."),
                    );
                }
                Op::DefRow { def, fields }
            }
            "edit.asset" => {
                let asset = json::str_param(params, "asset")?.to_string();
                let path = json::str_param(params, "path")?.to_string();
                let value = if json::bool_param(params, "remove", false)? {
                    None
                } else {
                    Some(params.get("value").cloned().ok_or_else(|| {
                        bad_params("give value, or remove: true")
                            .hint("edit.asset {tx, asset, path, value}")
                    })?)
                };
                // Checked now, so a path that cannot be made is refused
                // when queued, not at commit.
                let (_, _, text) = self.sidecar_of(&asset)?;
                crate::daemon::sidecar::edit_text(&text, &path, value.as_ref())?;
                Op::Asset { asset, path, value }
            }
            "edit.prefab" => {
                let prefab = json::str_param(params, "prefab")?.to_string();
                let child = json::opt_str(params, "child")?.unwrap_or("").to_string();
                let path = json::str_param(params, "path")?.to_string();
                let value = if json::bool_param(params, "remove", false)? {
                    None
                } else {
                    Some(params.get("value").cloned().ok_or_else(|| {
                        bad_params("give value, or remove: true")
                            .hint("edit.prefab {tx, prefab, path, value}")
                    })?)
                };
                // Checked now, so a prefab or a field there is not is refused
                // when queued, not at commit.
                let ses = self.require_session()?;
                if ses.project.prefabs.get(&prefab).is_none() {
                    return Err(bad_params(format!("there is no prefab {prefab:?}")).hint(
                        "world.prefabs lists the prefabs; a prefab is a file under prefabs/.",
                    ));
                }
                self.world.registry().resolve_path(&path)?;
                Op::Prefab {
                    prefab,
                    child,
                    path,
                    value,
                }
            }
            _ => Op::Despawn {
                entity: json::str_param(params, "entity")?.to_string(),
            },
        };
        let t = self.edits.tx_mut(tx)?;
        t.ops.push(op);
        t.calls.push(crate::edit::tool_op(method, params));
        Ok(json!({ "queued": t.ops.len() }))
    }

    fn edit_abort(&mut self, params: &Json) -> Result<Json> {
        let tx = json::u64_param(params, "tx", 0)? as u32;
        let t = self.edits.take(tx)?;
        Ok(json!({ "aborted": t.ops.len() }))
    }

    fn edit_commit(&mut self, params: &Json) -> Result<Json> {
        let tx = json::u64_param(params, "tx", 0)? as u32;
        let t = self.edits.take(tx)?;
        // The row's command is the whole transaction as the agent's `edit`
        // tool: `edit commit tx=4` alone would not say what changed.
        let line = argus_client::command::line(
            "edit",
            &json!({ "author": t.author, "note": t.note, "ops": t.calls }),
        );
        self.apply_commit_as(&t.author, &t.note, t.ops, CommitKind::Normal, line)
    }

    fn edit_undo(&mut self, params: &Json, redo: bool) -> Result<Json> {
        let author = json::str_param(params, "author")?.to_string();
        let stack = if redo {
            &mut self.edits.redo
        } else {
            &mut self.edits.undo
        };
        let id = stack.get_mut(&author).and_then(Vec::pop).ok_or_else(|| {
            ArgusError::new(
                codes::NOTHING_TO_UNDO,
                format!(
                    "nothing to {} for {author}",
                    if redo { "redo" } else { "undo" }
                ),
            )
            .hint("Undo is scoped per author; edit.feed shows whose commits exist.")
        })?;
        let c = self.edits.commit(id).expect("commit exists").clone();
        let kind = if redo {
            CommitKind::Redo(id)
        } else {
            CommitKind::Undo(id)
        };
        let note = format!("{} #{}: {}", if redo { "redo" } else { "undo" }, id, c.note);
        self.apply_commit(&author, &note, c.inverse, kind)
    }

    /// Apply operations atomically: on failure the ones already applied
    /// are reverted with their inverses and the error is returned.
    fn apply_commit(
        &mut self,
        author: &str,
        note: &str,
        ops: Vec<Op>,
        kind: CommitKind,
    ) -> Result<Json> {
        let line = self.call_line();
        self.apply_commit_as(author, note, ops, kind, line)
    }

    /// [`Daemon::apply_commit`] with the feed row's command line given.
    fn apply_commit_as(
        &mut self,
        author: &str,
        note: &str,
        ops: Vec<Op>,
        kind: CommitKind,
        command: String,
    ) -> Result<Json> {
        self.require_session()?;
        let forward = ops.clone();
        let mut inverse: Vec<Op> = Vec::new();
        let mut summaries = Vec::new();
        let mut touched: BTreeSet<String> = BTreeSet::new();
        let mut files: BTreeSet<String> = BTreeSet::new();
        // Tune fields this commit changes, for the tuning log (item 5): not
        // a test's live edits, not a tuning session's own commit.
        let log_tune = !self.live_edits && !self.tune_committing;
        let mut tuned: Vec<(String, String, Json, Json)> = Vec::new();
        for op in ops {
            summaries.push(op.summary());
            match self.apply_op(author, &op, &mut touched, &mut files) {
                Ok(Some(inv)) => {
                    if let (true, Op::Set { path, .. }, Op::Set { entity, value, .. }) =
                        (log_tune, &op, &inv)
                    {
                        if self.is_tune(path) {
                            let now = self
                                .world
                                .entity(entity)
                                .and_then(|e| self.world.get_field(e, path))
                                .map(|v| json::to_json(&self.world, &v))
                                .unwrap_or(Json::Null);
                            tuned.push((entity.clone(), path.clone(), value.clone(), now));
                        }
                    }
                    // A def row's tune field, as a component's (phase 4 item 3).
                    if let (true, Op::Def { path, .. }, Op::Def { def, value, .. }) =
                        (log_tune, &op, &inv)
                    {
                        if self.def_field(def, path).is_ok_and(|(_, f)| f.tune) {
                            let now = self
                                .def_field(def, path)
                                .map(|(v, _)| json::to_json(&self.world, &v))
                                .unwrap_or(Json::Null);
                            tuned.push((def.clone(), path.clone(), value.clone(), now));
                        }
                    }
                    // An effect's knobs, from its sidecar as it was (phase 5 item 3).
                    if let (true, Op::AssetText { asset, text }) = (log_tune, &inv) {
                        tuned.extend(self.knob_changes(asset, text));
                    }
                    inverse.push(inv)
                }
                Ok(None) => {}
                Err(e) => {
                    for inv in inverse.iter().rev() {
                        let _ =
                            self.apply_op(author, inv, &mut BTreeSet::new(), &mut BTreeSet::new());
                    }
                    return Err(e);
                }
            }
        }
        inverse.reverse();
        let id = self.edits.next_commit_id();
        let commit = Commit {
            id,
            author: author.to_string(),
            note: note.to_string(),
            frame: self.world.frame(),
            kind: kind.clone(),
            ops: summaries,
            inverse,
            touched: touched.into_iter().collect(),
            files: files.into_iter().collect(),
            command,
            call: None,
        };
        let row = commit.to_json();
        self.index_touched(&commit.touched);
        match kind {
            CommitKind::Normal => {
                self.edits
                    .undo
                    .entry(author.to_string())
                    .or_default()
                    .push(id);
                self.edits.redo.remove(author);
            }
            CommitKind::Undo(_) => self
                .edits
                .redo
                .entry(author.to_string())
                .or_default()
                .push(id),
            CommitKind::Redo(_) => self
                .edits
                .undo
                .entry(author.to_string())
                .or_default()
                .push(id),
            CommitKind::Proposed(_) | CommitKind::Rejected(_) | CommitKind::Command => {}
        }
        let via = match &commit.kind {
            CommitKind::Undo(_) => "undo",
            CommitKind::Redo(_) => "redo",
            _ => "edit",
        };
        if !self.live_edits
            && matches!(
                kind,
                CommitKind::Normal | CommitKind::Undo(_) | CommitKind::Redo(_)
            )
        {
            if let Some(ses) = self.session.as_mut().filter(|s| s.play_from.is_some()) {
                ses.kept_in_play.push((author.to_string(), forward));
            }
        }
        self.edits.feed.push(commit);
        self.feed_rows += 1;
        self.notify("edits", "edit.committed", row.clone());
        self.log_tuned(author, via, note, tuned);
        self.pump_events();
        Ok(row)
    }

    /// Apply an `edit.prefab` op (or its inverse, the file as it was):
    /// the prefab's file, then every instance in the scene resolved again,
    /// what changed written to the live ones, each one touched (§3: "the
    /// change feed reports how many entities the change touched").
    fn apply_prefab(
        &mut self,
        author: &str,
        op: &Op,
        touched: &mut BTreeSet<String>,
        files: &mut BTreeSet<String>,
    ) -> Result<Option<Op>> {
        let registry = self.world.registry().clone();
        if let Op::Prefab { prefab, path, .. } = op {
            self.edits
                .check_lock(author, &format!("prefab:{prefab}"), path)?;
        }
        let file_value = match op {
            Op::Prefab {
                path,
                value: Some(v),
                ..
            } => {
                let (_, fs) = registry.resolve_path(path)?;
                Some(match fs.ty {
                    // A file names an entity as its "@id".
                    argus_ecs::FieldType::Entity => {
                        Value::Str(v.as_str().unwrap_or("").to_string())
                    }
                    _ => json::from_json(&self.world, v, fs.ty)?,
                })
            }
            _ => None,
        };
        let ses = self.session.as_mut().expect("session");
        let (prefab, old) = match op {
            Op::Prefab {
                prefab,
                child,
                path,
                ..
            } => {
                let old = ses.project.prefabs.set_field(
                    &registry,
                    prefab,
                    child,
                    path,
                    file_value.as_ref(),
                )?;
                (prefab.clone(), old)
            }
            Op::PrefabText { prefab, text } => {
                let file = ses
                    .project
                    .prefabs
                    .get(prefab)
                    .map(|p| ses.project.root.join(&p.file));
                let old = file
                    .and_then(|f| std::fs::read_to_string(f).ok())
                    .unwrap_or_default();
                ses.project.prefabs.put_text(prefab, text)?;
                (prefab.clone(), old)
            }
            _ => return Ok(None),
        };
        if let Some(p) = ses.project.prefabs.get(&prefab) {
            files.insert(p.file.clone());
        }
        let ids = ses
            .scene
            .borrow_mut()
            .reapply_prefab(&ses.project, &mut self.world, &prefab)?;
        // Scripts spawn the prefab as it is now.
        ses.host
            .set_prefabs(Rc::new(argus_player::ProjectPrefabs::new(&ses.project)));
        touched.extend(ids.into_iter().map(|id| format!("@{id}")));
        touched.insert(format!("prefab:{prefab}"));
        Ok(Some(Op::PrefabText { prefab, text: old }))
    }

    /// Apply one op; returns its inverse.
    fn apply_op(
        &mut self,
        author: &str,
        op: &Op,
        touched: &mut BTreeSet<String>,
        files: &mut BTreeSet<String>,
    ) -> Result<Option<Op>> {
        match op {
            Op::Tiles { .. } | Op::Terrain { .. } => self.apply_tiles(op, touched, files),
            Op::File { file, text } => {
                let (rel, path) = self.project_file(file)?;
                let old = match std::fs::read(&path) {
                    Ok(bytes) => Some(String::from_utf8(bytes).map_err(|_| {
                        ArgusError::new(
                            codes::BAD_PARAMS,
                            format!("{rel} is not text, so a file edit does not replace it"),
                        )
                        .hint("Files written this way are text: scripts, scene and entity files, tests, sidecars.")
                    })?),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                    Err(e) => return Err(ArgusError::new(codes::BAD_PARAMS, format!("{rel}: {e}"))),
                };
                match text {
                    Some(t) => {
                        if let Some(d) = path.parent() {
                            std::fs::create_dir_all(d).map_err(|e| {
                                ArgusError::new(codes::BAD_PARAMS, format!("{rel}: {e}"))
                            })?;
                        }
                        std::fs::write(&path, t).map_err(|e| {
                            ArgusError::new(codes::BAD_PARAMS, format!("{rel}: {e}"))
                        })?;
                    }
                    None if old.is_some() => std::fs::remove_file(&path)
                        .map_err(|e| ArgusError::new(codes::BAD_PARAMS, format!("{rel}: {e}")))?,
                    None => {}
                }
                touched.insert(format!("file:{rel}"));
                files.insert(rel.clone());
                Ok(Some(Op::File {
                    file: rel,
                    text: old,
                }))
            }
            Op::Text { .. } => self.apply_text(op, touched, files),
            Op::Def { .. } => self.apply_def(author, op, touched, files),
            Op::Asset { .. } | Op::AssetText { .. } => self.apply_asset(author, op, touched, files),
            Op::Prefab { .. } | Op::PrefabText { .. } => {
                self.apply_prefab(author, op, touched, files)
            }
            Op::DefRow { .. } | Op::DefText { .. } => {
                self.apply_def_row(author, op, touched, files)
            }
            Op::Set {
                entity,
                path,
                value,
            } => {
                let e = self.world.entity(entity)?;
                let eref = json::entity_str(&self.world, e);
                self.edits.check_lock(author, &eref, path)?;
                let (cid, fs) = self.world.registry().resolve_path(path)?;
                let (fs, cid) = (fs.clone(), cid);
                let old = self.world.get_field(e, path)?;
                let v = json::from_json(&self.world, value, fs.ty)?;
                self.world.set_field_by(e, cid, &fs, v)?;
                if let Some(id) = self.world.string_id(e).map(str::to_string) {
                    let ses = self.session.as_mut().expect("session");
                    let file_value = match fs.ty {
                        argus_ecs::FieldType::Entity => {
                            Value::Str(value.as_str().unwrap_or("").to_string())
                        }
                        _ => json::from_json(&self.world, value, fs.ty)?,
                    };
                    let registry = self.world.registry().clone();
                    let ctx = ses.project.ctx(&registry);
                    let mut scene = ses.scene.borrow_mut();
                    if scene.is_authored(&id) && !self.live_edits {
                        scene.set_override(&ctx, &id, path, &file_value)?;
                        if let Some(f) = scene.entity_file(&id).or_else(|| {
                            let (fid, _) = split_child(&scene, &id);
                            scene.entity_file(&fid)
                        }) {
                            files.insert(f.rel.clone());
                        }
                    }
                }
                touched.insert(eref.clone());
                Ok(Some(Op::Set {
                    entity: eref,
                    path: path.clone(),
                    value: json::to_json(&self.world, &old),
                }))
            }
            Op::Add { entity, component } => {
                let e = self.world.entity(entity)?;
                let eref = json::entity_str(&self.world, e);
                self.edits.check_lock(author, &eref, component)?;
                let cid = self.world.registry().require(component)?;
                if self.world.has(e, cid) {
                    return Ok(None);
                }
                self.world.insert_by_name(e, component)?;
                self.authored_edit(e, files, |scene, ctx, id| {
                    scene.add_component(ctx, id, component)
                })?;
                touched.insert(eref.clone());
                Ok(Some(Op::Remove {
                    entity: eref,
                    component: component.clone(),
                }))
            }
            Op::Remove { entity, component } => {
                let e = self.world.entity(entity)?;
                let eref = json::entity_str(&self.world, e);
                self.edits.check_lock(author, &eref, component)?;
                let cid = self.world.registry().require(component)?;
                if !self.world.has(e, cid) {
                    return Ok(None);
                }
                let fields: Vec<(String, Json)> = self
                    .world
                    .get_component(e, cid)?
                    .into_iter()
                    .map(|(k, v)| (k, json::to_json(&self.world, &v)))
                    .collect();
                self.world.remove(e, cid)?;
                self.authored_edit(e, files, |scene, ctx, id| {
                    scene.remove_component(ctx, id, component)
                })?;
                touched.insert(eref.clone());
                Ok(Some(Op::Restore {
                    entity: eref,
                    component: component.clone(),
                    fields,
                }))
            }
            Op::Restore {
                entity,
                component,
                fields,
            } => {
                let e = self.world.entity(entity)?;
                let eref = json::entity_str(&self.world, e);
                let cid = self.world.registry().require(component)?;
                if !self.world.has(e, cid) {
                    self.world.insert_by_name(e, component)?;
                    self.authored_edit(e, files, |scene, ctx, id| {
                        scene.add_component(ctx, id, component)
                    })?;
                }
                for (f, v) in fields {
                    let path = format!("{component}.{f}");
                    let (cid, fs) = self.world.registry().resolve_path(&path)?;
                    let fs = fs.clone();
                    let val = json::from_json(&self.world, v, fs.ty)?;
                    self.world.set_field_by(e, cid, &fs, val.clone())?;
                    if self.world.is_authored(e) {
                        let file_value = match fs.ty {
                            argus_ecs::FieldType::Entity => {
                                Value::Str(v.as_str().unwrap_or("").to_string())
                            }
                            _ => val,
                        };
                        self.authored_edit(e, files, |scene, ctx, id| {
                            scene.set_override(ctx, id, &path, &file_value)
                        })?;
                    }
                }
                touched.insert(eref.clone());
                Ok(Some(Op::Remove {
                    entity: eref,
                    component: component.clone(),
                }))
            }
            Op::Spawn {
                id,
                room,
                prefab,
                tags,
                components,
            } => {
                let e = match id {
                    None => {
                        let e = self.world.spawn();
                        self.fill_entity(e, tags, components)?;
                        e
                    }
                    Some(local) if self.replaying_kept => {
                        let full = match room {
                            Some(r) => format!("{r}/{local}"),
                            None => local.clone(),
                        };
                        let ses = self.session.as_mut().expect("session");
                        let mut scene = ses.scene.borrow_mut();
                        scene.spawn_entity(&mut self.world, &full)?
                    }
                    Some(local) => {
                        self.edits.check_lock(author, &format!("@{local}"), "")?;
                        if self.live_edits {
                            return Err(ArgusError::new(
                                codes::BAD_PARAMS,
                                format!("a test or a preview cannot spawn @{local}: an id is a file in the scene"),
                            )
                            .hint("Tests and proposals' previews edit the live world, not the project's files; spawn without an id, or commit it."));
                        }
                        let mut doc = EntityDoc {
                            prefab: prefab.clone(),
                            tags: tags.clone(),
                            ..Default::default()
                        };
                        for (name, fields) in components {
                            let mut cd = argus_scene::ComponentDoc {
                                name: name.clone(),
                                fields: Vec::new(),
                                at: String::new(),
                            };
                            for (f, v) in fields {
                                let path = format!("{name}.{f}");
                                let (_, fs) = self.world.registry().resolve_path(&path)?;
                                let val = match fs.ty {
                                    argus_ecs::FieldType::Entity => {
                                        Value::Str(v.as_str().unwrap_or("").to_string())
                                    }
                                    _ => json::from_json(&self.world, v, fs.ty)?,
                                };
                                cd.fields.push(argus_scene::FieldDoc {
                                    name: f.clone(),
                                    raw: argus_scene::tomlv::from_value(&val)?,
                                    at: String::new(),
                                });
                            }
                            doc.components.push(cd);
                        }
                        self.spawn_doc(local, room.as_deref(), &doc, files)?
                    }
                };
                let eref = json::entity_str(&self.world, e);
                touched.insert(eref.clone());
                Ok(Some(Op::Despawn { entity: eref }))
            }
            Op::SpawnDoc { local, room, .. } if self.replaying_kept => {
                let full = match room {
                    Some(r) => format!("{r}/{local}"),
                    None => local.clone(),
                };
                let ses = self.session.as_mut().expect("session");
                let e = ses
                    .scene
                    .borrow_mut()
                    .spawn_entity(&mut self.world, &full)?;
                let eref = json::entity_str(&self.world, e);
                touched.insert(eref.clone());
                Ok(Some(Op::Despawn { entity: eref }))
            }
            Op::SpawnDoc { local, room, doc } => {
                let e = self.spawn_doc(local, room.as_deref(), doc, files)?;
                let eref = json::entity_str(&self.world, e);
                touched.insert(eref.clone());
                Ok(Some(Op::Despawn { entity: eref }))
            }
            Op::Despawn { entity } => {
                let e = self.world.entity(entity)?;
                let eref = json::entity_str(&self.world, e);
                self.edits.check_lock(author, &eref, "")?;
                touched.insert(eref.clone());
                let authored = if self.live_edits {
                    None
                } else {
                    self.world.string_id(e).map(str::to_string)
                };
                if let Some(id) = authored {
                    let ses = self.session.as_mut().expect("session");
                    let mut scene = ses.scene.borrow_mut();
                    let f = scene.entity_file(&id).ok_or_else(|| {
                        ArgusError::new(
                            argus_scene::codes::BAD_SCENE,
                            format!("@{id} is a child entity; remove it from its parent's file with -children"),
                        )
                    })?;
                    let (local, room, doc, rel) = (
                        f.local.clone(),
                        f.room.clone(),
                        f.doc.clone(),
                        f.rel.clone(),
                    );
                    scene.delete_entity(&mut self.world, &id)?;
                    files.insert(rel);
                    Ok(Some(Op::SpawnDoc { local, room, doc }))
                } else {
                    let mut tags = Vec::new();
                    let mut components = Vec::new();
                    for cid in self.world.components_of(e)?.to_vec() {
                        let schema = self.world.registry().schema(cid);
                        if schema.fields.is_empty() {
                            tags.push(schema.name.clone());
                        } else {
                            let fields = self
                                .world
                                .get_component(e, cid)?
                                .into_iter()
                                .map(|(k, v)| (k, json::to_json(&self.world, &v)))
                                .collect();
                            components.push((schema.name.clone(), fields));
                        }
                    }
                    self.world.despawn(e)?;
                    Ok(Some(Op::Respawn { tags, components }))
                }
            }
            Op::Respawn { tags, components } => {
                let e = self.world.spawn();
                self.fill_entity(e, tags, components)?;
                let eref = json::entity_str(&self.world, e);
                touched.insert(eref.clone());
                Ok(Some(Op::Despawn { entity: eref }))
            }
        }
    }

    fn fill_entity(
        &mut self,
        e: Entity,
        tags: &[String],
        components: &[(String, Vec<(String, Json)>)],
    ) -> Result<()> {
        for t in tags {
            self.world.insert_by_name(e, t)?;
        }
        for (name, fields) in components {
            self.world.insert_by_name(e, name)?;
            for (f, v) in fields {
                let path = format!("{name}.{f}");
                let (cid, fs) = self.world.registry().resolve_path(&path)?;
                let fs = fs.clone();
                let val = json::from_json(&self.world, v, fs.ty)?;
                self.world.set_field_by(e, cid, &fs, val)?;
            }
        }
        Ok(())
    }

    fn spawn_doc(
        &mut self,
        local: &str,
        room: Option<&str>,
        doc: &EntityDoc,
        files: &mut BTreeSet<String>,
    ) -> Result<Entity> {
        let ses = self.session.as_mut().expect("session");
        let registry = self.world.registry().clone();
        let ctx = ses.project.ctx(&registry);
        let mut scene = ses.scene.borrow_mut();
        let full = scene.create_entity(&ctx, local, room, doc)?;
        if let Some(issue) = scene.issues().first() {
            let issue = issue.clone();
            let _ = scene.delete_entity(&mut self.world, &full);
            return Err(issue);
        }
        if let Some(f) = scene.entity_file(&full) {
            files.insert(f.rel.clone());
        }
        scene.spawn_entity(&mut self.world, &full)
    }

    /// Make again, on the world `sim.stop` put back, what the person kept
    /// while playing (found by Kevin's hand check of the phase 4 gate:
    /// Keep wrote the colony's hunger to its file, and Stop put the world
    /// back to the hunger it had, so the game and the file disagreed). Only
    /// what the world holds: defs, assets, strings, and tile maps are not
    /// in keyframes and stayed. World only, since the files have it.
    fn make_kept_again(&mut self, kept: &[(String, Vec<Op>)]) {
        if kept.is_empty() {
            return;
        }
        let (live, replaying) = (self.live_edits, self.replaying_kept);
        self.live_edits = true;
        self.replaying_kept = true;
        for (author, ops) in kept {
            for op in ops {
                let in_world = matches!(
                    op,
                    Op::Set { .. }
                        | Op::Add { .. }
                        | Op::Remove { .. }
                        | Op::Spawn { .. }
                        | Op::SpawnDoc { .. }
                        | Op::Despawn { .. }
                );
                if in_world {
                    // Best effort: an entity the change was about may not
                    // be in the world play started from.
                    let _ = self.apply_op(author, op, &mut BTreeSet::new(), &mut BTreeSet::new());
                }
            }
        }
        self.live_edits = live;
        self.replaying_kept = replaying;
        self.mutations += 1;
    }

    /// Run a scene file edit for an authored entity, if `e` is one.
    fn authored_edit(
        &mut self,
        e: Entity,
        files: &mut BTreeSet<String>,
        f: impl FnOnce(&mut Scene, &argus_scene::resolve::ResolveCtx<'_>, &str) -> Result<()>,
    ) -> Result<()> {
        let Some(id) = self.world.string_id(e).map(str::to_string) else {
            return Ok(());
        };
        if self.live_edits {
            return Ok(());
        }
        let ses = self.session.as_mut().expect("session");
        let mut scene = ses.scene.borrow_mut();
        if !scene.is_authored(&id) {
            return Ok(());
        }
        let registry = self.world.registry().clone();
        let ctx = ses.project.ctx(&registry);
        f(&mut scene, &ctx, &id)?;
        let (fid, _) = split_child(&scene, &id);
        if let Some(file) = scene.entity_file(&fid) {
            files.insert(file.rel.clone());
        }
        Ok(())
    }

    fn edit_feed(&self, params: &Json) -> Result<Json> {
        let since = json::u64_param(params, "since", 0)? as u32;
        let author = json::opt_str(params, "author")?;
        let rows: Vec<Json> = self
            .edits
            .feed
            .iter()
            .filter(|c| c.id > since && author.is_none_or(|a| a == c.author))
            .map(Commit::to_json)
            .collect();
        let (limit, offset) = query::page(params)?;
        Ok(query::paged(rows, limit, offset))
    }

    fn edit_lock(&mut self, params: &Json) -> Result<Json> {
        self.require_session()?;
        let author = json::str_param(params, "author")?;
        let note = json::opt_str(params, "note")?.unwrap_or("");
        let mut entities = Vec::new();
        for r in json::str_list(params, "entities")? {
            let e = self.world.entity(&r)?;
            entities.push(json::entity_str(&self.world, e));
        }
        // An asset or a def row, whole (phase 4 item 8: their edits checked
        // locks, and nothing could lock them).
        let ses = self.require_session()?;
        for a in json::str_list(params, "assets")? {
            if !ses.assets.entries().contains_key(&a) {
                return Err(
                    ArgusError::new(codes::BAD_PARAMS, format!("no asset {a:?}"))
                        .hint("asset.list shows every asset."),
                );
            }
            entities.push(a);
        }
        for r in json::str_list(params, "defs")? {
            if ses.project.defs.resolve(&r).is_none() {
                return Err(
                    ArgusError::new(codes::BAD_PARAMS, format!("no def row {r:?}"))
                        .hint("A row is prefix:key, item:meal; world.defs lists them."),
                );
            }
            entities.push(r);
        }
        // A prefab, whole (phase 5 item 4).
        for p in json::str_list(params, "prefabs")? {
            if ses.project.prefabs.get(&p).is_none() {
                return Err(
                    ArgusError::new(codes::BAD_PARAMS, format!("no prefab {p:?}"))
                        .hint("world.prefabs lists the prefabs."),
                );
            }
            entities.push(format!("prefab:{p}"));
        }
        let paths = json::str_list(params, "paths")?;
        for p in &paths {
            if p.contains('.') {
                self.world.registry().resolve_path(p)?;
            } else {
                self.world.registry().require(p)?;
            }
        }
        Ok(json!({ "lock": self.edits.lock(author, note, entities, paths) }))
    }

    fn edit_unlock(&mut self, params: &Json) -> Result<Json> {
        let author = json::str_param(params, "author")?;
        let id = json::u64_param(params, "lock", 0)? as u32;
        self.edits.unlock(author, id)?;
        Ok(json!({ "unlocked": id }))
    }

    // ----- input ------------------------------------------------------------

    /// `input.bind {action, device, bindings, save}`: rebind an action at
    /// run time (§12 "Rebinding at runtime is a UI over the same table";
    /// phase 3 item 1); `save` also writes it into input.toml.
    fn input_bind(&mut self, params: &Json) -> Result<Json> {
        let action = json::str_param(params, "action")?.to_string();
        let device = json::opt_str(params, "device")?
            .unwrap_or("keyboard")
            .to_string();
        let names: Vec<String> = params
            .get("bindings")
            .and_then(Json::as_array)
            .ok_or_else(|| bad_params("bindings is an array of key, button, or input names"))?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| bad_params("bindings are names"))
            })
            .collect::<Result<_>>()?;
        let save = json::bool_param(params, "save", false)?;
        let st = self.input()?;
        let next = self.sched.tick();
        st.borrow_mut()
            .rebind(next, &action, &device, names.clone())?;
        if save {
            let root = self.require_session()?.root.clone();
            argus_input::ActionMap::save_bindings(&root, &action, &device, &names)?;
        }
        self.world.emit(Event::Game {
            name: "input.rebound".to_string(),
            fields: vec![
                ("action".into(), Value::Str(action.clone())),
                ("device".into(), Value::Str(device.clone())),
                ("binding".into(), Value::Str(names.join(", "))),
            ],
        });
        self.pump_events();
        Ok(json!({ "action": action, "device": device, "bindings": names, "saved": save }))
    }

    fn input(&self) -> Result<Rc<RefCell<InputState>>> {
        let ses = self.require_session()?;
        ses.input.clone().ok_or_else(|| {
            ArgusError::new(
                argus_input::codes::NO_INPUT_MAP,
                "this project has no input.toml, so there are no actions to inject",
            )
            .hint("Add input.toml with [actions.<name>] tables (type = button | axis | axis2 | pointer) and reopen; input.bindings shows the map.")
        })
    }

    fn input_inject(&mut self, params: &Json) -> Result<Json> {
        let action = json::str_param(params, "action")?;
        let slot = json::u64_param(params, "slot", 1)? as u8;
        let frames = match params.get("frames") {
            Some(Json::Number(n)) => n.as_u64(),
            _ => None,
        };
        let hold = json::bool_param(params, "hold", false)?;
        let now = self.sched.tick();
        let st = self.input()?;
        let r = st
            .borrow_mut()
            .inject(now, slot, action, params.get("value"), frames, hold)?;
        Ok(json!({ "action": action, "slot": slot, "tick": r.tick, "until": r.until }))
    }

    /// `input.message {channel, text, slot}`: text from the host for the
    /// next tick's scripts, as a `host.message` event (host channel item 0).
    fn input_message(&mut self, params: &Json) -> Result<Json> {
        let channel = json::str_param(params, "channel")?;
        let text = json::str_param(params, "text")?;
        let slot = json::u64_param(params, "slot", 1)? as u8;
        let now = self.sched.tick();
        let st = self.input()?;
        st.borrow_mut().message(now, slot, channel, text)?;
        Ok(json!({ "channel": channel, "slot": slot, "tick": now, "bytes": text.len() }))
    }

    fn input_script(&mut self, params: &Json) -> Result<Json> {
        let steps = params
            .get("steps")
            .and_then(Json::as_array)
            .ok_or_else(|| {
                bad_params("steps must be an array of {at, action, slot, value, frames, hold}")
            })?;
        let now = self.sched.tick();
        let st = self.input()?;
        let reports = st.borrow_mut().script(now, steps)?;
        Ok(json!({
            "scheduled": reports.len(),
            "from": now,
            "until": reports.iter().filter_map(|r| r.until.or(Some(r.tick))).max(),
        }))
    }

    fn input_record(&mut self, params: &Json) -> Result<Json> {
        let on = json::bool_param(params, "on", true)?;
        let st = self.input()?;
        if !on {
            let path = st.borrow_mut().stop_recording();
            let n = st.borrow().log().len();
            let dir = self.session.as_mut().and_then(|s| s.run_dir.take());
            return Ok(json!({
                "recording": false,
                "path": path.map(|p| shown_path(&p)),
                "run": dir.map(|d| d.to_string_lossy().into_owned()),
                "entries": n,
            }));
        }
        let ses = self.session.as_ref().expect("session");
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let dir = ses
            .root
            .join(".argus")
            .join("runs")
            .join(format!("{stamp}-{}", ses.seed));
        let log_path = dir.join("input.log");
        st.borrow_mut().start_recording(&log_path)?;
        // The run identity (§6): what reproduces this run.
        let run_toml = format!(
            "scene = {:?}\nseed = {}\ncode = {:?}\nclock = {:?}\ninputs = \"input.log\"\nstarted = {stamp}\ntick = {}\n",
            ses.scene_name,
            ses.seed,
            code_version(&ses.root),
            self.sched.driving().to_string(),
            self.sched.tick()
        );
        std::fs::write(dir.join("run.toml"), run_toml).map_err(|e| {
            ArgusError::new(
                argus_input::codes::BAD_LOG,
                format!("cannot write run.toml: {e}"),
            )
        })?;
        self.session.as_mut().expect("session").run_dir = Some(dir.clone());
        Ok(json!({
            "recording": true,
            "path": shown_path(&log_path),
            "run": dir.to_string_lossy(),
        }))
    }

    fn input_replay(&mut self, params: &Json) -> Result<Json> {
        let path = json::str_param(params, "path")?;
        let st = self.input()?;
        let root = self.session.as_ref().expect("session").root.clone();
        let p = if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            root.join(path)
        };
        let entries = InputState::read_log(&p, st.borrow().map())?;
        let now = self.sched.tick();
        let n = st.borrow_mut().replay(now, &entries)?;
        Ok(json!({
            "path": shown_path(&p),
            "entries": n,
            "last_tick": entries.iter().map(|e| e.tick).max(),
        }))
    }

    fn input_log_get(&self, params: &Json) -> Result<Json> {
        let since = json::u64_param(params, "since", 0)?;
        let slot = params.get("slot").and_then(Json::as_u64).map(|s| s as u8);
        let action = json::opt_str(params, "action")?;
        let st = self.input()?;
        let st = st.borrow();
        let mut rows: Vec<(u64, Json)> = st
            .log()
            .iter()
            .filter(|e| e.tick >= since)
            .filter(|e| slot.is_none_or(|s| s == e.slot))
            .filter(|e| action.is_none_or(|a| a == e.action))
            .map(|e| (e.tick, e.to_json()))
            .collect();
        // The host's messages are input too, journaled for the tick they
        // reach: action `host.message`, value {channel, text} (issue 15).
        for j in st.journal() {
            let argus_input::Command::Message {
                slot: s,
                channel,
                text,
            } = &j.command
            else {
                continue;
            };
            if j.now < since
                || slot.is_some_and(|x| x != *s)
                || action.is_some_and(|a| a != "host.message")
            {
                continue;
            }
            rows.push((
                j.now,
                json!([j.now, s, "host.message", { "channel": channel, "text": text }]),
            ));
        }
        rows.sort_by_key(|r| r.0);
        let rows: Vec<Json> = rows.into_iter().map(|r| r.1).collect();
        let (limit, offset) = query::page(params)?;
        let mut out = query::paged(rows, limit, offset);
        out["columns"] = json!(["tick", "slot", "action", "value"]);
        out["recording"] = json!(st.recording().map(|p| p.to_string_lossy().into_owned()));
        out["clock"] = json!(if st.is_discrete() { "turn" } else { "tick" });
        Ok(out)
    }

    // ----- sim --------------------------------------------------------------

    fn sim_status(&self) -> Json {
        let clock = self.sched.driving();
        json!({
            "playing": self.sched.is_playing(),
            "paused": self.sched.is_paused(),
            "clock": clock.to_string(),
            "driver": clock.kind.to_string(),
            "hz": if clock.kind == ClockKind::Continuous { json!(1.0 / clock.timestep) } else { Json::Null },
            "speed": self.sched.speed(),
            "tick": self.sched.tick(),
            "time": self.sched.time(),
            "frame": self.world.frame(),
            "seed": self.session.as_ref().map(|s| s.seed).unwrap_or(self.sched.seed()),
            "end": self.session.as_ref().map(|s| s.ring.borrow().end),
            "fork": self.active,
        })
    }

    // ----- forks (phase 2 item 0) -------------------------------------------

    fn swap_slot(&mut self, s: &mut Slot) {
        std::mem::swap(&mut self.world, &mut s.world);
        std::mem::swap(&mut self.sched, &mut s.sched);
        std::mem::swap(&mut self.session, &mut s.session);
        std::mem::swap(&mut self.events, &mut s.events);
    }

    fn unknown_fork(&self, name: &str) -> ArgusError {
        let mut names: Vec<&str> = self.forks.keys().map(String::as_str).collect();
        names.push(&self.active);
        names.sort();
        ArgusError::new(codes::BAD_PARAMS, format!("no fork named {name:?}")).hint(format!(
            "Worlds: {}; sim.fork {{name}} makes one.",
            names.join(", ")
        ))
    }

    fn forks_json(&self) -> Json {
        let row = |name: &str, w: &World, s: &Scheduler| json!({ "name": name, "tick": s.tick(), "frame": w.frame(), "entities": w.entity_count() });
        let mut rows = vec![row(&self.active, &self.world, &self.sched)];
        for (n, s) in &self.forks {
            rows.push(row(n, &s.world, &s.sched));
        }
        json!({ "active": self.active, "worlds": rows })
    }

    /// `sim.fork`: a second world from this one's state inside the same
    /// daemon, with its own clock (§6). It is built from the files as they
    /// are now, so a fork after a script edit runs the new code beside the
    /// old; it copies the input journal, so from a past frame it replays the
    /// same inputs until a command of its own branches it.
    fn sim_fork(&mut self, params: &Json) -> Result<Json> {
        self.require_session()?;
        let Some(name) = json::opt_str(params, "name")?.map(str::to_string) else {
            return Ok(self.forks_json());
        };
        if json::bool_param(params, "drop", false)? {
            if name == self.active {
                return Err(ArgusError::new(
                    codes::BAD_PARAMS,
                    format!("{name} is the world in use"),
                )
                .hint("Switch to another world first: sim.fork {name, switch: true}."));
            }
            self.forks
                .remove(&name)
                .ok_or_else(|| self.unknown_fork(&name))?;
            return Ok(self.forks_json());
        }
        let switch = json::bool_param(params, "switch", false)?;
        // Switching to a world there is; a new name is made first, then
        // switched to (the editor's Try from here, phase 4 item 5: one
        // command).
        if switch && (name == self.active || self.forks.contains_key(&name)) {
            self.switch_world(&name)?;
            return Ok(self.forks_json());
        }
        if name.is_empty() || name == self.active || self.forks.contains_key(&name) {
            return Err(ArgusError::new(
                codes::BAD_PARAMS,
                format!("{name:?} is already a world here"),
            )
            .hint("Give the fork a new name, or drop the old one: sim.fork {name, drop: true}."));
        }
        self.before_ticks();
        let now = self.sched.tick();
        let frame = match params.get("frame").or(params.get("turn")) {
            Some(v) => v
                .as_u64()
                .ok_or_else(|| bad_params("frame must be a whole number"))?,
            None => now,
        };
        let (root, scene_name, seed, input, key, every, budget) = {
            let s = self.session.as_ref().expect("session");
            let r = s.ring.borrow();
            if frame > r.end.max(now) || frame < r.floor {
                return Err(ArgusError::new(
                    codes::BAD_PARAMS,
                    format!(
                        "frame {frame} is outside this run ({} to {})",
                        r.floor,
                        r.end.max(now)
                    ),
                ));
            }
            // At the current frame the keyframe is taken now, so nothing
            // replays; before it, the ring's.
            let key = if frame == now {
                s.sources.capture(&self.world, &self.sched, true)
            } else {
                r.at_or_before(frame).cloned().ok_or_else(|| {
                    ArgusError::new(
                        codes::BAD_PARAMS,
                        format!("no keyframe at or before frame {frame}"),
                    )
                })?
            };
            (
                s.root.clone(),
                s.scene_name.clone(),
                s.seed,
                s.input.as_ref().map(|i| i.borrow().fork_copy()),
                key,
                r.every,
                r.budget,
            )
        };
        let cfg = ProjectConfig::load(&root)?;
        let mutations = self.mutations;
        let mut main = Slot::empty();
        self.swap_slot(&mut main);
        // The feed, undo, open transactions, and locks are the session's,
        // not the world's: a fork's scene load must not clear them (found
        // by item 4: every proposal is checked on a fork, and the feed lost
        // its rows).
        let edits = std::mem::take(&mut self.edits);
        self.building_fork = true;
        let built = self.open_scene(root, &cfg, &scene_name, Some(seed));
        self.building_fork = false;
        self.edits = edits;
        self.mutations = mutations;
        if let Err(e) = built {
            self.swap_slot(&mut main);
            return Err(e);
        }
        let result = (|| -> Result<()> {
            let s = self.session.as_mut().expect("the fork's session");
            if let (Some(fi), Some(copy)) = (&s.input, input) {
                *fi.borrow_mut() = copy;
            }
            s.sources.restore(&key, &mut self.world, &mut self.sched);
            let _ = self.world.drain_events();
            let n = frame - key.tick;
            self.sched.set_quiet(true);
            let run = match self.sched.driving().kind {
                ClockKind::Continuous => self.sched.step(&mut self.world, n),
                ClockKind::Discrete => self.sched.advance(&mut self.world, n),
            };
            self.sched.set_quiet(false);
            run?;
            let _ = self.world.drain_events();
            let s = self.session.as_ref().expect("the fork's session");
            s.history.borrow_mut().clear();
            let mut ring = crate::ring::Ring::new(every, budget);
            ring.push(s.sources.capture(&self.world, &self.sched, true));
            ring.floor = frame;
            ring.mutations = self.mutations;
            *s.ring.borrow_mut() = ring;
            self.events.records.clear();
            self.events.ticked.clear();
            self.events.through = frame;
            Ok(())
        })();
        let mut fork = Slot::empty();
        self.swap_slot(&mut fork);
        self.swap_slot(&mut main);
        result?;
        self.forks.insert(name.clone(), fork);
        if switch {
            self.switch_world(&name)?;
        }
        let mut out = self.forks_json();
        out["made"] = json!({ "name": name, "frame": frame, "from": key.tick });
        Ok(out)
    }

    /// Make the world named the one in use, the one the window and the
    /// editor show.
    fn switch_world(&mut self, name: &str) -> Result<()> {
        if name == self.active {
            return Ok(());
        }
        let mut slot = self
            .forks
            .remove(name)
            .ok_or_else(|| self.unknown_fork(name))?;
        self.swap_slot(&mut slot);
        let was = std::mem::replace(&mut self.active, name.to_string());
        self.forks.insert(was, slot);
        #[cfg(feature = "render")]
        if let Some((_, r)) = &mut self.render.window {
            r.forget_motion();
        }
        #[cfg(feature = "audio")]
        if let Some(a) = self.audio.as_mut() {
            a.silence();
        }
        Ok(())
    }

    /// `sim.seek {frame}`: the nearest earlier keyframe, restored, and the
    /// run replayed forward to `frame` (§6). The replay is quiet: its events
    /// are compared with the log instead of logged again, its history and
    /// keyframes are the first run's, and no sound plays. After it, stepping
    /// replays the recorded future until a live command branches the run.
    fn sim_seek(&mut self, params: &Json) -> Result<Json> {
        self.require_session()?;
        let target = match (params.get("frame"), params.get("turn")) {
            (Some(v), _) | (None, Some(v)) => v
                .as_u64()
                .ok_or_else(|| bad_params("frame must be a whole number"))?,
            (None, None) => return Err(bad_params("give frame (or turn, on a discrete clock)")),
        };
        let verify = json::bool_param(params, "verify", false)?;
        self.seek_to(target, None, verify)
    }

    /// Go to frame `target`: the keyframe at or before it restored and the
    /// run replayed forward. With `until`, go on from there a tick at a
    /// time to the first frame at which it holds (or the end of the run),
    /// so a seek can land on a moment rather than a number (a timeline's
    /// time, phase 3 item 5).
    pub(super) fn seek_to(
        &mut self,
        target: u64,
        mut until: Option<&mut dyn FnMut(&World) -> bool>,
        verify: bool,
    ) -> Result<Json> {
        // A seek ends an A/B trial: its worlds keep step going forward only.
        self.end_trial(true);
        self.before_ticks();
        let started = std::time::Instant::now();
        let (ring, sources) = {
            let s = self.session.as_ref().expect("session");
            (s.ring.clone(), s.sources.clone())
        };
        let (end, floor) = {
            let r = ring.borrow();
            (r.end.max(self.sched.tick()), r.floor)
        };
        if target > end {
            return Err(ArgusError::new(
                codes::BAD_PARAMS,
                format!("frame {target} is past the end of this run ({end})"),
            )
            .hint("Seek goes back, or forward along a run already played; step or run_until to go further."));
        }
        if target < floor {
            return Err(ArgusError::new(
                codes::BAD_PARAMS,
                format!("frame {target} is before the oldest keyframe kept ({floor})"),
            )
            .hint("The ring dropped older keyframes to stay under its budget; raise [ring] budget_mb in argus.toml."));
        }
        // Verify first: replay the keyframe before into the one a seek starts
        // from, and compare the two worlds field by field.
        let verified = if verify {
            Some(self.verify_keyframe(&ring, &sources, target)?)
        } else {
            None
        };
        let from = {
            let r = ring.borrow();
            let k = r.at_or_before(target).ok_or_else(|| {
                ArgusError::new(
                    codes::BAD_PARAMS,
                    format!("no keyframe at or before frame {target}"),
                )
            })?;
            sources.restore(k, &mut self.world, &mut self.sched);
            k.tick
        };
        // The keyframe's queue held events the log already has.
        let _ = self.world.drain_events();
        let n = target - from;
        self.sched.set_quiet(true);
        let kind = self.sched.driving().kind;
        let mut run = match kind {
            ClockKind::Continuous => self.sched.step(&mut self.world, n),
            ClockKind::Discrete => self.sched.advance(&mut self.world, n),
        };
        let mut reached = true;
        if let Some(f) = until.as_mut() {
            while run.is_ok() && !f(&self.world) {
                if self.sched.tick() >= end {
                    reached = false;
                    break;
                }
                run = match kind {
                    ClockKind::Continuous => self.sched.step(&mut self.world, 1),
                    ClockKind::Discrete => self.sched.advance(&mut self.world, 1),
                };
            }
        }
        self.sched.set_quiet(false);
        run?;
        let target = self.sched.tick();
        let n = target - from;
        let replayed = self.world.drain_events();
        let (checked, diverged) = self.check_replay(from, target, &replayed);
        self.cut_log(target);
        if let Some(s) = &self.session {
            s.history.borrow_mut().rewind(target);
        }
        #[cfg(feature = "audio")]
        if let Some(a) = self.audio.as_mut() {
            a.silence();
        }
        #[cfg(feature = "render")]
        if let Some((_, r)) = &mut self.render.window {
            r.forget_motion();
        }
        let mut out = self.sim_status();
        out["from"] = json!(from);
        out["replayed"] = json!(n);
        out["ms"] = json!((started.elapsed().as_secs_f64() * 1e4).round() / 10.0);
        out["checked"] = json!(checked);
        out["diverged"] = diverged.unwrap_or(Json::Null);
        if let Some(v) = verified {
            out["verified"] = v;
        }
        if until.is_some() {
            out["reached"] = json!(reached);
        }
        Ok(out)
    }

    /// Compare a replay's events with the log's for the same frames: what
    /// systems emitted and what the engine did in ticks, not the mixer's
    /// answers (a replay plays no sound). The first difference is the report.
    fn check_replay(
        &self,
        from: u64,
        to: u64,
        replayed: &[EventRecord],
    ) -> (Option<usize>, Option<Json>) {
        let answer = |r: &EventRecord| r.by.is_none() && matches!(r.event, Event::Game { .. });
        let oldest = self.events.records.front().map(|(_, r)| r.frame);
        if oldest.is_none_or(|f| f > from) && from > 0 {
            return (None, None);
        }
        // Only the frames this branch's log still covers: after a seek back,
        // a seek forward replays frames whose events were cut.
        let to = to.min(self.events.through);
        if to <= from {
            return (None, None);
        }
        let logged: Vec<&EventRecord> = self
            .events
            .records
            .iter()
            .zip(self.events.ticked.iter())
            .filter(|((_, r), t)| **t && r.frame >= from && r.frame < to && !answer(r))
            .map(|((_, r), _)| r)
            .collect();
        let again: Vec<&EventRecord> = replayed
            .iter()
            .filter(|r| r.frame < to && !answer(r))
            .collect();
        let n = logged.len().max(again.len());
        for i in 0..n {
            let (a, b) = (logged.get(i), again.get(i));
            if a != b {
                let show = |r: Option<&&EventRecord>| match r {
                    Some(r) => event_json(&self.world, 0, r),
                    None => Json::Null,
                };
                return (
                    Some(i),
                    Some(json!({
                        "at": i,
                        "frame": a.or(b).map(|r| r.frame),
                        "logged": show(a),
                        "replayed": show(b),
                    })),
                );
            }
        }
        (Some(n), None)
    }

    /// Replay from the keyframe before the one a seek to `target` starts
    /// from into that one, and compare the two worlds: a replay that does
    /// not reproduce the run shows here as fields that differ.
    /// The run's digest: the world's, with the runtime maps' cells when any
    /// holds a tile (phase 5 item 4).
    pub(super) fn run_digest(&self) -> u64 {
        let cells = self
            .session
            .as_ref()
            .map(|s| s.tiles.borrow().runtime_cells())
            .unwrap_or_default();
        argus_player::keyframe::run_digest(&self.world, &cells)
    }

    fn verify_keyframe(
        &mut self,
        ring: &Rc<RefCell<crate::ring::Ring>>,
        sources: &crate::ring::Sources,
        target: u64,
    ) -> Result<Json> {
        let (earlier, later_world, later_tick) = {
            let r = ring.borrow();
            let Some(later) = r.at_or_before(target) else {
                return Ok(json!({ "ok": null, "why": "no keyframe" }));
            };
            let Some(earlier) = later.tick.checked_sub(1).and_then(|t| r.at_or_before(t)) else {
                return Ok(json!({ "ok": null, "why": "the first keyframe has none before it" }));
            };
            if later.barrier {
                return Ok(
                    json!({ "ok": null, "why": "a change between ticks made that keyframe; a replay does not cross one" }),
                );
            }
            (
                earlier.tick,
                (later.sim.world.clone(), later.tiles.clone()),
                later.tick,
            )
        };
        let (later_world, later_cells) = later_world;
        {
            let r = ring.borrow();
            let k = r.at_or_before(earlier).expect("just found");
            sources.restore(k, &mut self.world, &mut self.sched);
        }
        let _ = self.world.drain_events();
        self.sched.set_quiet(true);
        let n = later_tick - earlier;
        let run = match self.sched.driving().kind {
            ClockKind::Continuous => self.sched.step(&mut self.world, n),
            ClockKind::Discrete => self.sched.advance(&mut self.world, n),
        };
        self.sched.set_quiet(false);
        run?;
        let _ = self.world.drain_events();
        let then = later_world.into_world();
        let d = crate::history::diff(&then, &self.world);
        // The runtime maps' cells too (phase 5 item 4): outside the world.
        let now_cells = sources
            .tiles
            .as_ref()
            .map(|t| t.borrow().runtime_cells())
            .unwrap_or_default();
        let tiles_same =
            argus_tiles::cells_digest(&now_cells) == argus_tiles::cells_digest(&later_cells);
        let ok =
            d.appeared.is_empty() && d.vanished.is_empty() && d.changed.is_empty() && tiles_same;
        Ok(json!({
            "ok": ok,
            "tiles_same": tiles_same,
            "from": earlier,
            "to": later_tick,
            "appeared": d.appeared.len(),
            "vanished": d.vanished.len(),
            "changed": d.changed.iter().take(5).map(|c| json!({
                "entity": crate::history::id_of(&then, c.entity),
                "fields": c.fields.iter().take(4).map(|(p, a, b)| json!([p, argus_index::shapes::value_json(a), argus_index::shapes::value_json(b)])).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        }))
    }

    /// `sim.snapshot`: a keyframe now, kept like a barrier, and the ring's
    /// report.
    fn sim_snapshot(&mut self, _params: &Json) -> Result<Json> {
        self.require_session()?;
        self.before_ticks();
        let s = self.session.as_ref().expect("session");
        let k = s.sources.capture(&self.world, &self.sched, true);
        s.ring.borrow_mut().push(k);
        let mut out = s.ring.borrow().status();
        out["tick"] = json!(self.sched.tick());
        Ok(out)
    }

    /// `sim.ring`: the run a scrubber draws (phase 4 item 5), with no
    /// keyframe taken: its range, its keyframes and barriers, and where the
    /// clock is.
    fn sim_ring(&mut self, _params: &Json) -> Result<Json> {
        self.require_session()?;
        let s = self.session.as_ref().expect("session");
        let mut out = s.ring.borrow().report();
        let clock = self.sched.driving();
        out["tick"] = json!(self.sched.tick());
        out["time"] = json!(self.sched.time());
        out["driver"] = json!(clock.kind.to_string());
        out["hz"] = if clock.kind == ClockKind::Continuous {
            json!(1.0 / clock.timestep)
        } else {
            Json::Null
        };
        out["fork"] = json!(self.active);
        out["worlds"] = self.forks_json()["worlds"].take();
        Ok(out)
    }

    fn sim_run_until(&mut self, params: &Json) -> Result<Json> {
        self.require_session()?;
        let max = json::u64_param(params, "max_ticks", 100_000)?;
        self.before_ticks();
        let r = if let Some(ev) = json::opt_str(params, "event")? {
            self.sched.run_until_event(&mut self.world, ev, max)?
        } else if let Some(Json::Number(t)) = params.get("time") {
            self.sched
                .run_until_time(&mut self.world, t.as_f64().unwrap_or(0.0), max)?
        } else if let Some(Json::Number(n)) = params.get("ticks") {
            // At most max_ticks a call; asked for more, it says it stopped
            // short (hit: false) instead of seeming done.
            let want = n.as_u64().unwrap_or(0);
            let n = want.min(max);
            self.sched.step(&mut self.world, n)?;
            argus_sim::RunUntil {
                ticks: n,
                hit: n == want,
            }
        } else {
            return Err(bad_params("give event, time, or ticks"));
        };
        self.pump_ticks();
        let mut st = self.sim_status();
        st["ticks"] = json!(r.ticks);
        st["hit"] = json!(r.hit);
        Ok(st)
    }

    // ----- script -----------------------------------------------------------

    fn script_reload(&mut self) -> Result<Json> {
        let ses = self
            .session
            .as_mut()
            .ok_or_else(|| ArgusError::new(codes::NO_PROJECT, "no project is open"))?;
        let report = ses.host.reload(&mut self.world, &mut self.sched)?;
        // A system a fault switched off runs again, its file changed or not:
        // the fault may have been the run's data, not its code (issue 13).
        let switched_on = self.sched.enable_faulted();
        // Rust plugins after the scripts, whose components they read: each
        // built again, and swapped in (snapshot, swap, restore) when its
        // library changed (§7).
        let plugins = argus_plugins::reload(
            &ses.root,
            &ses.plugin_names,
            &mut ses.plugins,
            &mut self.world,
            &mut self.sched,
        );
        let schemas: Vec<DefSchema> = ses
            .host
            .defs()
            .iter()
            .map(|d| {
                DefSchema::new(d.schema.clone())
                    .key(d.key.clone())
                    .prefix(d.prefix.clone())
            })
            .collect();
        ses.project
            .set_def_schemas(argus_player::with_engine_defs(schemas));
        ses.host
            .set_defs(Rc::new(argus_player::ProjectDefs(ses.project.defs.clone())));
        // Prefabs resolve again, against the components as they are now.
        ses.host
            .set_prefabs(Rc::new(argus_player::ProjectPrefabs::new(&ses.project)));
        let registry = self.world.registry().clone();
        ses.scene
            .borrow_mut()
            .reresolve(&ses.project.ctx(&registry));
        // An order the systems cannot take is an error of the scripts too,
        // listed in script.errors until a reload fixes it.
        let order = self.sched.validate();
        if let Some(ses) = self.session.as_mut() {
            ses.host.set_order_error(order.as_ref().err());
        }
        order?;
        // Words, conversations, and timelines are read again with the
        // scripts.
        self.reload_strings();
        self.reload_dialogue();
        self.reload_timelines();
        self.sync_index(true);
        let plugin_errors = self.session.as_ref().map_or(0, |s| s.plugins.errors.len());
        Ok(json!({
            "reloaded": report.reloaded,
            "removed": report.removed,
            "errors": report.errors + plugin_errors,
            "switched_on": switched_on,
            "plugins": plugins
                .iter()
                .map(|r| json!({
                    "plugin": r.plugin,
                    "swapped": r.swapped,
                    "build_ms": r.build_ms,
                    "reload_ms": (r.reload_ms * 10.0).round() / 10.0,
                }))
                .collect::<Vec<_>>(),
        }))
    }

    /// `sim.threads {n}`: how many threads a batch of parallel systems may
    /// use, and the batches. Not part of the run: the world after a tick is
    /// the same for any number (§6), so no keyframe marks a change.
    fn sim_threads(&mut self, params: &Json) -> Result<Json> {
        if let Some(n) = params.get("n").filter(|v| !v.is_null()) {
            let n = n.as_u64().ok_or_else(|| {
                ArgusError::new(codes::BAD_PARAMS, "n is a count of threads, 0 or more")
                    .hint("0 is one per core; 1 keeps every batch on the sim thread.")
            })?;
            self.sched.set_threads(n as usize);
        }
        let batches: Vec<Json> = self
            .sched
            .batches()?
            .iter()
            .map(|b| {
                json!({
                    "batch": b.batch,
                    "stage": b.stage.name(),
                    "systems": b.systems,
                    "threaded": b.threaded,
                })
            })
            .collect();
        Ok(json!({
            "threads": self.sched.threads(),
            "batches": batches,
        }))
    }

    fn script_systems(&mut self, params: &Json) -> Result<Json> {
        let plugins = self.session.as_ref().map(|s| &s.plugins);
        let rows: Vec<Json> = self
            .sched
            .systems(self.world.registry())?
            .iter()
            .map(|s| {
                let plugin = plugins.and_then(|p| p.of_system(&s.name));
                json!({
                    "name": s.name,
                    "stage": s.stage.name(),
                    "after": s.after,
                    "before": s.before,
                    "first": s.first,
                    "last": s.last,
                    "reads": s.reads,
                    "writes": s.writes,
                    "clock": format!("{:?}", s.clock).to_lowercase(),
                    "rng": s.rng,
                    "enabled": s.enabled,
                    "disabled_reason": s.disabled_reason,
                    "source": s.source,
                    "doc": s.doc,
                    "parallel": s.parallel,
                    "batch": s.batch,
                    "plugin": plugin,
                })
            })
            .collect();
        let (limit, offset) = query::page(params)?;
        Ok(query::paged(rows, limit, offset))
    }

    /// The length of `script.errors`, for the status calls.
    fn script_error_count(&self) -> u64 {
        self.script_errors()
            .map(|j| j["total"].as_u64().unwrap_or(0))
            .unwrap_or(0)
    }

    fn script_errors(&self) -> Result<Json> {
        let ses = self.require_session()?;
        let mut rows: Vec<Json> = ses
            .host
            .errors()
            .iter()
            .map(|e| {
                json!({
                    "file": e.file,
                    "line": e.line,
                    "message": e.message,
                    "phase": format!("{:?}", e.phase).to_lowercase(),
                    "ref": match e.line { Some(l) => format!("{}:{l}", e.file), None => e.file.clone() },
                })
            })
            .collect();
        // A system ordered after one that is missing, since the plugin it
        // is in did not build (phase 5 item 6): it runs, unordered there.
        for (system, name) in self.sched.dangling_after() {
            rows.push(json!({
                "file": "",
                "line": null,
                "message": format!("{system} runs after {name}, which is not registered; it runs without that order until {name} is"),
                "phase": "order",
                "ref": "",
                "system": system,
            }));
        }
        // Rust plugins that did not build, load, or check (phase 5 item 6):
        // the compiler's message at its file and line.
        for e in &ses.plugins.errors {
            rows.push(json!({
                "file": e.file,
                "line": e.line,
                "message": e.message,
                "phase": e.phase,
                "ref": e.at(),
                "plugin": e.plugin,
            }));
        }
        // Dialogue rows and string tables that do not read (phase 3 item 4):
        // content, but code-like enough to be listed with the scripts.
        rows.extend(self.text_errors());
        rows.extend(self.timeline_errors());
        // A system a fault disabled, while it stays disabled: its last fault,
        // with the file and line (found building Hollow: a script that
        // failed on its first tick was off, and this list said nothing).
        let mut faulted: Vec<String> = Vec::new();
        let walk = self.sched.any_disabled();
        for (_, rec) in self.events.records.iter().rev().filter(|_| walk) {
            let Event::SystemFault {
                system,
                message,
                r#ref,
            } = &rec.event
            else {
                continue;
            };
            if faulted.contains(system) || self.sched.is_enabled(system).unwrap_or(true) {
                continue;
            }
            faulted.push(system.clone());
            let at = r#ref.clone().unwrap_or_default();
            let (file, line) = match at.rsplit_once(':') {
                Some((f, l)) if l.parse::<u32>().is_ok() => (f.to_string(), l.parse::<u32>().ok()),
                _ => (at.clone(), None),
            };
            rows.push(json!({
                "file": file,
                "line": line,
                "message": message,
                "phase": "run",
                "ref": at,
                "system": system,
                "frame": rec.frame,
            }));
        }
        // Materials are code too (section 13): a shader that does not
        // compile, or a sidecar that does not read, is listed here with its
        // line, from the import that failed.
        for f in ses.assets.failures().values() {
            let code = f.error.code;
            if code != argus_material::codes::SHADER && code != argus_material::codes::BAD_MATERIAL
            {
                continue;
            }
            let at = f.error.r#ref.clone().unwrap_or_default();
            let (file, line) = match at.rsplit_once(':') {
                Some((file, l)) if l.parse::<u32>().is_ok() => {
                    (file.to_string(), l.parse::<u32>().ok())
                }
                _ => (at.clone(), None),
            };
            rows.push(json!({
                "file": file,
                "line": line,
                "message": f.error.message,
                "phase": "material",
                "ref": at,
                "asset": f.name,
            }));
        }
        #[cfg(feature = "render")]
        for (name, why) in self.render.material_errors() {
            rows.push(json!({ "file": name, "line": null, "message": why, "phase": "material", "ref": name, "asset": name }));
        }
        Ok(json!({ "total": rows.len(), "rows": rows }))
    }

    // ----- obs --------------------------------------------------------------

    // ----- the keyframe ring (phase 2 item 0) --------------------------------

    /// The first keyframe, at the load, and the hook that takes one every
    /// `every` ticks.
    fn start_ring(&mut self) {
        let Some(ses) = &self.session else {
            return;
        };
        let (ring, sources, saves) = (ses.ring.clone(), ses.sources.clone(), ses.saves.clone());
        let k = sources.capture(&self.world, &self.sched, true);
        {
            let mut r = ring.borrow_mut();
            r.push(k);
            r.mutations = self.mutations;
        }
        self.sched.on_tick_end(Box::new(move |world, sched| {
            let tick = sched.tick();
            let mut r = ring.borrow_mut();
            r.end = r.end.max(tick);
            if tick % r.every == 0 {
                let t = std::time::Instant::now();
                let k = sources.capture(world, sched, false);
                r.note_capture(t.elapsed());
                r.push(k);
            }
            argus_player::savegame::requested(world, sched, &sources, &mut saves.borrow_mut());
        }));
    }

    /// Before ticks run: a change made between ticks since the last keyframe
    /// (an edit, a room transition, a reload) gets a barrier keyframe, and
    /// ends any recorded future a seek left, since what comes next differs.
    fn before_ticks(&mut self) {
        let Some(ses) = &self.session else {
            return;
        };
        if ses.ring.borrow().mutations == self.mutations {
            return;
        }
        let now = self.sched.tick();
        if let Some(i) = &ses.input {
            i.borrow_mut().branch(now);
        }
        let k = ses.sources.capture(&self.world, &self.sched, true);
        let mut r = ses.ring.borrow_mut();
        r.prune_after(now);
        r.push(k);
        r.mutations = self.mutations;
    }

    /// A live input command where the run already went further (after a
    /// seek back) branches it: the recorded commands, values, and keyframes
    /// after it go.
    fn input_branched(&mut self) {
        let Some(ses) = &self.session else {
            return;
        };
        let Some(input) = &ses.input else {
            return;
        };
        let now = self.sched.tick();
        let (live, dropped) = {
            let mut i = input.borrow_mut();
            (i.take_live(), i.take_branch())
        };
        if live && (dropped.is_some() || ses.ring.borrow().end > now) {
            input.borrow_mut().branch(now);
            ses.ring.borrow_mut().prune_after(now);
        }
    }

    /// The run is back at `frame`: what ticks from it on emitted is the
    /// future (or a run that no longer happened), and a change made at it
    /// between ticks stays.
    fn cut_log(&mut self, frame: u64) {
        let keep: Vec<bool> = self
            .events
            .records
            .iter()
            .zip(self.events.ticked.iter())
            .map(|((_, r), t)| {
                if *t {
                    r.frame < frame
                } else {
                    r.frame <= frame
                }
            })
            .collect();
        let mut i = 0;
        self.events.records.retain(|_| {
            i += 1;
            keep[i - 1]
        });
        let mut i = 0;
        self.events.ticked.retain(|_| {
            i += 1;
            keep[i - 1]
        });
        self.events.through = self.events.through.min(frame);
    }

    /// Pump what ticks emitted.
    fn pump_ticks(&mut self) {
        self.from_ticks = true;
        self.pump_events();
        self.from_ticks = false;
        self.events.through = self.events.through.max(self.world.frame());
        self.answer_save_requests();
    }

    /// Serve the web client at this address (the server says where it
    /// bound).
    pub fn set_web(&mut self, addr: Option<std::net::SocketAddr>) {
        self.web = addr;
    }

    /// `token.issue {scope, hours, label}`, `token.list`, `token.revoke
    /// {id}` (section 5: remote access "through a token issued by `argus
    /// token` with a scope (observe, review, edit) and an expiry"; phase 5
    /// item 10). A client on this machine only: a browser's call is refused
    /// before it gets here, and refused here too.
    fn token_call(&mut self, conn: ConnId, method: &str, params: &Json) -> Result<Json> {
        use crate::web::tokens;
        if conn >= crate::web::WEB_CONN_BASE {
            return Err(ArgusError::new(
                codes::FORBIDDEN,
                format!("{method} is the daemon's own: only a client on this machine makes it"),
            ));
        }
        let root = self.require_session()?.root.clone();
        match method {
            "token.issue" => {
                let scope_name = json::opt_str(params, "scope")?.unwrap_or("review");
                let scope = tokens::Scope::parse(scope_name).ok_or_else(|| {
                    bad_params(format!("scope {scope_name:?}: observe, review, or edit"))
                })?;
                let hours = params.get("hours").and_then(Json::as_f64).unwrap_or(12.0);
                let label = json::opt_str(params, "label")?.unwrap_or("");
                let (secret, t) = tokens::issue(&root, scope, hours, label)?;
                let url = self.web.map(|w| format!("http://{w}/#t={secret}"));
                Ok(json!({
                    "token": secret,
                    "id": t.id,
                    "scope": scope.name(),
                    "label": t.label,
                    "expires_at": t.expires_at,
                    "url": url,
                    "hint": if url.is_some() {
                        "Open the url on the phone or tablet (on this machine's tailnet). The token is shown once; token.revoke {id} ends it."
                    } else {
                        "No web client is served: argus daemon start --web tailnet (or --web for this machine only) serves it."
                    },
                }))
            }
            "token.list" => {
                let now = tokens::now();
                let rows: Vec<Json> = tokens::load(&root).iter().map(|t| t.row(now)).collect();
                Ok(json!({
                    "total": rows.len(),
                    "rows": rows,
                    "web": self.web.map(|w| format!("http://{w}/")),
                }))
            }
            _ => {
                let id = json::opt_str(params, "id")?
                    .ok_or_else(|| bad_params("token.revoke needs the token's id (token.list)"))?;
                Ok(json!({ "id": id, "revoked": tokens::revoke(&root, id)? }))
            }
        }
    }

    /// `platform.status`: the null platform, the game's achievements on it,
    /// and `platform.toml`.
    fn platform_status(&mut self) -> Result<Json> {
        self.require_session()?;
        let mut j = argus_platform::status(&mut self.platform, &self.platform_cfg);
        j["config"] = self.platform_cfg.to_json();
        Ok(j)
    }

    /// `platform.overlay {open}`: the overlay opens or closes, as a
    /// player's Shift+Tab does on Steam: the action map's `platform =
    /// ["overlay"]` input, logged like a key.
    fn platform_overlay(&mut self, params: &Json) -> Result<Json> {
        self.require_session()?;
        let open = params.get("open").and_then(Json::as_bool).ok_or_else(|| {
            ArgusError::new(
                codes::BAD_PARAMS,
                "platform.overlay needs open: true or false",
            )
        })?;
        self.platform.set_overlay(open);
        let turns = self.device_input(&[argus_input::DeviceInput::Platform {
            name: "overlay".to_string(),
            on: open,
        }]);
        Ok(json!({ "open": open, "turns_taken": turns }))
    }

    /// `platform.reset {cloud?}`: the null platform forgets what it was
    /// told (achievements, progress, presence, overlay pages), and its
    /// cloud's files too with `cloud: true`.
    fn platform_reset(&mut self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        let mut deleted = 0;
        if params.get("cloud").and_then(Json::as_bool) == Some(true) {
            use argus_platform::Platform;
            for f in self.platform.cloud_files() {
                self.platform.cloud_delete(&f.name)?;
                deleted += 1;
            }
        }
        self.platform = argus_platform::NullPlatform::with_cloud(
            root.join(".argus").join("platform").join("cloud"),
        );
        Ok(json!({ "reset": true, "cloud_files_deleted": deleted }))
    }

    /// Move the world's pending events into the log and notify subscribers.
    fn pump_events(&mut self) {
        self.finish_capture();
        let drained = if self.world.events().is_empty() {
            Vec::new()
        } else {
            self.world.drain_events()
        };
        // The mixer answers each sound request right after it in the log,
        // then reports what moving sources and the music did (section 8).
        #[cfg(not(feature = "audio"))]
        let records = drained;
        #[cfg(feature = "audio")]
        let mut records: Vec<EventRecord> = Vec::with_capacity(drained.len());
        #[cfg(feature = "audio")]
        {
            // A trial's parked world catching up is not heard.
            let audio = if self.backstage {
                None
            } else {
                self.audio.as_mut()
            };
            let mut audio = audio;
            for rec in drained {
                let answer = match (&rec.event, audio.as_deref_mut()) {
                    (Event::Game { name, fields }, Some(a))
                        if name == "sound.play" || name == "sound.stop" =>
                    {
                        a.at_frame(rec.frame);
                        a.request(&self.world, name, fields)
                    }
                    _ => Vec::new(),
                };
                let frame = rec.frame;
                records.push(rec);
                // The mixer's answers are the daemon's, not a system's.
                records.extend(answer.into_iter().map(|(name, fields)| EventRecord {
                    frame,
                    event: Event::Game { name, fields },
                    by: None,
                }));
            }
            if let Some(a) = self.audio.as_mut().filter(|_| !self.backstage) {
                // The mixer reaches the sim's time first, so a music change
                // is measured from where the music is now, and its event's
                // frame and its in_ms agree (phase 3 item 7).
                a.advance_to(self.sched.driving().time());
                let frame = self.world.frame();
                records.extend(a.update(&self.world).into_iter().map(|(name, fields)| {
                    EventRecord {
                        frame,
                        event: Event::Game { name, fields },
                        by: None,
                    }
                }));
            }
        }
        // The platform answers each request right after it, as the mixer
        // does (phase 5 item 9); a trial's parked world is not heard.
        let records = if self.backstage {
            records
        } else {
            let mut out = Vec::with_capacity(records.len());
            for rec in records {
                let answers = match &rec.event {
                    Event::Game { name, fields } => {
                        argus_platform::answer(&mut self.platform, &self.platform_cfg, name, fields)
                    }
                    _ => None,
                };
                let frame = rec.frame;
                out.push(rec);
                for (name, fields) in answers.into_iter().flatten() {
                    out.push(EventRecord {
                        frame,
                        event: Event::Game { name, fields },
                        by: None,
                    });
                }
            }
            out
        };
        for rec in records {
            if let Event::Game { name, fields } = &rec.event {
                if name == "load.requested" && self.from_ticks && !self.backstage {
                    self.load_request = Some(argus_player::savegame::request_name(fields));
                }
            }
            let seq = self.events.next_seq;
            self.events.next_seq += 1;
            let row = event_json(&self.world, seq, &rec);
            self.events.records.push_back((seq, rec));
            self.events.ticked.push_back(self.from_ticks);
            if self.events.records.len() > self.events.cap {
                self.events.records.pop_front();
                self.events.ticked.pop_front();
            }
            if !self.backstage {
                self.notify("events", "event", row);
            }
        }
    }

    fn notify(&mut self, topic: &str, method: &str, params: Json) {
        let conns: Vec<ConnId> = self
            .subs
            .iter()
            .filter(|(_, t)| t.contains(topic))
            .map(|(c, _)| *c)
            .collect();
        for c in conns {
            self.outbox
                .push((c, rpc::notification(method, params.clone())));
        }
    }

    fn obs_events(&self, params: &Json) -> Result<Json> {
        let since = json::u64_param(params, "since", 0)?;
        let from_frame = json::u64_param(params, "from_frame", 0)?;
        let name = json::opt_str(params, "name")?;
        // How events name the entity. A gone one is still in the log by the
        // handle it had (found at the phase 5 gate: `entity=e:515` of a
        // dead enemy was refused as not alive).
        let entity = match json::opt_str(params, "entity")? {
            Some(r) if r.starts_with("e:") => {
                let e: Entity = r
                    .parse()
                    .map_err(|m: String| ArgusError::new(argus_ecs::codes::BAD_ID, m))?;
                Some(if self.world.is_alive(e) {
                    json::entity_str(&self.world, e)
                } else {
                    e.to_string()
                })
            }
            Some(r) => Some(json::entity_str(&self.world, self.world.entity(r)?)),
            None => None,
        };
        let rows: Vec<Json> = self
            .events
            .records
            .iter()
            .filter(|(seq, r)| *seq > since && r.frame >= from_frame)
            .map(|(seq, r)| event_json(&self.world, *seq, r))
            .filter(|row| {
                name.is_none_or(|n| row["name"].as_str() == Some(n))
                    && entity.as_deref().is_none_or(|s| {
                        let f = &row["fields"];
                        f.as_object()
                            .is_some_and(|o| o.values().any(|v| v.as_str() == Some(s)))
                    })
            })
            .collect();
        let (limit, offset) = query::page(params)?;
        let mut out = query::paged(rows, limit, offset);
        out["last_seq"] = json!(self.events.next_seq - 1);
        Ok(out)
    }

    // ----- obs.diff, obs.trace, obs.why (phase 1, item 13) -------------------

    /// What changed since a frame (section 8 L3): which entities appeared,
    /// vanished, or changed which fields, with counts, about 1 KB.
    fn obs_diff(&mut self, params: &Json) -> Result<Json> {
        let ses = self.require_session()?;
        let now = self.world.frame();
        let hist = ses.history.borrow();
        let asked = params.get("since").and_then(Json::as_u64);
        let since = match asked {
            Some(f) => f,
            None => match hist.baseline(now) {
                Some((f, _)) => f,
                None => {
                    // Nothing looked before: this look is the baseline.
                    return Ok(json!({
                        "since": now, "from": now, "to": now,
                        "note": "The first look at this world: nothing to compare with yet. This frame is kept; ask again after the world moves.",
                    }));
                }
            },
        };
        if since > now {
            return Err(ArgusError::new(
                codes::BAD_PARAMS,
                format!("frame {since} has not happened; the world is at frame {now}"),
            ));
        }
        let limit = json::u64_param(params, "limit", 12)?.min(500) as usize;
        let Some((from, then)) = hist.baseline(since) else {
            let frames = hist.baseline_frames();
            return Err(ArgusError::new(
                codes::BAD_PARAMS,
                format!("no baseline at or before frame {since}; the baselines are at frames {frames:?}"),
            )
            .hint("The world is kept at each frame the agent looked at it (any world.* or obs.* call), the last few; obs.diff with no since compares with the last look."));
        };
        let d = crate::history::diff(then, &self.world);
        let label = |w: &World, e: Entity| -> String {
            let comps = w.components_of(e).unwrap_or(&[]);
            let game: Vec<&str> = comps
                .iter()
                .map(|&c| w.registry().schema(c))
                .filter(|s| s.rust_type.is_none())
                .map(|s| s.name.as_str())
                .collect();
            if game.is_empty() {
                comps
                    .iter()
                    .map(|&c| w.registry().schema(c).name.as_str())
                    .collect::<Vec<_>>()
                    .join("+")
            } else {
                game.join("+")
            }
        };
        let kinds = |w: &World, es: &[Entity]| -> Json {
            let mut by: BTreeMap<String, (usize, Vec<String>)> = BTreeMap::new();
            for &e in es {
                let k = by.entry(label(w, e)).or_default();
                k.0 += 1;
                if k.1.len() < 3 {
                    k.1.push(crate::history::id_of(w, e));
                }
            }
            let mut rows: Vec<(String, usize, Vec<String>)> =
                by.into_iter().map(|(k, (n, ids))| (k, n, ids)).collect();
            rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            rows.truncate(8);
            json!({ "count": es.len(), "kinds": rows.into_iter().map(|(k, n, ids)| json!([k, n, ids])).collect::<Vec<_>>() })
        };
        let mut per_field: BTreeMap<&str, usize> = BTreeMap::new();
        let mut added: BTreeMap<&str, usize> = BTreeMap::new();
        let mut removed: BTreeMap<&str, usize> = BTreeMap::new();
        for c in &d.changed {
            for (p, _, _) in &c.fields {
                *per_field.entry(p.as_str()).or_default() += 1;
            }
            for n in &c.added {
                *added.entry(n.as_str()).or_default() += 1;
            }
            for n in &c.removed {
                *removed.entry(n.as_str()).or_default() += 1;
            }
        }
        let mut fields: Vec<(&str, usize)> = per_field.into_iter().collect();
        fields.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        let more_fields = fields.len().saturating_sub(16);
        fields.truncate(16);
        // Named entities first, each with what its fields went from and to.
        let mut listed: Vec<&crate::history::Changed> = d.changed.iter().collect();
        listed.sort_by_key(|c| (self.world.string_id(c.entity).is_none(), c.entity));
        let entities: Vec<Json> = listed
            .iter()
            .take(limit)
            .map(|c| {
                let mut o = serde_json::Map::new();
                for (p, x, y) in c.fields.iter().take(6) {
                    o.insert(
                        p.clone(),
                        json!([json::to_json(then, x), json::to_json(&self.world, y)]),
                    );
                }
                // [id, {field: [from, to]}] and, only when there is any,
                // what else: fields past six, components gained and lost.
                let mut row = vec![
                    json!(crate::history::id_of(&self.world, c.entity)),
                    Json::Object(o),
                ];
                let mut more = serde_json::Map::new();
                if c.fields.len() > 6 {
                    more.insert("more_fields".into(), json!(c.fields.len() - 6));
                }
                if !c.added.is_empty() {
                    more.insert("added".into(), json!(c.added));
                }
                if !c.removed.is_empty() {
                    more.insert("removed".into(), json!(c.removed));
                }
                if !more.is_empty() {
                    row.push(Json::Object(more));
                }
                Json::Array(row)
            })
            .collect();
        let mut out = json!({
            "since": since,
            "from": from,
            "to": now,
            "appeared": kinds(&self.world, &d.appeared),
            "vanished": kinds(then, &d.vanished),
            "changed": {
                "count": d.changed.len(),
                "fields": fields.iter().map(|(p, n)| json!([p, n])).collect::<Vec<_>>(),
                "components": { "added": added, "removed": removed },
                "entities": entities,
                "omitted": d.changed.len().saturating_sub(limit),
            },
        });
        if more_fields > 0 {
            out["changed"]["more_fields"] = json!(more_fields);
        }
        drop(hist);
        let bytes = serde_json::to_string(&out).map(|s| s.len()).unwrap_or(0);
        out["bytes"] = json!(bytes + 16);
        Ok(out)
    }

    /// One entity's component history over a window (section 8): every
    /// field that changed, when, and to what; the components it gained and
    /// lost; its events; and the systems that write what changed.
    fn obs_trace(&mut self, params: &Json) -> Result<Json> {
        let ses = self.require_session()?;
        let r = json::str_param(params, "entity")?;
        let live = self.world.entity(r).ok();
        let mut hist = ses.history.borrow_mut();
        // A gone entity is found by the id it was traced under.
        let e = live.or_else(|| {
            let id = if r.starts_with('@') || r.starts_with("e:") {
                r.to_string()
            } else {
                format!("@{r}")
            };
            hist.find(&id)
        });
        let Some(e) = e else {
            return Err(match self.world.entity(r).err() {
                // Gone, and not traced while it lived: say so, and how.
                Some(err) if err.code == argus_ecs::codes::ENTITY_NOT_ALIVE => ArgusError::new(
                    err.code,
                    format!("{r} is gone, and it was not being traced while it lived"),
                )
                .hint("Named entities are traced from the start; another is traced from the first obs.trace of it, so ask while it lives. Its events are still in obs.events {entity}."),
                Some(err) => err,
                None => ArgusError::new(codes::BAD_PARAMS, format!("no entity {r}")),
            });
        };
        let last = self.world.frame().saturating_sub(1);
        if !hist.traced(e) {
            // Not traced so far: it is from now on.
            hist.watch(e);
            return Ok(json!({
                "entity": crate::history::id_of(&self.world, e),
                "traced": false,
                "watching": true,
                "note": "Only named entities are traced from the start; this one is watched from now on. Step, then ask again.",
            }));
        }
        let t = hist.trace(e).expect("traced");
        let (from, to) = match params.get("frames") {
            None | Some(Json::Null) => (t.since, last),
            Some(Json::Number(n)) => {
                let n = n.as_u64().unwrap_or(1).max(1);
                (last.saturating_sub(n - 1).max(t.since), last)
            }
            Some(Json::Array(a)) if a.len() == 2 => {
                let f = a[0]
                    .as_u64()
                    .ok_or_else(|| bad_params("frames is a count or [from, to]"))?;
                let l = a[1]
                    .as_u64()
                    .ok_or_else(|| bad_params("frames is a count or [from, to]"))?;
                (f.max(t.since), l.min(last))
            }
            _ => {
                return Err(bad_params(
                    "frames is a count of recent frames or [from, to]",
                ))
            }
        };
        let want: Vec<String> = json::str_list(params, "fields").unwrap_or_default();
        let keep =
            |comp: &str, path: &str| want.is_empty() || want.iter().any(|w| w == comp || w == path);
        let points = json::u64_param(params, "points", 12)?.clamp(2, 1000) as usize;
        let story = t.story(&self.world, from, to, &keep);
        let v = |x: &Option<Value>| {
            x.as_ref()
                .map(|v| json::to_json(&self.world, v))
                .unwrap_or(Json::Null)
        };
        let fields: Vec<Json> = story
            .fields
            .iter()
            .map(|f| {
                let n = f.changes.len();
                let pick: Vec<&(u64, Option<Value>)> = if n <= points {
                    f.changes.iter().collect()
                } else {
                    f.changes
                        .iter()
                        .take(points / 2)
                        .chain(f.changes.iter().skip(n - points / 2))
                        .collect()
                };
                let mut o = json!({
                    "path": f.path,
                    "start": v(&f.start),
                    "end": v(&f.end),
                    "changes": n,
                    "points": pick.iter().map(|(fr, x)| json!([fr, v(x)])).collect::<Vec<_>>(),
                });
                if n > points {
                    o["skipped"] = json!(n - pick.len());
                }
                o
            })
            .collect();
        let id = t.id.clone();
        let gone = t.gone;
        let changed = story.changed.clone();
        drop(hist);
        // Its events, the latest last: components coming and going are in
        // `components` already, so those events are left out.
        let mut events: Vec<Json> = self
            .events
            .records
            .iter()
            .rev()
            .filter(|(_, rec)| rec.frame >= from && rec.frame <= to)
            .filter(|(_, rec)| {
                !matches!(
                    rec.event,
                    Event::ComponentAdded { .. } | Event::ComponentRemoved { .. }
                )
            })
            .map(|(seq, rec)| event_json(&self.world, *seq, rec))
            .filter(|row| {
                row["fields"]
                    .as_object()
                    .is_some_and(|o| o.values().any(|x| x.as_str() == Some(id.as_str())))
            })
            .take(24)
            .map(|row| {
                json!([
                    row["seq"],
                    row["frame"],
                    row["name"],
                    row.get("by").cloned().unwrap_or(Json::Null),
                    row["fields"]
                ])
            })
            .collect();
        events.reverse();
        let mut writers = serde_json::Map::new();
        for c in changed {
            let name = self.world.registry().schema(c).name.clone();
            writers.insert(name, json!(self.sched.who_writes(c).unwrap_or_default()));
        }
        let mut out = json!({
            "entity": id,
            "traced": true,
            "from": from,
            "to": to,
            "fields": fields,
            "steady": story.steady,
            "components": story.components.iter().map(|(f, c, a)| json!([f, c, if *a { "added" } else { "removed" }])).collect::<Vec<_>>(),
            "event_columns": ["seq", "frame", "name", "by", "fields"],
            "events": events,
            "writers": writers,
        });
        if let Some(g) = gone {
            out["gone"] = json!(g);
        }
        let bytes = serde_json::to_string(&out).map(|s| s.len()).unwrap_or(0);
        out["bytes"] = json!(bytes + 16);
        Ok(out)
    }

    /// Walk the declared write sets backwards from an event (section 8):
    /// the system that emitted it, the systems that write what it reads,
    /// and so on, in stage order. A static answer; obs.trace says what
    /// actually changed.
    fn obs_why(&mut self, params: &Json) -> Result<Json> {
        self.require_session()?;
        let depth = json::u64_param(params, "depth", 3)?.min(10) as usize;
        let rec = match params.get("event").and_then(Json::as_u64) {
            Some(seq) => self.events.records.iter().find(|(s, _)| *s == seq).cloned(),
            None => {
                let name = json::opt_str(params, "name")?.ok_or_else(|| {
                    bad_params("obs.why needs event (a seq from obs.events) or name (the latest such event)")
                })?;
                self.events
                    .records
                    .iter()
                    .rev()
                    .map(|(s, r)| (*s, r.clone()))
                    .find(|(s, r)| event_json(&self.world, *s, r)["name"] == name)
            }
        };
        let Some((seq, rec)) = rec else {
            return Err(
                ArgusError::new(codes::BAD_PARAMS, "no such event in the log").hint(
                    "obs.events lists events with their seq; the log keeps the last 200,000.",
                ),
            );
        };
        let ev = event_json(&self.world, seq, &rec);
        let infos = self.sched.systems(self.world.registry())?;
        let index: std::collections::HashMap<&str, usize> = infos
            .iter()
            .enumerate()
            .map(|(i, s)| (s.name.as_str(), i))
            .collect();
        // (system, depth, because)
        let mut found: Vec<(usize, usize, String)> = Vec::new();
        let mut frontier: Vec<usize> = Vec::new();
        match rec.by.as_deref().and_then(|b| index.get(b).copied()) {
            Some(i) => {
                found.push((i, 0, "emitted it".to_string()));
                frontier.push(i);
            }
            None => {
                // Emitted outside a system: what writes the entities it names.
                for v in ev["fields"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .map(|(_, v)| v)
                {
                    let Some(s) = v.as_str() else { continue };
                    let Ok(e) = self.world.entity(s) else {
                        continue;
                    };
                    for &c in self.world.components_of(e).unwrap_or(&[]) {
                        let cname = &self.world.registry().schema(c).name;
                        for (i, info) in infos.iter().enumerate() {
                            if info.writes.contains(cname) && !found.iter().any(|f| f.0 == i) {
                                found.push((i, 1, format!("writes {cname}, which {s} has")));
                                frontier.push(i);
                            }
                        }
                    }
                }
            }
        }
        for d in 1..=depth {
            let mut next = Vec::new();
            for &s in &frontier {
                let reads: Vec<&String> = infos[s]
                    .reads
                    .iter()
                    .chain(infos[s].writes.iter())
                    .collect();
                for r in reads {
                    for (i, info) in infos.iter().enumerate() {
                        if i == s || !info.writes.contains(r) || found.iter().any(|f| f.0 == i) {
                            continue;
                        }
                        found.push((i, d, format!("writes {r}, which {} reads", infos[s].name)));
                        next.push(i);
                    }
                }
            }
            frontier = next;
            if frontier.is_empty() {
                break;
            }
        }
        found.sort_by_key(|f| f.0);
        let rows: Vec<Json> = found
            .iter()
            .map(|(i, d, why)| {
                let s = &infos[*i];
                json!([s.name, s.stage.name(), d, why, s.source])
            })
            .collect();
        // What writes the entities the event names, whoever emitted it.
        let mut entities = Vec::new();
        for v in ev["fields"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(_, v)| v)
        {
            let Some(s) = v.as_str() else { continue };
            let Ok(e) = self.world.entity(s) else {
                continue;
            };
            let mut w = serde_json::Map::new();
            for &c in self.world.components_of(e).unwrap_or(&[]) {
                let ws = self.sched.who_writes(c).unwrap_or_default();
                if !ws.is_empty() {
                    w.insert(self.world.registry().schema(c).name.clone(), json!(ws));
                }
            }
            entities.push(json!({ "id": s, "writers": w }));
        }
        let mut out = json!({
            "event": ev,
            "columns": ["system", "stage", "depth", "because", "source"],
            "rows": rows,
            "entities": entities,
        });
        // The job system's events explain themselves: the worker's jobs,
        // each with what it makes of it (phase 3 item 3).
        if let Some(j) = self.jobs_for_event(&ev) {
            out["jobs"] = j;
        }
        if rec.by.is_none() {
            out["note"] = json!("No system emitted it (an edit, the daemon, or the mixer); the rows are the systems that write the entities it names.");
        }
        Ok(out)
    }

    fn obs_subscribe(&mut self, conn: ConnId, params: &Json) -> Result<Json> {
        let topics = json::str_list(params, "topics")?;
        for t in &topics {
            if t != "events" && t != "edits" && t != "tune" && t != "reviews" {
                return Err(bad_params(format!(
                    "unknown topic {t:?}; topics are events, edits, tune, and reviews"
                )));
            }
        }
        let off = json::bool_param(params, "off", false)?;
        let set = self.subs.entry(conn).or_default();
        for t in topics {
            if off {
                set.remove(&t);
            } else {
                set.insert(t);
            }
        }
        Ok(json!({ "topics": set.iter().collect::<Vec<_>>() }))
    }

    /// Rebuild the broadphase when the world changed outside a tick or a
    /// tick ran since the hash was built, so `within` sees current positions.
    fn ensure_broadphase(&mut self) {
        let stale = self.broadphase_epoch != self.mutations
            || self.world.broadphase().frame() != self.world.frame();
        if stale {
            self.world.rebuild_broadphase();
            self.broadphase_epoch = self.mutations;
        }
    }

    // ----- render -----------------------------------------------------------

    #[cfg(feature = "render")]
    fn obs_frame(&mut self, params: &Json) -> Result<Json> {
        self.require_session()?;
        let format = json::opt_str(params, "format")?.unwrap_or("png");
        if format != "png" && format != "semantic" {
            return Err(bad_params(format!(
                "format is \"png\" (the picture) or \"semantic\" (what the player sees, as data); got {format:?}"
            )));
        }
        let width = json::u64_param(params, "width", 640)?.clamp(1, 8192) as u32;
        let height = json::u64_param(params, "height", 360)?.clamp(1, 8192) as u32;
        let limit = json::u64_param(params, "limit", 24)?.min(1000) as usize;
        let groups = json::u64_param(params, "groups", 12)?.min(1000) as usize;
        let annotate = json::bool_param(params, "annotate", false)?;
        if format == "semantic" {
            // No picture, so no GPU: this works on a machine with no adapter.
            self.refresh_assets();
            let _ = self.sched.present(&mut self.world);
            return Ok(self.semantic_json(width, height, limit, groups, None));
        }
        let software = json::bool_param(params, "software", false)?;
        let see_as = see_as_param(params)?;
        let root = self.session.as_ref().expect("session").root.clone();
        let path = match json::opt_str(params, "path")? {
            Some(p) => {
                let p = PathBuf::from(p);
                if p.is_absolute() {
                    p
                } else {
                    root.join(p)
                }
            }
            None => root
                .join(".argus")
                .join("frames")
                .join(format!("frame-{}.png", self.sched.tick())),
        };
        let tick = self.sched.tick();
        // A picture of the world as it is now, the state world.query sees:
        // not part way from the tick before, as a window between ticks is.
        let alpha = 1.0;
        self.refresh_assets();
        let view = argus_render::view_for(&self.world, width, height);
        let extras = self.tile_extras(view);
        let edges = self.tile_occluders(view);
        let r = self.render.offscreen(
            width,
            height,
            software,
            &self.session.as_ref().expect("session").assets,
        )?;
        let _ = self.sched.present(&mut self.world);
        r.set_time(shader_time(&self.sched, alpha));
        r.set_ui(self.session.as_ref().map(|s| s.ui.clone()));
        r.set_occluders(edges);
        // Set every time: the renderer is kept between calls.
        r.gpu_mut().set_see_as(see_as);
        let store = self.session.as_ref().expect("session").particles.borrow();
        let rep = r.render_frame(&self.world, tick, alpha, &extras, Some(&store))?;
        drop(store);
        let mut img = r.capture()?;
        // Annotated: the ids and bounds of what was drawn, over the picture,
        // from the semantic frame of the same view.
        let marks = if annotate {
            let view = argus_render::View {
                center: rep.view_center,
                half: rep.view_half,
                angle: rep.view_angle,
            };
            let ses = self.session.as_ref().expect("session");
            let store = ses.particles.borrow();
            let sf = argus_render::semantic(
                &self.world,
                &argus_render::FrameSpec {
                    width,
                    height,
                    limit,
                    groups,
                    assets: Some(&ses.assets),
                    particles: Some(&store),
                    view: Some(view),
                    ui_canvas: self.ui_canvas(),
                },
            );
            let marks = argus_render::annotate(&mut img, &sf);
            Some(
                marks
                    .iter()
                    .map(|m| json!([m.label, m.bounds, m.labelled]))
                    .collect::<Vec<_>>(),
            )
        } else {
            None
        };
        img.save_png(&path)?;
        let mut out = json!({
            "path": shown_path(&path),
            "width": width,
            "height": height,
            "tick": tick,
            "see_as": see_as.map(|k| k.name()),
            "alpha": alpha,
            "sprites": rep.stats.sprites,
            "draw_calls": rep.stats.draw_calls,
            "atlases": rep.stats.atlases,
            "missing_atlases": rep.missing_atlases,
            "interpolated": rep.interpolated,
            "particles": rep.particles,
            "lights": rep.lights,
            "shadow_edges": rep.shadow_edges,
            "glowing": rep.glowing,
            "materials": rep.stats.materials,
            "missing_materials": rep.missing_materials,
            "material_problems": rep.material_problems,
            "camera": rep.camera.map(|e| json::entity_str(&self.world, e)),
            "view": {
                "center": rep.view_center,
                "half": rep.view_half,
                "angle": rep.view_angle,
                "rect": [
                    rep.view_center[0] - rep.view_half[0],
                    rep.view_center[1] - rep.view_half[1],
                    rep.view_center[0] + rep.view_half[0],
                    rep.view_center[1] + rep.view_half[1],
                ],
            },
            "adapter": self.render.offscreen.as_ref().map(|o| o.2.gpu().adapter.clone()),
            "software": self.render.offscreen.as_ref().is_some_and(|o| o.2.gpu().software),
            "ms": rep.ms,
        });
        if let Some(marks) = marks {
            out["annotated"] = json!(true);
            out["mark_columns"] = json!(["label", "bounds", "labelled"]);
            out["marks"] = Json::Array(marks);
        }
        // Against a golden picture (§9's image goldens, phase 5 item 0): how
        // many pixels a person would see differ, and a picture of where.
        if let Some(g) = json::opt_str(params, "golden")? {
            let gp = if Path::new(g).is_absolute() {
                PathBuf::from(g)
            } else {
                root.join(g)
            };
            let threshold = json::f64_param(params, "threshold", 0.1)?;
            let (w, h, rgba) = argus_assets::decode_png(&gp)?;
            let golden = argus_render::Image {
                width: w,
                height: h,
                rgba,
            };
            let c = img.compare(&golden, threshold)?;
            let diff = path.with_extension("diff.png");
            c.diff.save_png(&diff)?;
            out["compare"] = json!({
                "golden": g,
                "threshold": threshold,
                "pixels": c.pixels,
                "differing": c.differing,
                "fraction": c.fraction(),
                "worst": (c.worst * 1e4).round() / 1e4,
                "diff": diff.to_string_lossy(),
            });
        }
        Ok(out)
    }

    /// The semantic frame (section 8 L4) as the protocol returns it: the
    /// renderer's part, the tick, the collision of the tilemaps in view,
    /// and its own size.
    #[cfg(feature = "render")]
    fn semantic_json(
        &mut self,
        width: u32,
        height: u32,
        limit: usize,
        groups: usize,
        view: Option<argus_render::View>,
    ) -> Json {
        let started = std::time::Instant::now();
        let ses = self.session.as_ref().expect("session");
        let store = ses.particles.borrow();
        let sf = argus_render::semantic(
            &self.world,
            &argus_render::FrameSpec {
                width,
                height,
                limit,
                groups,
                assets: Some(&ses.assets),
                particles: Some(&store),
                view,
                ui_canvas: self.ui_canvas(),
            },
        );
        drop(store);
        let mut out = sf.to_json();
        out["tick"] = json!(self.sched.tick());
        out["time"] = json!((self.sched.time() * 1000.0).round() / 1000.0);
        let maps = self.semantic_tiles(&sf, width, height);
        if !maps.is_empty() {
            out["tiles"] = json!({
                "key": "# solid, - one-way, / and \\ slopes, _ partial, . empty",
                "maps": maps,
            });
        }
        out["ms"] = json!((started.elapsed().as_secs_f64() * 1e4).round() / 10.0);
        let bytes = serde_json::to_string(&out).map(|s| s.len()).unwrap_or(0);
        out["bytes"] = json!(bytes + 16);
        out
    }

    /// What the tilemaps in view collide with, one character a cell, top
    /// row first, with where the first cell is in the frame's pixels.
    #[cfg(feature = "render")]
    fn semantic_tiles(
        &mut self,
        sf: &argus_render::SemanticFrame,
        width: u32,
        height: u32,
    ) -> Vec<Json> {
        let Some(tm) = self.world.component_id_of::<argus_tiles::Tilemap>() else {
            return Vec::new();
        };
        let mut maps = Vec::new();
        for e in self.world.query().has(tm).entities() {
            let Ok(t) = self.world.get::<argus_tiles::Tilemap>(e) else {
                continue;
            };
            if !t.visible {
                continue;
            }
            let origin = self
                .world
                .get::<argus_ecs::GlobalTransform>(e)
                .map(|g| [g.pos.x, g.pos.y])
                .or_else(|_| {
                    self.world
                        .get::<argus_ecs::Transform>(e)
                        .map(|t| [t.pos.x, t.pos.y])
                })
                .unwrap_or([0.0, 0.0]);
            maps.push((
                json::entity_str(&self.world, e),
                self.world.strings().resolve(t.map),
                origin,
            ));
        }
        let Some(ses) = self.session.as_ref() else {
            return Vec::new();
        };
        let mut st = ses.tiles.borrow_mut();
        let v = sf.view;
        let (w, h) = (width as f32, height as f32);
        let mut out = Vec::new();
        for (id, file, origin) in maps {
            let Some(g) = st.grid(&file, origin) else {
                continue;
            };
            let Some((rows, cell, step)) = st.collision_rows(&g, sf.rect, [64, 36]) else {
                continue;
            };
            if rows.iter().all(|r| r.chars().all(|c| c == '.')) {
                continue;
            }
            // The first cell's top-left corner in pixels, and pixels a character.
            let (x, y) = (g.left(cell[0]), g.top(cell[1]));
            let (dx, dy) = (x - v.center[0], y - v.center[1]);
            let (sn, cs) = v.angle.sin_cos();
            let (vx, vy) = (dx * cs + dy * sn, -dx * sn + dy * cs);
            let px = vx / (2.0 * v.half[0]) * w + w / 2.0;
            let py = h / 2.0 - vy / (2.0 * v.half[1]) * h;
            let per = step as f32 * g.tile / (2.0 * v.half[0]) * w;
            out.push(json!({
                "entity": id,
                "map": file,
                "cell": cell,
                "step": step,
                "at": [px.round(), py.round(), (per * 10.0).round() / 10.0],
                "rows": rows,
            }));
        }
        out
    }

    #[cfg(feature = "render")]
    fn render_window(&mut self, params: &Json) -> Result<Json> {
        let open = json::bool_param(params, "open", true)?;
        if !open {
            self.render.close_window();
            self.exit_on_window_close = false;
            #[cfg(feature = "audio")]
            if let Some(a) = self.audio.as_mut() {
                a.close_device();
            }
            return Ok(self.render_status());
        }
        self.require_session()?;
        let width = json::u64_param(params, "width", 1280)?.clamp(64, 8192) as u32;
        let height = json::u64_param(params, "height", 720)?.clamp(64, 8192) as u32;
        let software = json::bool_param(params, "software", false)?;
        let visible = json::bool_param(params, "visible", true)?;
        let title = json::opt_str(params, "title")?
            .map(str::to_string)
            .unwrap_or_else(|| {
                let ses = self.session.as_ref().expect("session");
                format!("argus: {} ({})", ses.project.config.name, ses.scene_name)
            });
        self.render.open_window(
            &title,
            width,
            height,
            software,
            visible,
            &self.session.as_ref().expect("session").assets,
        )?;
        self.toast = Some((
            match self.waiting_line() {
                Some(w) => format!("F8 MARKS A MOMENT.  {w}"),
                None => "F8 MARKS THIS MOMENT FOR THE AGENT".to_string(),
            },
            std::time::Instant::now() + std::time::Duration::from_secs(4),
        ));
        // The renderer a mark draws its frame and clip with, made now so
        // the first F8 does not wait for it.
        let _ = self.render.offscreen(
            width,
            height,
            false,
            &self.session.as_ref().expect("session").assets,
        );
        #[cfg(feature = "audio")]
        if json::bool_param(params, "audio", true)? {
            if let Some(a) = self.audio.as_mut() {
                if let Err(e) = a.open_device() {
                    eprintln!("argusd: no audio output: {e}");
                }
            }
        }
        // Held from the first sample if the game is not playing.
        self.sync_audio();
        Ok(self.render_status())
    }

    #[cfg(feature = "render")]
    fn render_status(&self) -> Json {
        let (window, size, adapter, software, last, atlases) = match &self.render.window {
            Some((host, r)) => (
                true,
                host.size(),
                r.gpu().adapter.clone(),
                r.gpu().software,
                Some(r.last().clone()),
                r.gpu().atlas_names().to_vec(),
            ),
            None => match &self.render.offscreen {
                Some((w, h, r)) => (
                    false,
                    (*w, *h),
                    r.gpu().adapter.clone(),
                    r.gpu().software,
                    Some(r.last().clone()),
                    r.gpu().atlas_names().to_vec(),
                ),
                None => (false, (0, 0), String::new(), false, None, Vec::new()),
            },
        };
        json!({
            "window": window,
            "stream": self.render.stream.as_ref().map(|s| json!({
                "path": shown_path(s.writer.path()),
                "size": [s.writer.size().0, s.writer.size().1],
                "fps": s.fps,
                "frames": s.frames,
                "last_ms": (s.last_ms * 1000.0).round() / 1000.0,
                "error": s.error,
            })),
            "size": [size.0, size.1],
            "adapter": adapter,
            "software": software,
            "last": last.map(|l| json!({
                "sprites": l.stats.sprites,
                "draw_calls": l.stats.draw_calls,
                "atlases": l.stats.atlases,
                "interpolated": l.interpolated,
                "missing_atlases": l.missing_atlases,
                "lights": l.lights,
                "materials": l.stats.materials,
                "missing_materials": l.missing_materials,
                "material_problems": l.material_problems,
                "ms": l.ms,
            })),
            "atlases": atlases,
            "materials": self.render.materials(),
            "gamepads": self.render.pads.is_some(),
            "bar": self.render.bar.as_ref().map(|(t, _)| t.clone()),
        })
    }

    #[cfg(feature = "render")]
    fn render_present(&mut self) {
        if self.render.window.is_none() {
            return;
        }
        // A proposal being tried: its other worlds keep step.
        if let Err(e) = self.sync_trial() {
            self.end_trial(true);
            self.toast = Some((
                format!("TRIAL ENDED: {}", e.message),
                std::time::Instant::now() + std::time::Duration::from_secs(5),
            ));
        }
        let _ = self.sched.present(&mut self.world);
        let tick = self.sched.tick();
        let alpha = self.sched.alpha();
        let size = self
            .render
            .window
            .as_ref()
            .map(|(h, _)| h.size())
            .unwrap_or((1, 1));
        let view = argus_render::view_for(&self.world, size.0, size.1);
        let extras = self.tile_extras(view);
        let edges = self.tile_occluders(view);
        let t = shader_time(&self.sched, alpha);
        self.update_bar(size.0);
        if let (Some((_, r)), Some(ses)) = (&mut self.render.window, &self.session) {
            r.set_time(t);
            r.set_ui(Some(ses.ui.clone()));
            r.set_occluders(edges);
            let store = ses.particles.borrow();
            let _ = r.render_frame(&self.world, tick, alpha, &extras, Some(&store));
        }
        self.pump_events();
    }

    #[cfg(not(feature = "render"))]
    fn obs_frame(&mut self, _params: &Json) -> Result<Json> {
        Err(ArgusError::new(
            5005,
            "this daemon was built without the renderer",
        ))
    }
    #[cfg(not(feature = "render"))]
    fn render_window(&mut self, _params: &Json) -> Result<Json> {
        Err(ArgusError::new(
            5005,
            "this daemon was built without the renderer",
        ))
    }
    #[cfg(not(feature = "render"))]
    fn render_status(&self) -> Json {
        json!({ "window": false, "adapter": null })
    }
    #[cfg(not(feature = "render"))]
    fn render_stream(&mut self, _conn: ConnId, _params: &Json) -> Result<Json> {
        Err(ArgusError::new(
            5005,
            "this daemon was built without the renderer",
        ))
    }
    /// A client's stream opened the sound (the scene's mixer opens its
    /// device for it, as for a window).
    #[cfg(feature = "audio")]
    fn stream_has_audio(&self) -> bool {
        #[cfg(feature = "render")]
        {
            self.render.stream.as_ref().is_some_and(|s| s.audio)
        }
        #[cfg(not(feature = "render"))]
        {
            false
        }
    }

    /// `render.stream` (phase 4 item 1): frames for this connection, drawn
    /// by the daemon into a file both processes map, each announced by a
    /// `render.frame` notification naming its slot (§5, §11: "the viewport
    /// subscribes to frames"). The gamepads open with it, and the sound.
    #[cfg(feature = "render")]
    fn render_stream(&mut self, conn: ConnId, params: &Json) -> Result<Json> {
        use argus_client::frames::{FrameWriter, HEADER, SLOTS, SLOT_HEADER};
        if !json::bool_param(params, "open", true)? {
            self.close_stream(conn);
            return Ok(json!({ "stream": null }));
        }
        self.require_session()?;
        let width = json::u64_param(params, "width", 1280)?.clamp(16, 8192) as u32;
        let height = json::u64_param(params, "height", 720)?.clamp(16, 8192) as u32;
        let fps = json::f64_param(params, "fps", 60.0)?.clamp(1.0, 240.0);
        let software = json::bool_param(params, "software", false)?;
        let see_as = see_as_param(params)?;
        let audio = json::bool_param(params, "audio", true)?;
        if let Some(s) = &self.render.stream {
            if s.conn != conn {
                return Err(bad_params("another client is streaming this daemon's frames")
                    .hint("One viewport at a time; its client closes it with render.stream {open: false}, or by disconnecting."));
            }
        }
        let same = self.render.stream.as_ref().is_some_and(|s| {
            s.writer.size() == (width, height) && s.renderer.gpu().software == software
        });
        if !same {
            let old = self.render.stream.take();
            let renderer = match old {
                Some(s) if s.renderer.gpu().software == software => {
                    let mut r = s.renderer;
                    r.gpu_mut().resize(width, height);
                    r.forget_motion();
                    r
                }
                _ => {
                    let mut r = argus_render::Renderer::offscreen(width, height, software)?;
                    r.load_catalog(&self.session.as_ref().expect("session").assets)?;
                    r
                }
            };
            self.render.streams_made += 1;
            // In the project's .argus/streams/, which only this project's
            // daemon writes: files left there by an earlier stream (one a
            // client still mapped when it closed, or a daemon that did not
            // end cleanly) go now; one still mapped stays for the next.
            let dir = self
                .session
                .as_ref()
                .expect("session")
                .root
                .join(".argus")
                .join("streams");
            let _ = std::fs::create_dir_all(&dir);
            for f in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let _ = std::fs::remove_file(f.path());
            }
            let path = dir.join(format!(
                "frames-{}-{conn}-{}.bin",
                std::process::id(),
                self.render.streams_made
            ));
            let writer = FrameWriter::create(&path, width, height).map_err(|e| {
                ArgusError::new(
                    argus_render::codes::GPU,
                    format!("cannot make the frame stream {}: {e}", path.display()),
                )
            })?;
            self.render.stream = Some(render::Stream {
                conn,
                renderer,
                writer,
                fps,
                audio: false,
                next_due: std::time::Instant::now(),
                last: None,
                bar: None,
                frames: 0,
                last_ms: 0.0,
                error: None,
            });
        }
        self.render.open_pads();
        let mut sound = Json::Null;
        #[cfg(feature = "audio")]
        if audio {
            if let Some(a) = self.audio.as_mut() {
                sound = match a.open_device() {
                    Ok(()) => json!(a.device_name()),
                    Err(e) => json!({ "error": e.message }),
                };
            }
        }
        // Held from the first sample if the game is not playing.
        self.sync_audio();
        let s = self.render.stream.as_mut().expect("just made");
        s.renderer.gpu_mut().set_see_as(see_as);
        s.fps = fps;
        s.audio = audio && !sound.is_null() && sound.get("error").is_none();
        // The first frame goes at once.
        s.next_due = std::time::Instant::now();
        s.last = None;
        Ok(json!({
            "stream": {
                "path": s.writer.path().to_string_lossy(),
                "width": width, "height": height, "fps": fps,
                "slots": SLOTS, "header_bytes": HEADER, "slot_header_bytes": SLOT_HEADER,
                "format": "rgba8, rows top first",
            },
            "audio": sound,
            "adapter": s.renderer.gpu().adapter,
            "software": s.renderer.gpu().software,
            "see_as": see_as.map(|k| k.name()),
        }))
    }

    // ----- audio ------------------------------------------------------------

    #[cfg(feature = "audio")]
    fn audio_engine(&mut self) -> Result<&mut argus_audio::AudioEngine> {
        self.require_session()?;
        let err = self.audio_error.clone();
        self.audio.as_mut().ok_or_else(|| {
            let msg = err
                .as_ref()
                .and_then(|e| e["message"].as_str().map(str::to_string))
                .unwrap_or_else(|| "the mixer is not loaded".to_string());
            ArgusError::new(
                argus_audio::codes::BAD_AUDIO_TOML,
                format!("no mixer: {msg}"),
            )
            .hint("Fix audio.toml and scene.load again; audio.status shows the error.")
        })
    }

    #[cfg(feature = "audio")]
    fn audio_status(&mut self) -> Result<Json> {
        self.require_session()?;
        let Some(a) = self.audio.as_ref() else {
            return Ok(json!({ "device": null, "error": self.audio_error }));
        };
        let (meters, peak, rms, limiter) = a.meters();
        let voices: std::collections::BTreeMap<String, usize> =
            a.bus_voices().into_iter().collect();
        let r1 = |x: f32| (x as f64 * 10.0).round() / 10.0;
        let rows: Vec<Json> = meters
            .iter()
            .map(|m| {
                json!([
                    m.name,
                    m.parent,
                    r1(m.gain_db),
                    m.mute,
                    r1(m.duck_db),
                    r1(m.peak_db),
                    r1(m.rms_db),
                    voices.get(&m.name).copied().unwrap_or(0)
                ])
            })
            .collect();
        Ok(json!({
            "device": a.device_name(),
            "held": a.held(),
            "rate": a.rate(),
            "voices": a.voices(),
            "played": a.played,
            "master": { "peak_db": r1(peak), "rms_db": r1(rms), "limiter_db": r1(limiter) },
            "columns": ["bus", "parent", "gain_db", "mute", "duck_db", "peak_db", "rms_db", "voices"],
            "rows": rows,
            "music": a.music().map(|(t, l)| {
                let pos = a.music_position();
                json!({
                    "track": t,
                    "layers": l.iter().map(|(n, on)| json!([n, on])).collect::<Vec<_>>(),
                    "sounding": pos.as_ref().and_then(|p| p.track.clone()),
                    "bpm": pos.as_ref().and_then(|p| p.bpm),
                    "bar": pos.as_ref().and_then(|p| p.bar),
                    "beat": pos.as_ref().and_then(|p| p.beat),
                    "seconds": pos.as_ref().map(|p| (p.seconds * 1000.0).round() / 1000.0),
                    "queued": pos.as_ref().and_then(|p| p.queued.as_ref()).map(|(t, ms)| json!({ "track": t, "in_ms": (ms * 10.0).round() / 10.0 })),
                })
            }),
            "sounds": a.sound_names(),
            "missing": a.missing(),
            "warnings": a.warnings,
            "error": self.audio_error,
        }))
    }

    #[cfg(feature = "audio")]
    fn audio_play(&mut self, params: &Json) -> Result<Json> {
        self.audio_engine()?;
        let sound = json::str_param(params, "sound")?.to_string();
        let mut fields = vec![("sound".to_string(), Value::Str(sound))];
        if let Some(e) = json::opt_str(params, "entity")? {
            fields.push(("entity".to_string(), Value::Entity(self.world.entity(e)?)));
        }
        for k in ["gain", "pitch", "pan"] {
            if let Some(v) = params.get(k).and_then(Json::as_f64) {
                fields.push((k.to_string(), Value::Float(v)));
            }
        }
        if let Some(b) = json::opt_str(params, "bus")? {
            fields.push(("bus".to_string(), Value::Str(b.to_string())));
        }
        if json::bool_param(params, "loop", false)? {
            fields.push(("loop".to_string(), Value::Bool(true)));
        }
        fields.sort_by(|a, b| a.0.cmp(&b.0));
        let before = self.events.next_seq;
        self.world.emit(Event::Game {
            name: "sound.play".to_string(),
            fields,
        });
        self.pump_events();
        let answer = self.events.records.iter().rev().find(|(seq, r)| {
            *seq >= before
                && matches!(&r.event, Event::Game { name, .. } if name == "sound.played" || name == "sound.missing")
        });
        match answer {
            Some((seq, r)) => Ok(event_json(&self.world, *seq, r)),
            None => Ok(json!({ "name": "sound.missing", "fields": {} })),
        }
    }

    #[cfg(feature = "audio")]
    fn audio_stop(&mut self, params: &Json) -> Result<Json> {
        self.audio_engine()?;
        let mut fields = Vec::new();
        if let Some(v) = params.get("voice").and_then(Json::as_u64) {
            fields.push(("voice".to_string(), Value::Int(v as i64)));
        }
        if let Some(e) = json::opt_str(params, "entity")? {
            fields.push(("entity".to_string(), Value::Entity(self.world.entity(e)?)));
        }
        if let Some(f) = params.get("fade_ms").and_then(Json::as_f64) {
            fields.push(("fade_ms".to_string(), Value::Float(f)));
        }
        let before = self.events.next_seq;
        self.world.emit(Event::Game {
            name: "sound.stop".to_string(),
            fields,
        });
        self.pump_events();
        let stopped: Vec<Json> = self
            .events
            .records
            .iter()
            .filter(|(seq, r)| {
                *seq >= before
                    && matches!(&r.event, Event::Game { name, .. } if name == "sound.stopped")
            })
            .filter_map(|(_, r)| match &r.event {
                Event::Game { fields, .. } => fields
                    .iter()
                    .find(|f| f.0 == "voice")
                    .map(|f| json::to_json(&self.world, &f.1)),
                _ => None,
            })
            .collect();
        Ok(json!({ "stopped": stopped }))
    }

    #[cfg(feature = "audio")]
    fn audio_bus(&mut self, params: &Json) -> Result<Json> {
        let bus = json::str_param(params, "bus")?.to_string();
        let gain = params
            .get("gain_db")
            .and_then(Json::as_f64)
            .map(|g| g as f32);
        let mute = match params.get("mute") {
            None | Some(Json::Null) => None,
            Some(_) => Some(json::bool_param(params, "mute", false)?),
        };
        self.audio_engine()?.set_bus(&bus, gain, mute)?;
        self.audio_status()
    }

    #[cfg(feature = "audio")]
    fn audio_clip(&mut self, params: &Json) -> Result<Json> {
        let seconds = json::f64_param(params, "seconds", 5.0)?.clamp(0.01, 10.0) as f32;
        let root = self.require_session()?.root.clone();
        let path = match json::opt_str(params, "path")? {
            Some(p) if PathBuf::from(p).is_absolute() => PathBuf::from(p),
            Some(p) => root.join(p),
            None => root
                .join(".argus")
                .join("clips")
                .join(format!("audio-{}.wav", self.sched.tick())),
        };
        let (frames, peak) = self.audio_engine()?.clip(seconds, &path)?;
        Ok(json!({
            "path": path.to_string_lossy(),
            "seconds": seconds,
            "frames": frames,
            "peak_db": (peak as f64 * 10.0).round() / 10.0,
        }))
    }

    /// `audio.describe`: what a sound is like, for someone who cannot hear
    /// it, each variation measured from the samples the mixer plays.
    #[cfg(feature = "audio")]
    fn audio_describe(&mut self, params: &Json) -> Result<Json> {
        let name = json::str_param(params, "sound")?.to_string();
        let a = self.audio_engine()?;
        let heard = a.hear(&name).ok_or_else(|| {
            ArgusError::new(
                argus_audio::codes::UNKNOWN_SOUND,
                format!("there is no sound {name:?}"),
            )
            .hint("audio.toml's [sounds] and the sound assets (asset.list kind=sound) are the sounds; a sound made a moment ago plays once asset.import has taken it.")
        })?;
        let variations: Vec<Json> = heard
            .iter()
            .map(|(src, h)| {
                let mut o = h.to_json();
                o["source"] = json!(src);
                o
            })
            .collect();
        Ok(json!({ "sound": name, "variations": variations }))
    }

    /// `asset.generate`: a new asset's text, a sound or a sprite, from a
    /// kind and a seed, with what it is like. It writes nothing; the
    /// generate tool writes it, as an edit.
    fn asset_generate(&mut self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        let name = json::str_param(params, "name")?
            .trim()
            .trim_start_matches("assets/")
            .trim_end_matches(".toml")
            .trim_end_matches(".art.luau")
            .trim_matches('/')
            .to_string();
        if name.is_empty()
            || name
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == "..")
        {
            return Err(ArgusError::new(
                codes::BAD_PARAMS,
                format!("{name:?} is not an asset name"),
            )
            .hint("An asset's name is its path under assets/ without the extension: sfx/jump, sprites/slime."));
        }
        let seed = json::u64_param(params, "seed", 1)?;
        let Some(kind) = json::opt_str(params, "sprite")? else {
            return self.sound_generate(params, &root, &name, seed);
        };
        use argus_assets::artgen;
        let text = artgen::generate(kind, seed, &name).map_err(|m| {
            let kinds: Vec<String> = artgen::KINDS
                .iter()
                .map(|(k, d)| format!("{k} ({d})"))
                .collect();
            ArgusError::new(codes::BAD_PARAMS, m).hint(format!("The kinds: {}.", kinds.join("; ")))
        })?;
        let file = format!("assets/{name}.art.luau");
        let drawn = argus_assets::art::run(&file, &text)?;
        Ok(json!({
            "name": name,
            "file": file,
            "exists": root.join(&file).exists(),
            "text": text,
            "drawn": {
                "frames": drawn.frames.len(),
                "sizes": drawn.frames.iter().map(|f| [f.canvas.w, f.canvas.h]).collect::<Vec<_>>(),
                "clips": drawn.clips.keys().collect::<Vec<_>>(),
            },
        }))
    }

    /// `asset.preview`: a sprite's frames in a row, scaled up on a
    /// checkerboard (so its clear pixels show), as a PNG to look at.
    fn asset_preview(&mut self, params: &Json) -> Result<Json> {
        let name = json::str_param(params, "name")?.to_string();
        let ses = self.require_session()?;
        let root = ses.root.clone();
        let e = ses.assets.get(&name)?;
        let argus_assets::Asset::Sprite(s) = &e.asset else {
            return Err(ArgusError::new(
                codes::BAD_PARAMS,
                format!("{name} is a {}, not a sprite", e.kind()),
            ));
        };
        let (tw, _, sheet) = argus_assets::decode_png(&s.texture)?;
        let frames: Vec<[u32; 4]> = s.frames.iter().map(|f| f.rect).collect();
        let fh = frames.iter().map(|r| r[3]).max().unwrap_or(1);
        let fw: u32 = frames.iter().map(|r| r[2]).sum::<u32>() + frames.len() as u32;
        let k = json::u64_param(params, "scale", 0)? as u32;
        let k = if k > 0 {
            k.min(16)
        } else {
            (384 / fh.max(1)).clamp(1, 8)
        };
        let (w, h) = ((fw + 1) * k, (fh + 2) * k);
        if w as u64 * h as u64 > 16_000_000 {
            return Err(ArgusError::new(
                codes::BAD_PARAMS,
                format!("{name}'s preview at {k}x would be {w} x {h}; ask for a smaller scale"),
            ));
        }
        let mut out = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let c: u8 = if ((x / 8) + (y / 8)) % 2 == 0 { 58 } else { 74 };
                let i = ((y * w + x) * 4) as usize;
                out[i..i + 4].copy_from_slice(&[c, c, c + 8, 255]);
            }
        }
        let mut x0 = 1u32;
        for r in &frames {
            for y in 0..r[3] {
                for x in 0..r[2] {
                    let si = (((r[1] + y) * tw + r[0] + x) * 4) as usize;
                    let p = &sheet[si..si + 4];
                    if p[3] == 0 {
                        continue;
                    }
                    let a = p[3] as u32;
                    for dy in 0..k {
                        for dx in 0..k {
                            let oy = (1 + y + fh - r[3]) * k + dy;
                            let ox = (x0 + x) * k + dx;
                            let o = ((oy * w + ox) * 4) as usize;
                            for ch in 0..3 {
                                out[o + ch] = ((p[ch] as u32 * a + out[o + ch] as u32 * (255 - a))
                                    / 255) as u8;
                            }
                        }
                    }
                }
            }
            x0 += r[2] + 1;
        }
        let path = root
            .join(".argus")
            .join("previews")
            .join(format!("{}.png", name.replace('/', "_")));
        argus_assets::encode_png(&path, w, h, &out)?;
        Ok(json!({
            "name": name,
            "path": path.to_string_lossy(),
            "frames": frames.len(),
            "scale": k,
            "clips": s.clips.keys().collect::<Vec<_>>(),
        }))
    }

    /// The sound half of `asset.generate`.
    #[cfg(feature = "audio")]
    fn sound_generate(
        &mut self,
        params: &Json,
        root: &Path,
        name: &str,
        seed: u64,
    ) -> Result<Json> {
        use argus_audio::gen;
        let name = name.to_string();
        let file = format!("assets/{name}.toml");
        let exists = root.join(&file).exists();
        let kinds = || {
            gen::KINDS
                .iter()
                .map(|(k, d)| format!("{k} ({d})"))
                .collect::<Vec<_>>()
                .join("; ")
        };
        let (tone, text) = match (json::opt_str(params, "sound")?, json::opt_str(params, "from")?) {
            (Some(kind), None) => {
                let t = gen::generate(kind, seed).map_err(|m| {
                    ArgusError::new(codes::BAD_PARAMS, m).hint(format!("The kinds: {}.", kinds()))
                })?;
                let text = gen::sidecar(&name, kind, seed, None, &t);
                (t, text)
            }
            (None, Some(from)) => {
                let share = json::f64_param(params, "vary", 0.1)? as f32;
                let tones = self.audio_engine()?.tones(from).ok_or_else(|| {
                    ArgusError::new(
                        argus_audio::codes::UNKNOWN_SOUND,
                        format!("there is no sound {from:?} to vary"),
                    )
                })?;
                let Some(t) = tones.first() else {
                    return Err(ArgusError::new(
                        codes::BAD_PARAMS,
                        format!("{from} plays a file, not a tone; from= varies a tone"),
                    ));
                };
                let v = gen::vary(t, share, seed);
                let text = gen::sidecar(&name, "", seed, Some((from, share)), &v);
                (v, text)
            }
            _ => {
                return Err(ArgusError::new(
                    codes::BAD_PARAMS,
                    "asset.generate takes sound=<kind> (a new one) or from=<sound> (a variation of one)",
                )
                .hint(format!("The kinds: {}. For a sprite, give sprite=<kind>.", kinds())))
            }
        };
        let rate = 48_000;
        let heard = argus_audio::hear::hear(&argus_audio::sound::synth(&tone, rate), rate);
        Ok(json!({
            "name": name,
            "file": file,
            "exists": exists,
            "text": text,
            "heard": heard.to_json(),
        }))
    }

    #[cfg(feature = "audio")]
    fn audio_device(&mut self, params: &Json) -> Result<Json> {
        let open = json::bool_param(params, "open", true)?;
        let a = self.audio_engine()?;
        if open {
            a.open_device()?;
        } else {
            a.close_device();
        }
        self.audio_status()
    }

    #[cfg(not(feature = "audio"))]
    fn audio_status(&mut self) -> Result<Json> {
        Ok(json!({ "device": null, "error": "this daemon was built without audio" }))
    }
    #[cfg(not(feature = "audio"))]
    fn audio_play(&mut self, _p: &Json) -> Result<Json> {
        Err(ArgusError::new(5103, "this daemon was built without audio"))
    }
    #[cfg(not(feature = "audio"))]
    fn audio_stop(&mut self, _p: &Json) -> Result<Json> {
        Err(ArgusError::new(5103, "this daemon was built without audio"))
    }
    #[cfg(not(feature = "audio"))]
    fn audio_bus(&mut self, _p: &Json) -> Result<Json> {
        Err(ArgusError::new(5103, "this daemon was built without audio"))
    }
    #[cfg(not(feature = "audio"))]
    fn audio_clip(&mut self, _p: &Json) -> Result<Json> {
        Err(ArgusError::new(5103, "this daemon was built without audio"))
    }
    #[cfg(not(feature = "audio"))]
    fn audio_device(&mut self, _p: &Json) -> Result<Json> {
        Err(ArgusError::new(5103, "this daemon was built without audio"))
    }
    #[cfg(not(feature = "audio"))]
    fn audio_describe(&mut self, _p: &Json) -> Result<Json> {
        Err(ArgusError::new(5103, "this daemon was built without audio"))
    }
    #[cfg(not(feature = "audio"))]
    fn sound_generate(&mut self, _p: &Json, _r: &Path, _n: &str, _s: u64) -> Result<Json> {
        Err(ArgusError::new(5103, "this daemon was built without audio"))
    }

    // ----- test -------------------------------------------------------------

    /// `test.run`: run scenarios through this daemon's own methods, then
    /// put back the scene that was open (§9: headless in the daemon).
    fn test_run(&mut self, params: &Json) -> Result<Json> {
        let before = {
            let ses = self.require_session()?;
            (ses.scene_name.clone(), ses.seed)
        };
        let day = ProjectConfig::load(&self.require_session()?.root)?.day_seconds;
        let mods_before = self.mods_asked.clone();
        let play_before = self.play_state();
        let opts = argus_test::Options {
            day_seconds: Some(day),
            name: json::opt_str(params, "name")?.map(str::to_string),
            file: json::opt_str(params, "file")?.map(str::to_string),
            seeds: match params.get("seeds") {
                None | Some(Json::Null) => None,
                Some(Json::String(s)) => Some(s.clone()),
                Some(Json::Number(n)) => Some(n.to_string()),
                Some(other) => {
                    return Err(bad_params(format!(
                        "seeds must be a string like \"1..50\"; got {other}"
                    )))
                }
            },
            update_goldens: json::bool_param(params, "update", false)?,
            replays: json::bool_param(params, "replays", true)?,
            software: json::bool_param(params, "software", false)?,
            set: override_list(params.get("set"))?,
        };
        let sweep = sweep_of(params.get("sweep"))?;
        self.live_edits = true;
        let report = match &sweep {
            None => argus_test::run(self, &opts),
            Some((target, values)) => self.run_sweep(&opts, target, values),
        };
        // The scene the session had, stopped (whatever the last scenario
        // left playing), then the play state it had.
        let restore = self.call(
            0,
            "scene.load",
            &json!({ "scene": before.0, "seed": before.1, "mods": mods_before }),
        );
        self.live_edits = false;
        let mut report = report.map_err(|e| ArgusError::new(codes::TEST_RUNNER, e))?;
        match restore {
            Err(e) => report["restore_error"] = json::error_json(&e),
            // The game plays on after the tests if it was playing before.
            Ok(_) => self.resume(play_before)?,
        }
        Ok(report)
    }

    // ----- prof -------------------------------------------------------------

    fn prof_frame(&self) -> Json {
        match self.sched.prof_last() {
            Some(t) => tick_json(t),
            None => json!({ "tick": null }),
        }
    }

    fn prof_systems(&mut self, params: &Json) -> Result<Json> {
        let last = json::u64_param(params, "last", 600)? as usize;
        let host = self.session.as_ref().map(|s| &s.host);
        let rows: Vec<Json> = self
            .sched
            .prof_systems(last)?
            .iter()
            .map(|p| {
                // A script system's checks of the instruction budget, last
                // run and most (issue 13); null for the engine's own.
                let checks = host.and_then(|h| h.checks(&p.name));
                json!([
                    p.name,
                    p.stage.name(),
                    p.mean_ms,
                    p.p95_ms,
                    p.max_ms,
                    p.samples,
                    checks.map(|c| c.0),
                    checks.map(|c| c.1)
                ])
            })
            .collect();
        let mut out = json!({
            "last": last,
            "tick_p95_ms": self.sched.prof_tick_p95_ms(last),
            "columns": ["name", "stage", "mean_ms", "p95_ms", "max_ms", "samples", "checks", "checks_max"],
            "checks_budget": host.map(|h| h.config().interrupt_budget),
            "rows": rows,
        });
        if json::bool_param(params, "series", false)? {
            out["ticks"] = json!(self
                .sched
                .prof_totals(last)
                .iter()
                .map(|(t, ms)| json!([t, (ms * 1e3).round() / 1e3]))
                .collect::<Vec<_>>());
        }
        Ok(out)
    }

    fn prof_alloc(&mut self, params: &Json) -> Result<Json> {
        let last = json::u64_param(params, "last", 600)? as usize;
        let rows: Vec<Json> = self
            .sched
            .prof_alloc(last)?
            .iter()
            .map(|p| {
                json!([
                    p.name,
                    p.stage.name(),
                    p.mean_allocs,
                    p.mean_bytes,
                    p.max_allocs,
                    p.max_bytes,
                    p.samples
                ])
            })
            .collect();
        let (allocs, bytes) = self.sched.prof_tick_alloc_mean(last);
        Ok(json!({
            "last": last,
            "counted": argus_sim::alloc::installed(),
            "tick_mean_allocs": allocs,
            "tick_mean_bytes": bytes,
            "columns": ["name", "stage", "mean_allocs", "mean_bytes", "max_allocs", "max_bytes", "samples"],
            "rows": rows,
        }))
    }

    /// `prof.capture {ticks, path}`: arm a capture of the next ticks, or
    /// report the one in progress.
    fn prof_capture(&mut self, params: &Json) -> Result<Json> {
        let path = json::opt_str(params, "path")?.map(PathBuf::from);
        let ticks = match params.get("ticks") {
            None | Some(Json::Null) => None,
            Some(_) => Some(json::u64_param(params, "ticks", 300)?),
        };
        if let Some(n) = ticks {
            if !(1..=100_000).contains(&n) {
                return Err(bad_params(format!("ticks must be 1 to 100000; got {n}")));
            }
            let path = match path {
                Some(p) => p,
                None => {
                    let root = self.require_session()?.root.clone();
                    let stamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    root.join(".argus")
                        .join("captures")
                        .join(format!("{stamp}-tick{}.json", self.sched.tick()))
                }
            };
            self.sched.capture_start(n as usize);
            self.capture_path = Some(path.clone());
            return Ok(json!({
                "armed": true,
                "captured": 0,
                "ticks": n,
                "path": shown_path(&path),
                "from_tick": self.sched.tick(),
            }));
        }
        match self.sched.capture_status() {
            Some((got, want)) => Ok(json!({
                "armed": true,
                "captured": got,
                "ticks": want,
                "path": self.capture_path.as_deref().map(shown_path),
            })),
            None => Ok(json!({ "armed": false, "captured": 0, "ticks": 0, "path": null })),
        }
    }

    /// Write a finished capture and say so as an event (`prof.capture
    /// {path, ticks, bytes}`), so a client waiting on it has something to
    /// wait for; a write failure is the same event with an `error` field.
    fn finish_capture(&mut self) {
        let Some(cap) = self.sched.capture_take() else {
            return;
        };
        let path = self
            .capture_path
            .take()
            .unwrap_or_else(|| PathBuf::from("argus-capture.json"));
        let mut fields = vec![
            ("path".to_string(), Value::Str(shown_path(&path))),
            ("ticks".to_string(), Value::Int(cap.ticks.len() as i64)),
            (
                "from_tick".to_string(),
                Value::Int(cap.ticks.first().map(|t| t.tick as i64).unwrap_or(0)),
            ),
        ];
        match cap.write(&path) {
            Ok(n) => fields.push(("bytes".to_string(), Value::Int(n as i64))),
            Err(e) => fields.push(("error".to_string(), Value::Str(e.to_string()))),
        }
        self.world.emit(Event::Game {
            name: "prof.capture".to_string(),
            fields,
        });
    }

    fn prof_archetypes(&self) -> Json {
        let rows: Vec<Json> = self
            .world
            .archetype_stats()
            .iter()
            .map(|a| json!([a.components, a.entities, a.bytes]))
            .collect();
        json!({ "columns": ["components", "entities", "bytes"], "rows": rows })
    }
}

// ----- helpers ------------------------------------------------------------

/// A path as a person pastes it: without Windows' `\\?\` long-path prefix,
/// which file pickers (Perfetto's, a shell's) do not take.
fn shown_path(p: &Path) -> String {
    p.to_string_lossy().trim_start_matches(r"\\?\").to_string()
}

/// `rect {x, y, w, h}` from params.
fn rect_param(params: &Json) -> Result<argus_tiles::Rect> {
    let r = params
        .get("rect")
        .ok_or_else(|| bad_params("rect {x, y, w, h} is required"))?;
    Ok(argus_tiles::Rect::new(
        json::u64_param(r, "x", 0)? as u32,
        json::u64_param(r, "y", 0)? as u32,
        json::u64_param(r, "w", 1)? as u32,
        json::u64_param(r, "h", 1)? as u32,
    ))
}

/// Components with their field values, as `edit.spawn` takes them.
type ComponentValues = Vec<(String, Vec<(String, Json)>)>;

fn components_param(params: &Json) -> Result<ComponentValues> {
    let mut out = Vec::new();
    match params.get("components") {
        None | Some(Json::Null) => {}
        Some(Json::Object(o)) => {
            for (name, fields) in o {
                let mut fs = Vec::new();
                match fields {
                    Json::Object(fo) => {
                        for (f, v) in fo {
                            fs.push((f.clone(), v.clone()));
                        }
                    }
                    Json::Null => {}
                    _ => {
                        return Err(bad_params(format!(
                            "components.{name} must be an object of fields"
                        )))
                    }
                }
                out.push((name.clone(), fs));
            }
        }
        Some(_) => {
            return Err(bad_params(
                "components must be an object {Name: {field: value}}",
            ))
        }
    }
    Ok(out)
}

fn split_child(scene: &Scene, id: &str) -> (String, String) {
    let mut end = id.len();
    loop {
        let head = &id[..end];
        if scene.entity_file(head).is_some() {
            return (
                head.to_string(),
                id[end..].trim_start_matches('/').to_string(),
            );
        }
        match id[..end].rfind('/') {
            Some(i) => end = i,
            None => return (id.to_string(), String::new()),
        }
    }
}

fn tick_json(t: &argus_sim::TickRecord) -> Json {
    json!({
        "tick": t.tick,
        "clock": t.clock.to_string(),
        "total_ms": t.total.as_secs_f64() * 1000.0,
        "allocs": t.allocs,
        "bytes": t.bytes,
        "systems": t.systems.iter().map(|s| json!([s.name.as_ref(), s.elapsed.as_secs_f64() * 1000.0, s.allocs, s.bytes])).collect::<Vec<_>>(),
        "archetypes": t.archetypes.iter().map(|(id, n)| json!([id, n])).collect::<Vec<_>>(),
    })
}

/// `test.run`'s `set`: fields to put in each scenario's world after its
/// scene loads, live only (`sim.set`'s shape: `{entity | def, path, value}`).
fn override_list(v: Option<&Json>) -> Result<Vec<Json>> {
    let list = match v {
        None | Some(Json::Null) => return Ok(Vec::new()),
        Some(Json::Array(a)) => a.clone(),
        Some(o @ Json::Object(_)) => vec![o.clone()],
        Some(other) => {
            return Err(bad_params(format!(
                "set is a list of {{entity | def, path, value}}; got {other}"
            )))
        }
    };
    for s in &list {
        let target = s.get("entity").or_else(|| s.get("def"));
        if target.and_then(Json::as_str).is_none()
            || s.get("path").and_then(Json::as_str).is_none()
            || s.get("value").is_none()
        {
            return Err(bad_params(format!(
                "each of set is {{entity: \"@id\" | def: \"prefix:key\", path, value}}; got {s}"
            ))
            .hint("As sim.set takes it: set=[{entity: @colony, path: Colony.hunger_per_day, value: 1.2}]."));
        }
    }
    Ok(list)
}

/// `test.run`'s `sweep`: one target and the values to try it at.
fn sweep_of(v: Option<&Json>) -> Result<Option<(Json, Vec<Json>)>> {
    let Some(s) = v.filter(|s| !s.is_null()) else {
        return Ok(None);
    };
    let values = s
        .get("values")
        .and_then(Json::as_array)
        .filter(|a| !a.is_empty())
        .cloned()
        .ok_or_else(|| {
            bad_params("sweep needs values: the candidates to try")
                .hint("sweep={entity: @colony, path: Colony.hunger_per_day, values: [0.8, 1, 1.2]}")
        })?;
    let mut target = s.clone();
    if let Some(m) = target.as_object_mut() {
        m.remove("values");
    }
    let mut probe = target.clone();
    probe["value"] = values[0].clone();
    override_list(Some(&probe))?;
    Ok(Some((target, values)))
}

/// How early a stream's frame may be drawn: less than the jitter of the
/// server's frame loop, well under a frame.
#[cfg(feature = "render")]
const STREAM_SLACK: std::time::Duration = std::time::Duration::from_millis(4);

/// Refuse a parameter the method does not have (phase 4 item 0): the
/// editor's `sim.speed {value: 10}` set the speed to 1, since `x` was
/// missing and `value` was ignored. `fork` goes with every method.
fn check_params(method: &str, params: &Json) -> Result<()> {
    let (Some(doc), Json::Object(m)) = (docs::find(method), params) else {
        return Ok(());
    };
    for k in m.keys() {
        if k == "fork" || doc.params.iter().any(|p| p.name == k) {
            continue;
        }
        let names: Vec<&str> = doc.params.iter().map(|p| p.name).collect();
        let hint = if names.is_empty() {
            format!("{method} takes no parameters.")
        } else {
            format!(
                "Its parameters: {}. `argus {} -h` has their docs.",
                names.join(", "),
                method.replacen('.', " ", 1)
            )
        };
        return Err(bad_params(format!("{method} has no parameter {k:?}")).hint(hint));
    }
    Ok(())
}

impl argus_test::Driver for Daemon {
    fn call(&mut self, method: &str, params: &Json) -> std::result::Result<Json, Json> {
        Daemon::call(self, 0, method, params).map_err(|e| json::error_json(&e))
    }
    fn root(&self) -> PathBuf {
        self.session
            .as_ref()
            .map(|s| s.root.clone())
            .unwrap_or_default()
    }
}

/// The renderer's targets and their device events.
#[cfg(feature = "render")]
pub(crate) mod render {
    use argus_ecs::Result;
    use argus_input::DeviceInput;
    use argus_player::desktop::{window_input, Pads, WindowInput};
    use argus_render::window::WindowHost;
    use argus_render::Renderer;

    #[derive(Default)]
    pub(crate) struct RenderState {
        /// The window and its renderer, while open.
        pub window: Option<(WindowHost, Renderer)>,
        /// An offscreen renderer kept between `obs.frame` calls of one size.
        pub offscreen: Option<(u32, u32, Renderer)>,
        /// Gamepads, opened with the window (argus-player's, shared with a
        /// release build).
        pub pads: Option<Pads>,
        /// Set by `poll` when the user closed the window.
        pub closed_now: bool,
        /// Text typed since the last poll (a mark's note).
        pub typed: Vec<String>,
        /// The bar the window shows now: its text and width.
        pub bar: Option<(String, u32)>,
        /// A client's frame stream (phase 4 item 1, the editor's viewport).
        pub stream: Option<Stream>,
        /// Streams made so far: each file gets a new name.
        pub streams_made: u64,
    }

    /// Frames for one connection in a file both processes map (§5's
    /// out-of-band channel): its own offscreen renderer, the file, and
    /// when it last drew.
    pub(crate) struct Stream {
        /// The connection it is for.
        pub conn: super::ConnId,
        /// Its renderer, the stream's size.
        pub renderer: Renderer,
        /// The shared file.
        pub writer: argus_client::frames::FrameWriter,
        /// Frames a second at most.
        pub fps: f64,
        /// Whether it opened the audio output.
        pub audio: bool,
        /// Not before this.
        pub next_due: std::time::Instant,
        /// When it last drew, and what it drew: the world's frame, its
        /// mutations, and the device events seen by then.
        pub last: Option<(std::time::Instant, (u64, u64, u64))>,
        /// The bar it shows now.
        pub bar: Option<Option<String>>,
        /// Frames written.
        pub frames: u64,
        /// The last frame's drawing and copying, in ms.
        pub last_ms: f64,
        /// Why the last frame could not be written, if it could not.
        pub error: Option<String>,
    }

    impl Drop for RenderState {
        fn drop(&mut self) {
            self.close_window();
        }
    }

    impl RenderState {
        /// Close the window: the renderer's surface first, then the host,
        /// which destroys the window and keeps its event loop for the next
        /// one (see `WindowHost`).
        pub fn close_window(&mut self) {
            if let Some((host, r)) = self.window.take() {
                drop(r);
                drop(host);
            }
        }

        /// Open the gamepads, if they are not yet.
        pub fn open_pads(&mut self) {
            if self.pads.is_none() {
                self.pads = Some(Pads::open()).filter(Pads::is_open);
            }
        }

        /// Pump the window and the pads; device events in the input
        /// layer's names. Resizes are handled here.
        pub fn poll(&mut self) -> Vec<DeviceInput> {
            let mut out = Vec::new();
            let mut close = false;
            if let Some((host, r)) = &mut self.window {
                if !host.pump() {
                    close = true;
                }
                for ev in host.take_events() {
                    match window_input(ev) {
                        Some(WindowInput::Device(d)) => out.push(d),
                        Some(WindowInput::Text(t)) => self.typed.push(t),
                        Some(WindowInput::Resized(w, h)) => r.gpu_mut().resize(w, h),
                        // The mouse, on the UI canvas: the pointer action.
                        Some(WindowInput::Cursor(x, y)) => {
                            if let Some(f) = r.ui_fit() {
                                let [x, y] = f.to_canvas([x, y]);
                                out.push(DeviceInput::Pointer { x, y });
                            }
                        }
                        // The host reports the close as the pump's answer.
                        Some(WindowInput::Close) | None => {}
                    }
                }
            }
            if close {
                self.close_window();
                self.closed_now = true;
                out.push(DeviceInput::ReleaseAll);
            }
            if let Some(p) = &mut self.pads {
                out.extend(p.poll());
            }
            out
        }
        pub fn offscreen(
            &mut self,
            width: u32,
            height: u32,
            software: bool,
            assets: &argus_assets::Importer,
        ) -> Result<&mut Renderer> {
            let fresh = match &self.offscreen {
                Some((_, _, r)) => r.gpu().software != software,
                None => true,
            };
            if fresh {
                let mut r = Renderer::offscreen(width, height, software)?;
                r.load_catalog(assets)?;
                self.offscreen = Some((width, height, r));
            }
            let o = self.offscreen.as_mut().expect("just set");
            if (o.0, o.1) != (width, height) {
                o.2.gpu_mut().resize(width, height);
                o.2.forget_motion();
                (o.0, o.1) = (width, height);
            }
            Ok(&mut o.2)
        }

        /// Materials the GPU refused, from whichever renderers are open.
        pub fn material_errors(&self) -> Vec<(String, String)> {
            let mut out: Vec<(String, String)> = Vec::new();
            let rs = [
                self.window.as_ref().map(|w| &w.1),
                self.offscreen.as_ref().map(|o| &o.2),
            ];
            for r in rs.into_iter().flatten() {
                for (n, why) in r.material_errors() {
                    if !out.iter().any(|(m, _)| m == n) {
                        out.push((n.clone(), why.clone()));
                    }
                }
            }
            out
        }

        /// Loaded material names, from whichever renderer is open.
        pub fn materials(&self) -> Vec<String> {
            let r = self
                .window
                .as_ref()
                .map(|w| &w.1)
                .or(self.offscreen.as_ref().map(|o| &o.2));
            r.map(|r| r.materials().into_iter().map(|(n, _)| n).collect())
                .unwrap_or_default()
        }

        pub fn open_window(
            &mut self,
            title: &str,
            width: u32,
            height: u32,
            software: bool,
            visible: bool,
            assets: &argus_assets::Importer,
        ) -> Result<()> {
            if self.window.is_some() {
                return Ok(());
            }
            let host = WindowHost::open_with(title, width, height, visible)?;
            let window = host.window().expect("open");
            let mut r = Renderer::windowed(window, software)?;
            r.load_catalog(assets)?;
            self.window = Some((host, r));
            if self.pads.is_none() {
                // Pads already plugged in are reported as connected on the first poll.
                self.pads = Some(Pads::open()).filter(Pads::is_open);
            }
            Ok(())
        }
    }
}

/// One event as `obs.events` reports it.
pub fn event_json(world: &World, seq: u64, rec: &EventRecord) -> Json {
    let cname = |c: argus_ecs::ComponentId| world.registry().schema(c).name.clone();
    let (name, fields) = match &rec.event {
        Event::Game { name, fields } => (
            name.clone(),
            Json::Object(
                fields
                    .iter()
                    .map(|(k, v)| (k.clone(), json::to_json(world, v)))
                    .collect(),
            ),
        ),
        Event::SystemFault {
            system,
            message,
            r#ref,
        } => (
            "system.fault".into(),
            json!({ "system": system, "message": message, "ref": r#ref }),
        ),
        Event::SystemDisabled { system, reason } => (
            "system.disabled".into(),
            json!({ "system": system, "reason": reason }),
        ),
        Event::Spawned { entity, id } => (
            "spawned".into(),
            json!({ "entity": id.as_ref().map(|i| format!("@{i}")).unwrap_or_else(|| entity.to_string()) }),
        ),
        Event::Despawned { entity, id } => (
            "despawned".into(),
            json!({ "entity": id.as_ref().map(|i| format!("@{i}")).unwrap_or_else(|| entity.to_string()) }),
        ),
        Event::ComponentAdded { entity, component } => (
            "component.added".into(),
            json!({ "entity": json::entity_str(world, *entity), "component": cname(*component) }),
        ),
        Event::ComponentRemoved { entity, component } => (
            "component.removed".into(),
            json!({ "entity": json::entity_str(world, *entity), "component": cname(*component) }),
        ),
        Event::DanglingRefNulled {
            entity,
            component,
            field,
            target,
        } => (
            "ref.nulled".into(),
            json!({ "entity": json::entity_str(world, *entity), "component": cname(*component), "field": field, "target": target.to_string() }),
        ),
        Event::HierarchyCycle { entity } => (
            "hierarchy.cycle".into(),
            json!({ "entity": json::entity_str(world, *entity) }),
        ),
        Event::ComponentRegistered { name, .. } => {
            ("component.registered".into(), json!({ "component": name }))
        }
    };
    let mut row = json!({ "seq": seq, "frame": rec.frame, "name": name, "fields": fields });
    if let Some(by) = &rec.by {
        row["by"] = json!(by.as_ref());
    }
    row
}

/// Whether git can be run without asking the person anything. A Mac
/// without Apple's developer tools has a `/usr/bin/git` that only offers to
/// install them, in a dialog, each time it is run; `xcode-select -p` says
/// whether they are there, without one.
pub fn git_usable() -> bool {
    static USABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *USABLE.get_or_init(|| {
        if !cfg!(target_os = "macos") {
            return true;
        }
        let first = std::env::var_os("PATH").and_then(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join("git"))
                .find(|g| g.is_file())
        });
        match first {
            Some(g) if g == Path::new("/usr/bin/git") => {
                std::process::Command::new("/usr/bin/xcode-select")
                    .arg("-p")
                    .output()
                    .is_ok_and(|o| o.status.success())
            }
            Some(_) => true,
            None => false,
        }
    })
}

/// The code a run was made with (§6 run identity): `git:<revision>`, with
/// `+dirty.<hash>` when files under the project differ from it, or `none`
/// outside a repository (or with no git to ask). The hash is of the changed
/// files as they are now (`changes_hash`), so two builds with different
/// uncommitted changes say different things, and a mark from one opened
/// against the other says the code changed (issue 7: both said
/// `git:8940ef6+dirty`). No console window opens for git on Windows.
pub fn code_version(root: &Path) -> String {
    if !git_usable() {
        return "none".to_string();
    }
    let git = |args: &[&str]| -> Option<String> {
        let mut c = std::process::Command::new("git");
        c.arg("-C").arg(root).args(args);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000);
        }
        let out = c.output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    match git(&["rev-parse", "--short", "HEAD"]) {
        Some(rev) if !rev.is_empty() => {
            let dirty = git(&["status", "--porcelain", "--", "."]).is_some_and(|s| !s.is_empty());
            if !dirty {
                return format!("git:{rev}");
            }
            // Paths from the project's folder: changed since the commit
            // (staged or not, deleted too), and new and not ignored.
            let mut changed: Vec<String> = [
                git(&["diff", "--name-only", "--relative", "-z", "HEAD", "--", "."]),
                git(&[
                    "ls-files",
                    "-z",
                    "--others",
                    "--exclude-standard",
                    "--",
                    ".",
                ]),
            ]
            .into_iter()
            .flatten()
            .flat_map(|s| {
                s.split('\0')
                    .filter(|p| !p.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .collect();
            changed.sort();
            changed.dedup();
            format!("git:{rev}+dirty.{}", changes_hash(root, &changed))
        }
        _ => "none".to_string(),
    }
}

/// Eight hex digits naming what the changed files hold now, by their paths
/// from the project's folder. Left out are the files no run reads, so the
/// hash moves when the game does: anything under a dot-named folder
/// (`.argus/`, where marks and saves go), the builds (`dist/`) and dev
/// tools (`tools/`), the notes for people and agents at the top (README.md,
/// ARGUS.md, ...), and what `[build] exclude` names. A deleted file counts
/// as deleted.
pub fn changes_hash(root: &Path, changed: &[String]) -> String {
    let patterns = argus_scene::ProjectConfig::load(root)
        .ok()
        .and_then(|c| argus_player::pack::exclude_patterns(&c).ok())
        .unwrap_or_default();
    let read = |rel: &str| -> bool {
        let parts: Vec<&str> = rel.split('/').collect();
        if parts.iter().any(|p| p.starts_with('.')) {
            return false;
        }
        if parts.len() > 1 && matches!(parts[0], "dist" | "tools") {
            return false;
        }
        if parts.len() == 1 && argus_player::pack::LEFT_OUT_FILES.contains(&parts[0]) {
            return false;
        }
        !argus_player::pack::excluded_by_patterns(rel, &patterns)
    };
    let mut all = Vec::new();
    for rel in changed.iter().filter(|r| read(r)) {
        all.extend_from_slice(rel.as_bytes());
        all.push(0);
        match std::fs::read(root.join(rel)) {
            Ok(bytes) => all.extend_from_slice(&argus_player::sha256::sha256(&bytes)),
            Err(_) => all.extend_from_slice(b"deleted"),
        }
        all.push(0);
    }
    let h = argus_player::sha256::sha256(&all);
    h[..4].iter().map(|b| format!("{b:02x}")).collect()
}

/// Where a project's daemon writes its connection info.
pub fn daemon_info_path(root: &Path) -> PathBuf {
    argus_client::socket::daemon_info_path(root)
}

/// Sim seconds for shaders, at the moment the picture shows: a frame drawn
/// at `alpha` shows the tick before blended towards this one, so it is this
/// tick's time less the part of a tick not yet blended in. At alpha 1 (a
/// picture of now) it is the tick's time.
#[cfg(feature = "render")]
fn shader_time(sched: &argus_sim::Scheduler, alpha: f32) -> f32 {
    let hz = sched.driving().hz();
    let behind = if hz > 0.0 {
        (1.0 - alpha as f64) / hz
    } else {
        0.0
    };
    (sched.time() - behind).max(0.0) as f32
}

/// `see_as`: a kind of colour vision a frame is drawn as (phase 5 item 2),
/// or none.
#[cfg(feature = "render")]
fn see_as_param(params: &Json) -> Result<Option<argus_render::SeeAs>> {
    match json::opt_str(params, "see_as")? {
        None | Some("") | Some("normal") => Ok(None),
        Some(s) => argus_render::SeeAs::parse(s).map(Some).ok_or_else(|| {
            bad_params(format!(
                "see_as is protanopia, deuteranopia, tritanopia, achromatopsia, or normal; got {s:?}"
            ))
        }),
    }
}
