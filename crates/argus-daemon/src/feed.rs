//! What reaches the change feed (§5, §10, phase 4): every call that changes
//! something, from any client, as the `argus` command that makes it, with
//! its author. "Everything the editor does appears in the change feed as a
//! command the agent could have issued" (the phase 4 gate), and so does
//! everything the agent and the CLI do.
//!
//! Three kinds of call, and every method is exactly one of them:
//!
//! - [`CHANGES`]: the world, the clock, the project's files, or something
//!   another party sees (marks, proposals, locks, the tuning session)
//!   changes. Each is a feed row: a commit's row where the call commits,
//!   else a `command` row of its own.
//! - [`TX_PARTS`]: the pieces of a transaction. Nothing changes until
//!   `edit.commit`, whose row carries the whole transaction as one line
//!   (the agent's `edit` tool with its ops).
//! - [`READS`]: lookups and observations. A picture, a sound clip, or a
//!   profile written to `.argus/` is an observation too.
//! - [`INPUT`]: a person's keys and mouse, which the input log records as
//!   a window's are, not the feed key by key (decided with Kevin). What a
//!   key stands for (F8's mark, F9's proposal) is its call's row.
//!
//! Internal calls (connection 0: the launcher, the test harness inside
//! `test.run`) make no rows; the call that started them did.

/// Calls that change something: each is a feed row.
pub const CHANGES: &[&str] = &[
    "session.open",
    "session.close",
    "session.shutdown",
    "scene.load",
    "scene.transition",
    "scene.tiles.create",
    "nav.bake",
    "asset.import",
    "asset.move",
    "edit.commit",
    "edit.undo",
    "edit.redo",
    "edit.propose",
    "edit.lock",
    "edit.unlock",
    "input.inject",
    "input.message",
    "input.script",
    "input.record",
    "input.replay",
    "input.bind",
    "sim.play",
    "sim.pause",
    "sim.stop",
    "sim.step",
    "sim.advance",
    "sim.speed",
    "sim.threads",
    "sim.run_until",
    "sim.seek",
    "sim.fork",
    "sim.snapshot",
    "sim.save",
    "platform.overlay",
    "platform.reset",
    "token.issue",
    "token.revoke",
    "sim.load",
    "sim.set",
    "sim.tiles",
    "mark.drop",
    "mark.open",
    "mark.note",
    "mark.close",
    "mark.cancel",
    "mark.import",
    "mark.pull",
    "obs.review",
    "review.answer",
    "proposal.try",
    "proposal.show",
    "proposal.pick",
    "proposal.reject",
    "proposal.stop",
    "tune.begin",
    "tune.set",
    "tune.end",
    "replay.save",
    "replay.open",
    "timeline.play",
    "script.reload",
    "test.run",
    "timeline.seek",
    "render.window",
    "audio.play",
    "audio.stop",
    "audio.bus",
    "audio.device",
];

/// The pieces of a transaction: `edit.commit`'s row carries them.
pub const TX_PARTS: &[&str] = &[
    "edit.begin",
    "edit.file",
    "edit.set",
    "edit.add",
    "edit.remove",
    "edit.spawn",
    "edit.despawn",
    "edit.tiles",
    "edit.text",
    "edit.def",
    "edit.asset",
    "edit.def_row",
    "edit.prefab",
    "edit.abort",
];

/// Lookups and observations: no row.
pub const READS: &[&str] = &[
    "project.read",
    "session.status",
    "session.capabilities",
    "session.docs",
    "session.identify",
    "sim.ring",
    "replay.list",
    "scene.tiles.list",
    "asset.sidecar",
    "world.summary",
    "world.query",
    "world.get",
    "world.describe",
    "world.who_writes",
    "world.defs",
    "world.refs",
    "world.search",
    "world.instances",
    "world.prefabs",
    "scene.list",
    "scene.issues",
    "scene.rooms",
    "scene.index",
    "scene.tiles.get",
    "scene.tiles.info",
    "scene.tiles.collision",
    "mods.list",
    "nav.grids",
    "nav.grid",
    "nav.path",
    "nav.field",
    "strings.list",
    "jobs.why",
    "timeline.list",
    "asset.list",
    "asset.get",
    "edit.feed",
    "edit.locks",
    "input.log.get",
    "input.devices",
    "platform.status",
    "token.list",
    "input.bindings",
    "input.state",
    "mark.list",
    "mark.get",
    "review.list",
    "review.get",
    "proposal.list",
    "proposal.get",
    "tune.fields",
    "tune.log",
    "sim.status",
    "script.systems",
    "script.errors",
    "script.modules",
    "obs.events",
    "obs.diff",
    "obs.trace",
    "obs.why",
    "obs.frame",
    "obs.annotate",
    "obs.subscribe",
    "test.list",
    "render.status",
    "render.stream",
    "audio.status",
    "audio.clip",
    "audio.describe",
    "asset.generate",
    "asset.preview",
    "prof.frame",
    "prof.worst",
    "prof.systems",
    "prof.archetypes",
    "prof.alloc",
    "prof.capture",
    "prof.targets",
    "prof.calls",
];

/// A person's device events: the input log's, not the feed's.
pub const INPUT: &[&str] = &["input.device"];

/// Does this call change something (a feed row)? `sim.fork` with no name
/// only lists the worlds (found by the scrubber, which lists them: every
/// look was a row by the person).
pub fn changes(method: &str, params: &serde_json::Value) -> bool {
    CHANGES.contains(&method)
        && !(method == "sim.fork" && params.get("name").is_none_or(serde_json::Value::is_null))
}

/// The author a call is by: its own `author` param where it has one
/// (`tune.begin`, `edit.lock`), else who the connection said it is
/// (`session.identify`), else `cli`.
pub fn author_of(params: &serde_json::Value, identity: Option<&str>) -> String {
    params
        .get("author")
        .and_then(serde_json::Value::as_str)
        .or(identity)
        .unwrap_or("cli")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::METHODS;

    #[test]
    fn every_method_is_a_change_a_transaction_part_or_a_read() {
        let mut seen = std::collections::BTreeMap::new();
        for (kind, list) in [
            ("changes", CHANGES),
            ("tx", TX_PARTS),
            ("reads", READS),
            ("input", INPUT),
        ] {
            for m in list {
                assert!(
                    seen.insert(*m, kind).is_none(),
                    "{m} is listed twice ({kind} and {})",
                    seen[m]
                );
            }
        }
        for m in METHODS {
            assert!(
                seen.contains_key(m.name),
                "{} is not classified: add it to CHANGES, TX_PARTS, READS, or INPUT in feed.rs",
                m.name
            );
        }
        for m in seen.keys() {
            assert!(
                METHODS.iter().any(|d| d.name == *m),
                "{m} is classified but is no method"
            );
        }
    }
}
