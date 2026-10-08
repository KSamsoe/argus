# argus-plugin: spec trace

The plugin's side of Rust plugins (§7, §19): the crate a plugin depends on. It holds the boundary both sides share (`abi`) and the API a plugin is written in. The engine's side is `argus-plugins` (its own trace, with the end-to-end tests). Statuses: **Done**, **Partial**, **Deferred**.

## §7 Scripting and hot reload

| Spec | Status | Where | Test |
|---|---|---|---|
| A plugin system is declared as a Luau system is (§19: promote one "without changing anything around it"). | Done | `System::new(name, stage).after(..).reads(&[..]).writes(&[..]).spawns(&[..]).rng(..).run(f)`, plus `parallel`, `on_load`, `first`, `last`, `unfrozen`, `continuous`, `discrete`; its `file:line` is where `System::new` is called. `plugin!` exports the systems and the component layouts they read. | argus-plugins `plugins` |
| "A system may only touch components it declared." | Done | Components are `#[repr(C)]` structs declared field for field as the game declares them (`component!`), each with a layout fingerprint computed at compile time; every fetch carries it, and the engine refuses a layout or a component outside the declaration. | `tests::the_compile_time_fingerprint_is_the_abi_one`; argus-plugins `plugins::a_layout_the_game_does_not_have_is_refused_at_load` |
| "A system may not hold references across frames." | Done | Everything a `Ctx` gives lives as long as the run: a query's items are borrowed for one visit of the closure (`FnMut(Entity, F::Item<'_>)`), rows are copied, strings are borrowed from the context. Keeping any of them past the run is a compile error without `unsafe`. | the API's types |
| Hot reload keeps state, because all state is in components (P4). | Done | The SDK has no state of its own: its declaration is built once per library and read by the engine. | argus-plugins `plugins::reload_swaps_the_library_and_the_run_continues` |

## §2, §6

| Spec | Status | Where | Test |
|---|---|---|---|
| A panic is caught at the system boundary. | Done | Every system runs through `decl::trampoline`, which catches a panic and answers with its message; a panic in a query's visit is kept and raised again after the engine's call returns, so it never unwinds through the engine. | argus-plugins `plugins::a_plugin_s_systems_run_and_a_panic_stops_only_its_own` |
| Randomness comes from the system's declared stream (§6). | Done | `Ctx::rng()` draws from the engine's own generator (`u32`, `f32`, `f64`, `below`, `range`, `chance`), the same draws a Luau system's `q:rng()` makes. | argus-plugins `plugins::parallel_plugin_systems_share_a_batch_and_reach_the_same_world_on_any_threads` |
| Arithmetic a port can match. | Done | `Vec2` works component by component in f32, as Luau's vectors do, and `length` is `vector.magnitude`. | argus-player `release_swarm_plugin` |

## Known limitations, stated so they are decisions

- A component a plugin reads is declared twice, once in the game (Luau or Rust) and once in the plugin; the engine checks the two agree but does not generate one from the other.
- No `Option<&T>` in a fetch, and at most six columns.
- A spawn does not return its entity: spawns are applied when the run ends, so the plugin's run and a parallel batch agree.
- The boundary is versioned (`ABI_VERSION`); the engine writes the SDK it was built with into each project (`.argus/plugins/sdk`), so a plugin is always built against the engine that loads it.
