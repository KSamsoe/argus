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
use std::collections::BTreeMap as Map;
use std::io::Write as _;

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

    /// `tune.fields`: the tune fields in the world with their values, what
    /// the editor's panel will show: every one of an entity, or of every
    /// entity with an id.
    pub(super) fn tune_fields(&self, params: &Json) -> Result<Json> {
        self.require_session()?;
        let one = json::opt_str(params, "entity")?;
        let ents: Vec<Entity> = match one {
            Some(id) => vec![self.world.entity(id)?],
            None => {
                let mut v: Vec<Entity> = self
                    .world
                    .live()
                    .filter(|e| self.world.string_id(*e).is_some())
                    .collect();
                v.sort();
                v
            }
        };
        let mut rows = Vec::new();
        for e in ents {
            let Ok(comps) = self.world.components_of(e) else {
                continue;
            };
            for c in comps {
                let s = self.world.registry().schema(*c);
                for f in s.fields.iter().filter(|f| f.tune) {
                    let path = format!("{}.{}", s.name, f.name);
                    let value = self.world.get_field(e, &path).ok();
                    rows.push(json!({
                        "entity": json::entity_str(&self.world, e),
                        "path": path,
                        "value": value.map(|v| json::to_json(&self.world, &v)),
                        "default": json::to_json(&self.world, &f.default),
                        "range": f.range,
                        "doc": f.doc,
                    }));
                }
            }
        }
        Ok(json!({ "total": rows.len(), "rows": rows, "session": self.tune_session_json() }))
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
                .map(|((e, p), _, to)| Op::Set {
                    entity: e.clone(),
                    path: p.clone(),
                    value: to.clone(),
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

    /// Write a field in the live world only.
    fn put_field(&mut self, entity: &str, path: &str, value: &Json) -> Result<()> {
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
