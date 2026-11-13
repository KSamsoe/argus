//! Defs and scenes in a binary form of the same schema (section 15: "A
//! release pack is built from the import cache, not the source tree: ...
//! defs and scenes in a binary form of the same schema"; phase 5 item 1).
//!
//! What is stored is what the loaders make of the TOML: an entity file's
//! [`EntityDoc`], a def table's rows. Values stay as TOML values until a
//! schema types them, exactly as when they are read from text, and every
//! one keeps its line, so an error in a release build names the source
//! line it came from. A release pack holds, in place of the TOML:
//!
//! - `entities.bin` in a scene's or a room's directory: every entity file
//!   there that parsed, by its stem;
//! - `prefabs/prefabs.bin`: every prefab file, by its path under `prefabs/`;
//! - `defs/<table>.bin`: a table's rows.
//!
//! A file that did not parse stays as TOML, so the game reports it as it
//! would have.

use crate::doc::{ComponentDoc, EntityDoc, FieldDoc};
use argus_ecs::codec::{Reader, Writer};
use argus_ecs::Result;
use std::collections::HashMap;
use toml_edit::{Array, InlineTable, Value as TValue};

/// A scene's or room's entities, in its directory.
pub const ENTITIES: &str = "entities.bin";
/// A layer's prefabs, in its `prefabs/`.
pub const PREFABS: &str = "prefabs/prefabs.bin";

const ENTITIES_TAG: &[u8; 4] = b"ARGE";
const DEFS_TAG: &[u8; 4] = b"ARGD";
const VERSION: u32 = 1;

/// One row of a def table as the file wrote it: its key, where it is, and
/// its fields in file order (`None` for an entry that is not a value, which
/// the table reports when it types the row).
#[derive(Clone, Debug)]
pub struct RowDoc {
    /// The row's header.
    pub key: String,
    /// `file:line` of the row.
    pub at: String,
    /// Field name, value, and `file:line`.
    pub fields: Vec<(String, Option<TValue>, String)>,
}

/// Writes a bundle: every name and string once, in a table ahead of the
/// data that refers to them by number, and a place as its line in the file
/// its entry came from (Hollow's ten thousand entities said each file's
/// path in every field, and the bundles were three times the TOML).
struct Enc {
    body: Writer,
    strings: Vec<String>,
    index: HashMap<String, u32>,
}

impl Enc {
    fn new() -> Enc {
        Enc {
            body: Writer::new(),
            strings: Vec::new(),
            index: HashMap::new(),
        }
    }

    fn s(&mut self, x: &str) {
        let i = match self.index.get(x) {
            Some(&i) => i,
            None => {
                let i = self.strings.len() as u32;
                self.strings.push(x.to_string());
                self.index.insert(x.to_string(), i);
                i
            }
        };
        self.body.u32(i);
    }

    fn strs(&mut self, v: &[String]) {
        self.body.count(v.len());
        for s in v {
            self.s(s);
        }
    }

    /// `at` in `file`: its line, or it said whole when it is elsewhere.
    fn at(&mut self, at: &str, file: &str) {
        if at == file {
            self.body.u8(0);
        } else if let Some(line) = at
            .strip_prefix(file)
            .and_then(|r| r.strip_prefix(':'))
            .and_then(|l| l.parse::<u32>().ok())
        {
            self.body.u8(1);
            self.body.u32(line);
        } else {
            self.body.u8(2);
            self.s(at);
        }
    }

    fn value(&mut self, v: &TValue) {
        match v {
            TValue::String(s) => {
                self.body.u8(0);
                self.s(s.value());
            }
            TValue::Integer(i) => {
                self.body.u8(1);
                self.body.i64(*i.value());
            }
            TValue::Float(f) => {
                self.body.u8(2);
                self.body.f64(*f.value());
            }
            TValue::Boolean(b) => {
                self.body.u8(3);
                self.body.bool(*b.value());
            }
            TValue::Datetime(d) => {
                self.body.u8(4);
                self.s(&d.value().to_string());
            }
            TValue::Array(a) => {
                self.body.u8(5);
                self.body.count(a.len());
                for x in a.iter() {
                    self.value(x);
                }
            }
            TValue::InlineTable(t) => {
                self.body.u8(6);
                self.body.count(t.len());
                for (k, x) in t.iter() {
                    self.s(k);
                    self.value(x);
                }
            }
        }
    }

    fn entity(&mut self, d: &EntityDoc, file: &str) {
        match &d.prefab {
            Some(p) => {
                self.body.u8(1);
                self.s(p);
            }
            None => self.body.u8(0),
        }
        self.strs(&d.tags);
        self.strs(&d.removed_tags);
        self.body.count(d.components.len());
        for c in &d.components {
            self.s(&c.name);
            self.at(&c.at, file);
            self.body.count(c.fields.len());
            for f in &c.fields {
                self.s(&f.name);
                self.at(&f.at, file);
                self.value(&f.raw);
            }
        }
        self.strs(&d.removed);
        self.body.count(d.children.len());
        for (name, c) in &d.children {
            self.s(name);
            self.entity(c, file);
        }
        self.strs(&d.removed_children);
        self.at(&d.at, file);
    }

    fn finish(self, tag: &[u8; 4]) -> Vec<u8> {
        let mut head = Writer::new();
        head.magic(tag, VERSION);
        head.count(self.strings.len());
        for s in &self.strings {
            head.str(s);
        }
        let mut out = head.into_bytes();
        out.extend(self.body.into_bytes());
        out
    }
}

/// Reads what [`Enc`] wrote.
struct Dec<'a> {
    r: Reader<'a>,
    strings: Vec<String>,
}

impl<'a> Dec<'a> {
    fn open(bytes: &'a [u8], tag: &[u8; 4], what: &'static str) -> Result<Dec<'a>> {
        let mut r = Reader::new(bytes, what);
        r.magic(tag, VERSION)?;
        let strings = (0..r.count()?).map(|_| r.str()).collect::<Result<_>>()?;
        Ok(Dec { r, strings })
    }

    fn s(&mut self) -> Result<String> {
        let i = self.r.u32()? as usize;
        self.strings
            .get(i)
            .cloned()
            .ok_or_else(|| self.r.bad(format!("string {i} of {}", self.strings.len())))
    }

    fn strs(&mut self) -> Result<Vec<String>> {
        (0..self.r.count()?).map(|_| self.s()).collect()
    }

    fn at(&mut self, file: &str) -> Result<String> {
        Ok(match self.r.u8()? {
            0 => file.to_string(),
            1 => format!("{file}:{}", self.r.u32()?),
            2 => self.s()?,
            t => return Err(self.r.bad(format!("place tag {t}"))),
        })
    }

    fn value(&mut self) -> Result<TValue> {
        Ok(match self.r.u8()? {
            0 => TValue::from(self.s()?),
            1 => TValue::from(self.r.i64()?),
            2 => TValue::from(self.r.f64()?),
            3 => TValue::from(self.r.bool()?),
            4 => {
                let s = self.s()?;
                TValue::from(
                    s.parse::<toml_edit::Datetime>()
                        .map_err(|e| self.r.bad(format!("datetime {s:?}: {e}")))?,
                )
            }
            5 => {
                let mut a = Array::new();
                for _ in 0..self.r.count()? {
                    a.push(self.value()?);
                }
                TValue::Array(a)
            }
            6 => {
                let mut t = InlineTable::new();
                for _ in 0..self.r.count()? {
                    let k = self.s()?;
                    t.insert(&k, self.value()?);
                }
                TValue::InlineTable(t)
            }
            t => return Err(self.r.bad(format!("value tag {t}"))),
        })
    }

    fn entity(&mut self, file: &str) -> Result<EntityDoc> {
        let prefab = match self.r.u8()? {
            0 => None,
            _ => Some(self.s()?),
        };
        let tags = self.strs()?;
        let removed_tags = self.strs()?;
        let mut components = Vec::new();
        for _ in 0..self.r.count()? {
            let name = self.s()?;
            let at = self.at(file)?;
            let mut fields = Vec::new();
            for _ in 0..self.r.count()? {
                let name = self.s()?;
                let at = self.at(file)?;
                fields.push(FieldDoc {
                    name,
                    raw: self.value()?,
                    at,
                });
            }
            components.push(ComponentDoc { name, fields, at });
        }
        let removed = self.strs()?;
        let mut children = Vec::new();
        for _ in 0..self.r.count()? {
            let name = self.s()?;
            children.push((name, self.entity(file)?));
        }
        let removed_children = self.strs()?;
        let at = self.at(file)?;
        Ok(EntityDoc {
            prefab,
            tags,
            removed_tags,
            components,
            removed,
            children,
            removed_children,
            at,
        })
    }
}

/// Entity documents by name (a stem, or a path under `prefabs/`), each
/// with the file its places are in.
pub fn encode_entities(docs: &[(String, String, EntityDoc)]) -> Vec<u8> {
    let mut e = Enc::new();
    e.body.count(docs.len());
    for (name, file, d) in docs {
        e.s(name);
        e.entity(d, file);
    }
    e.finish(ENTITIES_TAG)
}

/// [`encode_entities`] read back; `file` names the file an entry's places
/// are in (where the loader would have read it).
pub fn decode_entities(
    bytes: &[u8],
    file: &dyn Fn(&str) -> String,
) -> Result<Vec<(String, EntityDoc)>> {
    let mut d = Dec::open(bytes, ENTITIES_TAG, "an entities bundle")?;
    (0..d.r.count()?)
        .map(|_| {
            let name = d.s()?;
            let doc = d.entity(&file(&name))?;
            Ok((name, doc))
        })
        .collect()
}

/// A def table's rows, and the TOML file they came from.
pub fn encode_rows(source: &str, rows: &[RowDoc]) -> Vec<u8> {
    let mut e = Enc::new();
    e.s(source);
    e.body.count(rows.len());
    for row in rows {
        e.s(&row.key);
        e.at(&row.at, source);
        e.body.count(row.fields.len());
        for (name, v, at) in &row.fields {
            e.s(name);
            e.at(at, source);
            match v {
                Some(v) => {
                    e.body.u8(1);
                    e.value(v);
                }
                None => e.body.u8(0),
            }
        }
    }
    e.finish(DEFS_TAG)
}

/// [`encode_rows`] read back: the source file and the rows.
pub fn decode_rows(bytes: &[u8]) -> Result<(String, Vec<RowDoc>)> {
    let mut d = Dec::open(bytes, DEFS_TAG, "a def table")?;
    let source = d.s()?;
    let mut rows = Vec::new();
    for _ in 0..d.r.count()? {
        let key = d.s()?;
        let at = d.at(&source)?;
        let mut fields = Vec::new();
        for _ in 0..d.r.count()? {
            let name = d.s()?;
            let at = d.at(&source)?;
            let v = match d.r.u8()? {
                0 => None,
                _ => Some(d.value()?),
            };
            fields.push((name, v, at));
        }
        rows.push(RowDoc { key, at, fields });
    }
    Ok((source, rows))
}

/// What [`pack`] made of a pack's files.
#[derive(Clone, Debug, Default)]
pub struct Packed {
    /// Entity files now in bundles.
    pub entities: usize,
    /// `entities.bin` bundles, one a scene or room directory.
    pub bundles: usize,
    /// Prefab files now in `prefabs/prefabs.bin`.
    pub prefabs: usize,
    /// Def tables now in binary.
    pub tables: usize,
    /// Files left as TOML because they did not parse: the game reports them
    /// as it would have.
    pub left_as_toml: Vec<String>,
}

enum Form {
    Entity { dir: String, stem: String },
    Prefab(String),
    Table(String),
}

/// What a pack path is to the loaders: an entity file directly in a scene's
/// or a room's directory, a prefab, or a def table.
fn form(path: &str) -> Option<Form> {
    if crate::bulk::is_data_index(path) {
        return None;
    }
    let stem = |f: &str| f.strip_suffix(".toml").map(str::to_string);
    match path.split('/').collect::<Vec<_>>().as_slice() {
        ["scenes", s, f] if *f != "scene.toml" => stem(f).map(|stem| Form::Entity {
            dir: format!("scenes/{s}"),
            stem,
        }),
        ["scenes", s, "rooms", r, f] if *f != "room.toml" => stem(f).map(|stem| Form::Entity {
            dir: format!("scenes/{s}/rooms/{r}"),
            stem,
        }),
        ["prefabs", ..] if path.ends_with(".toml") => {
            Some(Form::Prefab(path["prefabs/".len()..].to_string()))
        }
        ["defs", f] => stem(f).map(Form::Table),
        _ => None,
    }
}

/// Replace a pack's entity files, prefabs, and def tables with their binary
/// forms. `files` are paths in the pack with their bytes; `shown` turns one
/// into the project-relative path the loaders name in errors (a mod's are
/// under `mods/<id>/`).
pub fn pack(files: &mut Vec<(String, Vec<u8>)>, shown: &dyn Fn(&str) -> String) -> Packed {
    let mut out = Packed::default();
    let mut bundles: std::collections::BTreeMap<String, Vec<(String, String, EntityDoc)>> =
        Default::default();
    let mut prefabs: Vec<(String, String, EntityDoc)> = Vec::new();
    let mut tables: Vec<(String, Vec<u8>)> = Vec::new();
    files.retain(|(path, bytes)| {
        let (Some(form), Ok(text)) = (form(path), std::str::from_utf8(bytes)) else {
            return true;
        };
        let file = shown(path);
        let made = match form {
            Form::Entity { dir, stem } => EntityDoc::parse(&file, text)
                .map(|d| {
                    bundles
                        .entry(dir)
                        .or_default()
                        .push((stem, file.clone(), d))
                })
                .is_ok(),
            Form::Prefab(name) => EntityDoc::parse(&file, text)
                .map(|d| prefabs.push((name, file.clone(), d)))
                .is_ok(),
            Form::Table(name) => match crate::defs::row_docs(&file, text) {
                Ok(rows) if rows.iter().all(|r| r.is_ok()) => {
                    let rows: Vec<RowDoc> = rows.into_iter().flatten().collect();
                    tables.push((format!("defs/{name}.bin"), encode_rows(&file, &rows)));
                    true
                }
                _ => false,
            },
        };
        if !made {
            out.left_as_toml.push(path.clone());
        }
        !made
    });
    for (dir, mut docs) in bundles {
        // In the order the text loader spawns them, by file: the files came
        // in whatever order the caller listed them, which is the folder's
        // own on Linux, and a bundle spawned in another order gives its
        // entities other ids.
        docs.sort_by(|a, b| a.1.cmp(&b.1));
        out.entities += docs.len();
        out.bundles += 1;
        files.push((format!("{dir}/{ENTITIES}"), encode_entities(&docs)));
    }
    if !prefabs.is_empty() {
        out.prefabs = prefabs.len();
        files.push((PREFABS.to_string(), encode_entities(&prefabs)));
    }
    out.tables = tables.len();
    files.extend(tables);
    files.sort_by(|a, b| a.0.cmp(&b.0));
    out
}
