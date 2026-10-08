//! Asset sidecars edited through transactions (phase 4 item 8, carried in
//! from phase 4's planning): "with layout and comments kept, undoable per
//! author, in the feed, reloaded at commit", as `edit.text` does for
//! strings and `edit.def` for def tables.
//!
//! A value is named by its path in the sidecar: dotted keys, and `[n]` for
//! an array's element (`states.run.fps`, `states.run.events[0].frame`,
//! `states.idle.transitions[1].when`, `any[0].to`). `edit.asset {tx, asset,
//! path, value}` sets it (tables on the way are made; `[n]` one past the
//! end appends), `remove: true` takes it out. At commit the file is written
//! with its layout and comments kept and imported again; a sidecar the
//! importer refuses (an animator that does not parse) is put back and the
//! commit fails with the importer's sentence and line. The inverse is the
//! file as it was, so undo puts back its exact text. `asset.sidecar` reads
//! one as data, the paths the same.

use super::*;
use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, Value as TV};

/// One step of a path.
#[derive(Clone, Debug, PartialEq)]
enum Seg {
    Key(String),
    Index(usize),
}

fn bad_path(path: &str, why: &str) -> ArgusError {
    bad_params(format!("path {path:?}: {why}"))
        .hint("Dotted keys and [n] for an element: states.run.fps, states.run.events[0].frame, any[1].to.")
}

fn parse_path(path: &str) -> Result<Vec<Seg>> {
    let mut out = Vec::new();
    for part in path.split('.') {
        let (key, mut rest) = match part.find('[') {
            Some(i) => (&part[..i], &part[i..]),
            None => (part, ""),
        };
        if key.is_empty() && out.is_empty() {
            return Err(bad_path(path, "it starts with a key"));
        }
        if !key.is_empty() {
            out.push(Seg::Key(key.to_string()));
        }
        while !rest.is_empty() {
            let close = rest
                .find(']')
                .ok_or_else(|| bad_path(path, "a [ without its ]"))?;
            let n = rest[1..close]
                .parse::<usize>()
                .map_err(|_| bad_path(path, "an index is a whole number"))?;
            out.push(Seg::Index(n));
            rest = &rest[close + 1..];
            if !rest.is_empty() && !rest.starts_with('[') {
                return Err(bad_path(path, "a key after ] needs a dot"));
            }
        }
    }
    if out.is_empty() {
        return Err(bad_path(path, "it is empty"));
    }
    Ok(out)
}

/// A JSON value as a TOML value, an integer where the old one was one (or
/// where it is whole and there was none).
fn to_value(v: &Json, was: Option<&TV>) -> Result<TV> {
    Ok(match v {
        Json::Bool(b) => TV::from(*b),
        Json::Number(n) => {
            let int_before = matches!(was, Some(TV::Integer(_)));
            let float_before = matches!(was, Some(TV::Float(_)));
            match (n.as_i64(), n.as_f64()) {
                (Some(i), _) if !float_before => TV::from(i),
                (_, Some(f)) if int_before && f.fract() == 0.0 => TV::from(f as i64),
                (_, Some(f)) => TV::from(f),
                _ => return Err(bad_params(format!("{n} is not a number TOML holds"))),
            }
        }
        Json::String(s) => TV::from(s.as_str()),
        Json::Array(a) => {
            let mut arr = Array::new();
            for x in a {
                arr.push(to_value(x, None)?);
            }
            TV::Array(arr)
        }
        Json::Object(o) => {
            let mut t = InlineTable::new();
            for (k, x) in o {
                t.insert(k, to_value(x, None)?);
            }
            TV::InlineTable(t)
        }
        Json::Null => {
            return Err(bad_params(
                "a value is not null; remove: true takes one out",
            ))
        }
    })
}

/// A JSON object as a standard table (`[states.dash]`), for a table's key.
fn to_table(o: &serde_json::Map<String, Json>) -> Result<Table> {
    let mut t = Table::new();
    for (k, x) in o {
        t.insert(k, Item::Value(to_value(x, None)?));
    }
    Ok(t)
}

fn item_json(i: &Item) -> Json {
    match i {
        Item::None => Json::Null,
        Item::Value(v) => json::toml_to_json(v),
        Item::Table(t) => Json::Object(
            t.iter()
                .map(|(k, v)| (k.to_string(), item_json(v)))
                .collect(),
        ),
        Item::ArrayOfTables(a) => Json::Array(
            a.iter()
                .map(|t| {
                    Json::Object(
                        t.iter()
                            .map(|(k, v)| (k.to_string(), item_json(v)))
                            .collect(),
                    )
                })
                .collect(),
        ),
    }
}

/// The sidecar as data.
fn doc_json(doc: &DocumentMut) -> Json {
    Json::Object(
        doc.iter()
            .map(|(k, v)| (k.to_string(), item_json(v)))
            .collect(),
    )
}

/// The value at a path, as JSON, if there is one.
fn get_path(doc: &DocumentMut, segs: &[Seg]) -> Option<Json> {
    let mut cur = doc_json(doc);
    for s in segs {
        cur = match s {
            Seg::Key(k) => cur.get(k)?.clone(),
            Seg::Index(n) => cur.get(*n)?.clone(),
        };
    }
    Some(cur)
}

/// Set (or with `None` remove) the value at `segs` under an item.
fn put_item(item: &mut Item, segs: &[Seg], v: Option<&Json>, path: &str) -> Result<()> {
    let (first, rest) = segs.split_first().expect("a step");
    match (item, first) {
        (Item::Table(t), Seg::Key(k)) => {
            if rest.is_empty() {
                match v {
                    None => {
                        t.remove(k)
                            .ok_or_else(|| bad_path(path, &format!("there is no {k}")))?;
                    }
                    Some(Json::Object(o)) if !t.get(k).is_some_and(|i| i.is_value()) => {
                        t.insert(k, Item::Table(to_table(o)?));
                    }
                    Some(x) => {
                        let was = t.get(k).and_then(Item::as_value).cloned();
                        let mut nv = to_value(x, was.as_ref())?;
                        if let Some(w) = &was {
                            *nv.decor_mut() = w.decor().clone();
                        }
                        t.insert(k, Item::Value(nv));
                    }
                }
                return Ok(());
            }
            if !t.contains_key(k) {
                if v.is_none() {
                    return Err(bad_path(path, &format!("there is no {k}")));
                }
                match rest[0] {
                    Seg::Index(_) => t.insert(k, Item::Value(TV::Array(Array::new()))),
                    Seg::Key(_) => t.insert(k, Item::Table(Table::new())),
                };
            }
            put_item(t.get_mut(k).expect("just made"), rest, v, path)
        }
        (Item::ArrayOfTables(a), Seg::Index(n)) => {
            let n = *n;
            if rest.is_empty() {
                match v {
                    None if n < a.len() => {
                        a.remove(n);
                        Ok(())
                    }
                    Some(Json::Object(o)) if n == a.len() => {
                        a.push(to_table(o)?);
                        Ok(())
                    }
                    Some(Json::Object(o)) if n < a.len() => {
                        *a.get_mut(n).expect("in range") = to_table(o)?;
                        Ok(())
                    }
                    _ => Err(bad_path(
                        path,
                        &format!("element {n} of {} (tables only)", a.len()),
                    )),
                }
            } else {
                let len = a.len();
                let t = a
                    .get_mut(n)
                    .ok_or_else(|| bad_path(path, &format!("no element {n}; there are {len}")))?;
                let mut item = Item::Table(std::mem::take(t));
                let r = put_item(&mut item, rest, v, path);
                if let Item::Table(back) = item {
                    *a.get_mut(n).expect("in range") = back;
                }
                r
            }
        }
        (Item::Value(val), _) => put_value(val, segs, v, path),
        (_, Seg::Index(n)) => Err(bad_path(
            path,
            &format!("[{n}] of something that is not an array"),
        )),
        (_, Seg::Key(k)) => Err(bad_path(
            path,
            &format!("{k} of something that is not a table"),
        )),
    }
}

/// The same, inside an inline value (`events = [{frame = 0, ...}]`).
fn put_value(val: &mut TV, segs: &[Seg], v: Option<&Json>, path: &str) -> Result<()> {
    let (first, rest) = segs.split_first().expect("a step");
    match (val, first) {
        (TV::InlineTable(t), Seg::Key(k)) => {
            if rest.is_empty() {
                match v {
                    None => {
                        t.remove(k)
                            .ok_or_else(|| bad_path(path, &format!("there is no {k}")))?;
                    }
                    Some(x) => {
                        let was = t.get(k).cloned();
                        t.insert(k, to_value(x, was.as_ref())?);
                    }
                }
                return Ok(());
            }
            if !t.contains_key(k) {
                if v.is_none() {
                    return Err(bad_path(path, &format!("there is no {k}")));
                }
                match rest[0] {
                    Seg::Index(_) => t.insert(k, TV::Array(Array::new())),
                    Seg::Key(_) => t.insert(k, TV::InlineTable(InlineTable::new())),
                };
            }
            put_value(t.get_mut(k).expect("just made"), rest, v, path)
        }
        (TV::Array(a), Seg::Index(n)) => {
            let n = *n;
            if rest.is_empty() {
                match v {
                    None if n < a.len() => {
                        a.remove(n);
                    }
                    Some(x) if n == a.len() => a.push(to_value(x, None)?),
                    Some(x) if n < a.len() => {
                        let was = a.get(n).cloned();
                        a.replace(n, to_value(x, was.as_ref())?);
                    }
                    _ => return Err(bad_path(path, &format!("element {n} of {}", a.len()))),
                }
                return Ok(());
            }
            let len = a.len();
            let el = a
                .get_mut(n)
                .ok_or_else(|| bad_path(path, &format!("no element {n}; there are {len}")))?;
            put_value(el, rest, v, path)
        }
        (_, Seg::Index(n)) => Err(bad_path(
            path,
            &format!("[{n}] of something that is not an array"),
        )),
        (_, Seg::Key(k)) => Err(bad_path(
            path,
            &format!("{k} of something that is not a table"),
        )),
    }
}

/// Set or remove a path's value in a sidecar's text: the new text.
pub(crate) fn edit_text(text: &str, path: &str, value: Option<&Json>) -> Result<String> {
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|e| bad_params(format!("the sidecar does not read as TOML: {e}")))?;
    let segs = parse_path(path)?;
    let mut root = Item::Table(std::mem::take(doc.as_table_mut()));
    let r = put_item(&mut root, &segs, value, path);
    if let Item::Table(t) = root {
        *doc.as_table_mut() = t;
    }
    r?;
    Ok(doc.to_string())
}

impl Daemon {
    /// An asset's sidecar: the file, its path from the project's root, and
    /// its text (empty for one that is not there yet).
    pub(super) fn sidecar_of(&self, asset: &str) -> Result<(PathBuf, String, String)> {
        let ses = self.require_session()?;
        let (file, shown) = ses.assets.sidecar_file(asset).ok_or_else(|| {
            ArgusError::new(codes::BAD_PARAMS, format!("no asset {asset:?}"))
                .hint("asset.list shows every asset; a name is its path under assets/ without the extension.")
        })?;
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        Ok((file, shown, text))
    }

    /// `asset.sidecar`: a sidecar as data, with the file it is.
    pub(super) fn asset_sidecar(&self, params: &Json) -> Result<Json> {
        let asset = json::str_param(params, "name")?;
        let (_, shown, text) = self.sidecar_of(asset)?;
        let doc: DocumentMut = text
            .parse()
            .map_err(|e| ArgusError::new(codes::BAD_PARAMS, format!("{shown}: {e}")))?;
        let kind = self
            .session
            .as_ref()
            .and_then(|s| s.assets.entries().get(asset).map(|e| e.kind()));
        let mut out = json!({ "name": asset, "kind": kind, "file": shown, "doc": doc_json(&doc) });
        if let Some(p) = json::opt_str(params, "path")? {
            out["value"] = get_path(&doc, &parse_path(p)?).unwrap_or(Json::Null);
        }
        Ok(out)
    }

    /// Write a sidecar's text and import it; if the importer refuses it,
    /// put the old text back and say why.
    fn write_sidecar(
        &mut self,
        asset: &str,
        file: &Path,
        shown: &str,
        old: &str,
        new: &str,
    ) -> Result<()> {
        if let Some(d) = file.parent() {
            std::fs::create_dir_all(d)
                .map_err(|e| ArgusError::new(codes::BAD_PARAMS, format!("{shown}: {e}")))?;
        }
        std::fs::write(file, new)
            .map_err(|e| ArgusError::new(codes::BAD_PARAMS, format!("{shown}: {e}")))?;
        // Read again whatever the stamp says: two writes in a millisecond
        // at one length (a commit and its undo) keep the file's stamp.
        let assets = &mut self.session.as_mut().expect("session").assets;
        assets.touch(asset);
        let report = assets.scan();
        let failed = report
            .failed
            .iter()
            .find(|f| f.name == asset)
            .map(|f| (f.error.message.clone(), f.error.r#ref.clone()));
        self.apply_asset_report(report);
        if let Some((message, at)) = failed {
            let _ = std::fs::write(file, old);
            let assets = &mut self.session.as_mut().expect("session").assets;
            assets.touch(asset);
            let report = assets.scan();
            self.apply_asset_report(report);
            let mut e = ArgusError::new(
                codes::BAD_PARAMS,
                format!("{shown} would not import: {message}"),
            )
            .hint("Nothing was kept; the sidecar is as it was.");
            e = e.at(at);
            return Err(e);
        }
        Ok(())
    }

    /// Apply an `edit.asset` op (or its inverse, the file as it was).
    pub(super) fn apply_asset(
        &mut self,
        author: &str,
        op: &Op,
        touched: &mut BTreeSet<String>,
        files: &mut BTreeSet<String>,
    ) -> Result<Option<Op>> {
        let (asset, new, what) = match op {
            Op::Asset { asset, path, value } => {
                self.edits.check_lock(author, asset, path)?;
                let (_, _, old) = self.sidecar_of(asset)?;
                (
                    asset.clone(),
                    edit_text(&old, path, value.as_ref())?,
                    format!("{asset}#{path}"),
                )
            }
            Op::AssetText { asset, text } => (asset.clone(), text.clone(), asset.clone()),
            _ => return Ok(None),
        };
        let (file, shown, old) = self.sidecar_of(&asset)?;
        self.write_sidecar(&asset, &file, &shown, &old, &new)?;
        files.insert(shown);
        touched.insert(what);
        Ok(Some(Op::AssetText { asset, text: old }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HERO: &str = r#"# The hero.
type = "animator"
initial = "idle"

[states.idle]
clip = "idle"   # standing

[[states.idle.transitions]]
to = "run"
when = "Body.speed > 0.5"

[states.run]
clip = "run"
fps = 8
events = [{ frame = 0, name = "footstep" }]
"#;

    #[test]
    fn a_path_names_keys_and_elements() {
        assert_eq!(
            parse_path("states.run.events[0].frame").unwrap(),
            vec![
                Seg::Key("states".into()),
                Seg::Key("run".into()),
                Seg::Key("events".into()),
                Seg::Index(0),
                Seg::Key("frame".into())
            ]
        );
        assert!(parse_path("").is_err());
        assert!(parse_path("a[x]").is_err());
        assert!(parse_path("a[0]b").is_err());
    }

    #[test]
    fn edits_keep_the_comments_and_the_layout() {
        let t = edit_text(HERO, "states.run.fps", Some(&json!(10))).unwrap();
        assert!(t.contains("fps = 10\n"), "{t}");
        assert!(
            t.contains("# The hero.") && t.contains("clip = \"idle\"   # standing"),
            "{t}"
        );
        // A whole number where there was a float stays a float, and back.
        let t = edit_text(&t, "states.run.fps", Some(&json!(8.5))).unwrap();
        assert!(t.contains("fps = 8.5"), "{t}");
        // Into an inline table in an array; appended; removed.
        let t = edit_text(HERO, "states.run.events[0].frame", Some(&json!(2))).unwrap();
        assert!(t.contains("frame = 2"), "{t}");
        let t = edit_text(
            &t,
            "states.run.events[1]",
            Some(&json!({ "frame": 3, "name": "footstep" })),
        )
        .unwrap();
        assert!(t.contains("frame = 3"), "{t}");
        let t = edit_text(&t, "states.run.events[0]", None).unwrap();
        assert!(!t.contains("frame = 2"), "{t}");
        // An array of tables: a transition set, added, taken out.
        let t = edit_text(
            HERO,
            "states.idle.transitions[0].when",
            Some(&json!("Body.speed > 1")),
        )
        .unwrap();
        assert!(t.contains("when = \"Body.speed > 1\""), "{t}");
        let t = edit_text(
            &t,
            "states.idle.transitions[1]",
            Some(&json!({ "to": "fall", "when": "end" })),
        )
        .unwrap();
        assert_eq!(t.matches("[[states.idle.transitions]]").count(), 2, "{t}");
        let t = edit_text(&t, "states.idle.transitions[0]", None).unwrap();
        assert_eq!(t.matches("[[states.idle.transitions]]").count(), 1, "{t}");
        // A new state is a table of its own.
        let t = edit_text(
            HERO,
            "states.dash",
            Some(&json!({ "clip": "dash", "loop": false })),
        )
        .unwrap();
        assert!(t.contains("[states.dash]"), "{t}");
        let doc: DocumentMut = t.parse().unwrap();
        assert_eq!(
            get_path(&doc, &parse_path("states.dash.loop").unwrap()),
            Some(json!(false))
        );
        // A key that is not there cannot go; a step through a number cannot be made.
        assert!(edit_text(HERO, "states.run.nothing", None).is_err());
        assert!(edit_text(HERO, "states.run.fps.x", Some(&json!(1))).is_err());
    }
}
