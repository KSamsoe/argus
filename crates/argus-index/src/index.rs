//! `.argus/index.db`: the tables, how they are filled from a project and
//! a resolved scene, how staleness is detected from file metadata, and
//! the reads that mirror the daemon's `world.*` and `scene.*` methods.

use crate::query::{self, Source};
use crate::shapes::{self, Summary};
use crate::source::SceneSource;
use crate::{codes, sql_err};
use argus_ecs::{ArgusError, FieldType, Registry, Result, Value};
use argus_scene::{Defs, PrefabLibrary, Project, ResolvedEntity, Scene, SeedPolicy};
use argus_sim::SystemInfo;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::{json, Value as Json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

const SCHEMA_VERSION: &str = "7";

const DDL: &str = "
CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS files(path TEXT PRIMARY KEY, kind TEXT NOT NULL, scene TEXT, mtime INTEGER NOT NULL, size INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS scenes(name TEXT PRIMARY KEY, clock TEXT, seed INTEGER, has_rooms INTEGER NOT NULL, file TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS rooms(scene TEXT NOT NULL, name TEXT NOT NULL, file TEXT NOT NULL, bounds TEXT, neighbors TEXT NOT NULL, entities INTEGER NOT NULL, bulk TEXT NOT NULL, PRIMARY KEY(scene, name));
CREATE TABLE IF NOT EXISTS entities(scene TEXT NOT NULL, id TEXT NOT NULL, root TEXT NOT NULL, room TEXT, file TEXT NOT NULL, parent TEXT, prefab TEXT, chain TEXT NOT NULL, tags TEXT NOT NULL, overrides TEXT, PRIMARY KEY(scene, id)) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS entities_prefab ON entities(prefab);
CREATE INDEX IF NOT EXISTS entities_root ON entities(scene, root);
CREATE TABLE IF NOT EXISTS components(scene TEXT NOT NULL, entity TEXT NOT NULL, component TEXT NOT NULL, provenance TEXT NOT NULL, PRIMARY KEY(scene, entity, component)) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS components_by_name ON components(scene, component);
CREATE TABLE IF NOT EXISTS fields(scene TEXT NOT NULL, entity TEXT NOT NULL, path TEXT NOT NULL, ord INTEGER NOT NULL, value TEXT NOT NULL, provenance TEXT NOT NULL, at TEXT NOT NULL, PRIMARY KEY(scene, entity, path)) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS refs(scene TEXT, from_kind TEXT NOT NULL, from_id TEXT NOT NULL, root TEXT, path TEXT NOT NULL, kind TEXT NOT NULL, target TEXT NOT NULL, at TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS refs_target ON refs(kind, target);
CREATE INDEX IF NOT EXISTS refs_root ON refs(scene, root);
CREATE TABLE IF NOT EXISTS prefabs(path TEXT PRIMARY KEY, file TEXT NOT NULL, base TEXT, chain TEXT NOT NULL, components TEXT NOT NULL, tags TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS def_tables(name TEXT PRIMARY KEY, prefix TEXT NOT NULL, key_field TEXT NOT NULL, columns TEXT NOT NULL, file TEXT);
CREATE TABLE IF NOT EXISTS defs(table_name TEXT NOT NULL, key TEXT NOT NULL, at TEXT NOT NULL, row TEXT NOT NULL, PRIMARY KEY(table_name, key));
CREATE TABLE IF NOT EXISTS schemas(name TEXT PRIMARY KEY, ord INTEGER NOT NULL, doc TEXT NOT NULL, version INTEGER NOT NULL, source TEXT, kind TEXT NOT NULL, fields TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS systems(name TEXT PRIMARY KEY, ord INTEGER NOT NULL, stage TEXT NOT NULL, after TEXT NOT NULL, reads TEXT NOT NULL, writes TEXT NOT NULL, clock TEXT NOT NULL, rng TEXT, enabled INTEGER NOT NULL, source TEXT, doc TEXT NOT NULL);
CREATE VIRTUAL TABLE IF NOT EXISTS docs USING fts5(kind, name, doc, tokenize = 'porter');
";

/// What a sync did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SyncReport {
    /// Scenes re-indexed.
    pub scenes: Vec<String>,
    /// Entities written (children included).
    pub entities: usize,
    /// Scenes that could not be indexed, with the error.
    pub failed: Vec<(String, String)>,
    /// Wall-clock milliseconds.
    pub ms: f64,
}

/// The open index.
pub struct Index {
    conn: Connection,
    root: PathBuf,
}

impl Index {
    /// Where a project's index lives.
    pub fn path(root: &Path) -> PathBuf {
        root.join(".argus").join("index.db")
    }

    /// Open (creating) the project's index. A schema-version mismatch
    /// drops everything and starts over: it is a cache.
    pub fn open(root: &Path) -> Result<Index> {
        let p = Self::path(root);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| {
                ArgusError::new(codes::INDEX, format!("cannot create {}: {e}", d.display()))
            })?;
        }
        let conn = Connection::open(&p).map_err(sql_err)?;
        Self::init(conn, root)
    }

    /// An in-memory index (tests, or a project that must not write `.argus/`).
    pub fn open_memory(root: &Path) -> Result<Index> {
        let conn = Connection::open_in_memory().map_err(sql_err)?;
        Self::init(conn, root)
    }

    fn init(conn: Connection, root: &Path) -> Result<Index> {
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = OFF;")
            .map_err(sql_err)?;
        // Every write path uses prepare_cached; the default cache (16) is
        // smaller than the number of distinct statements.
        conn.set_prepared_statement_cache_capacity(256);
        conn.execute_batch(DDL).map_err(sql_err)?;
        let version: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |r| r.get(0),
            )
            .ok();
        if version.as_deref() != Some(SCHEMA_VERSION) {
            conn.execute_batch(
                "DROP TABLE IF EXISTS files; DROP TABLE IF EXISTS scenes; DROP TABLE IF EXISTS rooms;
                 DROP TABLE IF EXISTS entities; DROP TABLE IF EXISTS components; DROP TABLE IF EXISTS fields;
                 DROP TABLE IF EXISTS refs; DROP TABLE IF EXISTS prefabs; DROP TABLE IF EXISTS def_tables;
                 DROP TABLE IF EXISTS defs; DROP TABLE IF EXISTS schemas; DROP TABLE IF EXISTS systems;
                 DROP TABLE IF EXISTS docs; DELETE FROM meta;",
            )
            .map_err(sql_err)?;
            conn.execute_batch(DDL).map_err(sql_err)?;
            conn.execute(
                "INSERT INTO meta(key, value) VALUES ('schema_version', ?1)",
                params![SCHEMA_VERSION],
            )
            .map_err(sql_err)?;
        }
        Ok(Index {
            conn,
            root: root.to_path_buf(),
        })
    }

    /// The connection, for ad-hoc reads.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    // ----- sync -------------------------------------------------------------

    /// Component schemas and their doc strings.
    pub fn sync_schemas(&mut self, registry: &Registry) -> Result<()> {
        let fp = ("schemas", fingerprint(&[&registry.schemas()]));
        if self.written(&fp)? {
            return Ok(());
        }
        let tx = self.conn.transaction().map_err(sql_err)?;
        tx.execute_batch(
            "DELETE FROM schemas; DELETE FROM docs WHERE kind IN ('component', 'field');",
        )
        .map_err(sql_err)?;
        for (i, s) in registry.schemas().iter().enumerate() {
            let fields: Vec<Json> = s
                .fields
                .iter()
                .map(|f| {
                    json!({
                        "name": f.name, "type": f.ty.name(), "default": shapes::value_json(&f.default),
                        "range": f.range.map(|(lo, hi)| json!([lo, hi])), "doc": f.doc, "tune": f.tune,
                    })
                })
                .collect();
            tx.prepare_cached("INSERT INTO schemas(name, ord, doc, version, source, kind, fields) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")
            .map_err(sql_err)?
            .execute(params![
                    s.name,
                    i as i64,
                    s.doc,
                    s.version as i64,
                    s.source,
                    if s.fields.is_empty() { "tag" } else { "data" },
                    Json::Array(fields).to_string()
                ])
            .map_err(sql_err)?;
            tx.prepare_cached("INSERT INTO docs(kind, name, doc) VALUES ('component', ?1, ?2)")
                .map_err(sql_err)?
                .execute(params![s.name, s.doc])
                .map_err(sql_err)?;
            for f in &s.fields {
                if !f.doc.is_empty() {
                    tx.prepare_cached("INSERT INTO docs(kind, name, doc) VALUES ('field', ?1, ?2)")
                        .map_err(sql_err)?
                        .execute(params![format!("{}.{}", s.name, f.name), f.doc])
                        .map_err(sql_err)?;
                }
            }
        }
        tx.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)",
            params![format!("fp.{}", fp.0), fp.1],
        )
        .map_err(sql_err)?;
        tx.commit().map_err(sql_err)
    }

    /// Systems with stages and write sets.
    pub fn sync_systems(&mut self, systems: &[SystemInfo]) -> Result<()> {
        let fp = ("systems", fingerprint(&[&systems]));
        if self.written(&fp)? {
            return Ok(());
        }
        let tx = self.conn.transaction().map_err(sql_err)?;
        tx.execute_batch("DELETE FROM systems; DELETE FROM docs WHERE kind = 'system';")
            .map_err(sql_err)?;
        for (i, s) in systems.iter().enumerate() {
            tx.prepare_cached("INSERT INTO systems(name, ord, stage, after, reads, writes, clock, rng, enabled, source, doc) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)")
            .map_err(sql_err)?
            .execute(params![
                    s.name,
                    i as i64,
                    s.stage.name(),
                    json!(s.after).to_string(),
                    json!(s.reads).to_string(),
                    json!(s.writes).to_string(),
                    format!("{:?}", s.clock).to_lowercase(),
                    s.rng,
                    s.enabled as i64,
                    s.source,
                    s.doc
                ])
            .map_err(sql_err)?;
            tx.prepare_cached("INSERT INTO docs(kind, name, doc) VALUES ('system', ?1, ?2)")
                .map_err(sql_err)?
                .execute(params![s.name, s.doc])
                .map_err(sql_err)?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)",
            params![format!("fp.{}", fp.0), fp.1],
        )
        .map_err(sql_err)?;
        tx.commit().map_err(sql_err)
    }

    /// The prefab catalog.
    pub fn sync_prefabs(&mut self, prefabs: &PrefabLibrary) -> Result<()> {
        let stamps: Vec<(&str, (i64, i64))> = prefabs
            .paths()
            .into_iter()
            .filter_map(|p| prefabs.get(p))
            .map(|p| (p.file.as_str(), file_stamp(&self.root.join(&p.file))))
            .collect();
        let fp = ("prefabs", fingerprint(&[prefabs, &stamps]));
        if self.written(&fp)? {
            return Ok(());
        }
        let tx = self.conn.transaction().map_err(sql_err)?;
        tx.execute_batch("DELETE FROM prefabs; DELETE FROM refs WHERE from_kind = 'prefab'; DELETE FROM files WHERE kind = 'prefab';")
            .map_err(sql_err)?;
        for path in prefabs.paths() {
            let p = prefabs.get(path).expect("listed");
            let chain: Vec<String> = prefabs
                .chain(path, "", &[])
                .map(|c| c.iter().map(|x| x.path.clone()).collect())
                .unwrap_or_default();
            let comps: Vec<&str> = p.doc.components.iter().map(|c| c.name.as_str()).collect();
            tx.prepare_cached("INSERT INTO prefabs(path, file, base, chain, components, tags) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")
            .map_err(sql_err)?
            .execute(params![
                    p.path,
                    p.file,
                    p.doc.prefab,
                    json!(chain).to_string(),
                    json!(comps).to_string(),
                    json!(p.doc.tags).to_string()
                ])
            .map_err(sql_err)?;
            if let Some(base) = &p.doc.prefab {
                tx.prepare_cached("INSERT INTO refs(scene, from_kind, from_id, root, path, kind, target, at) VALUES (NULL, 'prefab', ?1, NULL, 'prefab', 'prefab', ?2, ?3)")
            .map_err(sql_err)?
            .execute(params![p.path, base, format!("{}:1", p.file)])
            .map_err(sql_err)?;
            }
            for c in &p.doc.components {
                for f in &c.fields {
                    if let Some(s) = f.raw.as_str() {
                        if let Some(kind) = ref_kind(s) {
                            tx.prepare_cached("INSERT INTO refs(scene, from_kind, from_id, root, path, kind, target, at) VALUES (NULL, 'prefab', ?1, NULL, ?2, ?3, ?4, ?5)")
            .map_err(sql_err)?
            .execute(params![p.path, format!("{}.{}", c.name, f.name), kind, s.trim_start_matches('@'), f.at])
            .map_err(sql_err)?;
                        }
                    }
                }
            }
            record_file(&tx, &self.root, &p.file, "prefab", None)?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)",
            params![format!("fp.{}", fp.0), fp.1],
        )
        .map_err(sql_err)?;
        tx.commit().map_err(sql_err)
    }

    /// Def tables and rows.
    pub fn sync_defs(&mut self, defs: &Defs) -> Result<()> {
        let stamps: Vec<(&str, (i64, i64))> = defs
            .tables()
            .filter_map(|t| t.file.as_deref())
            .map(|f| (f, file_stamp(&self.root.join(f))))
            .collect();
        let fp = ("defs", fingerprint(&[defs, &stamps]));
        if self.written(&fp)? {
            return Ok(());
        }
        let tx = self.conn.transaction().map_err(sql_err)?;
        tx.execute_batch("DELETE FROM def_tables; DELETE FROM defs; DELETE FROM refs WHERE from_kind = 'def'; DELETE FROM files WHERE kind = 'def';")
            .map_err(sql_err)?;
        for t in defs.tables() {
            tx.prepare_cached("INSERT INTO def_tables(name, prefix, key_field, columns, file) VALUES (?1, ?2, ?3, ?4, ?5)")
            .map_err(sql_err)?
            .execute(params![t.name, t.prefix, t.key, json!(t.columns()).to_string(), t.file])
            .map_err(sql_err)?;
            for r in &t.rows {
                let row: Vec<Json> = r.values.iter().map(shapes::value_json).collect();
                tx.prepare_cached(
                    "INSERT INTO defs(table_name, key, at, row) VALUES (?1, ?2, ?3, ?4)",
                )
                .map_err(sql_err)?
                .execute(params![t.name, r.key, r.at, Json::Array(row).to_string()])
                .map_err(sql_err)?;
                for (f, v) in t.schema.fields.iter().zip(&r.values) {
                    if let (FieldType::Ref, Value::Ref(s)) = (f.ty, v) {
                        if let Some(kind) = ref_kind(s) {
                            tx.prepare_cached("INSERT INTO refs(scene, from_kind, from_id, root, path, kind, target, at) VALUES (NULL, 'def', ?1, NULL, ?2, ?3, ?4, ?5)")
            .map_err(sql_err)?
            .execute(params![format!("{}:{}", t.prefix, r.key), f.name, kind, s, r.at])
            .map_err(sql_err)?;
                        }
                    }
                }
            }
            if let Some(f) = &t.file {
                record_file(&tx, &self.root, f, "def", None)?;
            }
        }
        tx.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES (?1, ?2)",
            params![format!("fp.{}", fp.0), fp.1],
        )
        .map_err(sql_err)?;
        tx.commit().map_err(sql_err)
    }

    /// True when a sync step last wrote exactly what `fp` describes.
    fn written(&self, fp: &(&str, String)) -> Result<bool> {
        let got: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                params![format!("fp.{}", fp.0)],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql_err)?;
        Ok(got.as_deref() == Some(fp.1.as_str()))
    }

    /// Replace one scene's rows from a resolved scene in memory.
    pub fn sync_scene(&mut self, scene: &Scene) -> Result<usize> {
        let tx = self.conn.transaction().map_err(sql_err)?;
        let name = scene.name.as_str();
        for sql in [
            "DELETE FROM entities WHERE scene = ?1",
            "DELETE FROM components WHERE scene = ?1",
            "DELETE FROM fields WHERE scene = ?1",
            "DELETE FROM refs WHERE scene = ?1",
            "DELETE FROM rooms WHERE scene = ?1",
            "DELETE FROM scenes WHERE name = ?1",
            "DELETE FROM files WHERE scene = ?1",
        ] {
            tx.execute(sql, params![name]).map_err(sql_err)?;
        }
        let seed = match scene.doc.seed {
            SeedPolicy::Fixed(s) => Some(s as i64),
            SeedPolicy::Random => None,
        };
        tx.prepare_cached(
            "INSERT INTO scenes(name, clock, seed, has_rooms, file) VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .map_err(sql_err)?
        .execute(params![
            name,
            scene.doc.clock,
            seed,
            scene.has_rooms() as i64,
            scene.doc.file
        ])
        .map_err(sql_err)?;
        record_file(&tx, &self.root, &scene.doc.file, "scene", Some(name))?;
        let graph = scene.doc.rooms.clone().unwrap_or_default();
        for room in scene.rooms() {
            let neighbors: Vec<&str> = graph
                .edges
                .iter()
                .filter_map(|(a, b)| {
                    if a == &room.name {
                        Some(b.as_str())
                    } else if b == &room.name {
                        Some(a.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            tx.prepare_cached("INSERT INTO rooms(scene, name, file, bounds, neighbors, entities, bulk) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")
            .map_err(sql_err)?
            .execute(params![
                    name,
                    room.name,
                    room.doc.file,
                    room.doc.bounds.map(|b| json!(b).to_string()),
                    json!(neighbors).to_string(),
                    room.entities.len() as i64,
                    json!(room.bulk).to_string()
                ])
            .map_err(sql_err)?;
            if room.doc.file != scene.doc.file {
                record_file(&tx, &self.root, &room.doc.file, "room", Some(name))?;
            }
        }
        let mut n = 0;
        for re in scene.all_resolved() {
            n += write_entity(&tx, scene, re)?;
            if let Some(f) = scene.entity_file(&re.id) {
                record_file(&tx, &self.root, &f.rel, "entity", Some(name))?;
            }
        }
        tx.commit().map_err(sql_err)?;
        Ok(n)
    }

    /// Replace the rows of some top-level entities (children included)
    /// after an edit, without touching the rest of the scene.
    pub fn sync_entities(&mut self, scene: &Scene, ids: &[&str]) -> Result<usize> {
        // File stamps from one directory listing per directory: a stat per
        // file is the slow part on Windows.
        let mut stamps: BTreeMap<String, (i64, i64)> = BTreeMap::new();
        let mut dirs: BTreeSet<PathBuf> = BTreeSet::new();
        for id in ids {
            if let Some(f) = scene.entity_file(id.strip_prefix('@').unwrap_or(id)) {
                if let Some(d) = f.path.parent() {
                    dirs.insert(d.to_path_buf());
                }
            }
        }
        for d in dirs {
            if let Ok(rd) = std::fs::read_dir(&d) {
                for e in rd.flatten() {
                    if let Ok(m) = e.metadata() {
                        let mtime = m
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_nanos() as i64)
                            .unwrap_or(0);
                        stamps.insert(
                            argus_scene::moves::rel(&self.root, &e.path()),
                            (mtime, m.len() as i64),
                        );
                    }
                }
            }
        }
        let tx = self.conn.transaction().map_err(sql_err)?;
        let mut n = 0;
        for id in ids {
            let id = id.strip_prefix('@').unwrap_or(id);
            delete_entity_tree(&tx, &scene.name, id)?;
            if let Some(re) = scene.resolved(id) {
                n += write_entity(&tx, scene, re)?;
                if let Some(f) = scene.entity_file(id) {
                    let (mtime, size) = stamps
                        .get(&f.rel)
                        .copied()
                        .unwrap_or_else(|| file_stamp(&f.path));
                    tx.prepare_cached("INSERT OR REPLACE INTO files(path, kind, scene, mtime, size) VALUES (?1, 'entity', ?2, ?3, ?4)")
                        .map_err(sql_err)?
                        .execute(params![f.rel, scene.name, mtime, size])
                        .map_err(sql_err)?;
                }
            } else if let Some(f) = scene.entity_file(id) {
                tx.prepare_cached("DELETE FROM files WHERE path = ?1")
                    .map_err(sql_err)?
                    .execute(params![f.rel])
                    .map_err(sql_err)?;
            }
        }
        tx.commit().map_err(sql_err)?;
        Ok(n)
    }

    /// Drop an entity (and its children) after a despawn whose file is gone.
    pub fn remove_entity(&mut self, scene: &str, id: &str, file: Option<&str>) -> Result<()> {
        let tx = self.conn.transaction().map_err(sql_err)?;
        let id = id.strip_prefix('@').unwrap_or(id);
        delete_entity_tree(&tx, scene, id)?;
        match file {
            Some(f) => {
                tx.prepare_cached("DELETE FROM files WHERE path = ?1")
                    .map_err(sql_err)?
                    .execute(params![f])
                    .map_err(sql_err)?;
            }
            None => {
                // The file went with the entity: forget its stamp so the
                // scene does not read as stale.
                let local = id.rsplit('/').next().unwrap_or(id);
                tx.prepare_cached("DELETE FROM files WHERE scene = ?1 AND path LIKE '%/' || ?2")
                    .map_err(sql_err)?
                    .execute(params![scene, format!("{local}.toml")])
                    .map_err(sql_err)?;
            }
        }
        tx.commit().map_err(sql_err)
    }

    /// Everything: schemas, systems, prefabs, defs, and every scene. The
    /// scene already open in memory is written from it; the others are
    /// re-indexed only when their files (or any prefab or def) changed.
    pub fn sync_project(
        &mut self,
        project: &Project,
        registry: &Registry,
        systems: &[SystemInfo],
        current: Option<&Scene>,
        force: bool,
    ) -> Result<SyncReport> {
        let start = Instant::now();
        if force {
            self.conn
                .execute("DELETE FROM meta WHERE key LIKE 'fp.%'", [])
                .map_err(sql_err)?;
        }
        let shared_changed = force || self.files_changed("prefab")? || self.files_changed("def")?;
        self.sync_schemas(registry)?;
        self.sync_systems(systems)?;
        self.sync_prefabs(&project.prefabs)?;
        self.sync_defs(&project.defs)?;
        let mut report = SyncReport::default();
        let mut seen = BTreeSet::new();
        for name in project.scenes() {
            seen.insert(name.clone());
            if let Some(cur) = current {
                if cur.name == name {
                    // The loaded scene is written from memory, and only when
                    // something about it could differ from the index.
                    if force || shared_changed || self.is_stale(&name)? {
                        report.entities += self.sync_scene(cur)?;
                        report.scenes.push(name);
                    }
                    continue;
                }
            }
            if !shared_changed && !self.is_stale(&name)? {
                continue;
            }
            match Scene::open(project, &name, registry) {
                Ok(s) => {
                    report.entities += self.sync_scene(&s)?;
                    report.scenes.push(name);
                }
                Err(e) => report.failed.push((name, e.message)),
            }
        }
        // Scenes whose directory disappeared.
        let known = self.scenes()?;
        for name in known {
            if !seen.contains(&name) {
                self.forget_scene(&name)?;
            }
        }
        report.ms = start.elapsed().as_secs_f64() * 1000.0;
        Ok(report)
    }

    fn forget_scene(&mut self, name: &str) -> Result<()> {
        let tx = self.conn.transaction().map_err(sql_err)?;
        for sql in [
            "DELETE FROM entities WHERE scene = ?1",
            "DELETE FROM components WHERE scene = ?1",
            "DELETE FROM fields WHERE scene = ?1",
            "DELETE FROM refs WHERE scene = ?1",
            "DELETE FROM rooms WHERE scene = ?1",
            "DELETE FROM scenes WHERE name = ?1",
            "DELETE FROM files WHERE scene = ?1",
        ] {
            tx.execute(sql, params![name]).map_err(sql_err)?;
        }
        tx.commit().map_err(sql_err)
    }

    /// True if any recorded file of `kind` changed, appeared, or vanished.
    fn files_changed(&self, kind: &str) -> Result<bool> {
        let mut st = self
            .conn
            .prepare("SELECT path, mtime, size FROM files WHERE kind = ?1")
            .map_err(sql_err)?;
        let rows: Vec<(String, i64, i64)> = st
            .query_map(params![kind], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(sql_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql_err)?;
        let dir = match kind {
            "prefab" => "prefabs",
            "def" => "defs",
            _ => return Ok(true),
        };
        let mut on_disk: BTreeMap<String, (i64, i64)> = BTreeMap::new();
        walk_toml(&self.root.join(dir), &self.root, &mut on_disk);
        if kind == "def" {
            on_disk.retain(|p, _| Path::new(p).parent() == Some(Path::new("defs")));
        }
        if on_disk.len() != rows.len() {
            return Ok(true);
        }
        for (path, mtime, size) in rows {
            match on_disk.get(&path) {
                Some(&(m, s)) if m == mtime && s == size => {}
                _ => return Ok(true),
            }
        }
        Ok(false)
    }

    /// True if the scene's files differ from what the index recorded (or
    /// the scene is not indexed).
    pub fn is_stale(&self, scene: &str) -> Result<bool> {
        Ok(self.stale_reason(scene)?.is_some())
    }

    /// Why a scene would be re-indexed, in one sentence, or `None` if it is
    /// current.
    pub fn stale_reason(&self, scene: &str) -> Result<Option<String>> {
        let mut st = self
            .conn
            .prepare("SELECT path, mtime, size FROM files WHERE scene = ?1")
            .map_err(sql_err)?;
        let rows: Vec<(String, i64, i64)> = st
            .query_map(params![scene], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(sql_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql_err)?;
        if rows.is_empty() {
            return Ok(Some("not indexed".to_string()));
        }
        let mut on_disk: BTreeMap<String, (i64, i64)> = BTreeMap::new();
        walk_toml(
            &self.root.join("scenes").join(scene),
            &self.root,
            &mut on_disk,
        );
        on_disk.retain(|p, _| !argus_scene::bulk::is_data_index(p));
        let recorded: BTreeSet<&str> = rows.iter().map(|r| r.0.as_str()).collect();
        if let Some(new) = on_disk.keys().find(|p| !recorded.contains(p.as_str())) {
            return Ok(Some(format!("{new} is new")));
        }
        for (path, mtime, size) in &rows {
            match on_disk.get(path) {
                Some(&(m, s)) if m == *mtime && s == *size => {}
                Some(_) => return Ok(Some(format!("{path} changed"))),
                None => return Ok(Some(format!("{path} is gone"))),
            }
        }
        Ok(None)
    }

    // ----- reads ------------------------------------------------------------

    /// Indexed scene names.
    pub fn scenes(&self) -> Result<Vec<String>> {
        let mut st = self
            .conn
            .prepare("SELECT name FROM scenes ORDER BY name")
            .map_err(sql_err)?;
        let rows = st
            .query_map([], |r| r.get(0))
            .map_err(sql_err)?
            .collect::<std::result::Result<Vec<String>, _>>()
            .map_err(sql_err)?;
        Ok(rows)
    }

    /// True if the scene is indexed.
    pub fn has_scene(&self, name: &str) -> Result<bool> {
        Ok(self.scenes()?.iter().any(|s| s == name))
    }

    fn require_scene(&self, name: &str) -> Result<()> {
        if self.has_scene(name)? {
            Ok(())
        } else {
            Err(ArgusError::new(
                codes::NOT_INDEXED,
                format!("scene {name:?} is not in the index"),
            )
            .hint(format!(
                "Indexed scenes: {}. scene.index rebuilds the index.",
                self.scenes()?.join(", ")
            )))
        }
    }

    /// `world.query` against the index.
    pub fn query(&self, scene: &str, params: &Json) -> Result<Json> {
        self.require_scene(scene)?;
        let src = SceneSource::new(&self.conn, scene)?;
        query::run(&src, params)
    }

    /// `world.get` against the index.
    pub fn get(&self, scene: &str, entity: &str, resolved: bool) -> Result<Json> {
        self.require_scene(scene)?;
        let src = SceneSource::new(&self.conn, scene)?;
        let id = src.lookup(entity)?;
        let sid = src.ids()[id as usize].clone();
        let mut comps = serde_json::Map::new();
        for c in &src.component_sets()[id as usize] {
            comps.insert(c.clone(), src.component(id, c).unwrap_or(json!({})));
        }
        let (prefab, chain, tags, overrides, file, parent): (Option<String>, String, String, Option<String>, String, Option<String>) = self
            .conn
            .query_row(
                "SELECT prefab, chain, tags, overrides, file, parent FROM entities WHERE scene = ?1 AND id = ?2",
                params![scene, sid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )
            .map_err(sql_err)?;
        let mut out = json!({
            "entity": null,
            "id": sid,
            "authored": true,
            "components": comps,
            "source": "index",
        });
        if resolved {
            // Rebuild the provenance block from the field rows.
            let mut st = self
                .conn
                .prepare_cached("SELECT path, value, provenance, at FROM fields WHERE scene = ?1 AND entity = ?2 ORDER BY ord")
                .map_err(sql_err)?;
            let rows: Vec<(String, String, String, String)> = st
                .query_map(params![scene, sid], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
                })
                .map_err(sql_err)?
                .collect::<std::result::Result<_, _>>()
                .map_err(sql_err)?;
            let mut cst = self
                .conn
                .prepare_cached(
                    "SELECT component, provenance FROM components WHERE scene = ?1 AND entity = ?2",
                )
                .map_err(sql_err)?;
            let cprov: BTreeMap<String, String> = cst
                .query_map(params![scene, sid], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(sql_err)?
                .collect::<std::result::Result<_, _>>()
                .map_err(sql_err)?;
            let mut comps = serde_json::Map::new();
            for (path, value, from, at) in rows {
                let (c, f) = path.split_once('.').unwrap_or((&path, ""));
                let entry = comps.entry(c.to_string()).or_insert_with(
                    || json!({ "from": cprov.get(c).cloned().unwrap_or_default(), "fields": {} }),
                );
                entry["fields"][f] = json!({
                    "value": serde_json::from_str::<Json>(&value).unwrap_or(Json::Null),
                    "from": from,
                    "at": at,
                });
            }
            // Tags and the synthetic Parent are not components in the resolved block.
            let tag_names: BTreeSet<String> = serde_json::from_str::<Vec<Json>>(&tags)
                .unwrap_or_default()
                .iter()
                .filter_map(|t| t[0].as_str().map(str::to_string))
                .collect();
            for (c, p) in &cprov {
                if tag_names.contains(c) || c == "Parent" {
                    continue;
                }
                comps
                    .entry(c.clone())
                    .or_insert_with(|| json!({ "from": p, "fields": {} }));
            }
            comps.retain(|c, _| !tag_names.contains(c) && c != "Parent");
            let children: Vec<String> = {
                let mut st = self
                    .conn
                    .prepare_cached(
                        "SELECT id FROM entities WHERE scene = ?1 AND parent = ?2 ORDER BY id",
                    )
                    .map_err(sql_err)?;
                let v: Vec<String> = st
                    .query_map(params![scene, sid], |r| r.get(0))
                    .map_err(sql_err)?
                    .collect::<std::result::Result<_, _>>()
                    .map_err(sql_err)?;
                v
            };
            let chain: Vec<String> = serde_json::from_str(&chain).unwrap_or_default();
            let tags: Json = serde_json::from_str(&tags).unwrap_or(json!([]));
            out["resolved"] = json!({
                "id": sid, "prefab": prefab, "chain": chain, "tags": tags,
                "components": comps, "children": children, "file": file,
            });
        } else if let Some(o) = overrides {
            out["overrides"] = serde_json::from_str(&o).unwrap_or(Json::Null);
        }
        let _ = parent;
        Ok(out)
    }

    /// `world.summary` against the index: counts and names from files, no
    /// live clock.
    pub fn summary(&self, scene: &str) -> Result<Json> {
        self.require_scene(scene)?;
        let src = SceneSource::new(&self.conn, scene)?;
        let (clock, seed, has_rooms): (Option<String>, Option<i64>, i64) = self
            .conn
            .query_row(
                "SELECT clock, seed, has_rooms FROM scenes WHERE name = ?1",
                params![scene],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(sql_err)?;
        let ord: BTreeMap<String, i64> = {
            let mut st = self
                .conn
                .prepare("SELECT name, ord FROM schemas")
                .map_err(sql_err)?;
            let v: BTreeMap<String, i64> = st
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(sql_err)?
                .collect::<std::result::Result<_, _>>()
                .map_err(sql_err)?;
            v
        };
        let mut groups: BTreeMap<Vec<String>, usize> = BTreeMap::new();
        let mut named = Vec::new();
        for (i, set) in src.component_sets().iter().enumerate() {
            let mut comps: Vec<String> = set.iter().cloned().collect();
            comps.sort_by_key(|c| ord.get(c).copied().unwrap_or(i64::MAX));
            *groups.entry(comps.clone()).or_default() += 1;
            if named.len() < 50 {
                named.push((src.ids()[i].clone(), comps));
            }
        }
        let mut archetypes: Vec<(Vec<String>, usize)> = groups.into_iter().collect();
        archetypes.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let rooms: Vec<String> = if has_rooms != 0 {
            let mut st = self
                .conn
                .prepare("SELECT name FROM rooms WHERE scene = ?1 ORDER BY name")
                .map_err(sql_err)?;
            let v: Vec<String> = st
                .query_map(params![scene], |r| r.get(0))
                .map_err(sql_err)?
                .collect::<std::result::Result<_, _>>()
                .map_err(sql_err)?;
            v
        } else {
            vec![String::new()]
        };
        let systems = self.systems_by_enabled()?;
        let components: i64 = self
            .conn
            .query_row("SELECT count(*) FROM schemas", [], |r| r.get(0))
            .map_err(sql_err)?;
        Ok(Summary {
            frame: 0,
            tick: 0,
            time: 0.0,
            seed: seed.unwrap_or(0) as u64,
            clock: clock.unwrap_or_default(),
            playing: false,
            paused: false,
            scene: Some(scene.to_string()),
            rooms,
            entities: src.len(),
            authored: src.len(),
            components: components as usize,
            archetypes,
            named_total: src.len(),
            named,
            systems: systems.0,
            disabled: systems.1,
            pending_events: 0,
            source: "index",
        }
        .to_json())
    }

    fn systems_by_enabled(&self) -> Result<(Vec<String>, Vec<String>)> {
        let mut st = self
            .conn
            .prepare("SELECT name, enabled FROM systems ORDER BY ord")
            .map_err(sql_err)?;
        let rows: Vec<(String, i64)> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(sql_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql_err)?;
        let (mut on, mut off) = (Vec::new(), Vec::new());
        for (n, e) in rows {
            if e != 0 {
                on.push(n)
            } else {
                off.push(n)
            }
        }
        Ok((on, off))
    }

    /// `world.describe` against the index.
    pub fn describe(&self, params: &Json) -> Result<Json> {
        let only = query::opt_str(params, "component")?;
        let mut st = self
            .conn
            .prepare("SELECT name, doc, version, source, kind, fields FROM schemas ORDER BY ord")
            .map_err(sql_err)?;
        let rows: Vec<(String, String, i64, Option<String>, String, String)> = st
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })
            .map_err(sql_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql_err)?;
        if let Some(o) = only {
            if !rows.iter().any(|r| r.0 == o) {
                return Err(ArgusError::new(
                    argus_ecs::codes::UNKNOWN_COMPONENT,
                    format!("no component named {o:?}"),
                ));
            }
        }
        let mut out = Vec::new();
        for (name, doc, version, source, kind, fields) in rows {
            if only.is_some_and(|o| o != name) {
                continue;
            }
            let ww = self.who_writes(&name)?;
            out.push(json!({
                "name": name, "doc": doc, "version": version, "source": source, "kind": kind,
                "fields": serde_json::from_str::<Json>(&fields).unwrap_or(json!([])),
                "written_by": ww["systems"], "read_by": ww["readers"],
            }));
        }
        let (limit, offset) = query::page(params)?;
        Ok(query::paged(out, limit, offset))
    }

    /// `world.who_writes` against the index.
    pub fn who_writes(&self, component: &str) -> Result<Json> {
        let mut st = self
            .conn
            .prepare("SELECT name, reads, writes FROM systems ORDER BY ord")
            .map_err(sql_err)?;
        let rows: Vec<(String, String, String)> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(sql_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql_err)?;
        let has = |list: &str| {
            serde_json::from_str::<Vec<String>>(list)
                .unwrap_or_default()
                .iter()
                .any(|c| c == component)
        };
        let systems: Vec<&str> = rows
            .iter()
            .filter(|r| has(&r.2))
            .map(|r| r.0.as_str())
            .collect();
        // A writer reads too (Access::allows_read), so readers include writers,
        // as the live who_reads does.
        let readers: Vec<&str> = rows
            .iter()
            .filter(|r| has(&r.1) || has(&r.2))
            .map(|r| r.0.as_str())
            .collect();
        Ok(json!({ "component": component, "systems": systems, "readers": readers }))
    }

    /// `world.defs` against the index.
    pub fn defs(&self, params: &Json) -> Result<Json> {
        let table = query::opt_str(params, "table")?;
        let mut st = self
            .conn
            .prepare("SELECT name, prefix, key_field, columns, file FROM def_tables ORDER BY name")
            .map_err(sql_err)?;
        let tables: Vec<(String, String, String, String, Option<String>)> = st
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .map_err(sql_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql_err)?;
        let Some(table) = table else {
            let mut out = Vec::new();
            for (name, prefix, key, columns, file) in &tables {
                let n: i64 = self
                    .conn
                    .query_row(
                        "SELECT count(*) FROM defs WHERE table_name = ?1",
                        params![name],
                        |r| r.get(0),
                    )
                    .map_err(sql_err)?;
                out.push(json!({ "name": name, "prefix": prefix, "key": key, "rows": n, "columns": serde_json::from_str::<Json>(columns).unwrap_or(json!([])), "file": file }));
            }
            return Ok(json!({ "tables": out }));
        };
        let Some(t) = tables.iter().find(|t| t.0 == table) else {
            return Err(ArgusError::new(
                argus_scene::codes::BAD_DEF,
                format!("no def table {table:?}"),
            )
            .hint(format!(
                "Tables: {}",
                tables
                    .iter()
                    .map(|t| t.0.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        };
        let columns: Vec<String> = serde_json::from_str(&t.3).unwrap_or_default();
        let mut where_ = Vec::new();
        if let Some(Json::Object(o)) = params.get("where") {
            for (k, v) in o {
                let i = columns.iter().position(|c| c == k).ok_or_else(|| {
                    ArgusError::new(
                        argus_ecs::codes::UNKNOWN_FIELD,
                        format!("{table} has no field {k:?}"),
                    )
                    .hint(format!("Fields: {}", columns.join(", ")))
                })?;
                where_.push((i, v.clone()));
            }
        }
        let mut st = self
            .conn
            .prepare("SELECT row FROM defs WHERE table_name = ?1 ORDER BY rowid")
            .map_err(sql_err)?;
        let rows: Vec<Json> = st
            .query_map(params![table], |r| r.get::<_, String>(0))
            .map_err(sql_err)?
            .filter_map(|r| r.ok())
            .filter_map(|s| serde_json::from_str::<Json>(&s).ok())
            .filter(|row| {
                where_.iter().all(|(i, v)| {
                    let cell = &row[*i];
                    match (cell.as_f64(), v.as_f64()) {
                        (Some(a), Some(b)) => a == b,
                        _ => cell == v,
                    }
                })
            })
            .collect();
        let (limit, offset) = query::page(params)?;
        let mut out = query::paged(rows, limit, offset);
        out["table"] = json!(table);
        out["columns"] = json!(columns);
        Ok(out)
    }

    /// `scene.rooms` against the index.
    pub fn rooms(&self, scene: &str) -> Result<Json> {
        self.require_scene(scene)?;
        let mut st = self
            .conn
            .prepare("SELECT name, bounds, neighbors, entities, bulk FROM rooms WHERE scene = ?1 ORDER BY name")
            .map_err(sql_err)?;
        let rows: Vec<Json> = st
            .query_map(params![scene], |r| {
                Ok(json!({
                    "name": r.get::<_, String>(0)?,
                    "loaded": false,
                    "entities": r.get::<_, i64>(3)?,
                    "neighbors": serde_json::from_str::<Json>(&r.get::<_, String>(2)?).unwrap_or(json!([])),
                    "bounds": r.get::<_, Option<String>>(1)?.and_then(|b| serde_json::from_str::<Json>(&b).ok()),
                    "tileset": null,
                    "bulk": serde_json::from_str::<Json>(&r.get::<_, String>(4)?).unwrap_or(json!([])),
                }))
            })
            .map_err(sql_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql_err)?;
        Ok(
            json!({ "rooms": rows, "current": null, "radius": null, "start": null, "source": "index" }),
        )
    }

    /// Every instance of a prefab across all scenes: `(scene, id)`.
    pub fn instances_of(&self, prefab: &str) -> Result<Vec<(String, String)>> {
        let mut st = self
            .conn
            .prepare("SELECT scene, id, chain FROM entities WHERE parent IS NULL OR prefab IS NOT NULL ORDER BY scene, id")
            .map_err(sql_err)?;
        let rows = st
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(sql_err)?;
        let mut out = Vec::new();
        for row in rows {
            let (scene, id, chain) = row.map_err(sql_err)?;
            let chain: Vec<String> = serde_json::from_str(&chain).unwrap_or_default();
            if chain.iter().any(|p| p == prefab) {
                out.push((scene, id));
            }
        }
        Ok(out)
    }

    /// Everything that references `target` (an entity id, prefab path, def
    /// ref, or file path): rows of `[scene, from_kind, from, path, kind, at]`.
    pub fn references_to(&self, target: &str, params: &Json) -> Result<Json> {
        let t = target.strip_prefix('@').unwrap_or(target);
        let mut st = self
            .conn
            .prepare("SELECT scene, from_kind, from_id, path, kind, at FROM refs WHERE target = ?1 ORDER BY scene, from_id, path")
            .map_err(sql_err)?;
        let rows: Vec<Json> = st
            .query_map(params![t], |r| {
                Ok(json!([
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?
                ]))
            })
            .map_err(sql_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql_err)?;
        let (limit, offset) = query::page(params)?;
        let mut out = query::paged(rows, limit, offset);
        out["columns"] = json!(["scene", "from_kind", "from", "path", "kind", "at"]);
        out["target"] = json!(t);
        Ok(out)
    }

    /// Full-text search over component, field, and system doc strings.
    pub fn search(&self, text: &str, params: &Json) -> Result<Json> {
        let mut st = self
            .conn
            .prepare("SELECT kind, name, doc FROM docs WHERE docs MATCH ?1 ORDER BY rank")
            .map_err(sql_err)?;
        let q = fts_query(text);
        let rows: Vec<Json> = st
            .query_map(params![q], |r| {
                Ok(json!([
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?
                ]))
            })
            .map_err(sql_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql_err)?;
        let (limit, offset) = query::page(params)?;
        let mut out = query::paged(rows, limit, offset);
        out["columns"] = json!(["kind", "name", "doc"]);
        Ok(out)
    }

    /// Counts per table, for `scene.index` and diagnostics.
    pub fn stats(&self) -> Result<Json> {
        let mut out = serde_json::Map::new();
        for t in [
            "scenes",
            "rooms",
            "entities",
            "components",
            "fields",
            "refs",
            "prefabs",
            "def_tables",
            "defs",
            "schemas",
            "systems",
            "files",
        ] {
            let n: i64 = self
                .conn
                .query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
                .map_err(sql_err)?;
            out.insert(t.to_string(), json!(n));
        }
        out.insert(
            "path".into(),
            json!(Self::path(&self.root).to_string_lossy()),
        );
        Ok(Json::Object(out))
    }
}

/// Quote every word so punctuation in a search never breaks FTS syntax.
fn fts_query(text: &str) -> String {
    text.split_whitespace()
        .map(|w| format!("\"{}\"", w.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" ")
}

fn ref_kind(s: &str) -> Option<&'static str> {
    if s.is_empty() {
        None
    } else if s.starts_with('@') {
        Some("entity")
    } else if s.contains(':') {
        Some("def")
    } else if s.contains('/') || s.contains('.') {
        Some("path")
    } else {
        None
    }
}

fn record_file(
    tx: &Transaction<'_>,
    root: &Path,
    rel: &str,
    kind: &str,
    scene: Option<&str>,
) -> Result<()> {
    let (mtime, size) = file_stamp(&root.join(rel));
    tx.prepare_cached(
        "INSERT OR REPLACE INTO files(path, kind, scene, mtime, size) VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .map_err(sql_err)?
    .execute(params![rel, kind, scene, mtime, size])
    .map_err(sql_err)?;
    Ok(())
}

/// What a sync step would write, as a hash of its inputs' contents. Every
/// scene load syncs the index, and rewriting the unchanged schemas, systems,
/// prefabs, and defs was a third of a reload once phase 1 had added its
/// components (found after the phase-1 gate, timing Delve's seed sweeps).
fn fingerprint(parts: &[&dyn std::fmt::Debug]) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for p in parts {
        format!("{p:?}").hash(&mut h);
    }
    format!("{:016x}", h.finish())
}

fn file_stamp(p: &Path) -> (i64, i64) {
    match std::fs::metadata(p) {
        Ok(m) => {
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos() as i64)
                .unwrap_or(0);
            (mtime, m.len() as i64)
        }
        Err(_) => (0, -1),
    }
}

fn walk_toml(dir: &Path, root: &Path, out: &mut BTreeMap<String, (i64, i64)>) {
    // Types and stamps come from the directory listing itself (free on
    // Windows); a stat per file was slower by a third over 10,000 files.
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else {
            continue;
        };
        if ft.is_dir() {
            walk_toml(&p, root, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("toml") {
            let rel = argus_scene::moves::rel(root, &p);
            let stamp = match e.metadata() {
                Ok(m) => (
                    m.modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_nanos() as i64)
                        .unwrap_or(0),
                    m.len() as i64,
                ),
                Err(_) => file_stamp(&p),
            };
            out.insert(rel, stamp);
        }
    }
}

fn delete_entity_tree(tx: &Transaction<'_>, scene: &str, id: &str) -> Result<()> {
    // The tree's ids come from the root index on entities; every row is
    // then removed by its clustered key. (Deleting entities by root through
    // the secondary index was measured 100x slower than by primary key.)
    let ids: Vec<String> = {
        let mut st = tx
            .prepare_cached("SELECT id FROM entities WHERE scene = ?1 AND root = ?2")
            .map_err(sql_err)?;
        let v: Vec<String> = st
            .query_map(params![scene, id], |r| r.get(0))
            .map_err(sql_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql_err)?;
        v
    };
    for eid in &ids {
        for sql in [
            "DELETE FROM components WHERE scene = ?1 AND entity = ?2",
            "DELETE FROM fields WHERE scene = ?1 AND entity = ?2",
            "DELETE FROM entities WHERE scene = ?1 AND id = ?2",
        ] {
            tx.prepare_cached(sql)
                .map_err(sql_err)?
                .execute(params![scene, eid])
                .map_err(sql_err)?;
        }
    }
    tx.prepare_cached("DELETE FROM refs WHERE scene = ?1 AND root = ?2")
        .map_err(sql_err)?
        .execute(params![scene, id])
        .map_err(sql_err)?;
    Ok(())
}

/// Insert rows one statement each through the statement cache; measured
/// faster than multi-row inserts on this schema.
fn insert_rows(
    tx: &Transaction<'_>,
    prefix: &str,
    width: usize,
    rows: &[rusqlite::types::Value],
) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let sql = format!("{prefix} ({})", vec!["?"; width].join(","));
    let mut st = tx.prepare_cached(&sql).map_err(sql_err)?;
    for row in rows.chunks(width) {
        st.execute(rusqlite::params_from_iter(row.iter()))
            .map_err(sql_err)?;
    }
    Ok(())
}

/// Write one resolved entity and its children; returns how many rows of
/// `entities` were written.
fn write_entity(tx: &Transaction<'_>, scene: &Scene, re: &ResolvedEntity) -> Result<usize> {
    use rusqlite::types::Value as Sv;
    let sname = scene.name.as_str();
    let root = re.id.as_str();
    let mut n = 0;
    let text = |s: &str| Sv::Text(s.to_string());
    for (e, parent) in re.flatten() {
        n += 1;
        let room = scene.room_of(&e.id).map(str::to_string);
        let overrides = scene
            .overrides(&e.id)
            .map(|d| shapes::overrides_json(d).to_string());
        let tags: Vec<Json> = e
            .tags
            .iter()
            .map(|(t, p)| json!([t, p.to_string()]))
            .collect();
        tx.prepare_cached("INSERT OR REPLACE INTO entities(scene, id, root, room, file, parent, prefab, chain, tags, overrides) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)")
            .map_err(sql_err)?
            .execute(params![
                sname,
                e.id,
                root,
                room,
                e.file,
                parent,
                e.prefab,
                json!(e.chain).to_string(),
                Json::Array(tags).to_string(),
                overrides
            ])
            .map_err(sql_err)?;
        let mut comps: Vec<Sv> = Vec::new();
        let mut fields: Vec<Sv> = Vec::new();
        let mut refs: Vec<Sv> = Vec::new();
        for (t, p) in &e.tags {
            comps.extend([text(sname), text(&e.id), text(t), text(&p.to_string())]);
        }
        if let Some(p) = parent {
            comps.extend([text(sname), text(&e.id), text("Parent"), text("own")]);
            fields.extend([
                text(sname),
                text(&e.id),
                text("Parent.entity"),
                Sv::Integer(0),
                text(&json!(format!("@{p}")).to_string()),
                text("own"),
                text(&e.file),
            ]);
        }
        let mut ord = 1i64;
        for c in &e.components {
            comps.extend([
                text(sname),
                text(&e.id),
                text(&c.name),
                text(&c.from.to_string()),
            ]);
            for f in &c.fields {
                let value = match (&f.ty, &f.value) {
                    (FieldType::Entity, Value::Str(s)) => entity_target(scene, room.as_deref(), s),
                    (_, v) => shapes::value_json(v),
                };
                let path = format!("{}.{}", c.name, f.name);
                fields.extend([
                    text(sname),
                    text(&e.id),
                    text(&path),
                    Sv::Integer(ord),
                    text(&value.to_string()),
                    text(&f.from.to_string()),
                    text(&f.at),
                ]);
                ord += 1;
                let r = match (&f.ty, &f.value) {
                    (FieldType::Entity, Value::Str(s)) if !s.is_empty() && s != "e:null" => value
                        .as_str()
                        .map(|t| ("entity", t.trim_start_matches('@').to_string())),
                    (FieldType::Ref, Value::Ref(s)) => ref_kind(s).map(|k| (k, s.clone())),
                    _ => None,
                };
                if let Some((kind, target)) = r {
                    refs.extend([
                        text(sname),
                        text("entity"),
                        text(&e.id),
                        text(root),
                        text(&path),
                        text(kind),
                        text(&target),
                        text(&f.at),
                    ]);
                }
            }
        }
        for p in &e.chain {
            refs.extend([
                text(sname),
                text("entity"),
                text(&e.id),
                text(root),
                text("prefab"),
                text("prefab"),
                text(p),
                text(&format!("{}:1", e.file)),
            ]);
        }
        insert_rows(
            tx,
            "INSERT OR REPLACE INTO components(scene, entity, component, provenance) VALUES",
            4,
            &comps,
        )?;
        insert_rows(
            tx,
            "INSERT OR REPLACE INTO fields(scene, entity, path, ord, value, provenance, at) VALUES",
            7,
            &fields,
        )?;
        insert_rows(
            tx,
            "INSERT INTO refs(scene, from_kind, from_id, root, path, kind, target, at) VALUES",
            8,
            &refs,
        )?;
    }
    Ok(n)
}

/// The protocol spelling of an entity reference as the live world would
/// report it: the full id (`@room/id`), or null.
fn entity_target(scene: &Scene, room: Option<&str>, s: &str) -> Json {
    if s.is_empty() || s == "e:null" || s == "@" {
        return Json::Null;
    }
    if s.starts_with("e:") {
        return json!(s);
    }
    let bare = s.strip_prefix('@').unwrap_or(s);
    if let Some(r) = room {
        let q = format!("{r}/{bare}");
        if scene.resolved(&q).is_some() {
            return json!(format!("@{q}"));
        }
    }
    if scene.resolved(bare).is_some() {
        return json!(format!("@{bare}"));
    }
    json!(format!("@{bare}"))
}
