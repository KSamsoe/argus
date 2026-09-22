//! Scenes and rooms (§4). A scene is a directory: `scene.toml` plus one
//! authored entity per file, or `rooms/<room>/room.toml` with the entities
//! inside each room. The scene file holds the room graph; rooms stream in
//! and out by graph distance; transitions are engine events.
//!
//! ```toml
//! # scenes/hollow/scene.toml
//! clock  = "continuous@60"          # which clock drives sim (§6); optional, defaults to argus.toml
//! seed   = 7                        # or "random"
//! bounds = [0.0, 0.0, 640.0, 360.0]
//!
//! [systems]
//! enable  = ["sim.*", "late.*"]     # system groups the daemon enables
//! disable = []
//!
//! [rooms]
//! start  = "cavern-03"
//! radius = 1                        # rooms within this graph distance stay loaded
//!
//! [rooms.graph]
//! "cavern-03" = ["cavern-04"]
//! ```
//!
//! Opening a scene parses and resolves every entity and records every
//! problem as an issue; instantiating into a world requires a clean scene,
//! so a run is always a reproducible value (§6).

use crate::codes;
use crate::doc::{self, EntityDoc};
use crate::moves;
use crate::project::Project;
use crate::resolve::{resolve, Provenance, ResolveCtx, ResolvedEntity};
use crate::tomlv;
use argus_ecs::{ArgusError, Entity, Event, FieldType, Result, SchemaBuilder, Value, World};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// How a run picks its seed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeedPolicy {
    /// Always this seed.
    Fixed(u64),
    /// The runner picks one and records it in the run identity.
    Random,
}

/// The room graph from `scene.toml`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoomGraph {
    /// Room the player starts in.
    pub start: Option<String>,
    /// Graph distance kept loaded around the current room.
    pub radius: u32,
    /// Undirected edges.
    pub edges: Vec<(String, String)>,
}

/// `scene.toml`.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneDoc {
    /// Clock spec overriding the project default, if given.
    pub clock: Option<String>,
    /// Seed policy.
    pub seed: SeedPolicy,
    /// World bounds `[x0, y0, x1, y1]`.
    pub bounds: Option<[f32; 4]>,
    /// System-group patterns to enable (`sim.*`).
    pub enable: Vec<String>,
    /// System-group patterns to disable.
    pub disable: Vec<String>,
    /// Room graph, for subdivided scenes.
    pub rooms: Option<RoomGraph>,
    /// Project-relative file.
    pub file: String,
}

/// `room.toml`.
#[derive(Clone, Debug, PartialEq)]
pub struct RoomDoc {
    /// Room bounds.
    pub bounds: Option<[f32; 4]>,
    /// Tileset path reference.
    pub tileset: Option<String>,
    /// Neighbours as written in the room file (must agree with the graph).
    pub neighbors: Vec<String>,
    /// Project-relative file.
    pub file: String,
}

/// One room directory.
#[derive(Clone, Debug, PartialEq)]
pub struct Room {
    /// Room name (directory name).
    pub name: String,
    /// Directory.
    pub dir: PathBuf,
    /// `room.toml`.
    pub doc: RoomDoc,
    /// Full ids of the entity files in it, sorted.
    pub entities: Vec<String>,
    /// Data index files beside the entities, project-relative: bulk
    /// indexes (`*.map.toml`) and tilemaps (`*.tilemap.toml`).
    pub bulk: Vec<String>,
}

/// One authored entity file.
#[derive(Clone, Debug)]
pub struct EntityFile {
    /// Full id (`cavern-03/door.east` in a room scene, `door.east` in a flat one).
    pub id: String,
    /// File stem.
    pub local: String,
    /// Room, if any.
    pub room: Option<String>,
    /// Absolute path.
    pub path: PathBuf,
    /// Project-relative path.
    pub rel: String,
    /// Parsed content.
    pub doc: EntityDoc,
}

/// What a load did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LoadReport {
    /// Entities spawned (children included).
    pub entities: usize,
    /// Rooms loaded, in order.
    pub rooms: Vec<String>,
    /// Tag components registered on the fly.
    pub tags_registered: Vec<String>,
    /// Entity references left null because their target's room is not
    /// loaded: `(entity, Component.field, target id)`. They are filled when
    /// the room streams in.
    pub pending: Vec<(String, String, String)>,
    /// Wall-clock milliseconds the load took (loader cost, not sim time).
    pub ms: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingRef {
    from: String,
    path: String,
    target: String,
}

/// An open scene.
#[derive(Debug)]
pub struct Scene {
    /// Scene name.
    pub name: String,
    /// Scene directory.
    pub dir: PathBuf,
    /// `scene.toml`.
    pub doc: SceneDoc,
    rooms: BTreeMap<String, Room>,
    entities: BTreeMap<String, EntityFile>,
    resolved: BTreeMap<String, ResolvedEntity>,
    issues: Vec<ArgusError>,
    loaded: BTreeSet<String>,
    current: Option<String>,
    spawned: BTreeMap<String, Entity>,
    pending: Vec<PendingRef>,
    /// Every entity id in the scene, children included (kept by `check_entity_refs`).
    all_ids: BTreeSet<String>,
}

const FLAT: &str = "";

/// One entity file to read: room, file stem, absolute path, project-relative path.
type Listed = (Option<String>, String, PathBuf, String);

impl Scene {
    // ----- opening ----------------------------------------------------------

    /// Open `scenes/<name>` and resolve every entity against the registry.
    /// Fails only if `scene.toml` is missing or unreadable; every other
    /// problem is an issue.
    pub fn open(project: &Project, name: &str, registry: &argus_ecs::Registry) -> Result<Scene> {
        let dir = project.scene_dir(name);
        let scene_rel = project.rel(&dir.join("scene.toml"));
        let text = tomlv::read(&dir.join("scene.toml")).map_err(|e| {
            e.hint(format!(
                "Scenes known to the project: {}",
                project.scenes().join(", ")
            ))
        })?;
        let doc = parse_scene_doc(&scene_rel, &text)?;
        let mut scene = Scene {
            name: name.to_string(),
            dir: dir.clone(),
            doc,
            rooms: BTreeMap::new(),
            entities: BTreeMap::new(),
            resolved: BTreeMap::new(),
            issues: Vec::new(),
            loaded: BTreeSet::new(),
            current: None,
            spawned: BTreeMap::new(),
            pending: Vec::new(),
            all_ids: BTreeSet::new(),
        };

        let mut all_todo = Vec::new();
        let rooms_dir = dir.join("rooms");
        if rooms_dir.is_dir() {
            let mut names: Vec<String> = std::fs::read_dir(&rooms_dir)
                .map(|rd| {
                    rd.flatten()
                        .filter(|e| e.path().is_dir())
                        .filter_map(|e| e.file_name().to_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            names.sort();
            for rn in names {
                let rdir = rooms_dir.join(&rn);
                let rrel = project.rel(&rdir.join("room.toml"));
                let rdoc = match tomlv::read(&rdir.join("room.toml")) {
                    Ok(t) => match parse_room_doc(&rrel, &t) {
                        Ok(d) => d,
                        Err(e) => {
                            scene.issues.push(e);
                            continue;
                        }
                    },
                    Err(e) => {
                        scene
                            .issues
                            .push(e.hint("Every room directory needs a room.toml."));
                        continue;
                    }
                };
                let (todo, bulk) = scene.scan_entities(project, &rdir, Some(&rn));
                all_todo.extend(todo);
                scene.rooms.insert(
                    rn.clone(),
                    Room {
                        name: rn,
                        dir: rdir,
                        doc: rdoc,
                        entities: Vec::new(),
                        bulk,
                    },
                );
            }
            if scene.doc.rooms.is_none() {
                scene.doc.rooms = Some(RoomGraph::default());
            }
            scene.check_graph();
        } else {
            if scene.doc.rooms.is_some() {
                scene.issues.push(
                    ArgusError::new(
                        codes::BAD_SCENE,
                        format!("{scene_rel} declares [rooms] but there is no rooms/ directory"),
                    )
                    .hint("Room scenes keep entities in rooms/<name>/; flat scenes keep them next to scene.toml.")
                    .at(Some(format!("{scene_rel}:1"))),
                );
                scene.doc.rooms = None;
            }
            let (todo, bulk) = scene.scan_entities(project, &dir, None);
            all_todo.extend(todo);
            scene.rooms.insert(
                FLAT.to_string(),
                Room {
                    name: FLAT.to_string(),
                    dir: dir.clone(),
                    doc: RoomDoc {
                        bounds: scene.doc.bounds,
                        tileset: None,
                        neighbors: Vec::new(),
                        file: scene_rel.clone(),
                    },
                    entities: Vec::new(),
                    bulk,
                },
            );
        }
        scene.ingest(all_todo);

        let ctx = project.ctx(registry);
        scene.resolve_all(&ctx);
        Ok(scene)
    }

    /// List a directory's entity files (`stem`, path, project-relative
    /// path) and its bulk index files. Parsing happens in [`Scene::ingest`].
    fn scan_entities(
        &self,
        project: &Project,
        dir: &Path,
        room: Option<&str>,
    ) -> (Vec<Listed>, Vec<String>) {
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                    .map(|e| e.path())
                    .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("toml"))
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        let mut todo = Vec::new();
        let mut bulk = Vec::new();
        for path in files {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            let rel = project.rel(&path);
            if stem == "scene" || stem == "room" {
                continue;
            }
            if crate::bulk::is_data_index(&rel) {
                bulk.push(rel);
                continue;
            }
            todo.push((room.map(str::to_string), stem, path, rel));
        }
        (todo, bulk)
    }

    /// Read and parse every listed entity file on every core (a room scene
    /// is thousands of small files and the file system is the slow part,
    /// §17), then file each one under its room. Order is by room, then id.
    fn ingest(&mut self, todo: Vec<Listed>) {
        let parsed = parallel_map(&todo, |(_, _, path, rel)| {
            let text = tomlv::read(path)?;
            EntityDoc::parse(rel, &text)
        });
        for ((room, stem, path, rel), doc) in todo.into_iter().zip(parsed) {
            let doc = match doc {
                Ok(d) => d,
                Err(e) => {
                    self.issues.push(e);
                    continue;
                }
            };
            let id = match &room {
                Some(r) => format!("{r}/{stem}"),
                None => stem.clone(),
            };
            if let Some(rm) = self.rooms.get_mut(room.as_deref().unwrap_or(FLAT)) {
                rm.entities.push(id.clone());
            }
            self.entities.insert(
                id.clone(),
                EntityFile {
                    id,
                    local: stem,
                    room,
                    path,
                    rel,
                    doc,
                },
            );
        }
    }

    fn check_graph(&mut self) {
        let Some(g) = self.doc.rooms.clone() else {
            return;
        };
        let file = self.doc.file.clone();
        for (a, b) in &g.edges {
            for r in [a, b] {
                if !self.rooms.contains_key(r) {
                    self.issues.push(
                        ArgusError::new(
                            codes::UNKNOWN_ROOM,
                            format!("room graph names {r:?}, which has no rooms/{r}/ directory"),
                        )
                        .hint(format!(
                            "Rooms found: {}",
                            self.rooms.keys().cloned().collect::<Vec<_>>().join(", ")
                        ))
                        .at(Some(format!("{file}:1"))),
                    );
                }
            }
        }
        if let Some(s) = &g.start {
            if !self.rooms.contains_key(s) {
                self.issues.push(
                    ArgusError::new(
                        codes::UNKNOWN_ROOM,
                        format!("rooms.start = {s:?} is not a room"),
                    )
                    .at(Some(format!("{file}:1"))),
                );
            }
        }
        // room.toml neighbours must agree with the scene's graph.
        let mut extra = Vec::new();
        for room in self.rooms.values() {
            for n in &room.doc.neighbors {
                let in_graph = g
                    .edges
                    .iter()
                    .any(|(a, b)| (a == &room.name && b == n) || (b == &room.name && a == n));
                if !in_graph {
                    extra.push(
                        ArgusError::new(
                            codes::BAD_SCENE,
                            format!(
                                "{} lists neighbor {n:?} but {file} [rooms.graph] has no such edge",
                                room.doc.file
                            ),
                        )
                        .hint("The scene file holds the room graph; room.toml neighbors are a mirror and must match.")
                        .at(Some(format!("{}:1", room.doc.file))),
                    );
                }
            }
        }
        self.issues.extend(extra);
    }

    fn resolve_all(&mut self, ctx: &ResolveCtx<'_>) {
        self.resolved.clear();
        let ids: Vec<String> = self.entities.keys().cloned().collect();
        for id in ids {
            self.resolve_one(ctx, &id);
        }
        self.check_entity_refs();
    }

    fn resolve_one(&mut self, ctx: &ResolveCtx<'_>, id: &str) {
        let f = &self.entities[id];
        let mut soft = Vec::new();
        match resolve(ctx, id, &f.doc, &f.rel, &mut soft) {
            Ok(r) => {
                self.resolved.insert(id.to_string(), r);
            }
            Err(e) => {
                self.resolved.remove(id);
                self.issues.push(e);
            }
        }
        self.issues.extend(soft);
    }

    fn check_entity_refs(&mut self) {
        self.issues
            .retain(|e| !e.message.starts_with("missing reference: entity"));
        let all: BTreeSet<String> = self
            .resolved
            .values()
            .flat_map(|r| r.flatten().into_iter().map(|(e, _)| e.id.clone()))
            .collect();
        let mut errs = Vec::new();
        for (id, r) in &self.resolved {
            let room = self.entities[id].room.clone();
            for (e, _) in r.flatten() {
                for c in &e.components {
                    for f in &c.fields {
                        if f.ty != FieldType::Entity {
                            continue;
                        }
                        let Value::Str(s) = &f.value else { continue };
                        if is_null_ref(s) || s.starts_with("e:") {
                            continue;
                        }
                        if self.target_in(&all, room.as_deref(), s).is_none() {
                            errs.push(moves::missing_reference("entity", s, &f.at, &[]).hint(
                                "Entity refs are @id, or @room/id across rooms; child entities are @id/child.",
                            ));
                        }
                    }
                }
            }
        }
        self.issues.extend(errs);
        self.all_ids = all;
    }

    /// Resolve an `@ref` written inside `room` to a spawned entity's full id.
    fn target_spawned(&self, room: Option<&str>, s: &str) -> Option<String> {
        let s = s.strip_prefix('@').unwrap_or(s);
        if let Some(r) = room {
            let q = format!("{r}/{s}");
            if self.spawned.contains_key(&q) {
                return Some(q);
            }
        }
        if self.spawned.contains_key(s) {
            return Some(s.to_string());
        }
        None
    }

    /// Resolve an `@ref` written inside `room` to a full id present in `all`.
    fn target_in(&self, all: &BTreeSet<String>, room: Option<&str>, s: &str) -> Option<String> {
        let s = s.strip_prefix('@').unwrap_or(s);
        if let Some(r) = room {
            let q = format!("{r}/{s}");
            if all.contains(&q) {
                return Some(q);
            }
        }
        if all.contains(s) {
            return Some(s.to_string());
        }
        None
    }

    // ----- inspection -------------------------------------------------------

    /// Problems found while opening. A scene with issues cannot spawn.
    pub fn issues(&self) -> &[ArgusError] {
        &self.issues
    }

    /// Rooms, by name (a flat scene has one room named `""`).
    pub fn rooms(&self) -> impl Iterator<Item = &Room> {
        self.rooms.values()
    }

    /// One room.
    pub fn room(&self, name: &str) -> Option<&Room> {
        self.rooms.get(name)
    }

    /// True if the scene is subdivided into rooms.
    pub fn has_rooms(&self) -> bool {
        !self.rooms.contains_key(FLAT)
    }

    /// Rooms currently instantiated in the world.
    pub fn loaded_rooms(&self) -> Vec<&str> {
        self.loaded.iter().map(String::as_str).collect()
    }

    /// The room the player is in (set by [`Scene::transition`] and the
    /// initial `spawn_around`).
    pub fn current_room(&self) -> Option<&str> {
        self.current.as_deref()
    }

    /// Graph distance between two rooms, if connected.
    pub fn room_distance(&self, from: &str, to: &str) -> Option<u32> {
        self.distances(from).get(to).copied()
    }

    fn distances(&self, from: &str) -> BTreeMap<String, u32> {
        let mut dist = BTreeMap::new();
        let Some(g) = &self.doc.rooms else {
            dist.insert(from.to_string(), 0);
            return dist;
        };
        let mut queue = VecDeque::new();
        dist.insert(from.to_string(), 0);
        queue.push_back(from.to_string());
        while let Some(cur) = queue.pop_front() {
            let d = dist[&cur];
            for (a, b) in &g.edges {
                let next = if a == &cur {
                    b
                } else if b == &cur {
                    a
                } else {
                    continue;
                };
                if !dist.contains_key(next) {
                    dist.insert(next.clone(), d + 1);
                    queue.push_back(next.clone());
                }
            }
        }
        dist
    }

    /// Top-level entity ids (one per file), sorted.
    pub fn ids(&self) -> Vec<&str> {
        self.entities.keys().map(String::as_str).collect()
    }

    /// The file behind a top-level id.
    pub fn entity_file(&self, id: &str) -> Option<&EntityFile> {
        self.entities.get(id)
    }

    /// Split a full id into its file id and child path.
    fn split_id<'a>(&self, id: &'a str) -> Option<(&'a str, &'a str)> {
        let mut end = id.len();
        loop {
            let head = &id[..end];
            if self.entities.contains_key(head) {
                let tail = id[end..].trim_start_matches('/');
                return Some((head, tail));
            }
            end = id[..end].rfind('/')?;
        }
    }

    /// `world.get {resolved: false}`: the entity's own overrides only.
    pub fn overrides(&self, id: &str) -> Option<&EntityDoc> {
        let (file, child) = self.split_id(id)?;
        self.entities.get(file)?.doc.child_path(child)
    }

    /// `world.get {resolved: true}`: the merged entity with provenance.
    pub fn resolved(&self, id: &str) -> Option<&ResolvedEntity> {
        let (file, _) = self.split_id(id)?;
        self.resolved.get(file)?.find(id)
    }

    /// All resolved top-level entities, by id.
    pub fn all_resolved(&self) -> impl Iterator<Item = &ResolvedEntity> {
        self.resolved.values()
    }

    /// True if the id names an authored entity of this scene.
    pub fn is_authored(&self, id: &str) -> bool {
        self.resolved(id.strip_prefix('@').unwrap_or(id)).is_some()
    }

    /// The live entity for a full id, if its room is loaded.
    pub fn entity_of(&self, id: &str) -> Option<Entity> {
        self.spawned
            .get(id.strip_prefix('@').unwrap_or(id))
            .copied()
    }

    /// Full ids of the entities that instance `prefab` (it is in their
    /// chain), sorted. A child that merely inherits through its parent's
    /// prefab is part of that instance, not an instance itself; a child
    /// naming its own prefab is.
    pub fn instances_of(&self, prefab: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .resolved
            .values()
            .flat_map(|r| r.flatten())
            .filter(|(e, parent)| {
                e.chain.iter().any(|p| p == prefab) && (parent.is_none() || e.prefab.is_some())
            })
            .map(|(e, _)| e.id.clone())
            .collect();
        out.sort();
        out
    }

    fn require_clean(&self) -> Result<()> {
        if let Some(first) = self.issues.first() {
            return Err(ArgusError::new(
                codes::SCENE_NOT_CLEAN,
                format!(
                    "scene {:?} has {} issue(s); the first: {}",
                    self.name,
                    self.issues.len(),
                    first.message
                ),
            )
            .hint("Fix every issue listed by scene.issues before loading; a run must be a reproducible value.")
            .at(first.r#ref.clone()));
        }
        Ok(())
    }

    // ----- instantiation ----------------------------------------------------

    /// Spawn every room (or the flat scene) into the world.
    pub fn spawn_all(&mut self, world: &mut World) -> Result<LoadReport> {
        self.require_clean()?;
        let names: Vec<String> = self.rooms.keys().cloned().collect();
        let mut report = self.load_rooms(world, &names)?;
        if self.current.is_none() {
            self.current = self
                .doc
                .rooms
                .as_ref()
                .and_then(|g| g.start.clone())
                .or_else(|| names.first().cloned());
        }
        report.rooms = names;
        Ok(report)
    }

    /// Spawn the rooms within the graph radius of `start` (the scene's
    /// `rooms.start` when `None`), and make it the current room.
    pub fn spawn_around(&mut self, world: &mut World, start: Option<&str>) -> Result<LoadReport> {
        self.require_clean()?;
        if !self.has_rooms() {
            return self.spawn_all(world);
        }
        let g = self.doc.rooms.clone().unwrap_or_default();
        let start = start
            .map(str::to_string)
            .or(g.start.clone())
            .ok_or_else(|| {
                ArgusError::new(
                    codes::UNKNOWN_ROOM,
                    "no start room: give one or set rooms.start in scene.toml",
                )
                .at(Some(format!("{}:1", self.doc.file)))
            })?;
        if !self.rooms.contains_key(&start) {
            return Err(ArgusError::new(
                codes::UNKNOWN_ROOM,
                format!("{start:?} is not a room of this scene"),
            )
            .hint(format!(
                "Rooms: {}",
                self.rooms.keys().cloned().collect::<Vec<_>>().join(", ")
            )));
        }
        let dist = self.distances(&start);
        let names: Vec<String> = self
            .rooms
            .keys()
            .filter(|r| dist.get(*r).map(|d| *d <= g.radius).unwrap_or(false))
            .cloned()
            .collect();
        let report = self.load_rooms(world, &names)?;
        self.current = Some(start);
        Ok(report)
    }

    /// Move the player to `to`: rooms within the radius stream in, rooms
    /// beyond it stream out, and a `room.transition` event is emitted for
    /// the game's hook (fade, camera slide).
    pub fn transition(&mut self, world: &mut World, to: &str) -> Result<LoadReport> {
        self.require_clean()?;
        if !self.rooms.contains_key(to) {
            return Err(ArgusError::new(
                codes::UNKNOWN_ROOM,
                format!("{to:?} is not a room of this scene"),
            )
            .hint(format!(
                "Rooms: {}",
                self.rooms.keys().cloned().collect::<Vec<_>>().join(", ")
            )));
        }
        let radius = self.doc.rooms.as_ref().map(|g| g.radius).unwrap_or(0);
        let dist = self.distances(to);
        let want: BTreeSet<String> = self
            .rooms
            .keys()
            .filter(|r| dist.get(*r).map(|d| *d <= radius).unwrap_or(false))
            .cloned()
            .collect();
        let from = self.current.clone();
        let out: Vec<String> = self.loaded.difference(&want).cloned().collect();
        for r in &out {
            self.unload_room(world, r)?;
        }
        let in_: Vec<String> = want.difference(&self.loaded).cloned().collect();
        let report = self.load_rooms(world, &in_)?;
        self.current = Some(to.to_string());
        world.emit(Event::Game {
            name: "room.transition".to_string(),
            fields: vec![
                ("from".to_string(), Value::Str(from.unwrap_or_default())),
                ("to".to_string(), Value::Str(to.to_string())),
            ],
        });
        Ok(report)
    }

    /// Spawn the named rooms (skipping ones already loaded), then fill any
    /// pending references into them.
    pub fn load_rooms(&mut self, world: &mut World, names: &[String]) -> Result<LoadReport> {
        self.require_clean()?;
        let start = Instant::now();
        let mut report = LoadReport::default();
        ensure_hierarchy(world)?;
        for name in names {
            if self.loaded.contains(name) {
                continue;
            }
            let room = self.rooms.get(name).ok_or_else(|| {
                ArgusError::new(
                    codes::UNKNOWN_ROOM,
                    format!("{name:?} is not a room of this scene"),
                )
            })?;
            let ids = room.entities.clone();
            let trees: Vec<ResolvedEntity> =
                ids.iter().map(|id| self.resolved[id].clone()).collect();
            let room_name = room.name.clone();
            let mut placed: Vec<(Entity, ResolvedEntity)> = Vec::new();
            for tree in &trees {
                self.spawn_tree(world, tree, None, &mut placed, &mut report)?;
            }
            let room_opt = if room_name == FLAT {
                None
            } else {
                Some(room_name.as_str())
            };
            for (e, re) in &placed {
                self.set_fields(world, *e, re, room_opt, &mut report, false)?;
            }
            self.loaded.insert(room_name.clone());
            report.rooms.push(room_name.clone());
            let n = placed.len();
            world.emit(Event::Game {
                name: "room.loaded".to_string(),
                fields: vec![
                    ("room".to_string(), Value::Str(room_name)),
                    ("entities".to_string(), Value::Int(n as i64)),
                ],
            });
        }
        self.fill_pending(world)?;
        report.ms = start.elapsed().as_secs_f64() * 1000.0;
        Ok(report)
    }

    fn spawn_tree(
        &mut self,
        world: &mut World,
        re: &ResolvedEntity,
        parent: Option<Entity>,
        placed: &mut Vec<(Entity, ResolvedEntity)>,
        report: &mut LoadReport,
    ) -> Result<()> {
        let e = world.spawn_authored(&re.id)?;
        self.spawned.insert(re.id.clone(), e);
        report.entities += 1;
        if let Some(p) = parent {
            world.insert(e, argus_ecs::Parent { entity: p })?;
        }
        for (tag, _) in &re.tags {
            if world.component_id(tag).is_none() {
                let schema = SchemaBuilder::new(tag.as_str())
                    .doc("Tag declared by use in a scene file.")
                    .source(re.file.clone())
                    .build()?;
                world.register_schema(schema)?;
                report.tags_registered.push(tag.clone());
            }
            world.insert_by_name(e, tag)?;
        }
        for c in &re.components {
            world.insert_by_name(e, &c.name)?;
        }
        placed.push((e, re.clone()));
        for child in &re.children {
            self.spawn_tree(world, child, Some(e), placed, report)?;
        }
        Ok(())
    }

    /// Write an entity's resolved fields into the world. With `all` false,
    /// schema defaults are skipped (a fresh insert already has them).
    fn set_fields(
        &mut self,
        world: &mut World,
        e: Entity,
        re: &ResolvedEntity,
        room: Option<&str>,
        report: &mut LoadReport,
        all: bool,
    ) -> Result<()> {
        for c in &re.components {
            for f in &c.fields {
                if !all && f.from == Provenance::Default {
                    continue;
                }
                let path = format!("{}.{}", c.name, f.name);
                let v = match (&f.ty, &f.value) {
                    (FieldType::Entity, Value::Str(s)) => {
                        if is_null_ref(s) {
                            Value::Entity(Entity::NULL)
                        } else if s.starts_with("e:") {
                            Value::Entity(world.entity(s)?)
                        } else {
                            match self.target_spawned(room, s) {
                                Some(t) => Value::Entity(self.spawned[&t]),
                                None => {
                                    // Exists in the scene (checked at open) but its room is not loaded.
                                    let target = self
                                        .target_in(&self.all_ids, room, s)
                                        .unwrap_or_else(|| s.trim_start_matches('@').to_string());
                                    self.pending.push(PendingRef {
                                        from: re.id.clone(),
                                        path: path.clone(),
                                        target: target.clone(),
                                    });
                                    report.pending.push((re.id.clone(), path.clone(), target));
                                    Value::Entity(Entity::NULL)
                                }
                            }
                        }
                    }
                    (_, v) => v.clone(),
                };
                world.set_field(e, &path, v)?;
            }
        }
        Ok(())
    }

    fn fill_pending(&mut self, world: &mut World) -> Result<()> {
        let mut still = Vec::new();
        for p in std::mem::take(&mut self.pending) {
            match (self.spawned.get(&p.from), self.spawned.get(&p.target)) {
                (Some(&from), Some(&target)) => {
                    world.set_field(from, &p.path, Value::Entity(target))?;
                }
                (Some(_), None) => still.push(p),
                (None, _) => {} // the referencing entity is gone; nothing to fill
            }
        }
        self.pending = still;
        Ok(())
    }

    /// Spawn one top-level authored entity (with its children) into the
    /// world, for an entity file created after its room was loaded.
    pub fn spawn_entity(&mut self, world: &mut World, id: &str) -> Result<Entity> {
        let id = id.strip_prefix('@').unwrap_or(id);
        let re = self
            .resolved
            .get(id)
            .ok_or_else(|| {
                ArgusError::new(
                    argus_ecs::codes::UNKNOWN_ID,
                    format!("no authored entity file {id:?} in scene {:?}", self.name),
                )
            })?
            .clone();
        if let Some(e) = self.spawned.get(id) {
            return Ok(*e);
        }
        self.require_clean()?;
        ensure_hierarchy(world)?;
        let room = self.entities[id].room.clone();
        let mut report = LoadReport::default();
        let mut placed = Vec::new();
        self.spawn_tree(world, &re, None, &mut placed, &mut report)?;
        for (e, r) in &placed {
            self.set_fields(world, *e, r, room.as_deref(), &mut report, false)?;
        }
        self.fill_pending(world)?;
        Ok(placed[0].0)
    }

    /// The room an entity id belongs to (`None` in a flat scene).
    pub fn room_of(&self, id: &str) -> Option<&str> {
        let id = id.strip_prefix('@').unwrap_or(id);
        let (fid, _) = self.split_id(id)?;
        self.entities.get(fid)?.room.as_deref()
    }

    /// Despawn a room's entities. References into it from other rooms are
    /// nulled and remembered so they refill when the room returns.
    pub fn unload_room(&mut self, world: &mut World, name: &str) -> Result<usize> {
        if !self.loaded.contains(name) {
            return Ok(0);
        }
        let room = &self.rooms[name];
        let leaving: BTreeSet<String> = room
            .entities
            .iter()
            .flat_map(|id| {
                self.resolved[id]
                    .flatten()
                    .into_iter()
                    .map(|(e, _)| e.id.clone())
            })
            .collect();
        // Refs from loaded entities outside the room into the leaving set.
        let mut nulls = Vec::new();
        let spawned_ids: Vec<String> = self.spawned.keys().cloned().collect();
        for sid in &spawned_ids {
            if leaving.contains(sid) {
                continue;
            }
            let Some((fid, _)) = self.split_id(sid) else {
                continue;
            };
            let room_opt = self.entities[fid].room.as_deref();
            let Some(e) = self.resolved(sid) else {
                continue;
            };
            let live = self.spawned[sid];
            {
                for c in &e.components {
                    for fld in &c.fields {
                        if fld.ty != FieldType::Entity {
                            continue;
                        }
                        let Value::Str(s) = &fld.value else { continue };
                        if let Some(t) = self.target_in(&leaving, room_opt, s) {
                            nulls.push((live, e.id.clone(), format!("{}.{}", c.name, fld.name), t));
                        }
                    }
                }
            }
        }
        for (live, from, path, target) in nulls {
            world.set_field(live, &path, Value::Entity(Entity::NULL))?;
            self.pending.push(PendingRef { from, path, target });
        }
        let mut n = 0;
        for id in leaving.iter().rev() {
            if let Some(e) = self.spawned.remove(id) {
                if world.is_alive(e) {
                    world.despawn(e)?;
                    n += 1;
                }
            }
        }
        self.pending.retain(|p| !leaving.contains(&p.from));
        self.loaded.remove(name);
        world.emit(Event::Game {
            name: "room.unloaded".to_string(),
            fields: vec![
                ("room".to_string(), Value::Str(name.to_string())),
                ("entities".to_string(), Value::Int(n as i64)),
            ],
        });
        Ok(n)
    }

    /// Bring a loaded entity's components and fields in line with its
    /// resolved document: components added, removed, every field written.
    /// Used after an override edit or a prefab change.
    pub fn sync_entity(&mut self, world: &mut World, id: &str) -> Result<bool> {
        let Some(e) = self.entity_of(id) else {
            return Ok(false);
        };
        let re = self
            .resolved(id)
            .ok_or_else(|| {
                ArgusError::new(
                    argus_ecs::codes::UNKNOWN_ID,
                    format!("no authored entity {id:?}"),
                )
            })?
            .clone();
        let room = self
            .split_id(id)
            .and_then(|(f, _)| self.entities[f].room.clone());
        // Components: add missing, remove extra (tags included).
        let want: BTreeSet<&str> = re
            .components
            .iter()
            .map(|c| c.name.as_str())
            .chain(re.tags.iter().map(|(t, _)| t.as_str()))
            .collect();
        let have: Vec<argus_ecs::ComponentId> = world.components_of(e)?.to_vec();
        for cid in have {
            let name = world.registry().schema(cid).name.clone();
            if name == "Parent" || name == "GlobalTransform" {
                continue;
            }
            if !want.contains(name.as_str()) {
                world.remove(e, cid)?;
            }
        }
        for name in &want {
            if world.component_id(name).is_none() {
                let schema = SchemaBuilder::new(*name)
                    .doc("Tag declared by use in a scene file.")
                    .build()?;
                world.register_schema(schema)?;
            }
            let cid = world.component_id(name).expect("registered");
            if !world.has(e, cid) {
                world.insert_by_name(e, name)?;
            }
        }
        let mut report = LoadReport::default();
        self.set_fields(world, e, &re, room.as_deref(), &mut report, true)?;
        self.fill_pending(world)?;
        Ok(true)
    }

    // ----- editing ----------------------------------------------------------

    fn file_and_child<'a>(&self, id: &'a str) -> Result<(String, &'a str)> {
        let id = id.strip_prefix('@').unwrap_or(id);
        self.split_id(id)
            .map(|(f, c)| (f.to_string(), c))
            .ok_or_else(|| {
                ArgusError::new(codes::NOT_AUTHORED, format!("{id:?} is not an authored entity of scene {:?}", self.name))
                    .hint("Only authored entities have files; runtime entities are live-only and never written (§4).")
            })
    }

    /// Set one override in the entity's file (`Component.field`), keeping
    /// the file's formatting, and re-resolve the entity. The live world is
    /// not touched; call [`Scene::sync_entity`] for that.
    pub fn set_override(
        &mut self,
        ctx: &ResolveCtx<'_>,
        id: &str,
        path: &str,
        value: &Value,
    ) -> Result<()> {
        let (fid, child) = self.file_and_child(id)?;
        let (comp, field) = path.split_once('.').ok_or_else(|| {
            ArgusError::new(
                argus_ecs::codes::BAD_PATH,
                format!("{path:?} is not Component.field"),
            )
        })?;
        let (cid, fs) = ctx.registry.resolve_path(path)?;
        let value = match fs.ty {
            FieldType::Entity => match value {
                Value::Str(_) => value.clone(),
                Value::Entity(e) if e.is_null() => Value::Str(String::new()),
                other => {
                    return Err(ArgusError::new(
                        codes::NOT_AUTHORED,
                        format!("{other} is a live handle; write entity refs to files as \"@id\""),
                    ))
                }
            },
            _ => ctx.registry.schema(cid).check(fs, value.clone())?,
        };
        let f = &self.entities[&fid];
        let (p, rel) = (f.path.clone(), f.rel.clone());
        let new_doc = doc::edit_file(&p, &rel, child, |t| {
            doc::set_override(t, comp, field, &value)
        })?;
        self.entities.get_mut(&fid).expect("exists").doc = new_doc;
        self.resolve_one(ctx, &fid);
        self.check_entity_refs();
        Ok(())
    }

    /// Remove one override from the file (the value falls back to the
    /// prefab or default) and re-resolve.
    pub fn clear_override(&mut self, ctx: &ResolveCtx<'_>, id: &str, path: &str) -> Result<()> {
        let (fid, child) = self.file_and_child(id)?;
        let (comp, field) = path.split_once('.').ok_or_else(|| {
            ArgusError::new(
                argus_ecs::codes::BAD_PATH,
                format!("{path:?} is not Component.field"),
            )
        })?;
        let f = &self.entities[&fid];
        let (p, rel) = (f.path.clone(), f.rel.clone());
        let new_doc = doc::edit_file(&p, &rel, child, |t| {
            doc::clear_override(t, comp, field);
            Ok(())
        })?;
        self.entities.get_mut(&fid).expect("exists").doc = new_doc;
        self.resolve_one(ctx, &fid);
        self.check_entity_refs();
        Ok(())
    }

    /// Add a component (with defaults) as an override in the file.
    pub fn add_component(&mut self, ctx: &ResolveCtx<'_>, id: &str, component: &str) -> Result<()> {
        let (fid, child) = self.file_and_child(id)?;
        ctx.registry.require(component)?;
        let f = &self.entities[&fid];
        let (p, rel) = (f.path.clone(), f.rel.clone());
        let new_doc = doc::edit_file(&p, &rel, child, |t| {
            t.remove(&format!("-{component}"));
            if !t.contains_key(component) {
                t[component] = toml_edit::Item::Table(toml_edit::Table::new());
            }
            Ok(())
        })?;
        self.entities.get_mut(&fid).expect("exists").doc = new_doc;
        self.resolve_one(ctx, &fid);
        Ok(())
    }

    /// Remove a component: if a prefab layer provides it, the file gets an
    /// explicit `-Component = true`; otherwise its table is dropped.
    pub fn remove_component(
        &mut self,
        ctx: &ResolveCtx<'_>,
        id: &str,
        component: &str,
    ) -> Result<()> {
        let (fid, child) = self.file_and_child(id)?;
        let inherited = self
            .resolved(id.strip_prefix('@').unwrap_or(id))
            .and_then(|r| r.component(component))
            .map(|c| c.from != Provenance::Own)
            .unwrap_or(false);
        let f = &self.entities[&fid];
        let (p, rel) = (f.path.clone(), f.rel.clone());
        let new_doc = doc::edit_file(&p, &rel, child, |t| {
            if inherited {
                doc::mark_removed(t, component);
            } else {
                doc::remove_component(t, component);
            }
            Ok(())
        })?;
        self.entities.get_mut(&fid).expect("exists").doc = new_doc;
        self.resolve_one(ctx, &fid);
        Ok(())
    }

    /// Create a new authored entity file. `local` is the file stem; `room`
    /// picks the directory in a room scene. Returns the full id.
    pub fn create_entity(
        &mut self,
        ctx: &ResolveCtx<'_>,
        local: &str,
        room: Option<&str>,
        doc: &EntityDoc,
    ) -> Result<String> {
        if local.is_empty() || local.contains(['/', '\\', ' ']) {
            return Err(ArgusError::new(
                argus_ecs::codes::BAD_ID,
                format!("{local:?} is not a valid entity id"),
            )
            .hint("Ids look like player or door.east; they become file names."));
        }
        if crate::bulk::reserved_stem(local) {
            return Err(ArgusError::new(
                argus_ecs::codes::BAD_ID,
                format!("{local:?} is a reserved file name, not an entity id"),
            )
            .hint("scene and room are the scene and room documents, and names ending .map or .tilemap are data indexes; pick another id."));
        }
        let (dir, full) = match (self.has_rooms(), room) {
            (true, Some(r)) => {
                let rm = self.rooms.get(r).ok_or_else(|| {
                    ArgusError::new(
                        codes::UNKNOWN_ROOM,
                        format!("{r:?} is not a room of this scene"),
                    )
                })?;
                (rm.dir.clone(), format!("{r}/{local}"))
            }
            (true, None) => {
                return Err(ArgusError::new(
                    codes::UNKNOWN_ROOM,
                    "this scene has rooms; say which one",
                )
                .hint(format!(
                    "Rooms: {}",
                    self.rooms.keys().cloned().collect::<Vec<_>>().join(", ")
                )))
            }
            (false, _) => (self.dir.clone(), local.to_string()),
        };
        if self.entities.contains_key(&full) {
            return Err(ArgusError::new(
                argus_ecs::codes::DUPLICATE_ID,
                format!("{full:?} already exists"),
            )
            .hint("Pick another id, or edit the existing file."));
        }
        let path = dir.join(format!("{local}.toml"));
        std::fs::write(&path, doc.to_toml_string()?).map_err(|e| {
            ArgusError::new(
                codes::MISSING_FILE,
                format!("cannot write {}: {e}", path.display()),
            )
        })?;
        let rel = moves::rel(ctx.root, &path);
        let text = tomlv::read(&path)?;
        let parsed = EntityDoc::parse(&rel, &text)?;
        self.entities.insert(
            full.clone(),
            EntityFile {
                id: full.clone(),
                local: local.to_string(),
                room: room.filter(|_| self.has_rooms()).map(str::to_string),
                path,
                rel,
                doc: parsed,
            },
        );
        let key = if self.has_rooms() {
            room.unwrap_or("")
        } else {
            FLAT
        };
        if let Some(rm) = self.rooms.get_mut(key) {
            rm.entities.push(full.clone());
            rm.entities.sort();
        }
        self.resolve_one(ctx, &full);
        self.check_entity_refs();
        Ok(full)
    }

    /// Delete an authored entity's file (and despawn it if loaded).
    pub fn delete_entity(&mut self, world: &mut World, id: &str) -> Result<()> {
        let (fid, child) = self.file_and_child(id)?;
        if !child.is_empty() {
            return Err(ArgusError::new(
                codes::BAD_SCENE,
                format!("{id:?} is a child; remove it with -children in the parent file"),
            )
            .hint("Child entities live in their parent's file."));
        }
        let f = self.entities.remove(&fid).expect("exists");
        for rm in self.rooms.values_mut() {
            rm.entities.retain(|x| x != &fid);
        }
        if let Some(r) = self.resolved.remove(&fid) {
            for (e, _) in r.flatten().into_iter().rev() {
                if let Some(live) = self.spawned.remove(&e.id) {
                    if world.is_alive(live) {
                        world.despawn(live)?;
                    }
                }
            }
        }
        std::fs::remove_file(&f.path).map_err(|e| {
            ArgusError::new(codes::MISSING_FILE, format!("cannot delete {}: {e}", f.rel))
        })?;
        self.check_entity_refs();
        Ok(())
    }

    /// Re-read every entity file from disk (after `asset.move` rewrote
    /// references, or an outside edit) and re-resolve, keeping the loaded
    /// rooms and spawned entities as they are. Values in the live world are
    /// not touched; call [`Scene::sync_entity`] for those.
    pub fn reload_files(&mut self, ctx: &ResolveCtx<'_>) {
        self.issues
            .retain(|e| e.code == codes::UNKNOWN_ROOM || e.code == codes::BAD_SCENE);
        let ids: Vec<String> = self.entities.keys().cloned().collect();
        for id in ids {
            let (path, rel) = {
                let f = &self.entities[&id];
                (f.path.clone(), f.rel.clone())
            };
            match tomlv::read(&path).and_then(|t| EntityDoc::parse(&rel, &t)) {
                Ok(doc) => self.entities.get_mut(&id).expect("exists").doc = doc,
                Err(e) => self.issues.push(e),
            }
        }
        self.resolve_all(ctx);
    }

    /// Re-resolve every entity (after prefab or def changes) and refresh
    /// the issue list.
    pub fn reresolve(&mut self, ctx: &ResolveCtx<'_>) {
        self.issues.retain(|e| {
            e.code == codes::BAD_TOML || e.code == codes::UNKNOWN_ROOM || e.code == codes::BAD_SCENE
        });
        self.resolve_all(ctx);
    }

    /// A prefab changed: re-resolve its instances and write the new values
    /// into every loaded one. Returns the instance ids touched, which is
    /// what the change feed reports (§3).
    pub fn reapply_prefab(
        &mut self,
        project: &Project,
        world: &mut World,
        prefab: &str,
    ) -> Result<Vec<String>> {
        // The world is mutated below, so resolve against a copy of its registry.
        let registry = world.registry().clone();
        let ctx = &project.ctx(&registry);
        let affected: BTreeSet<String> = self
            .resolved
            .iter()
            .filter(|(_, r)| r.uses_prefab(prefab))
            .map(|(id, _)| id.clone())
            .collect();
        for id in &affected {
            self.resolve_one(ctx, id);
        }
        self.check_entity_refs();
        let mut touched = Vec::new();
        for id in &affected {
            let Some(r) = self.resolved.get(id).cloned() else {
                continue;
            };
            for (e, _) in r.flatten() {
                if e.chain.iter().any(|p| p == prefab) {
                    touched.push(e.id.clone());
                    if self.spawned.contains_key(&e.id) && self.issues.is_empty() {
                        self.sync_entity(world, &e.id)?;
                    }
                }
            }
        }
        touched.sort();
        Ok(touched)
    }

    /// Bulk index files of a room (project-relative).
    pub fn bulk_files(&self, room: &str) -> Vec<&str> {
        self.rooms
            .get(room)
            .map(|r| r.bulk.iter().map(String::as_str).collect())
            .unwrap_or_default()
    }
}

fn is_null_ref(s: &str) -> bool {
    s.is_empty() || s == "e:null" || s == "@"
}

fn ensure_hierarchy(world: &mut World) -> Result<()> {
    if world.component_id("Parent").is_none() {
        argus_ecs::hierarchy::register(world)?;
    }
    Ok(())
}

impl SceneDoc {
    /// Read `scenes/<name>/scene.toml` under a project root without opening
    /// the whole scene: enough to pick the clock and seed before the world
    /// exists.
    pub fn load(root: &Path, name: &str) -> Result<SceneDoc> {
        let path = root.join("scenes").join(name).join("scene.toml");
        let rel = format!("scenes/{name}/scene.toml");
        let text = tomlv::read(&path)?;
        parse_scene_doc(&rel, &text)
    }
}

fn parse_scene_doc(rel: &str, text: &str) -> Result<SceneDoc> {
    let im = tomlv::parse(rel, text)?;
    let t = im.as_table();
    let at = |k: &str| {
        t.get(k)
            .map(|i| tomlv::item_ref(rel, text, i))
            .unwrap_or_else(|| format!("{rel}:1"))
    };
    let clock = match t.get("clock") {
        Some(i) => {
            let s = i.as_str().ok_or_else(|| {
                ArgusError::new(
                    codes::BAD_SCENE,
                    "clock must be a string like \"continuous@60\" or \"discrete\"",
                )
                .at(Some(at("clock")))
            })?;
            argus_sim::Clock::parse(s).map_err(|e| e.at(Some(at("clock"))))?;
            Some(s.to_string())
        }
        None => None,
    };
    let seed = match t.get("seed") {
        None => SeedPolicy::Random,
        Some(i) => match (i.as_integer(), i.as_str()) {
            (Some(n), _) if n >= 0 => SeedPolicy::Fixed(n as u64),
            (_, Some("random")) => SeedPolicy::Random,
            _ => {
                return Err(ArgusError::new(
                    codes::BAD_SCENE,
                    "seed must be a non-negative integer or \"random\"",
                )
                .at(Some(at("seed"))))
            }
        },
    };
    let bounds = t
        .get("bounds")
        .map(|i| tomlv::bounds(i, &at("bounds")))
        .transpose()?;
    let (mut enable, mut disable) = (Vec::new(), Vec::new());
    if let Some(s) = t.get("systems") {
        let st = tomlv::table_like(s, "systems", &at("systems"))?;
        if let Some(e) = st.get("enable") {
            enable = tomlv::string_list(e, "systems.enable", &at("systems"))?;
        }
        if let Some(d) = st.get("disable") {
            disable = tomlv::string_list(d, "systems.disable", &at("systems"))?;
        }
    }
    let rooms = match t.get("rooms") {
        None => None,
        Some(r) => {
            let rt = tomlv::table_like(r, "rooms", &at("rooms"))?;
            let start = rt.get("start").and_then(|i| i.as_str()).map(str::to_string);
            let radius = rt
                .get("radius")
                .map(|i| {
                    i.as_integer()
                        .and_then(|n| u32::try_from(n).ok())
                        .ok_or_else(|| {
                            ArgusError::new(
                                codes::BAD_SCENE,
                                "rooms.radius must be a non-negative integer",
                            )
                            .at(Some(at("rooms")))
                        })
                })
                .transpose()?
                .unwrap_or(1);
            let mut edges = Vec::new();
            if let Some(g) = rt.get("graph") {
                let gt = tomlv::table_like(g, "rooms.graph", &at("rooms"))?;
                for (from, item) in gt.iter() {
                    for to in
                        tomlv::string_list(item, &format!("rooms.graph.{from}"), &at("rooms"))?
                    {
                        let (a, b) = (from.to_string(), to);
                        if !edges.iter().any(|(x, y): &(String, String)| {
                            (x == &a && y == &b) || (x == &b && y == &a)
                        }) {
                            edges.push((a, b));
                        }
                    }
                }
            }
            Some(RoomGraph {
                start,
                radius,
                edges,
            })
        }
    };
    Ok(SceneDoc {
        clock,
        seed,
        bounds,
        enable,
        disable,
        rooms,
        file: rel.to_string(),
    })
}

fn parse_room_doc(rel: &str, text: &str) -> Result<RoomDoc> {
    let im = tomlv::parse(rel, text)?;
    let t = im.as_table();
    let at = |k: &str| {
        t.get(k)
            .map(|i| tomlv::item_ref(rel, text, i))
            .unwrap_or_else(|| format!("{rel}:1"))
    };
    let bounds = t
        .get("bounds")
        .map(|i| tomlv::bounds(i, &at("bounds")))
        .transpose()?;
    let tileset = t
        .get("tileset")
        .and_then(|i| i.as_str())
        .map(str::to_string);
    let neighbors = match t.get("neighbors") {
        Some(i) => tomlv::string_list(i, "neighbors", &at("neighbors"))?,
        None => Vec::new(),
    };
    Ok(RoomDoc {
        bounds,
        tileset,
        neighbors,
        file: rel.to_string(),
    })
}

/// Map over `items` on every available core; results come back in input
/// order, so loading stays deterministic.
fn parallel_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(items.len().max(1));
    if threads <= 1 || items.len() < 64 {
        return items.iter().map(&f).collect();
    }
    let chunk = items.len().div_ceil(threads);
    let mut out: Vec<Vec<R>> = Vec::with_capacity(threads);
    std::thread::scope(|scope| {
        let handles: Vec<_> = items
            .chunks(chunk)
            .map(|c| scope.spawn(|| c.iter().map(&f).collect::<Vec<R>>()))
            .collect();
        for h in handles {
            out.push(h.join().expect("scene loader worker panicked"));
        }
    });
    out.into_iter().flatten().collect()
}
