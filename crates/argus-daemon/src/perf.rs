//! The §17 targets beside what is measured (phase 4 item 6, the editor's
//! profiler view). §5: "this is what makes the §17 targets something the
//! agent can enforce rather than something a human notices: a perf
//! regression is a query result with a system name on it."
//!
//! `prof.targets` is §17's table as data: each row's measure, target, and
//! reason in the design's words, the test that gates it, and, for the rows
//! of the game that is open (and the rows for every game), what this
//! daemon measures now: the tick's p95, a system's time, how long the last
//! seek, reload, open, save, or load took, the fast-forward rate, a
//! keyframe's capture. A row over its bound names the system to look at.
//! `prof.calls` is the protocol's own side: how long each method took, the
//! loop an agent pays for.

use super::*;
use argus_sim::SystemProf;

/// How a row is measured here.
enum Live {
    /// The sim tick's p95 over the window, below this many ms.
    Tick(f64),
    /// A system's p95, below this many ms.
    System(&'static str, f64),
    /// A system's slowest tick, below this many ms.
    SystemMax(&'static str, f64),
    /// The last of these calls, below this many ms.
    Call(&'static [&'static str], f64),
    /// The last save and the last load, below these.
    SaveLoad(f64, f64),
    /// The last `sim.run_until`'s ticks a second, at least this.
    Rate(f64),
    /// A keyframe's capture, below this many ms.
    Keyframe(f64),
    /// A capacity, shown as what is here now.
    Count,
    /// Measured by its test only.
    Test,
}

/// What a row's number is about, beside it.
enum Context {
    None,
    Entities,
    Sprites,
    Particles,
    With(&'static str),
}

struct Target {
    game: &'static str,
    measure: &'static str,
    target: &'static str,
    why: &'static str,
    live: Live,
    context: Context,
    gate: &'static str,
}

/// §17, row by row, in the design's words.
const TARGETS: &[Target] = &[
    Target {
        game: "swarm",
        measure: "Live entities at 60 Hz sim",
        target: "100,000",
        why: "Late-game bullet heaven with bullets, enemies, particles, pickups all as entities",
        live: Live::Count,
        context: Context::Entities,
        gate: "argus-ecs perf (snapshot, sim pass, query at 100k); Swarm's stress scene",
    },
    Target {
        game: "swarm",
        measure: "Sim tick, p95",
        target: "< 6 ms",
        why: "Leaves headroom inside a 16.6 ms frame for present and render",
        live: Live::Tick(6.0),
        context: Context::Entities,
        gate: "argus-sim perf (the engine's share, a Rust swarm)",
    },
    Target {
        game: "swarm",
        measure: "Instanced sprites per frame",
        target: "200,000",
        why: "One draw call per texture atlas, GPU-side culling",
        live: Live::Count,
        context: Context::Sprites,
        gate: "argus-render perf_200k_sprites_per_frame",
    },
    Target {
        game: "swarm",
        measure: "Live particles across emitters",
        target: "250,000 at < 1 ms",
        why: "Emitter pools outside the ECS, updated in Rust, drawn instanced",
        live: Live::System("particles.update", 1.0),
        context: Context::Particles,
        gate: "argus-particles perf_a_quarter_million_particles",
    },
    Target {
        game: "hollow",
        measure: "Animators evaluated per tick",
        target: "2,000 at < 0.3 ms",
        why: "State machines are data; transitions are field comparisons, not script",
        live: Live::System("anim.update", 0.3),
        context: Context::With("Animator"),
        gate: "argus-anim perf_two_thousand_animators",
    },
    Target {
        game: "hollow",
        measure: "Post stack, 5 passes at 1080p",
        target: "< 1.5 ms GPU",
        why: "Flash, shake, aberration, vignette, palette must all be on at once",
        live: Live::Test,
        context: Context::None,
        gate: "argus-render perf_post_stack_five_passes_at_1080p",
    },
    Target {
        game: "all",
        measure: "Web build, cold load",
        target: "< 5 s on a 20 Mbit link",
        why: "A playtest link has to open before the tester loses interest",
        live: Live::Test,
        context: Context::None,
        gate: "not measured yet (phase 5 gates every row in CI); Hollow's page is about 1.5 MB compressed",
    },
    Target {
        game: "all",
        measure: "Release build, incremental",
        target: "< 30 s",
        why: "Content-addressed packs; only changed chunks rebuild",
        live: Live::Test,
        context: Context::None,
        gate: "not measured yet (phase 5 gates every row in CI); argus-player packs are content-addressed",
    },
    Target {
        game: "delve",
        measure: "Turn resolution, 500 monsters",
        target: "< 2 ms",
        why: "A turn must be imperceptible; seed sweeps of 200 floors must finish in seconds",
        live: Live::Tick(2.0),
        context: Context::Entities,
        gate: "argus-sim perf_delve_turn_500_monsters_under_2ms",
    },
    Target {
        game: "delve",
        measure: "Floor generation + index",
        target: "< 20 ms",
        why: "Runtime-generated rooms must be queryable the tick they exist",
        live: Live::Test,
        context: Context::None,
        gate: "argus-index perf (a floor's worth re-indexed, 15 to 19 ms)",
    },
    Target {
        game: "hollow",
        measure: "Authored entities per scene",
        target: "10,000 across 200 rooms",
        why: "File-per-entity storage and the index at real metroidvania scale",
        live: Live::Count,
        context: Context::Entities,
        gate: "argus-daemon perf_rooms (Hollow at 201 rooms)",
    },
    Target {
        game: "hollow",
        measure: "Cold scene open (index warm)",
        target: "< 300 ms",
        why: "Opening the project must not be the slow part of the agent's loop",
        live: Live::Call(&["session.open", "scene.load"], 300.0),
        context: Context::None,
        gate: "argus-daemon perf_rooms",
    },
    Target {
        game: "hollow",
        measure: "Room stream-in",
        target: "< 4 ms",
        why: "Must fit inside a frame so transitions never hitch",
        live: Live::SystemMax("rooms.stream", 4.0),
        context: Context::None,
        gate: "argus-daemon perf_rooms; Hollow's tests/deep.luau",
    },
    Target {
        game: "hollow",
        measure: "Tilemap",
        target: "4,096 × 4,096 tiles, 8 layers",
        why: "Chunked storage, rect edits, and auto-tiling at that size without pauses",
        live: Live::Test,
        context: Context::None,
        gate: "argus-tiles perf_4096_square_8_layers_without_pauses",
    },
    Target {
        game: "outpost",
        measure: "Sim tick at 10× speed, 200 colonists, 20k items",
        target: "< 1.6 ms",
        why: "Ten ticks per render frame with pathfinding, jobs, and needs all running",
        live: Live::Tick(1.6),
        context: Context::Entities,
        gate: "argus-daemon perf_outpost (the stress scene)",
    },
    Target {
        game: "outpost",
        measure: "Headless fast-forward",
        target: "> 2,000 ticks/s",
        why: "A 30-day balance scenario in under a minute per seed",
        live: Live::Rate(2000.0),
        context: Context::None,
        gate: "argus-daemon perf_outpost; argus-sim perf_headless_fast_forward",
    },
    Target {
        game: "outpost",
        measure: "Save / load at 8 h of play",
        target: "< 200 ms / < 500 ms with migrations",
        why: "Saves must be frequent enough to be automatic",
        live: Live::SaveLoad(200.0, 500.0),
        context: Context::None,
        gate: "argus-daemon perf_outpost",
    },
    Target {
        game: "outpost",
        measure: "UI tree, 2,000 nodes with text",
        target: "< 1 ms layout",
        why: "Inspector panels and overview screens are most of what the player looks at",
        live: Live::System("ui.layout", 1.0),
        context: Context::With("UiNode"),
        gate: "argus-ui perf_2000_nodes_with_text_lay_out_in_a_millisecond",
    },
    Target {
        game: "all",
        measure: "Full snapshot at 100k entities",
        target: "< 5 ms",
        why: "Keyframes every second must be invisible during play",
        live: Live::Keyframe(5.0),
        context: Context::Entities,
        gate: "argus-ecs perf_snapshot_100k_under_5ms; argus-daemon perf_seek",
    },
    Target {
        game: "all",
        measure: "Seek to any point in last 5 min",
        target: "< 500 ms",
        why: "Marks and A/B toggling must feel instant to the human",
        live: Live::Call(&["sim.seek"], 500.0),
        context: Context::None,
        gate: "argus-daemon perf_seek",
    },
    Target {
        game: "all",
        measure: "Luau hot reload",
        target: "< 100 ms",
        why: "Agent edit-reload-observe loop stays under a second end to end",
        live: Live::Call(&["script.reload"], 100.0),
        context: Context::None,
        gate: "argus-luau limits_reload",
    },
    Target {
        game: "all",
        measure: "Rust plugin reload",
        target: "< 2 s after compile",
        why: "Compile time is the cost; reload itself must not add to it",
        live: Live::Test,
        context: Context::None,
        gate: "phase 5 (Rust plugin crates)",
    },
    Target {
        game: "all",
        measure: "world.query over 100k entities",
        target: "< 2 ms",
        why: "Spatial predicates hit the broadphase hash, not a scan",
        live: Live::Call(&["world.query"], 2.0),
        context: Context::Entities,
        gate: "argus-ecs perf_query_100k_under_2ms",
    },
];

/// One method's calls: how many, the last, the slowest, the total.
#[derive(Clone, Debug, Default)]
pub(crate) struct CallStat {
    pub count: u64,
    pub last_ms: f64,
    pub max_ms: f64,
    pub sum_ms: f64,
    /// When it was last called, in calls.
    pub seq: u64,
}

fn ms1(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn fmt_ms(x: f64) -> String {
    if x >= 100.0 {
        format!("{x:.0} ms")
    } else if x >= 1.0 {
        format!("{x:.2} ms")
    } else {
        format!("{x:.3} ms")
    }
}

fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

impl Daemon {
    /// A client's call is over: how long it took, for `prof.calls` and the
    /// targets (its own `ms` where it reports one, as a seek or an open
    /// does, else the time the daemon spent on it).
    pub(super) fn time_call(&mut self, method: &str, r: &Result<Json>, wall: std::time::Duration) {
        let wall_ms = wall.as_secs_f64() * 1e3;
        let ms = r
            .as_ref()
            .ok()
            .and_then(|v| v.get("ms"))
            .and_then(Json::as_f64)
            .unwrap_or(wall_ms);
        self.call_seq += 1;
        let seq = self.call_seq;
        let s = self.calls.entry(method.to_string()).or_default();
        s.count += 1;
        s.last_ms = ms;
        s.max_ms = s.max_ms.max(ms);
        s.sum_ms += ms;
        s.seq = seq;
        if method == "sim.run_until" {
            if let Ok(v) = r {
                if let Some(t) = v["ticks"].as_u64().filter(|t| *t > 0) {
                    self.fast_forward = Some((t, wall_ms));
                }
            }
        }
    }

    /// `prof.calls`: each method's calls from clients, slowest last first.
    pub(super) fn prof_calls(&self) -> Json {
        let mut rows: Vec<(&String, &CallStat)> = self.calls.iter().collect();
        rows.sort_by_key(|r| std::cmp::Reverse(r.1.seq));
        json!({
            "columns": ["method", "count", "last_ms", "mean_ms", "max_ms"],
            "rows": rows.iter().map(|(m, s)| json!([
                m, s.count, ms1(s.last_ms), ms1(s.sum_ms / s.count.max(1) as f64), ms1(s.max_ms),
            ])).collect::<Vec<_>>(),
        })
    }

    /// The last of these calls, and which it was.
    fn last_call<'a>(&self, methods: &[&'a str]) -> Option<(&'a str, &CallStat)> {
        methods
            .iter()
            .filter_map(|m| self.calls.get(*m).map(|s| (*m, s)))
            .max_by_key(|(_, s)| s.seq)
    }

    fn with_count(&self, component: &str) -> u64 {
        self.world
            .archetype_stats()
            .iter()
            .filter(|a| a.components.iter().any(|c| c == component))
            .map(|a| a.entities as u64)
            .sum()
    }

    fn sprites_drawn(&self) -> Option<u64> {
        #[cfg(feature = "render")]
        {
            if let Some(s) = &self.render.stream {
                return Some(s.renderer.last().stats.sprites as u64);
            }
            if let Some((_, r)) = &self.render.window {
                return Some(r.last().stats.sprites as u64);
            }
        }
        None
    }

    fn context_text(&self, c: &Context) -> Option<String> {
        match c {
            Context::None => None,
            Context::Entities => Some(format!(
                "{} entities",
                thousands(self.world.entity_count() as u64)
            )),
            Context::Sprites => Some(match self.sprites_drawn() {
                Some(n) => format!("{} sprites drawn in the last frame", thousands(n)),
                None => "no window or viewport drawing".to_string(),
            }),
            Context::Particles => self.session.as_ref().map(|s| {
                format!(
                    "{} particles alive",
                    thousands(s.particles.borrow().live() as u64)
                )
            }),
            Context::With(name) => {
                Some(format!("{} with {name}", thousands(self.with_count(name))))
            }
        }
    }

    /// `prof.targets`: §17's rows, with what is measured here for the game
    /// that is open.
    pub(super) fn prof_targets(&mut self, params: &Json) -> Result<Json> {
        let last = json::u64_param(params, "last", 600)? as usize;
        let game = self
            .session
            .as_ref()
            .map(|s| s.project.config.name.to_lowercase())
            .filter(|n| ["swarm", "hollow", "delve", "outpost"].contains(&n.as_str()));
        let systems: Vec<SystemProf> = if self.session.is_some() {
            self.sched.prof_systems(last)?
        } else {
            Vec::new()
        };
        let ticked = self.sched.prof_len() > 0;
        let tick_p95 = self.sched.prof_tick_p95_ms(last);
        let heaviest = systems.iter().filter(|s| s.samples > 0).max_by(|a, b| {
            a.p95_ms
                .partial_cmp(&b.p95_ms)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let capture = self
            .session
            .as_ref()
            .and_then(|s| s.ring.borrow().capture_ms());
        let mut rows = Vec::new();
        for t in TARGETS {
            let live_here = t.game == "all" || game.as_deref() == Some(t.game);
            let mut row = json!({
                "game": t.game,
                "measure": t.measure,
                "target": t.target,
                "why": t.why,
                "gate": t.gate,
                "live": live_here && !matches!(t.live, Live::Test),
                "value": null,
                "unit": null,
                "text": null,
                "ok": null,
                "system": null,
                "context": null,
                "bound": null,
            });
            if !live_here || matches!(t.live, Live::Test) {
                rows.push(row);
                continue;
            }
            row["context"] = json!(self.context_text(&t.context));
            // The game's tick budget: a view draws its ticks against it.
            row["tick"] = json!(matches!(t.live, Live::Tick(_)));
            row["bound"] = match &t.live {
                Live::Tick(b)
                | Live::System(_, b)
                | Live::SystemMax(_, b)
                | Live::Call(_, b)
                | Live::Keyframe(b) => json!({ "below": b }),
                Live::SaveLoad(s, l) => json!({ "below": [s, l] }),
                Live::Rate(b) => json!({ "at_least": b }),
                Live::Count | Live::Test => Json::Null,
            };
            let (value, unit, text, ok, system): (
                Option<f64>,
                &str,
                String,
                Option<bool>,
                Option<String>,
            ) = match &t.live {
                Live::Tick(b) if ticked => (
                    Some(tick_p95),
                    "ms",
                    format!(
                        "{} p95 over {} ticks",
                        fmt_ms(tick_p95),
                        self.sched.prof_len().min(last)
                    ),
                    Some(tick_p95 < *b),
                    heaviest.map(|h| h.name.to_string()),
                ),
                Live::Tick(_) => (None, "ms", "no ticks yet: play or step".into(), None, None),
                Live::System(name, b) | Live::SystemMax(name, b) => {
                    let max = matches!(t.live, Live::SystemMax(..));
                    match systems.iter().find(|s| s.name == *name && s.samples > 0) {
                        Some(s) => {
                            let v = if max { s.max_ms } else { s.p95_ms };
                            (
                                Some(v),
                                "ms",
                                format!(
                                    "{} {} over {} ticks",
                                    fmt_ms(v),
                                    if max { "at worst" } else { "p95" },
                                    s.samples
                                ),
                                Some(v < *b),
                                Some(name.to_string()),
                            )
                        }
                        None => (
                            None,
                            "ms",
                            format!("{name} has not run"),
                            None,
                            Some(name.to_string()),
                        ),
                    }
                }
                Live::Call(methods, b) => match self.last_call(methods) {
                    Some((m, s)) => (
                        Some(s.last_ms),
                        "ms",
                        format!(
                            "{} the last {m} (slowest {})",
                            fmt_ms(s.last_ms),
                            fmt_ms(s.max_ms)
                        ),
                        Some(s.last_ms < *b),
                        None,
                    ),
                    None => (
                        None,
                        "ms",
                        format!("no {} yet", methods.join(" or ")),
                        None,
                        None,
                    ),
                },
                Live::SaveLoad(bs, bl) => {
                    let (s, l) = (self.calls.get("sim.save"), self.calls.get("sim.load"));
                    let text = match (s, l) {
                        (None, None) => "no sim.save or sim.load yet".to_string(),
                        _ => format!(
                            "save {}, load {}",
                            s.map_or("not yet".into(), |s| fmt_ms(s.last_ms)),
                            l.map_or("not yet".into(), |l| fmt_ms(l.last_ms))
                        ),
                    };
                    let ok = match (s, l) {
                        (None, None) => None,
                        _ => Some(
                            s.is_none_or(|s| s.last_ms < *bs) && l.is_none_or(|l| l.last_ms < *bl),
                        ),
                    };
                    (s.map(|s| s.last_ms), "ms", text, ok, None)
                }
                Live::Rate(b) => match self.fast_forward {
                    Some((ticks, ms)) if ms > 0.0 => {
                        let rate = ticks as f64 / (ms / 1e3);
                        (
                            Some(rate.round()),
                            "ticks/s",
                            format!(
                                "{} ticks a second, the last sim.run_until ({} ticks)",
                                thousands(rate.round() as u64),
                                thousands(ticks)
                            ),
                            Some(rate > *b),
                            None,
                        )
                    }
                    _ => (None, "ticks/s", "no sim.run_until yet".into(), None, None),
                },
                Live::Keyframe(b) => match capture {
                    Some((last, max)) => (
                        Some(last),
                        "ms",
                        format!(
                            "{} the last keyframe (slowest {})",
                            fmt_ms(last),
                            fmt_ms(max)
                        ),
                        Some(last < *b),
                        None,
                    ),
                    None => (
                        None,
                        "ms",
                        "no keyframe taken on a tick yet".into(),
                        None,
                        None,
                    ),
                },
                Live::Count => (None, "", "what is here now".into(), None, None),
                Live::Test => unreachable!(),
            };
            row["value"] = json!(value.map(ms1));
            row["unit"] = json!(unit);
            row["text"] = json!(text);
            row["ok"] = json!(ok);
            row["system"] = json!(system);
            rows.push(row);
        }
        let over: Vec<&Json> = rows.iter().filter(|r| r["ok"] == false).collect();
        Ok(json!({
            "game": game,
            "build": if cfg!(debug_assertions) { "debug" } else { "release" },
            "last": last,
            "tick_p95_ms": if ticked { json!(ms1(tick_p95)) } else { Json::Null },
            "heaviest": heaviest.map(|h| json!({ "name": h.name.as_str(), "p95_ms": ms1(h.p95_ms) })),
            "entities": self.world.entity_count(),
            "speed": self.sched.speed(),
            "over": over.len(),
            "rows": rows,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_of_section_17_is_here_once() {
        assert_eq!(TARGETS.len(), 23, "§17 has 23 rows");
        let mut seen = std::collections::BTreeSet::new();
        for t in TARGETS {
            assert!(seen.insert(t.measure), "{} twice", t.measure);
            assert!(["swarm", "hollow", "delve", "outpost", "all"].contains(&t.game));
            assert!(!t.gate.is_empty());
        }
        assert_eq!(thousands(1234567), "1,234,567");
        assert_eq!(thousands(12), "12");
    }
}
