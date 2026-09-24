//! The prefab library (§3 "Prefabs and overrides"): every file under
//! `prefabs/`, keyed by its path without the directory and extension
//! (`prefabs/door/locked.toml` is `door/locked`). A prefab is an entity
//! document whose `prefab` key names its base; [`PrefabLibrary::chain`]
//! walks base links and reports cycles and missing bases as errors.

use crate::codes;
use crate::doc::EntityDoc;
use crate::moves::{self, MoveRecord};
use crate::tomlv;
use argus_ecs::{ArgusError, Result};
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
        let mut lib = PrefabLibrary {
            root: root.to_path_buf(),
            prefabs: BTreeMap::new(),
        };
        let mut errors = Vec::new();
        let mut files = Vec::new();
        walk(&root.join("prefabs"), &mut files);
        files.sort();
        for p in files {
            if let Err(e) = lib.load_file(&p) {
                errors.push(e);
            }
        }
        (lib, errors)
    }

    fn load_file(&mut self, abs: &Path) -> Result<()> {
        let file = moves::rel(&self.root, abs);
        let path = file
            .strip_prefix("prefabs/")
            .and_then(|r| r.strip_suffix(".toml"))
            .unwrap_or(&file)
            .to_string();
        let text = tomlv::read(abs)?;
        let doc = EntityDoc::parse(&file, &text)?;
        self.prefabs
            .insert(path.clone(), PrefabFile { path, file, doc });
        Ok(())
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
        self.root.join("prefabs").join(format!("{path}.toml"))
    }

    /// Look one up.
    pub fn get(&self, path: &str) -> Option<&PrefabFile> {
        self.prefabs.get(path)
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
