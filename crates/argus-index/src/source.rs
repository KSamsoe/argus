//! A [`Source`] over one indexed scene, so `world.query` runs the same
//! evaluator against the index as against the live world.

use crate::query::Source;
use crate::sql_err;
use argus_ecs::{ArgusError, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Map, Value as Json};
use std::collections::{BTreeMap, BTreeSet};

/// One scene loaded from the index for querying.
pub struct SceneSource<'a> {
    conn: &'a Connection,
    scene: String,
    ids: Vec<String>,
    comps: Vec<BTreeSet<String>>,
    by_id: BTreeMap<String, u64>,
    schemas: BTreeMap<String, Vec<String>>,
}

impl<'a> SceneSource<'a> {
    /// Load the scene's entity list and component sets (one query each);
    /// fields are read on demand.
    pub fn new(conn: &'a Connection, scene: &str) -> Result<SceneSource<'a>> {
        let mut st = conn
            .prepare_cached("SELECT id FROM entities WHERE scene = ?1 ORDER BY id")
            .map_err(sql_err)?;
        let ids: Vec<String> = st
            .query_map(params![scene], |r| r.get(0))
            .map_err(sql_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql_err)?;
        let mut comps: Vec<BTreeSet<String>> = vec![BTreeSet::new(); ids.len()];
        let by_id: BTreeMap<String, u64> = ids
            .iter()
            .enumerate()
            .map(|(i, id)| (id.clone(), i as u64))
            .collect();
        let mut st = conn
            .prepare_cached("SELECT entity, component FROM components WHERE scene = ?1")
            .map_err(sql_err)?;
        let rows = st
            .query_map(params![scene], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(sql_err)?;
        for row in rows {
            let (e, c) = row.map_err(sql_err)?;
            if let Some(&i) = by_id.get(&e) {
                comps[i as usize].insert(c);
            }
        }
        let mut st = conn
            .prepare_cached("SELECT name, fields FROM schemas")
            .map_err(sql_err)?;
        let rows = st
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(sql_err)?;
        let mut schemas = BTreeMap::new();
        for row in rows {
            let (name, fields) = row.map_err(sql_err)?;
            let names: Vec<String> = serde_json::from_str::<Vec<Json>>(&fields)
                .unwrap_or_default()
                .iter()
                .filter_map(|f| f["name"].as_str().map(str::to_string))
                .collect();
            schemas.insert(name, names);
        }
        Ok(SceneSource {
            conn,
            scene: scene.to_string(),
            ids,
            comps,
            by_id,
            schemas,
        })
    }

    /// Entity count.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// True if the scene has no entities.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Component sets by entity index, for archetype counting.
    pub fn component_sets(&self) -> &[BTreeSet<String>] {
        &self.comps
    }

    /// Entity ids in order.
    pub fn ids(&self) -> &[String] {
        &self.ids
    }

    fn field_value(&self, id: u64, path: &str) -> Option<Json> {
        let mut st = self
            .conn
            .prepare_cached(
                "SELECT value FROM fields WHERE scene = ?1 AND entity = ?2 AND path = ?3",
            )
            .ok()?;
        let s: Option<String> = st
            .query_row(params![self.scene, self.ids.get(id as usize)?, path], |r| {
                r.get(0)
            })
            .ok();
        s.and_then(|s| serde_json::from_str(&s).ok())
    }
}

impl Source for SceneSource<'_> {
    fn frame(&self) -> u64 {
        0
    }

    fn require_component(&self, name: &str) -> Result<()> {
        if self.schemas.contains_key(name) {
            Ok(())
        } else {
            Err(ArgusError::new(
                argus_ecs::codes::UNKNOWN_COMPONENT,
                format!("no component named {name:?}"),
            )
            .hint("world.describe lists registered components."))
        }
    }

    fn require_path(&self, path: &str) -> Result<()> {
        let (c, f) = path.split_once('.').ok_or_else(|| {
            ArgusError::new(
                argus_ecs::codes::BAD_PATH,
                format!("{path:?} is not Component.field"),
            )
        })?;
        self.require_component(c)?;
        if self.schemas[c].iter().any(|x| x == f) {
            Ok(())
        } else {
            Err(ArgusError::new(
                argus_ecs::codes::UNKNOWN_FIELD,
                format!("{c} has no field {f:?}"),
            )
            .hint(format!("Fields: {}", self.schemas[c].join(", "))))
        }
    }

    fn candidates(&self, has: &[String], not: &[String]) -> Result<Vec<u64>> {
        Ok((0..self.ids.len() as u64)
            .filter(|&i| {
                let set = &self.comps[i as usize];
                has.iter().all(|c| set.contains(c)) && !not.iter().any(|c| set.contains(c))
            })
            .collect())
    }

    fn field(&self, id: u64, path: &str) -> Option<Json> {
        let (c, _) = path.split_once('.')?;
        if !self.comps.get(id as usize)?.contains(c) {
            return None;
        }
        self.field_value(id, path).or(Some(Json::Null))
    }

    fn component(&self, id: u64, name: &str) -> Option<Json> {
        if !self.comps.get(id as usize)?.contains(name) {
            return None;
        }
        let mut st = self
            .conn
            .prepare_cached("SELECT path, value FROM fields WHERE scene = ?1 AND entity = ?2 AND path LIKE ?3 ORDER BY ord")
            .ok()?;
        let like = format!("{name}.%");
        let rows = st
            .query_map(params![self.scene, self.ids.get(id as usize)?, like], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .ok()?;
        let mut out = Map::new();
        for row in rows.flatten() {
            let field = row
                .0
                .split_once('.')
                .map(|(_, f)| f.to_string())
                .unwrap_or(row.0);
            out.insert(field, serde_json::from_str(&row.1).unwrap_or(Json::Null));
        }
        Some(Json::Object(out))
    }

    fn entity_ref(&self, id: u64) -> Json {
        match self.ids.get(id as usize) {
            Some(s) => json!(format!("@{s}")),
            None => Json::Null,
        }
    }

    fn lookup(&self, r: &str) -> Result<u64> {
        let id = r.strip_prefix('@').unwrap_or(r);
        self.by_id.get(id).copied().ok_or_else(|| {
            ArgusError::new(
                argus_ecs::codes::UNKNOWN_ID,
                format!("no entity with string id {id:?} in scene {:?}", self.scene),
            )
            .hint(
                "The index knows authored entities only; runtime entities exist in the live world.",
            )
        })
    }
}
