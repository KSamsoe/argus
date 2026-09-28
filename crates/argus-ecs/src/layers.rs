//! Content layers (§15 "Modding", phase 3 item 8): the game, then its mods
//! in load order, each read by the same loaders.
//!
//! A mod is a directory shaped like a small project (`components/`,
//! `systems/`, `defs/`, `scenes/`, `assets/`, `strings/`, `audio.toml`),
//! found at `mods/<id>/` under the game's root: on disk in a project, in
//! memory in a release build that mounted it from the player's mods
//! folder. Every loader reads the game's files and then each mod's, so a
//! path in any file (a tilemap, a baked grid) is spelled from the game's
//! root, a mod's own files under `mods/<id>/`.
//!
//! Two rules, the §15 "load-order rule and a namespace prefix on mod
//! content":
//!
//! - **The namespace.** Everything a mod adds is named with its id first,
//!   then one of `.`, `_`, `/`, or `-`, whichever the kind of name uses:
//!   the `lanterns` mod adds the system `lanterns.flicker`, the component
//!   `lanterns_Lantern`, the asset `lanterns/lantern`, the room
//!   `lanterns-vigil`. A name without its id is refused, unless it names
//!   something a layer before it has, which the mod then changes.
//! - **The load order.** The game first, then the mods in the order
//!   declared; where two layers give the same thing (a def row, a string,
//!   an asset, a sound, an entity), the later one wins, and each such
//!   change is reported with who made it.
//!
//! Scripts, components, and def tables are never changed by a mod, only
//! added: a mod's systems run beside the game's, and a component or table
//! the game declares stays the game's.

use crate::ArgusError;
use std::path::{Path, PathBuf};

/// Error codes for mods: the 60xx block.
pub mod codes {
    /// A mod's `mod.toml`, pack, or load order is not usable.
    pub const BAD_MOD: u32 = 6001;
    /// A mod names something outside its namespace that no layer before it
    /// has, or changes what a mod may only add.
    pub const NAMESPACE: u32 = 6002;
    /// A mod needs another that does not load before it, or is for another
    /// game.
    pub const LOAD_ORDER: u32 = 6003;
    /// The game does not take mods (`[mods] enabled = false`).
    pub const MODS_OFF: u32 = 6004;
}

/// Where a mod's files are under the game's root: `mods/<id>`.
pub fn dir_of(id: &str) -> String {
    format!("mods/{id}")
}

/// A mod's id: a lowercase letter, then lowercase letters, digits, or `_`.
/// It is the namespace prefix, and the mod's directory under `mods/`.
pub fn valid_id(id: &str) -> bool {
    let mut c = id.chars();
    c.next().is_some_and(|f| f.is_ascii_lowercase())
        && c.all(|x| x.is_ascii_lowercase() || x.is_ascii_digit() || x == '_')
        && id.len() <= 40
}

/// Whether `name` is in mod `id`'s namespace: the id, then one of `.`,
/// `_`, `/`, `-`, then at least one more character.
pub fn owns(id: &str, name: &str) -> bool {
    let name = name.strip_prefix('@').unwrap_or(name);
    name.strip_prefix(id).is_some_and(|rest| {
        rest.len() > 1 && matches!(rest.as_bytes()[0], b'.' | b'_' | b'/' | b'-')
    })
}

/// The refusal for a name outside a mod's namespace: `kind` says what it
/// is ("system", "asset"), `at` where the mod wrote it.
pub fn outside(id: &str, kind: &str, name: &str, at: impl Into<String>) -> ArgusError {
    ArgusError::new(
        codes::NAMESPACE,
        format!("mod {id}'s {kind} {name:?} is outside its namespace and names nothing the game or an earlier mod has"),
    )
    .hint(format!(
        "Everything a mod adds starts with its id and a separator ({id}.name, {id}_Name, {id}/name, or {id}-name); a name without it may only change something a layer before it already has."
    ))
    .at(Some(at.into()))
}

/// The refusal for a mod changing what a mod may only add to (a system, a
/// component, a def table).
pub fn not_changeable(id: &str, kind: &str, name: &str, at: impl Into<String>) -> ArgusError {
    ArgusError::new(
        codes::NAMESPACE,
        format!("mod {id} declares the {kind} {name:?}, which is the game's (or an earlier mod's)"),
    )
    .hint(format!(
        "A mod adds {kind}s under its own id ({id}.name, {id}_Name); it cannot replace or redeclare one it did not make."
    ))
    .at(Some(at.into()))
}

/// Something a mod changed that a layer before it had: the load-order
/// rule at work, reported so a person or an agent can see who changed
/// what.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// What kind of thing: "def row", "string", "asset", "sound", "track",
    /// "entity", "prefab".
    pub kind: String,
    /// Its name.
    pub name: String,
    /// The mod that changed it.
    pub by: String,
    /// Whose it was: "the game", or "mod <id>".
    pub over: String,
    /// Where the mod wrote it.
    pub at: String,
}

impl Change {
    /// A change by mod `by` of `name`, which the layer that wrote `was_at`
    /// (a path from the game's root) had.
    pub fn new(kind: &str, name: &str, by: &str, was_at: &str, at: impl Into<String>) -> Change {
        Change {
            kind: kind.to_string(),
            name: name.to_string(),
            by: by.to_string(),
            over: whose(was_at),
            at: at.into(),
        }
    }
}

/// "the game", or "mod <id>" when `at` (a path from the game's root) is
/// under a mod's directory.
pub fn whose(at: &str) -> String {
    match owner_of(at) {
        Some(id) => format!("mod {id}"),
        None => "the game".to_string(),
    }
}

/// One mod over the game: its id, and where its files are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModLayer {
    /// The id: the namespace prefix, and the directory under `mods/`.
    pub id: String,
    /// Its root: `<game>/mods/<id>`.
    pub root: PathBuf,
    /// `mods/<id>/`: how a path under it is spelled from the game's root.
    pub prefix: String,
}

impl ModLayer {
    /// Mod `id` of the game at `game`.
    pub fn new(game: &Path, id: &str) -> ModLayer {
        ModLayer {
            id: id.to_string(),
            root: game.join("mods").join(id),
            prefix: format!("{}/", dir_of(id)),
        }
    }
}

/// The game and its mods, in load order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layers {
    /// The game's root.
    pub game: PathBuf,
    /// The mods, in load order.
    pub mods: Vec<ModLayer>,
}

/// One layer as a loader walks them.
#[derive(Clone, Copy, Debug)]
pub struct Layer<'a> {
    /// The mod's id; `None` for the game.
    pub id: Option<&'a str>,
    /// Its root.
    pub root: &'a Path,
    /// What a path under it is spelled with from the game's root: empty
    /// for the game, `mods/<id>/` for a mod.
    pub prefix: &'a str,
}

impl Layer<'_> {
    /// A path under this layer, spelled from the game's root.
    pub fn show(&self, rel: &str) -> String {
        format!("{}{rel}", self.prefix)
    }

    /// Who this layer is, for a report: "the game" or "mod <id>".
    pub fn who(&self) -> String {
        match self.id {
            Some(id) => format!("mod {id}"),
            None => "the game".to_string(),
        }
    }
}

impl Layers {
    /// The game alone.
    pub fn game(root: &Path) -> Layers {
        Layers {
            game: root.to_path_buf(),
            mods: Vec::new(),
        }
    }

    /// The game with mods `ids`, in that order, each at `mods/<id>`.
    pub fn with_mods(root: &Path, ids: &[String]) -> Layers {
        Layers {
            game: root.to_path_buf(),
            mods: ids.iter().map(|id| ModLayer::new(root, id)).collect(),
        }
    }

    /// The mods' ids, in load order.
    pub fn ids(&self) -> Vec<String> {
        self.mods.iter().map(|m| m.id.clone()).collect()
    }

    /// Whether there are any mods.
    pub fn has_mods(&self) -> bool {
        !self.mods.is_empty()
    }

    /// Every layer, the game first.
    pub fn each(&self) -> Vec<Layer<'_>> {
        let mut out = vec![Layer {
            id: None,
            root: &self.game,
            prefix: "",
        }];
        out.extend(self.mods.iter().map(|m| Layer {
            id: Some(m.id.as_str()),
            root: &m.root,
            prefix: m.prefix.as_str(),
        }));
        out
    }

    /// The loaded mod a path (spelled from the game's root) belongs to.
    pub fn owner(&self, rel: &str) -> Option<&str> {
        let id = owner_of(rel)?;
        self.mods.iter().find(|m| m.id == id).map(|m| m.id.as_str())
    }
}

/// The mod whose directory a path spelled from the game's root is under
/// (`mods/<id>/...`), whether or not it is loaded.
pub fn owner_of(rel: &str) -> Option<String> {
    let rel = rel.replace('\\', "/");
    let tail = rel
        .strip_prefix("./")
        .unwrap_or(&rel)
        .strip_prefix("mods/")?;
    let id = tail.split('/').next()?;
    valid_id(id).then(|| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_the_mods_when_it_starts_with_its_id_and_a_separator() {
        assert!(owns("lanterns", "lanterns.flicker"));
        assert!(owns("lanterns", "lanterns_Lantern"));
        assert!(owns("lanterns", "lanterns/lantern"));
        assert!(owns("lanterns", "lanterns-vigil"));
        assert!(owns("lanterns", "@lanterns.keeper"));
        assert!(!owns("lanterns", "lanterns"));
        assert!(!owns("lanterns", "lanterns."));
        assert!(!owns("lanterns", "lanternsx.y"));
        assert!(!owns("lanterns", "Lantern"));
        assert!(!owns("lantern", "lanterns.x"));
    }

    #[test]
    fn ids_are_lowercase_words() {
        assert!(valid_id("lanterns"));
        assert!(valid_id("hard_mode2"));
        assert!(!valid_id("Lanterns"));
        assert!(!valid_id("2fast"));
        assert!(!valid_id("a-b"));
        assert!(!valid_id(""));
    }

    #[test]
    fn a_path_under_mods_belongs_to_that_mod() {
        assert_eq!(
            owner_of("mods/lanterns/systems/x.luau").as_deref(),
            Some("lanterns")
        );
        assert_eq!(
            owner_of("mods\\lanterns\\defs\\a.toml").as_deref(),
            Some("lanterns")
        );
        assert_eq!(owner_of("systems/x.luau"), None);
        assert_eq!(owner_of("mods/Bad/x"), None);
        let l = Layers::with_mods(Path::new("/g"), &["lanterns".to_string()]);
        let each = l.each();
        assert_eq!(each.len(), 2);
        assert_eq!(each[1].show("audio.toml"), "mods/lanterns/audio.toml");
        assert_eq!(l.owner("mods/lanterns/a"), Some("lanterns"));
        assert_eq!(l.owner("mods/other/a"), None);
    }
}
