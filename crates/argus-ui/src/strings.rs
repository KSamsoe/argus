//! String tables (§12 "Dialogue and localization", phase 3 item 4): "all
//! player-facing strings go through a string table keyed by id; the agent
//! can author in one language and a translator can fill the others without
//! opening the engine."
//!
//! A language is a file, `strings/<code>.toml`: keys are TOML's own, tables
//! or dotted, so `[menu] resume = "Resume"` is `menu.resume`. The project's
//! base language (`[strings] base` in argus.toml, "en" when it says
//! nothing) is the one the game is written in; any other falls back to it
//! key by key, and a key no table has shows as `[key]`, which is what a
//! missing translation looks like on screen. A string may hold holes: a
//! `UiText`'s `{Component.field}` and `{@id:Component.field}`, a script's
//! `{name}` (`q:text("hud.day", {day = 3})`). By convention each file says
//! its own name as `language.name` ("Français"), for a settings screen.
//!
//! A release build ships the base language in the game's pack and each
//! other language in a pack of its own (§15: "a language can be added
//! without a rebuild"); they are all mounted, so a player can switch.

use argus_ecs::layers::{self, Change};
use argus_ecs::{ArgusError, Layers, Result, World};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Strings the engine itself shows, used when no table has the key.
pub const ENGINE: &[(&str, &str)] = &[
    ("dialogue.continue", "Continue"),
    ("dialogue.end", "Goodbye"),
    ("input.press_a_key", "press a key"),
];

/// A project's string tables, every language.
#[derive(Clone, Debug, Default)]
pub struct Strings {
    base: String,
    langs: BTreeMap<String, BTreeMap<String, String>>,
    problems: Vec<String>,
    /// Strings a mod changed (phase 3 item 8).
    changes: Vec<Change>,
    /// The mod file each string a mod wrote came from, by (language, key).
    origins: BTreeMap<(String, String), String>,
    /// The mod file that first wrote each key a mod added.
    made: BTreeMap<String, String>,
}

fn bad(msg: impl Into<String>) -> ArgusError {
    ArgusError::new(crate::codes::BAD_STRINGS, msg).hint(
        "strings/<language>.toml holds strings only: key = \"text\", grouped in tables or dotted keys ([menu] resume = \"Resume\" is menu.resume).",
    )
}

/// The file of a language.
pub fn file_of(root: &Path, lang: &str) -> PathBuf {
    root.join("strings").join(format!("{lang}.toml"))
}

fn flatten(
    prefix: &str,
    t: &dyn toml_edit::TableLike,
    out: &mut BTreeMap<String, String>,
    file: &str,
    problems: &mut Vec<String>,
) {
    for (k, v) in t.iter() {
        let key = if prefix.is_empty() {
            k.to_string()
        } else {
            format!("{prefix}.{k}")
        };
        if let Some(s) = v.as_str() {
            out.insert(key, s.to_string());
        } else if let Some(sub) = v.as_table_like() {
            flatten(&key, sub, out, file, problems);
        } else {
            problems.push(format!("{file}: {key} is not a string"));
        }
    }
}

impl Strings {
    /// Every `strings/*.toml` under `root` (read through the project's file
    /// system, so a release build's packs answer), with `base` the language
    /// the others fall back to. A file that does not parse is a problem, and
    /// the rest still load.
    pub fn load(root: &Path, base: &str) -> Strings {
        let mut s = Strings {
            base: base.to_string(),
            ..Strings::default()
        };
        let dir = root.join("strings");
        let mut files: Vec<PathBuf> = argus_os::fs::read_dir(&dir)
            .map(|rd| {
                rd.into_iter()
                    .filter(|e| !e.is_dir)
                    .map(|e| e.path)
                    .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("toml"))
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        for p in files {
            let lang = p
                .file_stem()
                .and_then(|x| x.to_str())
                .unwrap_or_default()
                .to_string();
            let rel = format!("strings/{lang}.toml");
            match argus_os::fs::read_to_string(&p) {
                Ok(text) => s.add(&lang, &rel, &text),
                Err(e) => s.problems.push(format!("{rel}: {e}")),
            }
        }
        s
    }

    /// [`load`](Strings::load) for the game and its mods (phase 3 item 8):
    /// each mod's `strings/<code>.toml` after the game's. A key in the
    /// mod's namespace (`lanterns.keeper.hello`) is added; a key some
    /// table already has (the game's, in any language) is changed, the
    /// later mod winning, and reported; any other key is a problem, and
    /// left out.
    pub fn load_layers(layers: &Layers, base: &str) -> Strings {
        let mut s = Strings::load(&layers.game, base);
        for l in layers.each().into_iter().skip(1) {
            let Some(id) = l.id else { continue };
            let dir = l.root.join("strings");
            let mut files: Vec<PathBuf> = argus_os::fs::read_dir(&dir)
                .map(|rd| {
                    rd.into_iter()
                        .filter(|e| !e.is_dir)
                        .map(|e| e.path)
                        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("toml"))
                        .collect()
                })
                .unwrap_or_default();
            files.sort();
            for p in files {
                let lang = p
                    .file_stem()
                    .and_then(|x| x.to_str())
                    .unwrap_or_default()
                    .to_string();
                let rel = l.show(&format!("strings/{lang}.toml"));
                let text = match argus_os::fs::read_to_string(&p) {
                    Ok(t) => t,
                    Err(e) => {
                        s.problems.push(format!("{rel}: {e}"));
                        continue;
                    }
                };
                let doc = match text.parse::<toml_edit::ImDocument<String>>() {
                    Ok(d) => d,
                    Err(e) => {
                        s.problems
                            .push(format!("{rel} does not parse: {}", e.message().trim()));
                        continue;
                    }
                };
                let mut table = BTreeMap::new();
                flatten("", doc.as_table(), &mut table, &rel, &mut s.problems);
                for (k, v) in table {
                    // Who made the key: this mod (in another language), an
                    // earlier one, or the game.
                    let maker = s.made.get(&k).cloned();
                    let had = s.langs.values().any(|t| t.contains_key(&k));
                    let theirs = had
                        && maker
                            .as_deref()
                            .is_none_or(|m| layers::owner_of(m).as_deref() != Some(id));
                    let was = theirs.then(|| s.origin(&lang, &k));
                    match was {
                        Some(from) => {
                            s.changes
                                .push(Change::new("string", &k, id, &from, rel.clone()))
                        }
                        None if had || layers::owns(id, &k) => {
                            s.made.entry(k.clone()).or_insert_with(|| rel.clone());
                        }
                        None => {
                            s.problems.push(format!(
                                "{rel}: mod {id}'s string {k:?} is outside its namespace and names no key the game or an earlier mod has; a mod's keys start with its id, like {id}.{k}"
                            ));
                            continue;
                        }
                    }
                    s.origins.insert((lang.clone(), k.clone()), rel.clone());
                    s.langs.entry(lang.clone()).or_default().insert(k, v);
                }
            }
        }
        s
    }

    /// Where a key's string in `lang` came from, for a change's report:
    /// the mod's file that wrote it last, else the game's.
    fn origin(&self, lang: &str, key: &str) -> String {
        self.origins
            .get(&(lang.to_string(), key.to_string()))
            .or_else(|| self.made.get(key))
            .cloned()
            .unwrap_or_else(|| format!("strings/{lang}.toml"))
    }

    /// What mods changed of the game's (or an earlier mod's) strings.
    pub fn changes(&self) -> &[Change] {
        &self.changes
    }

    /// A table from text (tests, and a file just written).
    pub fn add(&mut self, lang: &str, file: &str, text: &str) {
        match text.parse::<toml_edit::ImDocument<String>>() {
            Ok(doc) => {
                let mut table = BTreeMap::new();
                flatten("", doc.as_table(), &mut table, file, &mut self.problems);
                self.langs.insert(lang.to_string(), table);
            }
            Err(e) => self
                .problems
                .push(format!("{file} does not parse: {}", e.message().trim())),
        }
    }

    /// Tables built from `(language, text)` pairs.
    pub fn from_texts(base: &str, texts: &[(&str, &str)]) -> Strings {
        let mut s = Strings {
            base: base.to_string(),
            ..Strings::default()
        };
        for (l, t) in texts {
            s.add(l, &format!("strings/{l}.toml"), t);
        }
        s
    }

    /// The base language.
    pub fn base(&self) -> &str {
        &self.base
    }

    /// The languages there are tables for, the base first.
    pub fn languages(&self) -> Vec<String> {
        let mut v = vec![self.base.clone()];
        v.extend(self.langs.keys().filter(|l| **l != self.base).cloned());
        v
    }

    /// Whether there is a table for `lang` (the base always counts).
    pub fn has(&self, lang: &str) -> bool {
        lang == self.base || self.langs.contains_key(lang)
    }

    /// A key's string in `lang`, else the base's, else the engine's own.
    pub fn get(&self, lang: &str, key: &str) -> Option<&str> {
        self.langs
            .get(lang)
            .and_then(|t| t.get(key))
            .or_else(|| self.langs.get(&self.base).and_then(|t| t.get(key)))
            .map(String::as_str)
            .or_else(|| ENGINE.iter().find(|(k, _)| *k == key).map(|(_, v)| *v))
    }

    /// A key's string, or `[key]` where no table has it.
    pub fn text(&self, lang: &str, key: &str) -> String {
        match self.get(lang, key) {
            Some(s) => s.to_string(),
            None => format!("[{key}]"),
        }
    }

    /// One language's own table, without falling back.
    pub fn table(&self, lang: &str) -> Option<&BTreeMap<String, String>> {
        self.langs.get(lang)
    }

    /// The base's keys `lang` does not have.
    pub fn missing(&self, lang: &str) -> Vec<String> {
        let Some(base) = self.langs.get(&self.base) else {
            return Vec::new();
        };
        let have = self.langs.get(lang);
        base.keys()
            .filter(|k| have.is_none_or(|t| !t.contains_key(*k)))
            .cloned()
            .collect()
    }

    /// Files that did not read.
    pub fn problems(&self) -> &[String] {
        &self.problems
    }
}

/// The language the world is in: the first `UiSettings.language`, or
/// empty (the base) when none says.
pub fn language(world: &World) -> String {
    let Some(id) = world.component_id_of::<crate::UiSettings>() else {
        return String::new();
    };
    let mut all = world.query().has(id).entities();
    all.sort();
    all.first()
        .and_then(|e| world.get::<crate::UiSettings>(*e).ok())
        .map(|s| world.strings().resolve(s.language).to_string())
        .unwrap_or_default()
}

/// The touch controls' words in the world's language: a control's label is
/// a string key when the tables have it, else the key `touch.<name>` is,
/// else its words are shown as written (an unlabelled button's are its
/// action's name). So a game translates its controls in its string tables,
/// as it does the rest of its words.
pub fn touch_words(world: &World, strings: &Strings, controls: &mut [argus_input::TouchShown]) {
    if controls.is_empty() {
        return;
    }
    let lang = language(world);
    for c in controls {
        let found = strings
            .get(&lang, &c.label)
            .or_else(|| strings.get(&lang, &format!("touch.{}", c.name)));
        if let Some(s) = found {
            c.label = s.to_string();
        }
    }
}

/// Fill `{name}` holes from `args`; other holes are left for the world's.
pub fn fill_args(text: &str, args: &[(String, String)]) -> String {
    if args.is_empty() || !text.contains('{') {
        return text.to_string();
    }
    let mut out = text.to_string();
    for (k, v) in args {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

/// Set (or with `None` remove) one key in `strings/<lang>.toml`, keeping
/// the file's layout and comments; the file is made if there is none.
/// Returns the text the key had.
pub fn set_in_file(
    root: &Path,
    lang: &str,
    key: &str,
    text: Option<&str>,
) -> Result<Option<String>> {
    if key.is_empty() || key.split('.').any(|p| p.trim().is_empty()) {
        return Err(bad(format!("{key:?} is not a string key")));
    }
    let path = file_of(root, lang);
    let src = argus_os::fs::read_to_string(&path).unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = src.parse().map_err(|e: toml_edit::TomlError| {
        bad(format!(
            "strings/{lang}.toml does not parse: {}",
            e.message().trim()
        ))
    })?;
    let parts: Vec<&str> = key.split('.').collect();
    let (leaf, groups) = parts.split_last().expect("not empty");
    let mut t: &mut dyn toml_edit::TableLike = doc.as_table_mut();
    for g in groups {
        if t.get(g).is_none() {
            if text.is_none() {
                return Ok(None);
            }
            let mut nt = toml_edit::Table::new();
            nt.set_implicit(true);
            t.insert(g, toml_edit::Item::Table(nt));
        }
        let item = t.get_mut(g).expect("just made");
        t = item.as_table_like_mut().ok_or_else(|| {
            bad(format!(
                "strings/{lang}.toml: {g} is a string, so {key} cannot be in it"
            ))
        })?;
    }
    let old = t.get(leaf).and_then(|i| i.as_str()).map(str::to_string);
    if t.get(leaf).is_some_and(|i| i.as_table_like().is_some()) {
        return Err(bad(format!(
            "strings/{lang}.toml: {key} is a group of strings, not one"
        )));
    }
    match text {
        // The words change; a comment beside them stays.
        Some(s) => match t.get_mut(leaf).and_then(|i| i.as_value_mut()) {
            Some(v) => {
                let decor = v.decor().clone();
                *v = toml_edit::Value::from(s);
                *v.decor_mut() = decor;
            }
            None => {
                t.insert(leaf, toml_edit::value(s));
            }
        },
        None => {
            t.remove(leaf);
        }
    }
    let _ = argus_os::fs::create_dir_all(root.join("strings"));
    argus_os::fs::write(&path, doc.to_string())
        .map_err(|e| bad(format!("cannot write strings/{lang}.toml: {e}")))?;
    Ok(old)
}
