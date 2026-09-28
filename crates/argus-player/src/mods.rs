//! Mods (§15 "Modding", phase 3 item 8): "a release build can load mod
//! packs — additional defs, scenes, assets, and Luau systems — from a user
//! directory, in a declared load order, with the same sandbox and
//! instruction budgets as the game's own scripts and the same schema
//! validation."
//!
//! A mod is a directory shaped like a small project, with a `mod.toml`:
//!
//! ```toml
//! id = "lanterns"          # the namespace: everything it adds starts with it
//! name = "Lanterns"
//! version = "1.0"
//! game = "hollow"          # the game it is for
//! requires = []            # mods that must load before it
//! doc = "Lanterns along the cave, and a keeper who lights them."
//! ```
//!
//! In a project it lives at `mods/<id>/` (which `argus build` leaves out
//! of the game's pack); `argus build --mod <id>` makes it a mod pack, a
//! content-addressed pack like the game's. A player puts packs (or mod
//! directories) in the mods folder beside the game's saves, and says the
//! order in `load-order.toml` there (`order = ["lanterns", "brighter"]`);
//! without that file every mod there loads, in order of id.
//!
//! Loading lays each mod's files at `mods/<id>/` under the game's root and
//! lets every loader read the game's files and then each mod's
//! (`argus_ecs::layers` has the rules: everything a mod adds is named with
//! its id first; where two layers give the same thing, the later wins).
//! A mod that is for another game, that needs a mod not loaded before it,
//! or whose content breaks a rule or fails the game's validation is left
//! out whole, with the reasons, and the game runs without it. A game that
//! does not want mods sets `[mods] enabled = false` in `argus.toml`.

use crate::pack::{self, Files, Kind, Pack};
use argus_ecs::layers::{self, codes};
use argus_ecs::{ArgusError, Result};
use serde_json::{json, Value as Json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// A mod's manifest file.
pub const MANIFEST: &str = "mod.toml";
/// The load order a mods folder declares.
pub const LOAD_ORDER: &str = "load-order.toml";

/// `mod.toml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    /// The id: the namespace prefix, and the directory under `mods/`.
    pub id: String,
    /// A name to show a player.
    pub name: String,
    /// The mod's own version.
    pub version: String,
    /// The game it is for (`argus.toml`'s name); `None` claims none.
    pub game: Option<String>,
    /// Mods that must load before it.
    pub requires: Vec<String>,
    /// What it does.
    pub doc: String,
}

fn bad(msg: impl Into<String>, at: &str) -> ArgusError {
    ArgusError::new(codes::BAD_MOD, msg)
        .hint("mod.toml has id = \"...\" (lowercase letters, digits, _), and may have name, version, game, requires = [...], and doc.")
        .at(Some(format!("{at}:1")))
}

impl Manifest {
    /// Parse `mod.toml`'s text; `file` is where it is, for errors.
    pub fn parse(file: &str, text: &str) -> Result<Manifest> {
        let doc: toml_edit::ImDocument<String> =
            text.parse().map_err(|e: toml_edit::TomlError| {
                bad(
                    format!("{file} does not parse: {}", e.message().trim()),
                    file,
                )
            })?;
        let t = doc.as_table();
        let s = |k: &str| t.get(k).and_then(|i| i.as_str()).map(str::to_string);
        let id = s("id").ok_or_else(|| bad(format!("{file} has no id"), file))?;
        if !layers::valid_id(&id) {
            return Err(bad(
                format!("{file}: id {id:?} is not an id: a lowercase letter, then lowercase letters, digits, or _"),
                file,
            ));
        }
        let requires = match t.get("requires") {
            None => Vec::new(),
            Some(i) => i
                .as_array()
                .ok_or_else(|| bad(format!("{file}: requires is a list of mod ids"), file))?
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| bad(format!("{file}: requires is a list of mod ids"), file))
                })
                .collect::<Result<Vec<_>>>()?,
        };
        Ok(Manifest {
            name: s("name").unwrap_or_else(|| id.clone()),
            version: s("version").unwrap_or_else(|| "0".to_string()),
            game: s("game"),
            requires,
            doc: s("doc").unwrap_or_default(),
            id,
        })
    }
}

/// Where a mod was found.
#[derive(Clone, Debug)]
pub enum Source {
    /// A directory with a `mod.toml`: its files as they are.
    Dir(PathBuf),
    /// A mod pack (`argus build --mod`), its files resolved.
    Pack(PathBuf, Files),
}

/// A mod found in a folder.
#[derive(Clone, Debug)]
pub struct Found {
    /// Its `mod.toml`.
    pub manifest: Manifest,
    /// Where it is.
    pub source: Source,
    /// Its content's address: the same files, the same hash. A replay or a
    /// save records it, so it can say when a mod changed under it.
    pub hash: String,
}

impl Found {
    /// Its files, relative to its root.
    pub fn files(&self) -> Result<Files> {
        match &self.source {
            Source::Pack(_, f) => Ok(f.clone()),
            Source::Dir(d) => read_tree(d),
        }
    }

    /// Where it is, for a report.
    pub fn place(&self) -> String {
        match &self.source {
            Source::Dir(d) | Source::Pack(d, _) => d.display().to_string(),
        }
    }

    /// "pack" or "directory".
    pub fn kind(&self) -> &'static str {
        match self.source {
            Source::Dir(_) => "directory",
            Source::Pack(..) => "pack",
        }
    }
}

/// Every file under `dir` a mod pack would hold: not those whose names
/// start with a dot, nor a top-level `tests/`, `tools/`, `dist/`, or
/// `mods/` ([`pack::LEFT_OUT`]); relative, `/`-separated, sorted. A mod's
/// directory and its pack hash the same.
fn read_tree(dir: &Path) -> Result<Files> {
    fn walk(d: &Path, root: &Path, out: &mut Files) -> std::io::Result<()> {
        for e in argus_os::fs::read_dir(d)? {
            let name = e.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || (d == root && pack::LEFT_OUT.contains(&name)) {
                continue;
            }
            if e.is_dir {
                walk(&e.path, root, out)?;
            } else if let Ok(rel) = e.path.strip_prefix(root) {
                out.push((
                    rel.to_string_lossy().replace('\\', "/"),
                    argus_os::fs::read(&e.path)?,
                ));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out)
        .map_err(|e| ArgusError::new(codes::BAD_MOD, format!("{}: {e}", dir.display())))?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Where a mod pack keeps the hash of the directory it was built from, so
/// the pack and the directory are the same mod to a replay or a save.
pub const SOURCE_HASH: &str = "argus.cache/mod.hash";

/// The hash of a mod's directory: what [`find`] says of it, and what
/// `argus build --mod` writes into its pack.
pub fn source_hash(dir: &Path) -> Result<String> {
    Ok(hash_files(&read_tree(dir)?))
}

/// The address of a set of files: each path and blob address, hashed.
fn hash_files(files: &Files) -> String {
    let mut all = Vec::new();
    for (p, b) in files {
        all.extend(p.as_bytes());
        all.push(0);
        all.extend(pack::address(b));
    }
    pack::address(&all)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A mod pack's files: a mod pack holds all of them.
pub fn pack_files(p: &Pack) -> Result<Files> {
    let mut out = Vec::new();
    for (path, h, _) in &p.files {
        let b = p
            .blobs
            .get(h)
            .ok_or_else(|| ArgusError::new(codes::BAD_MOD, format!("the mod pack lacks {path}")))?;
        out.push((path.clone(), b.clone()));
    }
    Ok(out)
}

/// Every mod in `dir`: directories with a `mod.toml`, and `*.pack` files
/// that are mod packs. What could not be read is a problem, by id where
/// one is known, else by file name; a second mod with an id already found
/// is one too.
pub fn find(dir: &Path) -> (Vec<Found>, Vec<(String, ArgusError)>) {
    let mut found: Vec<Found> = Vec::new();
    let mut problems = Vec::new();
    let Ok(entries) = argus_os::fs::read_dir(dir) else {
        return (found, problems);
    };
    for e in entries {
        let fname = e
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if fname.starts_with('.') {
            continue;
        }
        let got: Result<Option<Found>> = if e.is_dir {
            let mf = e.path.join(MANIFEST);
            if !argus_os::fs::is_file(&mf) {
                continue;
            }
            let shown = format!("{fname}/{MANIFEST}");
            argus_os::fs::read_to_string(&mf)
                .map_err(|x| bad(format!("{shown}: {x}"), &shown))
                .and_then(|text| Manifest::parse(&shown, &text))
                .and_then(|m| {
                    if m.id != fname {
                        return Err(bad(
                            format!("{shown} says id {:?}, but its directory is {fname:?}; the directory is the id", m.id),
                            &shown,
                        ));
                    }
                    let files = read_tree(&e.path)?;
                    Ok(Some(Found {
                        hash: hash_files(&files),
                        manifest: m,
                        source: Source::Dir(e.path.clone()),
                    }))
                })
        } else if fname.ends_with(".pack") {
            argus_os::fs::read(&e.path)
                .map_err(|x| bad(format!("{fname}: {x}"), &fname))
                .and_then(|b| Pack::decode(&b))
                .and_then(|p| {
                    let Kind::Mod(id) = &p.kind else {
                        return Err(bad(format!("{fname} is a game's pack, not a mod's; it belongs in the game's data/"), &fname));
                    };
                    let files = pack_files(&p)?;
                    let text = files
                        .iter()
                        .find(|(n, _)| n == MANIFEST)
                        .map(|(_, b)| String::from_utf8_lossy(b).into_owned())
                        .ok_or_else(|| bad(format!("{fname} has no {MANIFEST}"), &fname))?;
                    let m = Manifest::parse(&format!("{fname}:{MANIFEST}"), &text)?;
                    if &m.id != id {
                        return Err(bad(format!("{fname} is mod {id:?} but its {MANIFEST} says {:?}", m.id), &fname));
                    }
                    let hash = files
                        .iter()
                        .find(|(n, _)| n == SOURCE_HASH)
                        .map(|(_, b)| String::from_utf8_lossy(b).trim().to_string())
                        .unwrap_or_else(|| hash_files(&files));
                    Ok(Some(Found {
                        hash,
                        manifest: m,
                        source: Source::Pack(e.path.clone(), files),
                    }))
                })
        } else {
            continue;
        };
        match got {
            Ok(Some(f)) => {
                if found.iter().any(|g| g.manifest.id == f.manifest.id) {
                    problems.push((
                        f.manifest.id.clone(),
                        ArgusError::new(
                            codes::BAD_MOD,
                            format!(
                                "two mods in the folder are {:?}: {} is left out",
                                f.manifest.id, fname
                            ),
                        )
                        .hint("Keep one of them (a pack and a directory of the same mod are two)."),
                    ));
                } else {
                    found.push(f);
                }
            }
            Ok(None) => {}
            Err(e) => problems.push((fname.trim_end_matches(".pack").to_string(), e)),
        }
    }
    (found, problems)
}

/// `load-order.toml` in a mods folder: `order = ["a", "b"]`. `None` where
/// there is no such file.
pub fn read_order(dir: &Path) -> Result<Option<Vec<String>>> {
    let p = dir.join(LOAD_ORDER);
    let Ok(text) = argus_os::fs::read_to_string(&p) else {
        return Ok(None);
    };
    let doc: toml_edit::ImDocument<String> = text.parse().map_err(|e: toml_edit::TomlError| {
        ArgusError::new(
            codes::BAD_MOD,
            format!("{LOAD_ORDER} does not parse: {}", e.message().trim()),
        )
    })?;
    let order = doc.get("order").and_then(|i| i.as_array()).ok_or_else(|| {
        ArgusError::new(
            codes::BAD_MOD,
            format!("{LOAD_ORDER} needs order = [\"id\", ...]"),
        )
        .hint("The mods load in that order; a mod not listed does not load.")
    })?;
    Ok(Some(
        order
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
    ))
}

/// Which mods load, in what order, and which do not and why.
#[derive(Clone, Debug, Default)]
pub struct Plan {
    /// Ids to load, in order.
    pub ids: Vec<String>,
    /// Mods left out, each with why.
    pub left_out: Vec<(String, ArgusError)>,
    /// "declared" (a load order was given) or "by id" (none was).
    pub rule: &'static str,
}

/// The load order (§15): the one declared, else every mod found by id,
/// each after the mods it requires.
/// A declared id not found, a mod for another game, and a mod whose
/// `requires` are not loaded before it are left out; so is a found mod not
/// declared, when `unlisted` says a mod not declared is one left out (a
/// folder's `load-order.toml`) rather than one not asked for (a session
/// naming the mods it wants).
pub fn plan(found: &[Found], declared: Option<&[String]>, game: &str, unlisted: bool) -> Plan {
    let mut plan = Plan {
        rule: if declared.is_some() {
            "declared"
        } else {
            "by id"
        },
        ..Plan::default()
    };
    let by_id: BTreeMap<&str, &Found> = found.iter().map(|f| (f.manifest.id.as_str(), f)).collect();
    let wanted: Vec<String> = match declared {
        Some(d) => {
            let listed: BTreeSet<&str> = d.iter().map(String::as_str).collect();
            for f in found.iter().filter(|_| unlisted) {
                if !listed.contains(f.manifest.id.as_str()) {
                    plan.left_out.push((
                        f.manifest.id.clone(),
                        ArgusError::new(
                            codes::LOAD_ORDER,
                            format!("{} is not in {LOAD_ORDER}", f.manifest.id),
                        )
                        .hint("Add its id to order = [...] where it should load."),
                    ));
                }
            }
            d.to_vec()
        }
        // By id, each after what it requires: the first by id whose
        // requirements are placed, again and again; what can never be
        // placed (a requirement missing, or a loop) last, by id, to be left
        // out below.
        None => {
            let mut placed: Vec<String> = Vec::new();
            let mut rest: Vec<&str> = by_id.keys().copied().collect();
            while let Some(i) = rest.iter().position(|id| {
                by_id[id]
                    .manifest
                    .requires
                    .iter()
                    .all(|r| placed.contains(r))
            }) {
                placed.push(rest.remove(i).to_string());
            }
            placed.extend(rest.into_iter().map(str::to_string));
            placed
        }
    };
    for id in wanted {
        if plan.ids.contains(&id) {
            continue;
        }
        let Some(f) = by_id.get(id.as_str()) else {
            plan.left_out.push((
                id.clone(),
                ArgusError::new(
                    codes::LOAD_ORDER,
                    format!("{id:?} is asked for, but it is not in the mods folder"),
                ),
            ));
            continue;
        };
        if let Some(g) = &f.manifest.game {
            if g != game {
                plan.left_out.push((
                    id.clone(),
                    ArgusError::new(
                        codes::LOAD_ORDER,
                        format!("{id} is a mod for {g:?}, not for {game:?}"),
                    ),
                ));
                continue;
            }
        }
        if let Some(r) = f.manifest.requires.iter().find(|r| !plan.ids.contains(r)) {
            let why = if by_id.contains_key(r.as_str()) {
                format!("{id} needs {r}, which does not load before it")
            } else {
                format!("{id} needs {r}, which is not in the mods folder")
            };
            plan.left_out.push((
                id.clone(),
                ArgusError::new(codes::LOAD_ORDER, why).hint(format!(
                    "Load {r} before {id} ({LOAD_ORDER}: order = [..., \"{r}\", \"{id}\", ...])."
                )),
            ));
            continue;
        }
        plan.ids.push(id);
    }
    plan
}

/// Put mods' files in memory at `mods/<id>/` under `root` (a release
/// build's mounted game), so the loaders read them there.
pub fn mount(root: &Path, found: &[Found], ids: &[String]) -> Result<()> {
    for id in ids {
        let Some(f) = found.iter().find(|f| &f.manifest.id == id) else {
            continue;
        };
        let files = f
            .files()?
            .into_iter()
            .map(|(p, b)| (format!("{}/{p}", layers::dir_of(id)), b))
            .collect();
        argus_os::fs::mount(root, files);
    }
    Ok(())
}

/// What became of the mods, for a report: the rule, the mods loaded
/// (manifest, where, content hash), the mods left out with why, and what
/// the mods changed.
#[derive(Clone, Debug, Default)]
pub struct Report {
    /// Whether the game takes mods.
    pub enabled: bool,
    /// "declared", "by id", or "given" (the session named them).
    pub rule: String,
    /// Loaded, in order.
    pub loaded: Vec<Loaded>,
    /// Left out, each with every reason found.
    pub left_out: Vec<(String, Vec<ArgusError>)>,
    /// What the mods changed of the game's or an earlier mod's.
    pub changes: Vec<layers::Change>,
}

/// One mod loaded.
#[derive(Clone, Debug)]
pub struct Loaded {
    /// Its manifest.
    pub manifest: Manifest,
    /// "directory" or "pack".
    pub kind: String,
    /// Where it was.
    pub place: String,
    /// Its content's address.
    pub hash: String,
    /// What it added, by kind.
    pub adds: BTreeMap<String, usize>,
}

impl Report {
    /// The mods loaded, as a replay or a save records them: id, version,
    /// hash, in order.
    pub fn identity(&self) -> Json {
        json!(self
            .loaded
            .iter()
            .map(|l| json!({ "id": l.manifest.id, "version": l.manifest.version, "hash": l.hash }))
            .collect::<Vec<_>>())
    }

    /// The ids loaded, in order.
    pub fn ids(&self) -> Vec<String> {
        self.loaded.iter().map(|l| l.manifest.id.clone()).collect()
    }

    /// As JSON.
    pub fn to_json(&self) -> Json {
        let err = |e: &ArgusError| json!({ "code": e.code, "message": e.message, "hint": e.hint, "at": e.r#ref });
        json!({
            "enabled": self.enabled,
            "rule": self.rule,
            "loaded": self.loaded.iter().map(|l| json!({
                "id": l.manifest.id,
                "name": l.manifest.name,
                "version": l.manifest.version,
                "game": l.manifest.game,
                "requires": l.manifest.requires,
                "doc": l.manifest.doc,
                "kind": l.kind,
                "from": l.place,
                "hash": l.hash,
                "adds": l.adds,
            })).collect::<Vec<_>>(),
            "left_out": self.left_out.iter().map(|(id, why)| json!({
                "id": id,
                "why": why.iter().map(err).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "changes": self.changes.iter().map(|c| json!({
                "kind": c.kind, "name": c.name, "by": c.by, "over": c.over, "at": c.at,
            })).collect::<Vec<_>>(),
        })
    }

    /// One line a player can read about the mods left out, if any.
    pub fn left_out_line(&self) -> Option<String> {
        if self.left_out.is_empty() {
            return None;
        }
        let parts: Vec<String> = self
            .left_out
            .iter()
            .map(|(id, why)| match why.first() {
                Some(e) => format!("{id} ({})", e.message),
                None => id.clone(),
            })
            .collect();
        Some(format!("Mods not loaded: {}", parts.join("; ")))
    }
}

/// A project's own mods folder, where mods are made and tested:
/// `mods/` under its root.
pub fn project_dir(root: &Path) -> PathBuf {
    root.join("mods")
}

/// Where the load order comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Order {
    /// The folder's `load-order.toml`, else every mod there by id (a
    /// release build's mods folder).
    Folder,
    /// These ids, in this order (a session, a scenario, a replay).
    Given(Vec<String>),
}

/// The mods a game loads from a folder, planned (phase 3 item 8): none
/// when the game does not take mods (`[mods] enabled = false`), which is
/// an error only when some were asked for by name; else what [`find`] and
/// [`plan`] make of the folder. Also returns what was found, for
/// [`mount`].
pub fn choose(
    cfg: &argus_scene::ProjectConfig,
    dir: &Path,
    order: Order,
) -> Result<(Report, Vec<Found>)> {
    let mut report = Report {
        enabled: cfg.mods_enabled,
        rule: match &order {
            Order::Folder => "by id".to_string(),
            Order::Given(_) => "given".to_string(),
        },
        ..Report::default()
    };
    if !cfg.mods_enabled {
        if let Order::Given(ids) = &order {
            if !ids.is_empty() {
                return Err(ArgusError::new(
                    codes::MODS_OFF,
                    format!(
                        "{} does not take mods ([mods] enabled = false in argus.toml)",
                        cfg.name
                    ),
                )
                .hint("Take the flag out of argus.toml, or open the scene without mods."));
            }
        }
        return Ok((report, Vec::new()));
    }
    if matches!(&order, Order::Given(ids) if ids.is_empty()) {
        return Ok((report, Vec::new()));
    }
    let (found, problems) = find(dir);
    let declared = match &order {
        Order::Given(ids) => Some(ids.clone()),
        Order::Folder => read_order(dir)?,
    };
    let p = plan(
        &found,
        declared.as_deref(),
        &cfg.name,
        order == Order::Folder,
    );
    if matches!(order, Order::Folder) {
        report.rule = p.rule.to_string();
    }
    for (id, e) in problems {
        report.left_out.push((id, vec![e]));
    }
    for (id, e) in p.left_out {
        report.left_out.push((id, vec![e]));
    }
    for id in &p.ids {
        let Some(f) = found.iter().find(|f| &f.manifest.id == id) else {
            continue;
        };
        report.loaded.push(Loaded {
            manifest: f.manifest.clone(),
            kind: f.kind().to_string(),
            place: f.place(),
            hash: f.hash.clone(),
            adds: BTreeMap::new(),
        });
    }
    Ok((report, found))
}
