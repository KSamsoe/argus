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
#[derive(Clone, Debug, PartialEq, Eq)]
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
}

impl ProjectConfig {
    /// Read `argus.toml` under `root` without opening the rest of the project.
    pub fn load(root: &Path) -> Result<ProjectConfig> {
        let text = std::fs::read_to_string(root.join("argus.toml")).map_err(|e| {
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
        Ok(ProjectConfig {
            name,
            clock,
            default_scene,
            plugins,
            ring_every,
            ring_budget_mb,
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
        let text = std::fs::read_to_string(&cfg_path).map_err(|e| {
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
            std::fs::create_dir_all(root.join(d)).map_err(|e| {
                ArgusError::new(codes::MISSING_FILE, format!("cannot create {d}: {e}"))
            })?;
        }
        let cfg = root.join("argus.toml");
        if !cfg.exists() {
            let c = ProjectConfig {
                name: name.to_string(),
                clock: "continuous@60".to_string(),
                default_scene: None,
                plugins: Vec::new(),
                ring_every: None,
                ring_budget_mb: None,
            };
            std::fs::write(&cfg, c.to_toml()).map_err(|e| {
                ArgusError::new(codes::MISSING_FILE, format!("cannot write argus.toml: {e}"))
            })?;
        }
        let gi = root.join(".gitignore");
        let existing = std::fs::read_to_string(&gi).unwrap_or_default();
        if !existing.lines().any(|l| l.trim() == ".argus/") {
            let mut s = existing;
            if !s.is_empty() && !s.ends_with('\n') {
                s.push('\n');
            }
            s.push_str(".argus/\n");
            std::fs::write(&gi, s).map_err(|e| {
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
        let mut out: Vec<String> = std::fs::read_dir(self.root.join("scenes"))
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().join("scene.toml").is_file())
                    .filter_map(|e| e.file_name().to_str().map(str::to_string))
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
