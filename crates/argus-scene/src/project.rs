//! The project (§4 "project layout"): `argus.toml` plus the content
//! directories. Opening a project reads the config, the prefab library, the
//! def tables, and the moves log; scenes are opened on demand.
//!
//! ```toml
//! # argus.toml
//! name          = "hollow"
//! clock         = "continuous@60"   # default sim clock; a scene may override
//! default_scene = "hollow"
//! plugins       = []
//! ```

use crate::codes;
use crate::defs::{DefSchema, Defs};
use crate::moves::{self, MoveRecord, MoveReport};
use crate::prefab::PrefabLibrary;
use crate::resolve::ResolveCtx;
use crate::tomlv;
use argus_ecs::{ArgusError, Registry, Result};
use argus_sim::Clock;
use std::path::{Path, PathBuf};

/// Contents of `argus.toml`.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectConfig {
    /// Project name.
    pub name: String,
    /// Default clock spec (`continuous@60` or `discrete`).
    pub clock: String,
    /// Scene opened by `session.open` when none is given.
    pub default_scene: Option<String>,
    /// Plugin crate names (phase 5; parsed now so the file shape is fixed).
    pub plugins: Vec<String>,
    /// `[ring] every`: ticks between keyframes for seek (phase 2). `None`:
    /// one second of the continuous clock, ten turns of the discrete one.
    pub ring_every: Option<u64>,
    /// `[ring] budget_mb`: memory the keyframes may use before the ring
    /// thins further than its age schedule. `None`: 1024.
    pub ring_budget_mb: Option<u64>,
    /// `[marks] key`: the key that drops a mark while playing, by the name
    /// the window sends (`F8`, `Backquote`, `KeyM`). `None`: F8 (§10).
    pub mark_key: Option<String>,
    /// `[time] day`: sim seconds in one of the game's days, which the
    /// harness's `days()` and `hours()` count in (phase 3, decided with
    /// Kevin: Outpost's day is 60 s, so §9's 30-day scenario fits §17's
    /// minute a seed). 86,400 when the project does not say.
    pub day_seconds: f64,
    /// `[strings] base`: the language the game is written in, whose string
    /// table (`strings/<base>.toml`) every other falls back to (phase 3
    /// item 4). "en" when the project does not say.
    pub strings_base: String,
}

/// A day, when a project does not say how long its days are.
pub const DAY_SECONDS: f64 = 86_400.0;

impl ProjectConfig {
    /// Read `argus.toml` under `root` without opening the rest of the project.
    pub fn load(root: &Path) -> Result<ProjectConfig> {
        let text = argus_os::fs::read_to_string(root.join("argus.toml")).map_err(|e| {
            ArgusError::new(
                codes::MISSING_FILE,
                format!("{} is not an Argus project: {e}", root.display()),
            )
            .hint("A project root has an argus.toml; run `argus init` to create one.")
        })?;
        ProjectConfig::parse(&text)
    }

    fn parse(text: &str) -> Result<ProjectConfig> {
        let im = tomlv::parse("argus.toml", text)?;
        let t = im.as_table();
        let at = |k: &str| {
            t.get(k)
                .map(|i| tomlv::item_ref("argus.toml", text, i))
                .unwrap_or_else(|| "argus.toml:1".to_string())
        };
        let name = t
            .get("name")
            .and_then(|i| i.as_str())
            .ok_or_else(|| {
                ArgusError::new(codes::BAD_SCENE, "argus.toml needs `name = \"...\"`")
                    .at(Some("argus.toml:1".to_string()))
            })?
            .to_string();
        let clock = t
            .get("clock")
            .and_then(|i| i.as_str())
            .unwrap_or("continuous@60")
            .to_string();
        Clock::parse(&clock).map_err(|e| e.at(Some(at("clock"))))?;
        let default_scene = t
            .get("default_scene")
            .and_then(|i| i.as_str())
            .map(str::to_string);
        let plugins = match t.get("plugins") {
            Some(i) => tomlv::string_list(i, "plugins", &at("plugins"))?,
            None => Vec::new(),
        };
        let ring = t.get("ring").and_then(|i| i.as_table_like());
        let positive = |key: &str| -> Result<Option<u64>> {
            match ring.and_then(|r| r.get(key)) {
                None => Ok(None),
                Some(i) => match i.as_integer() {
                    Some(n) if n >= 1 => Ok(Some(n as u64)),
                    _ => Err(ArgusError::new(
                        codes::BAD_SCENE,
                        format!("argus.toml [ring] {key} must be a whole number of at least 1"),
                    )
                    .at(Some(at("ring")))),
                },
            }
        };
        let ring_every = positive("every")?;
        let ring_budget_mb = positive("budget_mb")?;
        let mark_key = match t
            .get("marks")
            .and_then(|i| i.as_table_like())
            .and_then(|m| m.get("key"))
        {
            None => None,
            Some(i) => match i.as_str().map(str::trim) {
                Some(k) if !k.is_empty() && !k.contains(char::is_whitespace) => Some(k.to_string()),
                _ => {
                    return Err(ArgusError::new(
                        codes::BAD_SCENE,
                        "argus.toml [marks] key must be a key's name, like \"F8\" or \"Backquote\"",
                    )
                    .at(Some(at("marks"))))
                }
            },
        };
        let day_seconds =
            match t
                .get("time")
                .and_then(|i| i.as_table_like())
                .and_then(|m| m.get("day"))
            {
                None => DAY_SECONDS,
                Some(i) => match i.as_float().or_else(|| i.as_integer().map(|n| n as f64)) {
                    Some(d) if d > 0.0 && d.is_finite() => d,
                    _ => return Err(ArgusError::new(
                        codes::BAD_SCENE,
                        "argus.toml [time] day must be a positive number of sim seconds, like 60",
                    )
                    .at(Some(at("time")))),
                },
            };
        let strings_base = match t
            .get("strings")
            .and_then(|i| i.as_table_like())
            .and_then(|m| m.get("base"))
        {
            None => "en".to_string(),
            Some(i) => match i.as_str() {
                Some(s) if !s.trim().is_empty() => s.trim().to_string(),
                _ => {
                    return Err(ArgusError::new(
                        codes::BAD_SCENE,
                        "argus.toml [strings] base must be a language's code, like \"en\"",
                    )
                    .at(Some(at("strings"))))
                }
            },
        };
        Ok(ProjectConfig {
            name,
            clock,
            default_scene,
            plugins,
            ring_every,
            ring_budget_mb,
            mark_key,
            day_seconds,
            strings_base,
        })
    }

    fn to_toml(&self) -> String {
        let mut doc = toml_edit::DocumentMut::new();
        doc["name"] = toml_edit::value(self.name.as_str());
        doc["clock"] = toml_edit::value(self.clock.as_str());
        if let Some(s) = &self.default_scene {
            doc["default_scene"] = toml_edit::value(s.as_str());
        }
        doc["plugins"] = toml_edit::value(toml_edit::Array::from_iter(
            self.plugins.iter().map(String::as_str),
        ));
        doc.to_string()
    }
}

/// An open project.
#[derive(Debug)]
pub struct Project {
    /// Absolute root directory.
    pub root: PathBuf,
    /// `argus.toml`.
    pub config: ProjectConfig,
    /// `prefabs/`.
    pub prefabs: PrefabLibrary,
    /// `defs/`.
    pub defs: Defs,
    /// `.argus/moves.log`.
    pub moves: Vec<MoveRecord>,
    def_schemas: Vec<DefSchema>,
    issues: Vec<ArgusError>,
}

impl Project {
    /// Open the project at `root`. `def_schemas` are the declared table
    /// schemas (from script or Rust); tables without one are reported in
    /// [`Project::issues`].
    pub fn open(root: &Path, def_schemas: Vec<DefSchema>) -> Result<Project> {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let cfg_path = root.join("argus.toml");
        let text = argus_os::fs::read_to_string(&cfg_path).map_err(|e| {
            ArgusError::new(
                codes::MISSING_FILE,
                format!("{} is not an Argus project: {e}", root.display()),
            )
            .hint("A project root has an argus.toml; run `argus init` to create one.")
        })?;
        let config = ProjectConfig::parse(&text)?;
        let moves_log = moves::read_log(&root);
        let (prefabs, mut issues) = PrefabLibrary::load(&root);
        let (defs, def_issues) = Defs::load(&root, &def_schemas, &moves_log);
        issues.extend(def_issues);
        Ok(Project {
            root,
            config,
            prefabs,
            defs,
            moves: moves_log,
            def_schemas,
            issues,
        })
    }

    /// Create the §4 layout at `root` and open it. Existing files are kept.
    pub fn init(root: &Path, name: &str) -> Result<Project> {
        for d in [
            "components",
            "systems",
            "defs",
            "prefabs",
            "scenes",
            "assets",
            "tests",
            "plugins",
            ".argus",
        ] {
            argus_os::fs::create_dir_all(root.join(d)).map_err(|e| {
                ArgusError::new(codes::MISSING_FILE, format!("cannot create {d}: {e}"))
            })?;
        }
        let cfg = root.join("argus.toml");
        if !argus_os::fs::exists(&cfg) {
            let c = ProjectConfig {
                name: name.to_string(),
                clock: "continuous@60".to_string(),
                default_scene: None,
                plugins: Vec::new(),
                ring_every: None,
                ring_budget_mb: None,
                mark_key: None,
                day_seconds: DAY_SECONDS,
                strings_base: "en".to_string(),
            };
            argus_os::fs::write(&cfg, c.to_toml()).map_err(|e| {
                ArgusError::new(codes::MISSING_FILE, format!("cannot write argus.toml: {e}"))
            })?;
        }
        let gi = root.join(".gitignore");
        let existing = argus_os::fs::read_to_string(&gi).unwrap_or_default();
        if !existing.lines().any(|l| l.trim() == ".argus/") {
            let mut s = existing;
            if !s.is_empty() && !s.ends_with('\n') {
                s.push('\n');
            }
            s.push_str(".argus/\n");
            argus_os::fs::write(&gi, s).map_err(|e| {
                ArgusError::new(codes::MISSING_FILE, format!("cannot write .gitignore: {e}"))
            })?;
        }
        Project::open(root, Vec::new())
    }

    /// Problems found while opening (bad prefab files, def rows, missing
    /// def schemas). The project is usable; scenes that touch a broken
    /// prefab report it again as their own issue.
    pub fn issues(&self) -> &[ArgusError] {
        &self.issues
    }

    /// Scene names: every `scenes/<name>/scene.toml`, sorted.
    pub fn scenes(&self) -> Vec<String> {
        let mut out: Vec<String> = argus_os::fs::read_dir(self.root.join("scenes"))
            .map(|rd| {
                rd.into_iter()
                    .filter(|e| argus_os::fs::is_file(e.path.join("scene.toml")))
                    .filter_map(|e| e.path.file_name()?.to_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        out.sort();
        out
    }

    /// `scenes/<name>`.
    pub fn scene_dir(&self, name: &str) -> PathBuf {
        self.root.join("scenes").join(name)
    }

    /// `.argus/`.
    pub fn argus_dir(&self) -> PathBuf {
        self.root.join(".argus")
    }

    /// Project-relative spelling of a path.
    pub fn rel(&self, p: &Path) -> String {
        moves::rel(&self.root, p)
    }

    /// Re-read every prefab file.
    pub fn reload_prefabs(&mut self) {
        let (lib, issues) = PrefabLibrary::load(&self.root);
        self.prefabs = lib;
        self.issues
            .retain(|e| e.code != codes::BAD_TOML && e.code != codes::BAD_SCENE);
        self.issues.extend(issues);
    }

    /// Re-read every def table.
    pub fn reload_defs(&mut self) {
        let (defs, issues) = Defs::load(&self.root, &self.def_schemas, &self.moves);
        self.defs = defs;
        self.issues
            .retain(|e| e.code != codes::BAD_DEF && e.code != codes::MISSING_REFERENCE);
        self.issues.extend(issues);
    }

    /// Replace the def schemas (after a script reload) and re-read tables.
    pub fn set_def_schemas(&mut self, schemas: Vec<DefSchema>) {
        self.def_schemas = schemas;
        self.reload_defs();
    }

    /// The project's default clock.
    pub fn clock(&self) -> Result<Clock> {
        Clock::parse(&self.config.clock)
    }

    /// `asset.move`: see [`moves::move_asset`].
    pub fn move_asset(&mut self, from: &str, to: &str) -> Result<MoveReport> {
        moves::move_asset(self, from, to)
    }

    /// Everything resolution needs, for a given registry.
    pub fn ctx<'a>(&'a self, registry: &'a Registry) -> ResolveCtx<'a> {
        ResolveCtx {
            root: &self.root,
            prefabs: &self.prefabs,
            defs: &self.defs,
            moves: &self.moves,
            registry,
        }
    }
}
