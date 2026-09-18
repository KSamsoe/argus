//! `world.query` over the live world. The evaluator lives in `argus-index`
//! (`argus_index::query`) so the index and the live world run the same
//! code and return identical shapes (§4); this module is the adapter that
//! presents a [`World`] as a [`Source`].

use crate::json;
use argus_ecs::{Entity, Result, World};
use argus_index::query::Source;
use serde_json::{Map, Value as Json};

pub use argus_index::query::{page, paged, DEFAULT_LIMIT};

/// The live world as a query source. Ids are entity handles packed as
/// `generation << 32 | index`.
pub struct WorldSource<'a>(pub &'a World);

fn pack(e: Entity) -> u64 {
    ((e.generation() as u64) << 32) | e.index() as u64
}

fn unpack(id: u64) -> Entity {
    Entity::from_parts(id as u32, (id >> 32) as u32)
}

impl Source for WorldSource<'_> {
    fn frame(&self) -> u64 {
        self.0.frame()
    }

    fn require_component(&self, name: &str) -> Result<()> {
        self.0.registry().require(name).map(|_| ())
    }

    fn require_path(&self, path: &str) -> Result<()> {
        self.0.registry().resolve_path(path).map(|_| ())
    }

    fn candidates(&self, has: &[String], not: &[String]) -> Result<Vec<u64>> {
        let mut q = self.0.query();
        for c in has {
            q = q.has(self.0.registry().require(c)?);
        }
        for c in not {
            q = q.not(self.0.registry().require(c)?);
        }
        let mut ids: Vec<u64> = q.entities().into_iter().map(pack).collect();
        ids.sort_unstable();
        Ok(ids)
    }

    fn field(&self, id: u64, path: &str) -> Option<Json> {
        self.0
            .get_field(unpack(id), path)
            .ok()
            .map(|v| json::to_json(self.0, &v))
    }

    fn component(&self, id: u64, name: &str) -> Option<Json> {
        let c = self.0.component_id(name)?;
        let fields = self.0.get_component(unpack(id), c).ok()?;
        Some(Json::Object(
            fields
                .into_iter()
                .map(|(k, v)| (k, json::to_json(self.0, &v)))
                .collect::<Map<_, _>>(),
        ))
    }

    fn entity_ref(&self, id: u64) -> Json {
        json::entity_ref(self.0, unpack(id))
    }

    fn lookup(&self, r: &str) -> Result<u64> {
        self.0.entity(r).map(pack)
    }

    fn near(&self, center: (f64, f64), r: f64) -> Option<Vec<u64>> {
        // Cover float rounding at the edge: the evaluator measures exactly.
        let r = (r as f32) * 1.0001 + 1e-3;
        Some(
            self.0
                .within(center.0 as f32, center.1 as f32, r)
                .into_iter()
                .map(|(e, _)| pack(e))
                .collect(),
        )
    }
}

/// Run a query against the live world.
pub fn run(world: &World, params: &Json) -> Result<Json> {
    argus_index::query::run(&WorldSource(world), params)
}

/// One selected column of one entity (used by `world.get`).
pub fn project(world: &World, e: Entity, sel: &str) -> Json {
    argus_index::query::project(&WorldSource(world), pack(e), sel)
}
