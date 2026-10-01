//! The tuning log (§3 `tune = true`, §10 "Tuning sessions", phase 2 item
//! 5). Every change to a tune field is logged with its frame, so the agent
//! sees the search path and not only the final value: "human set
//! HitStop.scale 0→0.15 at frame 3011, →0.08 at 3090, →0.1 at 3140".
//!
//! Changes come three ways until the editor's slider panel (phase 4):
//! `tune.set`, the slider, which changes the live world only and belongs to
//! a tuning session; `edit.set` commits, and proposals picked; and undo or
//! redo of those. Previews and a test's edits are not changes anyone made,
//! and are not logged.
//!
//! A tuning session opens with `tune.begin` (or the first `tune.set`) and
//! ends with `tune.end`, which commits the final values as one ordinary
//! transaction with the session's author: one row in the feed, one undo
//! that goes back to where the session started. Closing the game's window
//! ends an open session the same way.
//!
//! The log is `.argus/tuning.jsonl`, one JSON object a line: a change
//! (`set`), a session begun or ended. It is on disk because the human
//! tunes, closes the game, and the agent reads the log in another daemon.
//! `obs.subscribe {topics: ["tune"]}` streams each row as `tune.changed`.

use super::*;
use argus_ecs::FieldSchema;
use std::collections::BTreeMap as Map;
use std::io::Write as _;

/// Runtime entities of one component listed one by one in `tune.fields`
/// (Outpost's job kinds); more, and they are one row with their count
/// (Swarm's enemies), tuned through what they are made from.
const RUNTIME_ROWS: usize = 32;

/// An open tuning session.
pub(super) struct TuneSession {
    /// Its number, counting sessions in the log.
    pub id: u64,
    /// Who is tuning (the transaction's author at the end).
    pub author: String,
    /// Why.
    pub note: String,
    /// Each field's value when the session first touched it, by (entity, path).
    pub start: Map<(String, String), Json>,
    /// Each field's value now.
    pub now: Map<(String, String), Json>,
}

fn log_path(root: &Path) -> PathBuf {
    root.join(".argus").join("tuning.jsonl")
}

/// Every row of the log, oldest first.
fn rows(root: &Path) -> Vec<Json> {
    std::fs::read_to_string(log_path(root))
        .map(|t| {
            t.lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect()
        })
        .unwrap_or_default()
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A number as the story tells it: no trailing zeros, no float noise.
fn show(v: &Json) -> String {
    match v.as_f64() {
        Some(x) if v.is_f64() => {
            let r = (x * 1e4).round() / 1e4;
            format!("{r}")
        }
        _ => v.to_string(),
    }
}

/// The search path of each field, one line each, in the design's words.
fn stories(rows: &[Json]) -> Vec<String> {
    // (author, entity, path) -> "from→to at frame n, →to at m, ...", in the
    // order each was first touched.
    let mut order: Vec<(String, String, String)> = Vec::new();
    let mut lines: Map<(String, String, String), String> = Map::new();
    for r in rows.iter().filter(|r| r["kind"] == "set") {
        let key = (
            r["author"].as_str().unwrap_or("?").to_string(),
            r["entity"].as_str().unwrap_or("?").to_string(),
            r["path"].as_str().unwrap_or("?").to_string(),
        );
        let step = format!("→{} at frame {}", show(&r["to"]), r["frame"]);
        match lines.get_mut(&key) {
            Some(l) => {
                l.push_str(", ");
                l.push_str(&step);
            }
            None => {
                order.push(key.clone());
                lines.insert(
                    key.clone(),
                    format!(
                        "{} set {} {} {}{}",
                        key.0,
                        key.1,
                        key.2,
                        show(&r["from"]),
                        step
                    ),
                );
            }
        }
    }
    order.into_iter().filter_map(|k| lines.remove(&k)).collect()
}

impl Daemon {
    /// Append a row to the log and stream it to subscribers.
    pub(super) fn tune_row(&mut self, mut row: Json) {
        let Some(root) = self.session.as_ref().map(|s| s.root.clone()) else {
            return;
        };
        row["at"] = json!(unix_now());
        row["frame"] = json!(self.world.frame());
        let path = log_path(&root);
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(f, "{row}");
        }
        self.notify("tune", "tune.changed", row);
    }

    /// Log the tune fields a commit changed (`edit.set`, a picked proposal,
    /// undo, redo). `changes`: entity, path, value before, value after.
    pub(super) fn log_tuned(
        &mut self,
        author: &str,
        via: &str,
        note: &str,
        changes: Vec<(String, String, Json, Json)>,
    ) {
        for (entity, path, from, to) in changes {
            if from == to {
                continue;
            }
            self.tune_row(json!({
                "kind": "set", "author": author, "entity": entity, "path": path,
                "from": from, "to": to, "via": via, "note": note,
            }));
        }
    }

    /// Is `path` a tune field?
    pub(super) fn is_tune(&self, path: &str) -> bool {
        self.world
            .registry()
            .resolve_path(path)
            .is_ok_and(|(_, f)| f.tune)
    }

    /// `tune.fields`: the tune fields with their values, what the editor's
    /// tuning panel shows (§10): every one of an entity; or of every entity
    /// with an id, of the runtime entities (one by one while there are few
    /// of a component, else one row with their count), and of every def
    /// table's rows (phase 4 item 3: Outpost's food and job priorities).
    pub(super) fn tune_fields(&self, params: &Json) -> Result<Json> {
        self.require_session()?;
        let one = json::opt_str(params, "entity")?;
        let ents: Vec<Entity> = match one {
            Some(id) => vec![self.world.entity(id)?],
            None => {
                let mut v: Vec<Entity> = self.world.live().collect();
                v.sort();
                v
            }
        };
        let mut rows = Vec::new();
        let mut runtime: Map<String, Vec<Entity>> = Map::new();
        for e in ents {
            let authored = one.is_some() || self.world.string_id(e).is_some();
            let Ok(comps) = self.world.components_of(e) else {
                continue;
            };
            for c in comps {
                let s = self.world.registry().schema(*c);
                if !s.fields.iter().any(|f| f.tune) {
                    continue;
                }
                if authored {
                    let label = json::entity_str(&self.world, e);
                    self.tune_rows(e, &s.name, &label, false, &mut rows);
                } else {
                    runtime.entry(s.name.clone()).or_default().push(e);
                }
            }
        }
        for (comp, es) in &runtime {
            if es.len() <= RUNTIME_ROWS {
                for e in es {
                    let label = self.tune_label(*e, comp);
                    self.tune_rows(*e, comp, &label, true, &mut rows);
                }
                continue;
            }
            let Ok(cid) = self.world.registry().require(comp) else {
                continue;
            };
            let s = self.world.registry().schema(cid);
            for f in s.fields.iter().filter(|f| f.tune) {
                rows.push(json!({
                    "component": comp,
                    "path": format!("{comp}.{}", f.name),
                    "runtime": es.len(),
                    "label": format!("{} {comp} entities", es.len()),
                    "default": json::to_json(&self.world, &f.default),
                    "range": f.range,
                    "doc": f.doc,
                }));
            }
        }
        if one.is_none() {
            let ses = self.require_session()?;
            for t in ses.project.defs.tables() {
                for f in t.schema.fields.iter().filter(|f| f.tune) {
                    for row in &t.rows {
                        rows.push(json!({
                            "def": format!("{}:{}", t.prefix, row.key),
                            "path": f.name,
                            "value": t.value(row, &f.name).map(|v| json::to_json(&self.world, v)),
                            "default": json::to_json(&self.world, &f.default),
                            "range": f.range,
                            "doc": f.doc,
                            "label": format!("{} {}", t.name, row.key),
                            "table": t.name,
                        }));
                    }
                }
            }
        }
        Ok(json!({ "total": rows.len(), "rows": rows, "session": self.tune_session_json() }))
    }

    /// The rows of one entity's tune fields in one component.
    fn tune_rows(&self, e: Entity, comp: &str, label: &str, runtime: bool, rows: &mut Vec<Json>) {
        let Ok(cid) = self.world.registry().require(comp) else {
            return;
        };
        let s = self.world.registry().schema(cid);
        for f in s.fields.iter().filter(|f| f.tune) {
            let path = format!("{}.{}", s.name, f.name);
            let value = self.world.get_field(e, &path).ok();
            let mut r = json!({
                "entity": json::entity_str(&self.world, e),
                "path": path,
                "value": value.map(|v| json::to_json(&self.world, &v)),
                "default": json::to_json(&self.world, &f.default),
                "range": f.range,
                "doc": f.doc,
                "label": label,
            });
            if runtime {
                r["runtime"] = json!(true);
            }
            rows.push(r);
        }
    }

    /// What to call a runtime entity: its component's first word it holds
    /// (a `JobKind`'s name), else what `Describe` says, else its reference.
    fn tune_label(&self, e: Entity, comp: &str) -> String {
        let eref = json::entity_str(&self.world, e);
        if let Ok(cid) = self.world.registry().require(comp) {
            let s = self.world.registry().schema(cid);
            for f in &s.fields {
                if !matches!(f.ty, argus_ecs::FieldType::Str) {
                    continue;
                }
                if let Ok(v) = self.world.get_field(e, &format!("{comp}.{}", f.name)) {
                    if let Some(w) = json::to_json(&self.world, &v)
                        .as_str()
                        .filter(|w| !w.is_empty())
                    {
                        return format!("{comp} {w}");
                    }
                }
            }
        }
        if let Ok(v) = self.world.get_field(e, "Describe.name") {
            if let Some(w) = json::to_json(&self.world, &v)
                .as_str()
                .filter(|w| !w.is_empty())
            {
                return w.to_string();
            }
        }
        eref
    }

    /// A def row's value and its field's schema.
    pub(super) fn def_field(&self, def: &str, path: &str) -> Result<(Value, FieldSchema)> {
        let ses = self.require_session()?;
        let v = ses.project.defs.value_of(def, path)?;
        let (t, _) = ses.project.defs.resolve(def).expect("value_of found it");
        let fs = t
            .schema
            .fields
            .iter()
            .find(|f| f.name == path)
            .cloned()
            .expect("value_of found it");
        Ok((v, fs))
    }

    /// Is this target a def row (`item:meal`) rather than an entity?
    pub(super) fn is_def(&self, target: &str) -> bool {
        !target.starts_with('@')
            && self
                .session
                .as_ref()
                .is_some_and(|s| s.project.defs.is_def_ref(target))
    }

    /// Set a def row's value in memory and give it to everything that reads
    /// the tables; the value before and after, as JSON.
    pub(super) fn set_def_live(
        &mut self,
        def: &str,
        path: &str,
        value: &Json,
    ) -> Result<(Json, Json)> {
        let (_, fs) = self.def_field(def, path)?;
        let v = json::from_json(&self.world, value, fs.ty)?;
        let ses = self.session.as_mut().expect("def_field needs a session");
        let old = ses.project.defs.set_value(def, path, v)?;
        let new = ses.project.defs.value_of(def, path)?;
        self.defs_changed();
        self.mutations += 1;
        Ok((
            json::to_json(&self.world, &old),
            json::to_json(&self.world, &new),
        ))
    }

    /// The def tables changed in memory: scripts (`q:def`), conversations,
    /// and timelines read them again, as after `asset.move`.
    pub(super) fn defs_changed(&mut self) {
        let Some(ses) = self.session.as_mut() else {
            return;
        };
        ses.host
            .set_defs(Rc::new(argus_player::ProjectDefs(ses.project.defs.clone())));
        *ses.dialogue.borrow_mut() = argus_dialogue::Graph::from_defs(&ses.project.defs);
        *ses.timelines.borrow_mut() = argus_player::read_timelines(&ses.project.defs, &self.world);
    }

    /// Apply an `edit.def_row` op (or its inverse, the table's file as it
    /// was): the row written into its table's file, the tables read again,
    /// and scripts, conversations, and timelines given them. A row the
    /// tables refuse (a field of the wrong kind), or one that leaves a
    /// timeline a problem it did not have (a cue on no timeline), is put
    /// back and the commit fails with the problem and its line.
    pub(super) fn apply_def_row(
        &mut self,
        author: &str,
        op: &Op,
        touched: &mut BTreeSet<String>,
        files: &mut BTreeSet<String>,
    ) -> Result<Option<Op>> {
        let root = self.require_session()?.root.clone();
        let (file, old, what) = match op {
            Op::DefRow { def, fields } => {
                self.edits.check_lock(author, def, "")?;
                let (prefix, key) = def.split_once(':').unwrap_or(("", def));
                let ses = self.require_session()?;
                let table = ses
                    .project
                    .defs
                    .table_for_prefix(prefix)
                    .cloned()
                    .ok_or_else(|| bad_params(format!("no def table with prefix {prefix:?}")))?;
                let values = match fields {
                    None => None,
                    Some(Json::Object(o)) => {
                        let mut out = Vec::new();
                        for (k, v) in o {
                            if *k == table.key {
                                continue;
                            }
                            let fs = table
                                .schema
                                .fields
                                .iter()
                                .find(|f| f.name == *k)
                                .ok_or_else(|| {
                                    bad_params(format!("{} has no field {k:?}", table.name))
                                        .hint(format!("Fields: {}", table.columns().join(", ")))
                                })?;
                            out.push((k.clone(), json::from_json(&self.world, v, fs.ty)?));
                        }
                        Some(out)
                    }
                    Some(_) => return Err(bad_params("fields is an object")),
                };
                let (file, old) =
                    argus_scene::defs::write_row(&root, &table, key, values.as_deref())?;
                (file, old, def.clone())
            }
            Op::DefText { file, text } => {
                let old = std::fs::read_to_string(root.join(file)).unwrap_or_default();
                std::fs::write(root.join(file), text)
                    .map_err(|e| ArgusError::new(codes::BAD_PARAMS, format!("{file}: {e}")))?;
                (file.clone(), old, file.clone())
            }
            _ => return Ok(None),
        };
        // What the tables and the timelines made of it.
        let problems = |d: &Daemon| -> Vec<String> {
            let Some(ses) = d.session.as_ref() else {
                return Vec::new();
            };
            let mut out: Vec<String> = ses
                .project
                .issues()
                .iter()
                .filter(|e| {
                    e.code == argus_scene::codes::BAD_DEF
                        || e.code == argus_scene::codes::MISSING_REFERENCE
                })
                .map(|e| format!("{} ({})", e.message, e.r#ref.clone().unwrap_or_default()))
                .collect();
            out.extend(
                ses.timelines
                    .borrow()
                    .problems
                    .iter()
                    .map(|(at, m)| format!("{m} ({at})")),
            );
            out
        };
        let before = problems(self);
        self.session
            .as_mut()
            .expect("session")
            .project
            .reload_defs();
        self.defs_changed();
        let new: Vec<String> = problems(self)
            .into_iter()
            .filter(|p| !before.contains(p))
            .collect();
        if let Some(first) = new.first() {
            let _ = std::fs::write(root.join(&file), &old);
            self.session
                .as_mut()
                .expect("session")
                .project
                .reload_defs();
            self.defs_changed();
            return Err(ArgusError::new(
                codes::BAD_PARAMS,
                format!("{what} would not read: {first}"),
            )
            .hint("Nothing was kept; the table is as it was."));
        }
        files.insert(file.clone());
        touched.insert(what);
        Ok(Some(Op::DefText { file, text: old }))
    }

    /// Apply an `edit.def` op: the value in memory, and in the table's file
    /// unless a test's live edits; its inverse.
    pub(super) fn apply_def(
        &mut self,
        author: &str,
        op: &Op,
        touched: &mut BTreeSet<String>,
        files: &mut BTreeSet<String>,
    ) -> Result<Option<Op>> {
        let Op::Def { def, path, value } = op else {
            return Ok(None);
        };
        self.edits.check_lock(author, def, path)?;
        let before = self.timeline_problems();
        let (old, _) = self.set_def_live(def, path, value)?;
        // A cue's field a timeline cannot read (effects that do not parse,
        // a kind there is not): put back, and the commit fails with why.
        let new: Vec<String> = self
            .timeline_problems()
            .into_iter()
            .filter(|p| !before.contains(p))
            .collect();
        if let Some(first) = new.first() {
            self.set_def_live(def, path, &old)?;
            return Err(ArgusError::new(
                codes::BAD_PARAMS,
                format!("{def} {path} would not read: {first}"),
            )
            .hint("Nothing was kept; the row is as it was."));
        }
        if !self.live_edits {
            let ses = self.require_session()?;
            let root = ses.root.clone();
            let (t, _) = ses.project.defs.resolve(def).expect("just set");
            let key = def.split_once(':').map(|(_, k)| k).unwrap_or(def);
            let v = ses.project.defs.value_of(def, path)?;
            files.insert(argus_scene::defs::write_value(&root, t, key, path, &v)?);
        }
        touched.insert(def.clone());
        Ok(Some(Op::Def {
            def: def.clone(),
            path: path.clone(),
            value: old,
        }))
    }

    /// `sim.set`: one field in the live world now, an entity's or a def
    /// row's, with no transaction, no file, and no log (a scenario's setup,
    /// a sweep's candidate). A client's write respects locks and is a row in
    /// the change feed; the test harness's is neither.
    pub(super) fn sim_set(&mut self, params: &Json) -> Result<Json> {
        self.require_session()?;
        let path = json::str_param(params, "path")?.to_string();
        let value = params
            .get("value")
            .cloned()
            .ok_or_else(|| bad_params("give value"))?;
        let client = self.calling.is_some();
        let author = self.key_author();
        match (json::opt_str(params, "entity")?, json::opt_str(params, "def")?) {
            (Some(ent), None) => {
                let e = self.world.entity(ent)?;
                let eref = json::entity_str(&self.world, e);
                if client {
                    self.edits.check_lock(&author, &eref, &path)?;
                }
                let old = json::to_json(&self.world, &self.world.get_field(e, &path)?);
                self.put_field(&eref, &path, &value)?;
                self.mutations += 1;
                let new = json::to_json(&self.world, &self.world.get_field(e, &path)?);
                Ok(json!({ "entity": eref, "path": path, "from": old, "to": new }))
            }
            (None, Some(def)) => {
                let def = def.to_string();
                if client {
                    self.edits.check_lock(&author, &def, &path)?;
                }
                let (from, to) = self.set_def_live(&def, &path, &value)?;
                Ok(json!({ "def": def, "path": path, "from": from, "to": to }))
            }
            _ => Err(bad_params("sim.set takes entity or def, not both")
                .hint("entity: \"@colony\" with path Colony.hunger_per_day, or def: \"item:meal\" with path food.")),
        }
    }

    /// `test.run` with a sweep: every scenario chosen, once for each value
    /// of the target (§10: "runs the twenty-seed, thirty-day sweep from §9
    /// against each candidate value and reports which ones starve"). Each
    /// candidate is the run's `set` with the target at that value.
    pub(super) fn run_sweep(
        &mut self,
        opts: &argus_test::Options,
        target: &Json,
        values: &[Json],
    ) -> std::result::Result<Json, String> {
        let started = std::time::Instant::now();
        let mut candidates = Vec::new();
        let mut all_ok = true;
        for v in values {
            let mut o = opts.clone();
            let mut s = target.clone();
            s["value"] = v.clone();
            o.set.push(s);
            let t0 = std::time::Instant::now();
            let report = argus_test::run(self, &o)?;
            let mut failing = Vec::new();
            for sc in report["scenarios"].as_array().into_iter().flatten() {
                for r in sc["seeds"].as_array().into_iter().flatten() {
                    if r["ok"] == false && failing.len() < 20 {
                        // Which expectations failed, in words: what the
                        // agent reads to say a candidate starves (a death
                        // count) rather than runs low (a stock).
                        let failed: Vec<Json> = r["expectations"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|e| e["ok"] == false)
                            .map(|e| {
                                let m = e["message"].as_str().filter(|m| !m.is_empty());
                                json!(m.or(e["desc"].as_str()).unwrap_or(""))
                            })
                            .collect();
                        failing.push(json!({ "scenario": sc["name"], "seed": r["seed"], "error": r["error"], "failed": failed }));
                    }
                }
            }
            let ok = report["ok"] == true;
            all_ok &= ok;
            candidates.push(json!({
                "value": v,
                "ok": ok,
                "runs": report["total_runs"],
                "failed": report["failed_runs"],
                "failing": failing,
                "ms": (t0.elapsed().as_secs_f64() * 1e3).round(),
            }));
        }
        Ok(json!({
            "ok": all_ok,
            "sweep": target,
            "candidates": candidates,
            "ms": (started.elapsed().as_secs_f64() * 1e3).round(),
        }))
    }

    fn tune_session_json(&self) -> Json {
        match &self.tuning {
            None => Json::Null,
            Some(s) => json!({
                "id": s.id, "author": s.author, "note": s.note,
                "fields": s.now.keys().map(|(e, p)| format!("{e} {p}")).collect::<Vec<_>>(),
            }),
        }
    }

    /// `tune.begin`: open a tuning session.
    pub(super) fn tune_begin(&mut self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        if let Some(s) = &self.tuning {
            return Err(ArgusError::new(
                codes::BAD_PARAMS,
                format!("tuning session {} is open", s.id),
            )
            .hint("tune.end it first; tune.set adds to the open one."));
        }
        let author = json::opt_str(params, "author")?
            .unwrap_or("human")
            .to_string();
        let note = json::opt_str(params, "note")?.unwrap_or("").to_string();
        self.open_tuning(&root, &author, &note);
        Ok(json!({ "session": self.tune_session_json() }))
    }

    fn open_tuning(&mut self, root: &Path, author: &str, note: &str) {
        let id = rows(root).iter().filter(|r| r["kind"] == "begin").count() as u64 + 1;
        self.tuning = Some(TuneSession {
            id,
            author: author.to_string(),
            note: note.to_string(),
            start: Map::new(),
            now: Map::new(),
        });
        self.tune_row(json!({ "kind": "begin", "session": id, "author": author, "note": note }));
    }

    /// `tune.set`: the slider. The live world takes the value now, no file
    /// changes until the session ends, and the log gets a row. Opens a
    /// session if none is.
    pub(super) fn tune_set(&mut self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        if let Some(def) = json::opt_str(params, "def")? {
            let def = def.to_string();
            return self.tune_set_def(&root, def, params);
        }
        let entity = json::str_param(params, "entity")?;
        let path = json::str_param(params, "path")?.to_string();
        let value = params
            .get("value")
            .cloned()
            .ok_or_else(|| bad_params("give value"))?;
        let e = self.world.entity(entity)?;
        let eref = json::entity_str(&self.world, e);
        let (cid, fs) = self.world.registry().resolve_path(&path)?;
        let fs = fs.clone();
        if !fs.tune {
            return Err(
                ArgusError::new(codes::BAD_PARAMS, format!("{path} is not a tune field"))
                    .hint("tune.fields lists the tune fields; edit.set changes any field."),
            );
        }
        if self.tuning.is_none() {
            let author = json::opt_str(params, "author")?
                .unwrap_or("human")
                .to_string();
            self.open_tuning(&root, &author, "");
        }
        let author = self.tuning.as_ref().expect("open").author.clone();
        self.edits.check_lock(&author, &eref, &path)?;
        let old = json::to_json(&self.world, &self.world.get_field(e, &path)?);
        let v = json::from_json(&self.world, &value, fs.ty)?;
        self.world.set_field_by(e, cid, &fs, v)?;
        // A change between ticks: the next tick takes a keyframe first.
        self.mutations += 1;
        let new = json::to_json(&self.world, &self.world.get_field(e, &path)?);
        let s = self.tuning.as_mut().expect("open");
        let key = (eref.clone(), path.clone());
        s.start.entry(key.clone()).or_insert_with(|| old.clone());
        s.now.insert(key, new.clone());
        let session = s.id;
        self.tune_row(json!({
            "kind": "set", "author": author, "entity": eref, "path": path,
            "from": old, "to": new, "via": "tune", "session": session,
        }));
        Ok(json!({ "entity": eref, "path": path, "from": old, "to": new, "session": session }))
    }

    /// `tune.set` of a def row's tune field (phase 4 item 3): the value in
    /// memory, logged as a component's is, the session keeping it until its
    /// end commits it to the table's file.
    fn tune_set_def(&mut self, root: &Path, def: String, params: &Json) -> Result<Json> {
        let path = json::str_param(params, "path")?.to_string();
        let value = params
            .get("value")
            .cloned()
            .ok_or_else(|| bad_params("give value"))?;
        let (_, fs) = self.def_field(&def, &path)?;
        if !fs.tune {
            return Err(ArgusError::new(
                codes::BAD_PARAMS,
                format!("{def} {path} is not a tune field"),
            )
            .hint("tune.fields lists the tune fields; edit.def changes any def row's field."));
        }
        if self.tuning.is_none() {
            let author = json::opt_str(params, "author")?
                .unwrap_or("human")
                .to_string();
            self.open_tuning(root, &author, "");
        }
        let author = self.tuning.as_ref().expect("open").author.clone();
        self.edits.check_lock(&author, &def, &path)?;
        let (old, new) = self.set_def_live(&def, &path, &value)?;
        let s = self.tuning.as_mut().expect("open");
        let key = (def.clone(), path.clone());
        s.start.entry(key.clone()).or_insert_with(|| old.clone());
        s.now.insert(key, new.clone());
        let session = s.id;
        self.tune_row(json!({
            "kind": "set", "author": author, "entity": def, "def": true, "path": path,
            "from": old, "to": new, "via": "tune", "session": session,
        }));
        Ok(json!({ "def": def, "path": path, "from": old, "to": new, "session": session }))
    }

    /// `tune.end`: commit the session's final values as one transaction by
    /// its author (or, with `commit: false`, put the values back as they
    /// were), and log it.
    pub(super) fn tune_end(&mut self, params: &Json) -> Result<Json> {
        self.require_session()?;
        let commit = json::bool_param(params, "commit", true)?;
        let note = json::opt_str(params, "note")?.map(str::to_string);
        self.end_tuning(commit, note)
    }

    /// End the open session, if any.
    pub(super) fn end_tuning(&mut self, commit: bool, note: Option<String>) -> Result<Json> {
        let Some(s) = self.tuning.take() else {
            return Err(
                ArgusError::new(codes::BAD_PARAMS, "no tuning session is open")
                    .hint("tune.begin opens one; tune.set opens one too."),
            );
        };
        let note = note
            .filter(|n| !n.is_empty())
            .or_else(|| (!s.note.is_empty()).then(|| s.note.clone()))
            .unwrap_or_else(|| format!("tuning session {}", s.id));
        let changed: Vec<((String, String), Json, Json)> = s
            .now
            .iter()
            .map(|(k, v)| (k.clone(), s.start[k].clone(), v.clone()))
            .filter(|(_, from, to)| from != to)
            .collect();
        // Back to where the session started, so the commit's inverse (and
        // so undo) goes there; then the final values, as one transaction.
        for ((e, p), from, _) in &changed {
            self.put_field(e, p, from)?;
        }
        let mut out = json!({ "session": s.id, "fields": changed.len(), "commit": Json::Null });
        if commit && !changed.is_empty() {
            let ops: Vec<Op> = changed
                .iter()
                .map(|((e, p), _, to)| {
                    if self.is_def(e) {
                        Op::Def {
                            def: e.clone(),
                            path: p.clone(),
                            value: to.clone(),
                        }
                    } else {
                        Op::Set {
                            entity: e.clone(),
                            path: p.clone(),
                            value: to.clone(),
                        }
                    }
                })
                .collect();
            self.tune_committing = true;
            let r = self.apply_commit(&s.author, &note, ops, CommitKind::Normal);
            self.tune_committing = false;
            out["commit"] = r?;
        }
        self.mutations += 1;
        let values: Vec<Json> = changed
            .iter()
            .map(|((e, p), from, to)| json!([e, p, from, if commit { to } else { from }]))
            .collect();
        self.tune_row(json!({
            "kind": "end", "session": s.id, "author": s.author, "note": note,
            "committed": commit, "commit": out["commit"]["id"], "values": values,
        }));
        Ok(out)
    }

    /// Write a field in the live world only: an entity's, or a def row's.
    fn put_field(&mut self, entity: &str, path: &str, value: &Json) -> Result<()> {
        if self.is_def(entity) {
            self.set_def_live(entity, path, value)?;
            return Ok(());
        }
        let e = self.world.entity(entity)?;
        let (cid, fs) = self.world.registry().resolve_path(path)?;
        let fs = fs.clone();
        let v = json::from_json(&self.world, value, fs.ty)?;
        self.world.set_field_by(e, cid, &fs, v)
    }

    /// `tune.log`: the rows, and each field's search path as a line.
    pub(super) fn tune_log(&self, params: &Json) -> Result<Json> {
        let root = self.require_session()?.root.clone();
        let since = json::u64_param(params, "since", 0)?;
        let session = params.get("session").and_then(Json::as_u64);
        let path = json::opt_str(params, "path")?;
        let limit = json::u64_param(params, "limit", 200)? as usize;
        let all = rows(&root);
        let total = all.len();
        let picked: Vec<Json> = all
            .into_iter()
            .enumerate()
            .map(|(i, mut r)| {
                r["seq"] = json!(i as u64 + 1);
                r
            })
            .filter(|r| r["seq"].as_u64().unwrap_or(0) > since)
            .filter(|r| session.is_none_or(|s| r["session"].as_u64() == Some(s)))
            .filter(|r| path.is_none_or(|p| r["path"].as_str().is_some_and(|q| q.starts_with(p))))
            .collect();
        let story = stories(&picked);
        let skip = picked.len().saturating_sub(limit);
        Ok(json!({
            "total": total,
            "story": story,
            "rows": picked[skip..],
            "session": self.tune_session_json(),
        }))
    }
}
