//! The prefab library (§3 "Prefabs and overrides"): every file under
//! `prefabs/`, keyed by its path without the directory and extension
//! (`prefabs/door/locked.toml` is `door/locked`). A prefab is an entity
//! document whose `prefab` key names its base; [`PrefabLibrary::chain`]
//! walks base links and reports cycles and missing bases as errors.

use crate::codes;
use crate::doc::EntityDoc;
use crate::moves::{self, MoveRecord};
use crate::tomlv;
use argus_ecs::layers::{self, Change};
use argus_ecs::{ArgusError, Layers, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One prefab file.
#[derive(Clone, Debug)]
pub struct PrefabFile {
    /// Reference path: `door/locked`.
    pub path: String,
    /// Project-relative file: `prefabs/door/locked.toml`.
    pub file: String,
    /// Parsed content.
    pub doc: EntityDoc,
}

/// All prefabs in a project.
#[derive(Clone, Debug, Default)]
pub struct PrefabLibrary {
    root: PathBuf,
    prefabs: BTreeMap<String, PrefabFile>,
}

impl PrefabLibrary {
    /// Load `prefabs/` under `root`. Files that do not parse are reported,
    /// not fatal: the rest of the library still loads.
    pub fn load(root: &Path) -> (PrefabLibrary, Vec<ArgusError>) {
        let (lib, errors, _) = PrefabLibrary::load_layers(&Layers::game(root));
        (lib, errors)
    }

    /// [`load`](PrefabLibrary::load) for the game and its mods (phase 3
    /// item 8): a mod's `prefabs/` after the game's. A prefab path in the
    /// mod's namespace (`lanterns/lamp`) is added; one the game (or an
    /// earlier mod) has is replaced, and reported; any other is refused.
    pub fn load_layers(layers: &Layers) -> (PrefabLibrary, Vec<ArgusError>, Vec<Change>) {
        let mut lib = PrefabLibrary {
            root: layers.game.clone(),
            prefabs: BTreeMap::new(),
        };
        let mut errors = Vec::new();
        let mut changes = Vec::new();
        for layer in layers.each() {
            let dir = layer.root.join("prefabs");
            let mut on_disk = Vec::new();
            walk(&dir, &mut on_disk);
            let mut files: Vec<(PathBuf, Option<EntityDoc>)> =
                on_disk.into_iter().map(|p| (p, None)).collect();
            // A release pack's bundle holds them parsed (phase 5 item 1).
            let bundle = layer.root.join(crate::binary::PREFABS);
            if argus_os::fs::is_file(&bundle) {
                let read = argus_os::fs::read(&bundle)
                    .map_err(|e| {
                        ArgusError::new(
                            codes::MISSING_FILE,
                            format!("cannot read {}: {e}", moves::rel(&lib.root, &bundle)),
                        )
                    })
                    .and_then(|b| {
                        crate::binary::decode_entities(&b, &|name| {
                            moves::rel(&lib.root, &dir.join(name))
                        })
                    });
                match read {
                    Ok(docs) => files.extend(docs.into_iter().map(|(n, d)| (dir.join(n), Some(d)))),
                    Err(e) => errors.push(e),
                }
            }
            files.sort_by(|a, b| a.0.cmp(&b.0));
            for (p, doc) in files {
                if let Some(id) = layer.id {
                    let file = moves::rel(&lib.root, &p);
                    let path = prefab_path(&file);
                    match lib.prefabs.get(&path) {
                        Some(old) => {
                            changes.push(Change::new("prefab", &path, id, &old.file, file.clone()))
                        }
                        None if !layers::owns(id, &path) => {
                            errors.push(layers::outside(id, "prefab", &path, file));
                            continue;
                        }
                        None => {}
                    }
                }
                let loaded = match doc {
                    Some(doc) => {
                        lib.insert(&p, doc);
                        Ok(())
                    }
                    None => lib.load_file(&p),
                };
                if let Err(e) = loaded {
                    errors.push(e);
                }
            }
        }
        (lib, errors, changes)
    }

    fn load_file(&mut self, abs: &Path) -> Result<()> {
        let file = moves::rel(&self.root, abs);
        let doc = EntityDoc::parse(&file, &tomlv::read(abs)?)?;
        self.insert(abs, doc);
        Ok(())
    }

    fn insert(&mut self, abs: &Path, doc: EntityDoc) {
        let file = moves::rel(&self.root, abs);
        let path = prefab_path(&file);
        self.prefabs
            .insert(path.clone(), PrefabFile { path, file, doc });
    }

    /// Re-read one prefab by reference path (after an edit), or drop it if
    /// the file is gone.
    pub fn reload(&mut self, path: &str) -> Result<()> {
        let abs = self.file_path(path);
        if argus_os::fs::exists(&abs) {
            self.load_file(&abs)
        } else {
            self.prefabs.remove(path);
            Ok(())
        }
    }

    /// Where a prefab path lives on disk.
    pub fn file_path(&self, path: &str) -> PathBuf {
        match self.prefabs.get(path) {
            // A mod's prefab lives in its directory.
            Some(p) => self.root.join(&p.file),
            None => self.root.join("prefabs").join(format!("{path}.toml")),
        }
    }

    /// Look one up.
    pub fn get(&self, path: &str) -> Option<&PrefabFile> {
        self.prefabs.get(path)
    }

    /// Set one field of a prefab, or of one of its children (`child`, a
    /// path such as `light`; empty for the prefab itself), in its file with
    /// its comments kept, or with `None` take the field out; then read it
    /// again. A prefab edited as a transaction (§3, phase 5 item 4): the
    /// caller re-resolves the instances. Returns the file's text as it
    /// was, which [`PrefabLibrary::put_text`] puts back.
    pub fn set_field(
        &mut self,
        registry: &argus_ecs::Registry,
        prefab: &str,
        child: &str,
        path: &str,
        value: Option<&argus_ecs::Value>,
    ) -> Result<String> {
        let Some(p) = self.prefabs.get(prefab) else {
            return Err(ArgusError::new(
                codes::MISSING_REFERENCE,
                format!("there is no prefab {prefab:?}"),
            )
            .hint("world.prefabs lists the prefabs; a prefab is a file under prefabs/."));
        };
        let (comp, field) = path.split_once('.').ok_or_else(|| {
            ArgusError::new(
                argus_ecs::codes::BAD_PATH,
                format!("{path:?} is not Component.field"),
            )
        })?;
        let checked = match value {
            None => None,
            Some(v) => {
                let (cid, fs) = registry.resolve_path(path)?;
                Some(match (fs.ty, v) {
                    // An entity is written to a file as its "@id".
                    (argus_ecs::FieldType::Entity, argus_ecs::Value::Str(_)) => v.clone(),
                    _ => registry.schema(cid).check(fs, v.clone())?,
                })
            }
        };
        let (abs, rel) = (self.root.join(&p.file), p.file.clone());
        let old = tomlv::read(&abs)?;
        let doc = crate::doc::edit_file(&abs, &rel, child, |t| match &checked {
            Some(v) => crate::doc::set_override(t, comp, field, v),
            None => {
                crate::doc::clear_override(t, comp, field);
                Ok(())
            }
        })?;
        self.prefabs.get_mut(prefab).expect("found").doc = doc;
        Ok(old)
    }

    /// Put a prefab's file back as `text` (the inverse of
    /// [`PrefabLibrary::set_field`]) and read it again.
    pub fn put_text(&mut self, prefab: &str, text: &str) -> Result<()> {
        let Some(p) = self.prefabs.get(prefab) else {
            return Err(ArgusError::new(
                codes::MISSING_REFERENCE,
                format!("there is no prefab {prefab:?}"),
            ));
        };
        let (abs, rel) = (self.root.join(&p.file), p.file.clone());
        let doc = EntityDoc::parse(&rel, text)?;
        argus_os::fs::write(&abs, text).map_err(|e| {
            ArgusError::new(codes::MISSING_FILE, format!("cannot write {rel}: {e}"))
        })?;
        self.prefabs.get_mut(prefab).expect("found").doc = doc;
        Ok(())
    }

    /// All prefab paths, sorted.
    pub fn paths(&self) -> Vec<&str> {
        self.prefabs.keys().map(String::as_str).collect()
    }

    /// Number of prefabs.
    pub fn len(&self) -> usize {
        self.prefabs.len()
    }

    /// True if empty.
    pub fn is_empty(&self) -> bool {
        self.prefabs.is_empty()
    }

    /// The inheritance chain starting at `path`: the prefab itself first,
    /// then its base, then the base's base. `at` is the `file:line` of the
    /// reference for the error, and `moves` lets a missing base say where it
    /// went.
    pub fn chain(
        &self,
        path: &str,
        at: &str,
        moves_log: &[MoveRecord],
    ) -> Result<Vec<&PrefabFile>> {
        let mut out: Vec<&PrefabFile> = Vec::new();
        let mut cur = path.to_string();
        let mut at = at.to_string();
        loop {
            if out.iter().any(|p| p.path == cur) {
                let loop_path: Vec<&str> = out.iter().map(|p| p.path.as_str()).collect();
                return Err(ArgusError::new(
                    codes::PREFAB_CYCLE,
                    format!(
                        "prefab {cur:?} inherits from itself: {} -> {cur}",
                        loop_path.join(" -> ")
                    ),
                )
                .hint("Remove the prefab key from one of the files in the loop.")
                .at(Some(at)));
            }
            let Some(p) = self.prefabs.get(&cur) else {
                return Err(moves::missing_reference("prefab", &cur, &at, moves_log));
            };
            out.push(p);
            match &p.doc.prefab {
                Some(base) => {
                    at = format!("{}:1", p.file);
                    cur = base.clone();
                }
                None => return Ok(out),
            }
        }
    }

    /// Prefabs whose chain includes `path` (itself included), sorted.
    pub fn dependents(&self, path: &str) -> Vec<&str> {
        self.prefabs
            .keys()
            .filter(|k| {
                self.chain(k, "", &[])
                    .map(|c| c.iter().any(|p| p.path == path))
                    .unwrap_or(false)
            })
            .map(String::as_str)
            .collect()
    }

    /// Write a prefab document to its file (creating directories), and
    /// reload it into the library.
    pub fn write(&mut self, path: &str, doc: &EntityDoc) -> Result<()> {
        let abs = self.file_path(path);
        if let Some(dir) = abs.parent() {
            argus_os::fs::create_dir_all(dir).map_err(|e| {
                ArgusError::new(
                    codes::MISSING_FILE,
                    format!("cannot create {}: {e}", dir.display()),
                )
            })?;
        }
        argus_os::fs::write(&abs, doc.to_toml_string()?).map_err(|e| {
            ArgusError::new(
                codes::MISSING_FILE,
                format!("cannot write {}: {e}", abs.display()),
            )
        })?;
        self.load_file(&abs)
    }
}

/// A prefab's reference path from its file (spelled from the game's root):
/// `prefabs/door/locked.toml` and `mods/<id>/prefabs/door/locked.toml` are
/// both `door/locked`.
fn prefab_path(file: &str) -> String {
    let inner = match layers::owner_of(file) {
        Some(id) => file
            .strip_prefix(&format!("{}/", layers::dir_of(&id)))
            .unwrap_or(file),
        None => file,
    };
    inner
        .strip_prefix("prefabs/")
        .and_then(|r| r.strip_suffix(".toml"))
        .unwrap_or(inner)
        .to_string()
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = argus_os::fs::read_dir(dir) else {
        return;
    };
    for entry in rd {
        let p = entry.path;
        if entry.is_dir {
            walk(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("toml") {
            out.push(p);
        }
    }
}
