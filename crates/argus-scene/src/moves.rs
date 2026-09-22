//! Path references and renames (§4): "Renames are performed through the
//! protocol (`asset.move`), which rewrites references and appends to
//! `.argus/moves.log` so an out-of-date client can catch up. A rename done
//! outside the tool leaves a clear 'missing reference' error with the old
//! path in it."
//!
//! The log is one tab-separated line per move: unix seconds, kind, from, to,
//! number of references rewritten. Missing-reference errors consult it, so
//! a client holding an old path is told where the thing went.

use crate::codes;
use crate::project::Project;
use crate::tomlv;
use argus_ecs::{ArgusError, Result};
use std::path::{Path, PathBuf};
use toml_edit::{Item, Value as TValue};

/// What kind of thing moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveKind {
    /// A prefab file: references are its path without `prefabs/` and `.toml`.
    Prefab,
    /// An authored entity file: references are `@id` within its scene.
    Entity,
    /// A def row: references are `prefix:key`.
    Def,
    /// Any other project file (an asset): references are its project path.
    Asset,
}

impl MoveKind {
    /// Log spelling.
    pub fn name(self) -> &'static str {
        match self {
            MoveKind::Prefab => "prefab",
            MoveKind::Entity => "entity",
            MoveKind::Def => "def",
            MoveKind::Asset => "asset",
        }
    }

    fn parse(s: &str) -> Option<MoveKind> {
        Some(match s {
            "prefab" => MoveKind::Prefab,
            "entity" => MoveKind::Entity,
            "def" => MoveKind::Def,
            "asset" => MoveKind::Asset,
            _ => return None,
        })
    }
}

/// One line of `.argus/moves.log`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveRecord {
    /// Unix seconds when the move happened.
    pub time: u64,
    /// Kind.
    pub kind: MoveKind,
    /// The reference spelling before (`door/locked`, `@door.east`, `item:key.brass`).
    pub from: String,
    /// The reference spelling after.
    pub to: String,
    /// How many references were rewritten.
    pub refs: usize,
}

/// Result of an `asset.move`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveReport {
    /// The log record that was appended.
    pub record: MoveRecord,
    /// Files whose references were rewritten (project-relative).
    pub rewritten: Vec<String>,
}

/// Read the moves log (missing file is an empty log).
pub fn read_log(root: &Path) -> Vec<MoveRecord> {
    let Ok(text) = std::fs::read_to_string(root.join(".argus").join("moves.log")) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let time = parts.next()?.parse().ok()?;
            let kind = MoveKind::parse(parts.next()?)?;
            let from = parts.next()?.to_string();
            let to = parts.next()?.to_string();
            let refs = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            Some(MoveRecord {
                time,
                kind,
                from,
                to,
                refs,
            })
        })
        .collect()
}

fn append_log(root: &Path, rec: &MoveRecord) -> Result<()> {
    use std::io::Write;
    let dir = root.join(".argus");
    std::fs::create_dir_all(&dir).map_err(|e| {
        ArgusError::new(
            codes::MISSING_FILE,
            format!("cannot create {}: {e}", dir.display()),
        )
    })?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("moves.log"))
        .map_err(|e| ArgusError::new(codes::MISSING_FILE, format!("cannot open moves.log: {e}")))?;
    writeln!(
        f,
        "{}\t{}\t{}\t{}\t{}",
        rec.time,
        rec.kind.name(),
        rec.from,
        rec.to,
        rec.refs
    )
    .map_err(|e| ArgusError::new(codes::MISSING_FILE, format!("cannot write moves.log: {e}")))
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The §4 "missing reference" error: names the old path, and if the moves
/// log knows where it went, says so.
pub fn missing_reference(kind: &str, target: &str, at: &str, moves: &[MoveRecord]) -> ArgusError {
    let e = ArgusError::new(
        codes::MISSING_REFERENCE,
        format!("missing reference: {kind} {target:?} does not exist"),
    )
    .at(Some(at.to_string()));
    match moves.iter().rev().find(|m| m.from == target) {
        Some(m) => e.hint(format!(
            "It was moved to {:?} through asset.move (see .argus/moves.log); update the reference.",
            m.to
        )),
        None => e.hint(
            "If it was renamed outside the tool, run asset.move with the old and new path to rewrite every reference.",
        ),
    }
}

/// Every TOML file under `prefabs/`, `scenes/`, and `defs/`, project-relative
/// with forward slashes, sorted.
pub fn content_files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for top in ["prefabs", "scenes", "defs"] {
        walk(&root.join(top), root, &mut out);
    }
    out.sort();
    out
}

fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk(&p, root, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("toml") {
            out.push(rel(root, &p));
        }
    }
}

/// Project-relative path with forward slashes.
pub fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Rewrite string values in a TOML file. `f` maps an old string to its
/// replacement (or `None` to leave it). Returns whether the file changed.
pub fn rewrite_strings(path: &Path, rel: &str, f: &dyn Fn(&str) -> Option<String>) -> Result<bool> {
    let text = tomlv::read(path)?;
    let mut doc = tomlv::parse_mut(rel, &text)?;
    let mut changed = false;
    rewrite_table(doc.as_table_mut(), f, &mut changed);
    if changed {
        std::fs::write(path, doc.to_string()).map_err(|e| {
            ArgusError::new(
                codes::MISSING_FILE,
                format!("cannot write {}: {e}", path.display()),
            )
        })?;
    }
    Ok(changed)
}

fn rewrite_table(
    t: &mut dyn toml_edit::TableLike,
    f: &dyn Fn(&str) -> Option<String>,
    changed: &mut bool,
) {
    for (_, item) in t.iter_mut() {
        rewrite_item(item, f, changed);
    }
}

fn rewrite_item(item: &mut Item, f: &dyn Fn(&str) -> Option<String>, changed: &mut bool) {
    match item {
        Item::Value(v) => rewrite_value(v, f, changed),
        Item::Table(t) => rewrite_table(t, f, changed),
        Item::ArrayOfTables(a) => {
            for t in a.iter_mut() {
                rewrite_table(t, f, changed);
            }
        }
        Item::None => {}
    }
}

fn rewrite_value(v: &mut TValue, f: &dyn Fn(&str) -> Option<String>, changed: &mut bool) {
    match v {
        TValue::String(s) => {
            if let Some(new) = f(s.value()) {
                let decor = s.decor().clone();
                let mut n = toml_edit::Formatted::new(new);
                *n.decor_mut() = decor;
                *s = n;
                *changed = true;
            }
        }
        TValue::Array(a) => {
            for x in a.iter_mut() {
                rewrite_value(x, f, changed);
            }
        }
        TValue::InlineTable(t) => rewrite_table(t, f, changed),
        _ => {}
    }
}

/// What a project path is, for `asset.move`.
fn classify(root: &Path, from: &str) -> Result<(MoveKind, String)> {
    if let Some(rest) = from.strip_prefix("prefabs/") {
        let p = rest.strip_suffix(".toml").ok_or_else(|| {
            ArgusError::new(
                codes::BAD_MOVE,
                format!("{from} is not a prefab file (.toml)"),
            )
        })?;
        return Ok((MoveKind::Prefab, p.to_string()));
    }
    if let Some((file, key)) = from.split_once('#') {
        if file.starts_with("defs/") && file.ends_with(".toml") {
            return Ok((MoveKind::Def, key.to_string()));
        }
    }
    if from.starts_with("scenes/") && from.ends_with(".toml") {
        let name = Path::new(from)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if !crate::bulk::reserved_stem(name) {
            return Ok((MoveKind::Entity, name.to_string()));
        }
    }
    if root.join(from).is_file() {
        return Ok((MoveKind::Asset, from.to_string()));
    }
    Err(ArgusError::new(
        codes::BAD_MOVE,
        format!("{from} is not a prefab, entity file, def row, or asset"),
    )
    .hint("Give a project-relative path: prefabs/door/locked.toml, scenes/arena/player.toml, defs/items.toml#key.brass, or assets/x.png."))
}

/// Perform `asset.move {from, to}`: rename the file (or def row), rewrite
/// every reference in the project's TOML content, append to the moves log,
/// and reload the affected library in the project.
pub fn move_asset(project: &mut Project, from: &str, to: &str) -> Result<MoveReport> {
    let root = project.root.clone();
    let (kind, old_ref) = classify(&root, from)?;
    let (to_kind, new_ref) = match kind {
        MoveKind::Def => {
            let key = to
                .split_once('#')
                .map(|(_, k)| k)
                .unwrap_or_else(|| to.trim_start_matches('#'));
            (MoveKind::Def, key.to_string())
        }
        _ => classify_target(kind, to)?,
    };
    if to_kind != kind {
        return Err(ArgusError::new(
            codes::BAD_MOVE,
            format!("cannot move a {} to a {} path", kind.name(), to_kind.name()),
        ));
    }

    let mut rewritten = Vec::new();
    let mut refs = 0usize;

    // The reference spellings to rewrite, and the files to look in.
    let (spellings, files): (Vec<(String, String)>, Vec<String>) = match kind {
        MoveKind::Prefab | MoveKind::Asset => (
            vec![(old_ref.clone(), new_ref.clone())],
            content_files(&root),
        ),
        MoveKind::Entity => {
            // Within the scene only: @id, @room/id, and child paths @id/x.
            let scene_dir = scene_dir_of(from).ok_or_else(|| {
                ArgusError::new(
                    codes::BAD_MOVE,
                    format!("{from} is not inside scenes/<name>/"),
                )
            })?;
            let to_dir = scene_dir_of(to).unwrap_or_default();
            if to_dir != scene_dir {
                return Err(ArgusError::new(
                    codes::BAD_MOVE,
                    "an entity can only be renamed within its scene",
                )
                .hint("Ids are unique per scene; create the entity in the other scene and despawn it here."));
            }
            let room = room_of(from);
            let files = content_files(&root)
                .into_iter()
                .filter(|f| f.starts_with(&format!("{scene_dir}/")))
                .collect();
            let mut sp = vec![(format!("@{old_ref}"), format!("@{new_ref}"))];
            if let Some(r) = room {
                sp.push((format!("@{r}/{old_ref}"), format!("@{r}/{new_ref}")));
            }
            (sp, files)
        }
        MoveKind::Def => {
            let file = from.split('#').next().unwrap_or("");
            let table = Path::new(file)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let prefix = project
                .defs
                .table(&table)
                .map(|t| t.prefix.clone())
                .unwrap_or_else(|| table.clone());
            (
                vec![(format!("{prefix}:{old_ref}"), format!("{prefix}:{new_ref}"))],
                content_files(&root),
            )
        }
    };

    let map = |s: &str| -> Option<String> {
        for (old, new) in &spellings {
            if s == old {
                return Some(new.clone());
            }
            // Child paths and room-qualified forms extend the id with '/'.
            if kind == MoveKind::Entity {
                if let Some(rest) = s.strip_prefix(old.as_str()) {
                    if rest.starts_with('/') {
                        return Some(format!("{new}{rest}"));
                    }
                }
            }
        }
        None
    };

    // Move the thing itself first, so a crash between steps leaves the log
    // without the rename rather than the reverse.
    match kind {
        MoveKind::Def => {
            let file = from.split('#').next().unwrap_or("");
            let path = root.join(file);
            let text = tomlv::read(&path)?;
            let mut doc = tomlv::parse_mut(file, &text)?;
            let item = doc.remove(&old_ref).ok_or_else(|| {
                ArgusError::new(codes::BAD_MOVE, format!("{file} has no row {old_ref:?}"))
            })?;
            if doc.contains_key(&new_ref) {
                return Err(ArgusError::new(
                    codes::BAD_MOVE,
                    format!("{file} already has a row {new_ref:?}"),
                ));
            }
            doc.insert(&new_ref, item);
            std::fs::write(&path, doc.to_string()).map_err(|e| {
                ArgusError::new(codes::MISSING_FILE, format!("cannot write {file}: {e}"))
            })?;
        }
        _ => {
            let src = root.join(from);
            let dst = root.join(to);
            if !src.exists() {
                return Err(ArgusError::new(
                    codes::MISSING_FILE,
                    format!("{from} does not exist"),
                ));
            }
            if dst.exists() {
                return Err(
                    ArgusError::new(codes::BAD_MOVE, format!("{to} already exists"))
                        .hint("Move it aside first; asset.move never overwrites."),
                );
            }
            if let Some(dir) = dst.parent() {
                std::fs::create_dir_all(dir).map_err(|e| {
                    ArgusError::new(
                        codes::MISSING_FILE,
                        format!("cannot create {}: {e}", dir.display()),
                    )
                })?;
            }
            std::fs::rename(&src, &dst).map_err(|e| {
                ArgusError::new(
                    codes::BAD_MOVE,
                    format!("cannot rename {from} to {to}: {e}"),
                )
            })?;
        }
    }

    for rel in files {
        let path = root.join(&rel);
        if !path.exists() {
            continue;
        }
        let mut count = 0usize;
        let counting = |s: &str| map(s);
        let changed = rewrite_strings(&path, &rel, &counting)?;
        if changed {
            // Count by re-reading: cheap, and exact.
            let text = tomlv::read(&path)?;
            for (_, new) in &spellings {
                count += text.matches(&format!("\"{new}")).count();
            }
            refs += count.max(1);
            rewritten.push(rel);
        }
    }

    let record = MoveRecord {
        time: now(),
        kind,
        from: old_ref,
        to: new_ref,
        refs,
    };
    append_log(&root, &record)?;
    project.moves.push(record.clone());
    // References were rewritten in any content file, so both libraries re-read.
    project.reload_prefabs();
    project.reload_defs();
    Ok(MoveReport { record, rewritten })
}

fn classify_target(kind: MoveKind, to: &str) -> Result<(MoveKind, String)> {
    match kind {
        MoveKind::Prefab => {
            let p = to
                .strip_prefix("prefabs/")
                .and_then(|r| r.strip_suffix(".toml"))
                .ok_or_else(|| {
                    ArgusError::new(codes::BAD_MOVE, format!("{to} must be prefabs/<path>.toml"))
                })?;
            Ok((MoveKind::Prefab, p.to_string()))
        }
        MoveKind::Entity => {
            let name = Path::new(to)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            if !to.starts_with("scenes/") || !to.ends_with(".toml") || name.is_empty() {
                return Err(ArgusError::new(
                    codes::BAD_MOVE,
                    format!("{to} must be scenes/<scene>/[rooms/<room>/]<id>.toml"),
                ));
            }
            if crate::bulk::reserved_stem(name) {
                return Err(ArgusError::new(
                    codes::BAD_MOVE,
                    format!("{name:?} is a reserved file name, not an entity id"),
                )
                .hint("scene and room are the scene and room documents, and names ending .map or .tilemap are data indexes."));
            }
            Ok((MoveKind::Entity, name.to_string()))
        }
        MoveKind::Asset => Ok((MoveKind::Asset, to.to_string())),
        MoveKind::Def => unreachable!(),
    }
}

fn scene_dir_of(p: &str) -> Option<String> {
    let mut it = p.split('/');
    if it.next()? != "scenes" {
        return None;
    }
    Some(format!("scenes/{}", it.next()?))
}

fn room_of(p: &str) -> Option<String> {
    let parts: Vec<&str> = p.split('/').collect();
    // scenes/<scene>/rooms/<room>/<id>.toml
    if parts.len() == 5 && parts[2] == "rooms" {
        Some(parts[3].to_string())
    } else {
        None
    }
}

/// Where a project-relative path lives on disk.
pub fn abs(root: &Path, rel: &str) -> PathBuf {
    root.join(rel)
}
