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
use argus_luau::{ScriptConfig, ScriptHost};
use argus_scene::{
    Bulk, DefSchema, EntityDoc, Project, ProjectConfig, Rect, Scene, SceneDoc, SeedPolicy,
};
use argus_sim::{Clock, ClockKind, Scheduler, SchedulerConfig};
use serde_json::{json, Map, Value as Json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

/// Identifies a client connection for subscriptions.
pub type ConnId = u64;

/// Protocol version string.
pub const VERSION: &str = "0.0.1";

struct Session {
    root: PathBuf,
    project: Project,
    scene: Scene,
    scene_name: String,
    host: ScriptHost,
    seed: u64,
    /// The world was instantiated (no issues).
    loaded: bool,
    /// `.argus/index.db`, when it could be opened.
    index: Option<Index>,
    index_error: Option<String>,
}

struct EventLog {
    next_seq: u64,
    records: VecDeque<(u64, EventRecord)>,
    cap: usize,
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
    /// Notifications waiting for the transport, per connection.
    pub outbox: Vec<(ConnId, Json)>,
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
            events: EventLog {
                next_seq: 1,
                records: VecDeque::new(),
                cap: 200_000,
            },
            subs: BTreeMap::new(),
            outbox: Vec::new(),
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

    /// Dispatch a method. This is the whole protocol.
    pub fn call(&mut self, conn: ConnId, method: &str, params: &Json) -> Result<Json> {
        let r = match method {
            "session.open" => self.session_open(params),
            "session.status" => Ok(self.session_status()),
            "session.capabilities" => Ok(self.capabilities()),
            "session.docs" => Ok(json!({ "text": docs::text(json::opt_str(params, "method")?) })),
            "session.close" => {
                self.close();
                Ok(json!({ "closed": true }))
            }
            "world.summary" => match self.index_target(params)? {
                Some(s) => self.index()?.summary(&s),
                None => self.world_summary(),
            },
            "world.query" => match self.index_target(params)? {
                Some(s) => self.index()?.query(&s, params),
                None => {
                    self.require_session()?;
                    query::run(&self.world, params)
                }
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
            "asset.move" => self.asset_move(params),
            "scene.list" => self.scene_list(),
            "scene.load" => self.scene_load(params),
            "scene.issues" => self.scene_issues(),
            "scene.rooms" => match self.index_target(params)? {
                Some(s) => self.index()?.rooms(&s),
                None => self.scene_rooms(),
            },
            "scene.transition" => self.scene_transition(params),
            "scene.tiles.get" => self.scene_tiles_get(params),
            "edit.begin" => self.edit_begin(params),
            "edit.set" | "edit.add" | "edit.remove" | "edit.spawn" | "edit.despawn" => {
                self.edit_queue(method, params)
            }
            "edit.commit" => self.edit_commit(params),
            "edit.abort" => self.edit_abort(params),
            "edit.undo" => self.edit_undo(params, false),
            "edit.redo" => self.edit_undo(params, true),
            "edit.feed" => self.edit_feed(params),
            "edit.lock" => self.edit_lock(params),
            "edit.unlock" => self.edit_unlock(params),
            "edit.locks" => Ok(json!({ "rows": self.edits.locks.iter().map(|l| l.to_json()).collect::<Vec<_>>() })),
            "sim.play" => {
                self.require_session()?;
                self.sched.play(&self.world);
                Ok(self.sim_status())
            }
            "sim.pause" => {
                self.sched.pause();
                Ok(self.sim_status())
            }
            "sim.stop" => {
                self.sched.stop(&mut self.world);
                self.pump_events();
                Ok(self.sim_status())
            }
            "sim.step" => {
                let n = json::u64_param(params, "n", 1)?;
                self.sched.step(&mut self.world, n)?;
                self.pump_events();
                Ok(self.sim_status())
            }
            "sim.advance" => {
                let n = json::u64_param(params, "n", 1)?;
                if let Some(by) = json::opt_str(params, "by")? {
                    if by != "turn" {
                        return Err(bad_params(format!("by must be \"turn\"; got {by:?}")));
                    }
                }
                self.sched.advance(&mut self.world, n)?;
                self.pump_events();
                Ok(self.sim_status())
            }
            "sim.speed" => {
                let x = json::f64_param(params, "x", 1.0)?;
                self.sched.set_speed(x)?;
                Ok(self.sim_status())
            }
            "sim.run_until" => self.sim_run_until(params),
            "sim.status" => Ok(self.sim_status()),
            "script.reload" => self.script_reload(),
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
            other => Err(ArgusError::new(
                codes::UNKNOWN_METHOD,
                format!("no method {other:?}"),
            )
            .hint("session.capabilities lists every method; obs.frame, sim.seek, and input.* arrive in later phases.")),
        };
        self.pump_events();
        r
    }

    /// One real-time frame while playing on the continuous clock. The
    /// server calls this at the clock rate; `present` runs every frame.
    pub fn frame(&mut self) -> Result<()> {
        if self.session.is_some() {
            self.sched.frame(&mut self.world)?;
            self.pump_events();
        }
        Ok(())
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

    /// Forget a connection's subscriptions.
    pub fn drop_conn(&mut self, conn: ConnId) {
        self.subs.remove(&conn);
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
        self.open_scene(root, &cfg, &scene_name, seed)
    }

    fn open_scene(
        &mut self,
        root: PathBuf,
        cfg: &ProjectConfig,
        scene_name: &str,
        seed: Option<u64>,
    ) -> Result<Json> {
        let scene_doc = SceneDoc::load(&root, scene_name)?;
        let clock = Clock::parse(scene_doc.clock.as_deref().unwrap_or(&cfg.clock))?;
        let seed = seed.unwrap_or(match scene_doc.seed {
            SeedPolicy::Fixed(s) => s,
            SeedPolicy::Random => std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(1),
        });
        let sched_cfg = match clock.kind {
            ClockKind::Continuous => SchedulerConfig::continuous(1.0 / clock.timestep),
            ClockKind::Discrete => SchedulerConfig::discrete(),
        }
        .seed(seed);

        // Fresh world and scheduler; scripts declare components and systems.
        self.close();
        self.world = World::new();
        argus_ecs::hierarchy::register(&mut self.world)?;
        self.sched = Scheduler::new(sched_cfg);
        let mut host = ScriptHost::new(ScriptConfig::default())?;
        host.load_project(&mut self.world, &mut self.sched, &root)?;
        let def_schemas: Vec<DefSchema> = host
            .defs()
            .iter()
            .map(|d| {
                DefSchema::new(d.schema.clone())
                    .key(d.key.clone())
                    .prefix(d.prefix.clone())
            })
            .collect();
        let project = Project::open(&root, def_schemas)?;
        let mut scene = Scene::open(&project, scene_name, self.world.registry())?;
        let mut issues: Vec<ArgusError> = project.issues().to_vec();
        issues.extend(scene.issues().iter().cloned());
        let mut load = None;
        if issues.is_empty() {
            let report = if scene.has_rooms() {
                scene.spawn_around(&mut self.world, None)?
            } else {
                scene.spawn_all(&mut self.world)?
            };
            load = Some(report);
        }
        apply_system_groups(
            &mut self.sched,
            &scene_doc.enable,
            &scene_doc.disable,
            self.world.registry(),
        )?;
        self.sched.validate()?;
        let (index, index_error) = match Index::open(&root) {
            Ok(i) => (Some(i), None),
            Err(e) => (None, Some(e.message)),
        };
        self.session = Some(Session {
            root,
            project,
            scene,
            scene_name: scene_name.to_string(),
            host,
            seed,
            loaded: issues.is_empty(),
            index,
            index_error,
        });
        self.pump_events();
        self.sync_index(false);
        let mut out = self.world_summary()?;
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
        out["script_errors"] = json!(self
            .session
            .as_ref()
            .map(|s| s.host.errors().len())
            .unwrap_or(0));
        Ok(out)
    }

    fn session_status(&self) -> Json {
        match &self.session {
            None => json!({ "project": null, "scene": null, "playing": false }),
            Some(s) => {
                let mut st = self.sim_status();
                st["project"] = json!(s.project.config.name);
                st["root"] = json!(s.root.to_string_lossy().trim_start_matches(r"\\?\"));
                st["scene"] = json!(s.scene_name);
                st["script_errors"] = json!(s.host.errors().len());
                st["loaded"] = json!(s.loaded);
                st["index"] = json!(s
                    .index
                    .as_ref()
                    .map(|_| Index::path(&s.root).to_string_lossy().into_owned()));
                st["index_error"] = json!(s.index_error);
                st
            }
        }
    }

    fn capabilities(&self) -> Json {
        json!({
            "version": VERSION,
            "phase": 0,
            "transports": ["stdio", "local-socket"],
            "methods": docs::METHODS.iter().map(|m| json!({
                "name": m.name, "doc": m.doc, "params": m.params, "returns": m.returns
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
        match idx.sync_project(&ses.project, &registry, &systems, Some(&ses.scene), force) {
            Ok(r) => Some(r),
            Err(e) => {
                ses.index_error = Some(e.message);
                None
            }
        }
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
            let (fid, _) = split_child(&ses.scene, id);
            if ses.scene.resolved(&fid).is_some() {
                let _ = idx.sync_entities(&ses.scene, &[&fid]);
            } else {
                let _ = idx.remove_entity(&ses.scene.name, &fid, None);
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
        ses.scene.reload_files(&ses.project.ctx(&registry));
        let entity_move = report.record.kind == argus_scene::MoveKind::Entity;
        self.sync_index(true);
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
            if resolved {
                if let Some(re) = ses.scene.resolved(id) {
                    out["resolved"] = resolved_json(re);
                }
            } else if let Some(doc) = ses.scene.overrides(id) {
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
        let cfg = ProjectConfig::load(&root)?;
        self.open_scene(root, &cfg, &name, seed)
    }

    fn scene_issues(&self) -> Result<Json> {
        let ses = self.require_session()?;
        let rows: Vec<Json> = ses
            .project
            .issues()
            .iter()
            .chain(ses.scene.issues())
            .map(json::error_json)
            .collect();
        Ok(json!({ "total": rows.len(), "rows": rows }))
    }

    fn scene_rooms(&self) -> Result<Json> {
        let ses = self.require_session()?;
        let s = &ses.scene;
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
                    "entities": r.entities.len(),
                    "neighbors": neighbors,
                    "bounds": r.doc.bounds,
                    "tileset": r.doc.tileset,
                    "bulk": r.bulk,
                })
            })
            .collect();
        Ok(
            json!({ "rooms": rows, "current": s.current_room(), "radius": graph.radius, "start": graph.start }),
        )
    }

    fn scene_transition(&mut self, params: &Json) -> Result<Json> {
        let room = json::str_param(params, "room")?.to_string();
        let ses = self
            .session
            .as_mut()
            .ok_or_else(|| ArgusError::new(codes::NO_PROJECT, "no project is open"))?;
        let before: BTreeSet<String> = ses
            .scene
            .loaded_rooms()
            .into_iter()
            .map(str::to_string)
            .collect();
        ses.scene.transition(&mut self.world, &room)?;
        let after: BTreeSet<String> = ses
            .scene
            .loaded_rooms()
            .into_iter()
            .map(str::to_string)
            .collect();
        Ok(json!({
            "loaded": after.difference(&before).collect::<Vec<_>>(),
            "unloaded": before.difference(&after).collect::<Vec<_>>(),
            "current": ses.scene.current_room(),
        }))
    }

    fn scene_tiles_get(&self, params: &Json) -> Result<Json> {
        let ses = self.require_session()?;
        let file = json::str_param(params, "file")?;
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

    // ----- edit -------------------------------------------------------------

    fn edit_begin(&mut self, params: &Json) -> Result<Json> {
        self.require_session()?;
        let author = json::str_param(params, "author")?;
        let note = json::opt_str(params, "note")?.unwrap_or("");
        Ok(json!({ "tx": self.edits.begin(author, note) }))
    }

    fn edit_queue(&mut self, method: &str, params: &Json) -> Result<Json> {
        let tx = json::u64_param(params, "tx", 0)? as u32;
        let op = match method {
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
            _ => Op::Despawn {
                entity: json::str_param(params, "entity")?.to_string(),
            },
        };
        let t = self.edits.tx_mut(tx)?;
        t.ops.push(op);
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
        self.apply_commit(&t.author, &t.note, t.ops, CommitKind::Normal)
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
        self.require_session()?;
        let mut inverse: Vec<Op> = Vec::new();
        let mut summaries = Vec::new();
        let mut touched: BTreeSet<String> = BTreeSet::new();
        let mut files: BTreeSet<String> = BTreeSet::new();
        for op in ops {
            summaries.push(op.summary());
            match self.apply_op(author, &op, &mut touched, &mut files) {
                Ok(Some(inv)) => inverse.push(inv),
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
        }
        self.edits.feed.push(commit);
        self.notify("edits", "edit.committed", row.clone());
        self.pump_events();
        Ok(row)
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
                    if ses.scene.is_authored(&id) {
                        ses.scene.set_override(&ctx, &id, path, &file_value)?;
                        if let Some(f) = ses.scene.entity_file(&id).or_else(|| {
                            let (fid, _) = split_child(&ses.scene, &id);
                            ses.scene.entity_file(&fid)
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
                    Some(local) => {
                        self.edits.check_lock(author, &format!("@{local}"), "")?;
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
                if let Some(id) = self.world.string_id(e).map(str::to_string) {
                    let ses = self.session.as_mut().expect("session");
                    let f = ses.scene.entity_file(&id).ok_or_else(|| {
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
                    ses.scene.delete_entity(&mut self.world, &id)?;
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
        let full = ses.scene.create_entity(&ctx, local, room, doc)?;
        if let Some(issue) = ses.scene.issues().first() {
            let issue = issue.clone();
            let _ = ses.scene.delete_entity(&mut self.world, &full);
            return Err(issue);
        }
        if let Some(f) = ses.scene.entity_file(&full) {
            files.insert(f.rel.clone());
        }
        ses.scene.spawn_entity(&mut self.world, &full)
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
        let ses = self.session.as_mut().expect("session");
        if !ses.scene.is_authored(&id) {
            return Ok(());
        }
        let registry = self.world.registry().clone();
        let ctx = ses.project.ctx(&registry);
        f(&mut ses.scene, &ctx, &id)?;
        let (fid, _) = split_child(&ses.scene, &id);
        if let Some(file) = ses.scene.entity_file(&fid) {
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
        })
    }

    fn sim_run_until(&mut self, params: &Json) -> Result<Json> {
        self.require_session()?;
        let max = json::u64_param(params, "max_ticks", 100_000)?;
        let r = if let Some(ev) = json::opt_str(params, "event")? {
            self.sched.run_until_event(&mut self.world, ev, max)?
        } else if let Some(Json::Number(t)) = params.get("time") {
            self.sched
                .run_until_time(&mut self.world, t.as_f64().unwrap_or(0.0), max)?
        } else if let Some(Json::Number(n)) = params.get("ticks") {
            let n = n.as_u64().unwrap_or(0).min(max);
            self.sched.step(&mut self.world, n)?;
            argus_sim::RunUntil {
                ticks: n,
                hit: true,
            }
        } else {
            return Err(bad_params("give event, time, or ticks"));
        };
        self.pump_events();
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
        ses.project.set_def_schemas(schemas);
        let registry = self.world.registry().clone();
        ses.scene.reresolve(&ses.project.ctx(&registry));
        self.sched.validate()?;
        self.sync_index(true);
        Ok(json!({
            "reloaded": report.reloaded,
            "removed": report.removed,
            "errors": report.errors,
        }))
    }

    fn script_systems(&mut self, params: &Json) -> Result<Json> {
        let rows: Vec<Json> = self
            .sched
            .systems(self.world.registry())?
            .iter()
            .map(|s| {
                json!({
                    "name": s.name,
                    "stage": s.stage.name(),
                    "after": s.after,
                    "reads": s.reads,
                    "writes": s.writes,
                    "clock": format!("{:?}", s.clock).to_lowercase(),
                    "rng": s.rng,
                    "enabled": s.enabled,
                    "disabled_reason": s.disabled_reason,
                    "source": s.source,
                    "doc": s.doc,
                })
            })
            .collect();
        let (limit, offset) = query::page(params)?;
        Ok(query::paged(rows, limit, offset))
    }

    fn script_errors(&self) -> Result<Json> {
        let ses = self.require_session()?;
        let rows: Vec<Json> = ses
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
        Ok(json!({ "total": rows.len(), "rows": rows }))
    }

    // ----- obs --------------------------------------------------------------

    /// Move the world's pending events into the log and notify subscribers.
    fn pump_events(&mut self) {
        if self.world.events().is_empty() {
            return;
        }
        let drained = self.world.drain_events();
        for rec in drained {
            let seq = self.events.next_seq;
            self.events.next_seq += 1;
            let row = event_json(&self.world, seq, &rec);
            self.events.records.push_back((seq, rec));
            if self.events.records.len() > self.events.cap {
                self.events.records.pop_front();
            }
            self.notify("events", "event", row);
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
        let entity = json::opt_str(params, "entity")?
            .map(|r| self.world.entity(r))
            .transpose()?;
        let rows: Vec<Json> = self
            .events
            .records
            .iter()
            .filter(|(seq, r)| *seq > since && r.frame >= from_frame)
            .map(|(seq, r)| event_json(&self.world, *seq, r))
            .filter(|row| {
                name.is_none_or(|n| row["name"].as_str() == Some(n))
                    && entity.is_none_or(|e| {
                        let s = json::entity_str(&self.world, e);
                        let f = &row["fields"];
                        f.as_object()
                            .is_some_and(|o| o.values().any(|v| v.as_str() == Some(&s)))
                    })
            })
            .collect();
        let (limit, offset) = query::page(params)?;
        let mut out = query::paged(rows, limit, offset);
        out["last_seq"] = json!(self.events.next_seq - 1);
        Ok(out)
    }

    fn obs_subscribe(&mut self, conn: ConnId, params: &Json) -> Result<Json> {
        let topics = json::str_list(params, "topics")?;
        for t in &topics {
            if t != "events" && t != "edits" {
                return Err(bad_params(format!(
                    "unknown topic {t:?}; topics are events and edits"
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

    // ----- prof -------------------------------------------------------------

    fn prof_frame(&self) -> Json {
        match self.sched.prof_last() {
            Some(t) => tick_json(t),
            None => json!({ "tick": null }),
        }
    }

    fn prof_systems(&mut self, params: &Json) -> Result<Json> {
        let last = json::u64_param(params, "last", 600)? as usize;
        let rows: Vec<Json> = self
            .sched
            .prof_systems(last)?
            .iter()
            .map(|p| {
                json!([
                    p.name,
                    p.stage.name(),
                    p.mean_ms,
                    p.p95_ms,
                    p.max_ms,
                    p.samples
                ])
            })
            .collect();
        Ok(json!({
            "last": last,
            "tick_p95_ms": self.sched.prof_tick_p95_ms(last),
            "columns": ["name", "stage", "mean_ms", "p95_ms", "max_ms", "samples"],
            "rows": rows,
        }))
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

fn apply_system_groups(
    sched: &mut Scheduler,
    enable: &[String],
    disable: &[String],
    registry: &argus_ecs::Registry,
) -> Result<()> {
    let names: Vec<String> = sched
        .systems(registry)?
        .into_iter()
        .map(|s| s.name)
        .collect();
    let matches = |pat: &str, name: &str| {
        pat == "*" || pat == name || pat.strip_suffix('*').is_some_and(|p| name.starts_with(p))
    };
    for n in &names {
        if !enable.is_empty() && !enable.iter().any(|p| matches(p, n)) {
            sched.disable(n, "not in scene.toml systems.enable")?;
        }
        if disable.iter().any(|p| matches(p, n)) {
            sched.disable(n, "listed in scene.toml systems.disable")?;
        }
    }
    Ok(())
}

fn tick_json(t: &argus_sim::TickRecord) -> Json {
    json!({
        "tick": t.tick,
        "clock": t.clock.to_string(),
        "total_ms": t.total.as_secs_f64() * 1000.0,
        "systems": t.systems.iter().map(|(n, d)| json!([n.as_ref(), d.as_secs_f64() * 1000.0])).collect::<Vec<_>>(),
        "archetypes": t.archetypes.iter().map(|(id, n)| json!([id, n])).collect::<Vec<_>>(),
    })
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
    json!({ "seq": seq, "frame": rec.frame, "name": name, "fields": fields })
}

/// Where a project's daemon writes its connection info.
pub fn daemon_info_path(root: &Path) -> PathBuf {
    root.join(".argus").join("daemon.json")
}
