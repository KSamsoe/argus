//! The job system through the protocol (§12 "Pathfinding and jobs", phase
//! 3 item 3): `jobs.why {worker}` is every job and what that worker makes
//! of it, in the order it weighs them, and why it has none. `obs.why` on a
//! `job.idle` (or any `job.*` event naming a worker) carries the same
//! answer, so "why is this colonist idle" is one question. The job queue
//! itself is a query: `world.query {group_by: "Job.kind", select:
//! ["count()"]}`.

use super::*;

impl Daemon {
    /// A worker's jobs, judged, as the protocol answers it.
    pub(super) fn jobs_explain(&self, worker: Entity) -> Result<Json> {
        let nav = self.require_session()?.nav.clone();
        let (w, rows, why) = argus_jobs::explain(&self.world, &mut nav.borrow_mut(), worker)?;
        let name = |e: Entity| json::entity_ref(&self.world, e);
        let rows: Vec<Json> = rows
            .iter()
            .map(|r| {
                json!([
                    name(r.job),
                    r.kind,
                    r.priority,
                    r.stage,
                    r.distance.map(|d| (d * 100.0).round() / 100.0),
                    r.verdict
                ])
            })
            .collect();
        let mut out = json!({
            "worker": name(worker),
            "job": if w.job.is_null() { Json::Null } else { name(w.job) },
            "columns": ["job", "kind", "priority", "stage", "distance", "verdict"],
            "total": rows.len(),
            "rows": rows,
        });
        if !why.is_empty() {
            out["why"] = json!(why);
        }
        Ok(out)
    }

    pub(super) fn jobs_why(&self, params: &Json) -> Result<Json> {
        let r = json::str_param(params, "worker")?;
        let e = self.world.entity(r)?;
        let (limit, offset) = query::page(params)?;
        let mut out = self.jobs_explain(e)?;
        let rows = out["rows"].as_array().cloned().unwrap_or_default();
        let paged = query::paged(rows, limit, offset);
        out["rows"] = paged["rows"].clone();
        out["truncated"] = paged["truncated"].clone();
        if let Some(n) = paged.get("next") {
            out["next"] = n.clone();
        }
        Ok(out)
    }

    /// For `obs.why`: a `job.*` event naming a worker still in the world
    /// gets that worker's jobs judged now.
    pub(super) fn jobs_for_event(&self, ev: &Json) -> Option<Json> {
        let name = ev["name"].as_str()?;
        if !name.starts_with("job.") {
            return None;
        }
        let r = ev["fields"]["worker"].as_str()?;
        let e = self.world.entity(r).ok()?;
        let mut out = self.jobs_explain(e).ok()?;
        // The first dozen: the rest are a jobs.why away.
        if let Some(rows) = out["rows"].as_array_mut() {
            rows.truncate(12);
        }
        Some(out)
    }
}
