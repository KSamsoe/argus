//! The scenario API's reference (§9, §16): what a `tests/*.luau` file can
//! write, with what it does. `argus docs` and each project's ARGUS.md print
//! it; a test checks it names every function the harness gives a scenario.
//!
//! Found at the phase 5 gate: four agents adding features with only the
//! project, the tools, and their docs learned the scenario API from the
//! games' own tests, because nothing described it.

/// One name and what it does.
pub struct ApiDoc {
    /// How it is written.
    pub name: &'static str,
    /// What it does.
    pub doc: &'static str,
}

/// A part of the API.
pub struct ApiSection {
    /// Its heading.
    pub title: &'static str,
    /// A paragraph before its entries.
    pub intro: &'static str,
    /// Its entries.
    pub entries: &'static [ApiDoc],
}

const fn d(name: &'static str, doc: &'static str) -> ApiDoc {
    ApiDoc { name, doc }
}

/// The scenario API, part by part.
pub const SECTIONS: &[ApiSection] = &[
    ApiSection {
        title: "Scenarios",
        intro: "A file in `tests/` returns one scenario, or a table of them (`return { scenario \"a\" {...}, scenario \"b\" {...} }`). `argus test` runs them all; `argus test name=...` those whose name contains it; `argus test seeds=1..20` overrides their seeds; `update=true` rewrites goldens that no longer match. Each seed is a fresh load of the scene. A run passes when every expectation holds and nothing errors; a failing run's replay is saved under `.argus/replays/` (`argus replay list`). Each seed's report row carries the world's digest when it ended.",
        entries: &[
            d("scenario \"name\" { scene, seeds, run, ... }", "A scenario. `scene` (required) is the scene to load; `run = function(t) ... end` (required) is the scenario; `seeds` the seeds to run it with: `range(1, 20)`, a list `{1, 2, 7}`, or `seed = 3` for one (none: the scene's own); `mods` a list of mod ids to load, in order; `perf_margin` how far over a `perf` bound still passes (1.5 by default)."),
            d("range(a, b)", "The seeds a to b, both included."),
            d("seconds(n), minutes(n), hours(n), days(n)", "Sim seconds, for `sim_time` and series: an hour is a 24th of the project's day (`[time] day` in argus.toml, 86,400 s when it says nothing)."),
        ],
    },
    ApiSection {
        title: "t: running the game",
        intro: "The argument `run` receives. `t.scene` and `t.seed` are this run's.",
        entries: &[
            d("t:step(n)", "Run n ticks (one by default)."),
            d("t:run_until { event = name, max_frames = n }", "Run until the event happens (its first occurrence from now), at most n ticks (100,000 by default); returns `{hit, ticks, ...}`. Also `{ sim_time = days(7) }`: until the sim clock reaches that time; `{ frames = n }`: n ticks."),
            d("t:advance { by = \"turn\", n = 1 }", "On a discrete clock: pass n turns."),
            d("t:status()", "The sim's status: tick, time, clock, hz, seed, ..."),
            d("t:call(method, params)", "Any protocol method (`argus docs` lists them), its result; an error fails the run with the method's message. `t:call(\"world.get\", { entity = \"@player\" }).components.Health.cur`, `t:call(\"sim.set\", {...})`, `t:call(\"edit.begin\", ...)`. Note that `obs.events` here is the daemon's whole log: pass `since` (or use `t.expect:events`, which counts only this run's)."),
            d("t:log(msg)", "A line in the run's report."),
            d("t.input:hold(action, value, frames)", "Hold an action at a value (`{1, 0}` for an axis2, `true` for a button) for n ticks, or until released when `frames` is nil. On a discrete clock hold across the turn: a press and release with no tick between is never seen."),
            d("t.input:tap(action, value)", "The action at a value for one tick."),
            d("t.input:release(action)", "The action back at rest (false, 0, or {0, 0})."),
        ],
    },
    ApiSection {
        title: "t.expect: what must hold",
        intro: "Each expectation is checked when it is written, so write it after the run that should make it true. Most give a number to compare (below); a failed comparison fails the run with its line in `tests/`. A helper of your own that calls `error(msg)` names its own line: `error(msg, 2)` names the caller's.",
        entries: &[
            d("t.expect:field(entity, \"Comp.field\")", "A field's value now: `t.expect:field(\"@player\", \"Health.cur\"):eq(10)`."),
            d("t.expect:count(where)", "How many entities match a `world.query` `where` (`{has = {\"Enemy\"}, within = {of = \"@player\", r = 3}}`)."),
            d("t.expect:sum(\"Comp.field\", where)", "A field summed over the matching entities."),
            d("t.expect:events(name, filter)", "This run's events with that name (and fields matching `filter`): `:count()` a number, `:exists()` an expectation, `:first()` the first (a table of fields, or nil)."),
            d("t.expect:path(entity, where)", "`:exists()`: a path from the entity to one matching `where`, over the navigation grid when there is one."),
            d("t.expect:perf(stage)", "`:p95_ms()`: the stage's (or `tick`) 95th-percentile milliseconds over the last 600 ticks, compared with `perf_margin` slack."),
            d("t.expect:series(\"Comp.field\", { every = 1, where = ... })", "Sample a field (summed over `where`'s entities) every `every` sim seconds (turns on a discrete clock) from here on: `:samples()`, `:last()`, `:min()`, `:max()`, `:trend()` (the slope per sample, at least two). Declare it before the run it should watch."),
            d("t.expect:snapshot(name, { select, where })", "A golden: the rows of a `world.query` kept in `tests/goldens/<scenario>/<name>.seed<n>.json`, recorded on the first run and compared after (added, removed, changed rows)."),
            d("t.expect:image(name, { width, height, threshold, max })", "A picture golden, drawn with the software rasteriser at 640 x 360 and compared with `tests/goldens/<scenario>/<name>.seed<n>.png`: `threshold` is how different a pixel must be (0.1), `max` the share of pixels that may differ (0.001). Recorded on the first run; the same picture on machines of one system only (Windows and Linux rasterise text edges differently)."),
        ],
    },
    ApiSection {
        title: "Comparing numbers",
        intro: "What `field`, `count`, `sum`, `events():count()`, `perf():p95_ms()`, and a series' `last`, `min`, `max`, and `trend` give.",
        entries: &[
            d("num:eq(x)", "Equal (any value, compared deeply)."),
            d("num:ne(x)", "Not equal."),
            d("num:lt(x), num:lte(x), num:gt(x), num:gte(x)", "Less, at most, greater, at least."),
            d("num:near(x, eps)", "Within eps of x."),
            d("num:between(lo, hi)", "From lo to hi, both included."),
            d("num:get()", "The value itself, to use in Luau."),
        ],
    },
];

/// The reference as Markdown, headings from `level`.
pub fn markdown(level: usize) -> String {
    let h = "#".repeat(level);
    let mut o = String::new();
    for s in SECTIONS {
        o.push_str(&format!("{h} {}\n\n{}\n\n", s.title, s.intro));
        for e in s.entries {
            o.push_str(&format!("- `{}`: {}\n", e.name, e.doc));
        }
        o.push('\n');
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every function the harness hands a scenario, by reading its source:
    /// `function t:x`, `function t.input:x`, `function expect:x`, the
    /// matchers' `function Num:x` and `function ev:x`, and the globals.
    #[test]
    fn every_function_a_scenario_can_call_is_in_the_reference() {
        let src = include_str!("harness.luau");
        let all: String = SECTIONS
            .iter()
            .flat_map(|s| s.entries.iter().map(|e| e.name))
            .collect::<Vec<_>>()
            .join(" ");
        let mut checked = 0;
        for line in src.lines() {
            let l = line.trim_start();
            let Some(rest) = l.strip_prefix("function ") else {
                continue;
            };
            let name = &rest[..rest.find('(').unwrap_or(rest.len())];
            let want = if let Some(m) = name.strip_prefix("t:") {
                format!("t:{m}")
            } else if let Some(m) = name.strip_prefix("t.input:") {
                format!("t.input:{m}")
            } else if let Some(m) = name.strip_prefix("expect:") {
                format!("t.expect:{m}")
            } else if let Some(m) = name.strip_prefix("Num:") {
                format!("num:{m}")
            } else if let Some(m) = name
                .strip_prefix("ev:")
                .or_else(|| name.strip_prefix("p:"))
                .or_else(|| name.strip_prefix("handle:"))
            {
                // Methods of what an expect gives back, named in its doc.
                assert!(
                    SECTIONS
                        .iter()
                        .flat_map(|s| s.entries.iter())
                        .any(|e| e.doc.contains(&format!(":{m}()"))),
                    "{name} is not in the scenario reference (api.rs)"
                );
                checked += 1;
                continue;
            } else if ["scenario", "range", "seconds", "minutes", "hours", "days"].contains(&name) {
                name.to_string()
            } else {
                continue;
            };
            assert!(
                all.contains(&want),
                "{want} is not in the scenario reference (api.rs)"
            );
            checked += 1;
        }
        assert!(
            checked > 30,
            "only {checked} functions found: the harness changed shape"
        );
    }
}
