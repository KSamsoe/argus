//! A scene's world, assembled one way (phase 2 item 7): the daemon's
//! `session.open` and `scene.load`, the web page, and a release build all
//! call [`assemble`], so a game runs the same systems in the same order
//! wherever it runs. What each adds around it (the daemon's index, history,
//! and window; a player's devices and recorder) is its own.

use argus_ecs::layers;
use argus_ecs::{ArgusError, Layers, Result, World};
use argus_input::{ActionMap, InputState};
use argus_luau::{ScriptConfig, ScriptHost};
use argus_scene::{DefSchema, LoadReport, Project, ProjectConfig, Scene, SceneDoc, SeedPolicy};
use argus_sim::{Clock, ClockKind, Scheduler, SchedulerConfig};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;

/// How to assemble.
#[derive(Clone, Debug)]
pub struct Options {
    /// The seed when none is given and the scene's policy is random: the
    /// daemon takes the clock's, a player that must agree with the desktop
    /// takes a fixed one.
    pub random_seed: u64,
    /// Load the mixer (the daemon skips it for a fork).
    pub audio: bool,
    /// The mods to lay over the game, in load order, and those already
    /// left out (phase 3 item 8): [`crate::mods::choose`] makes it.
    pub mods: crate::mods::Report,
    /// Where the project's Rust plugins come from (phase 5 item 6): built
    /// from `plugins/` by default, or a release build's own libraries.
    pub plugins: argus_plugins::Source,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            random_seed: 1,
            audio: true,
            mods: crate::mods::Report::default(),
            plugins: argus_plugins::Source::default(),
        }
    }
}

/// A scene's world and everything that feeds it.
pub struct Assembly {
    /// The world, the scene spawned into it (unless it has issues).
    pub world: World,
    /// The scheduler, every system installed and validated, `on_load`
    /// systems not yet run (the caller runs them once it has hooked in).
    pub sched: Scheduler,
    /// The scene's driving clock.
    pub clock: Clock,
    /// The run seed.
    pub seed: u64,
    /// `scene.toml`.
    pub scene_doc: SceneDoc,
    /// The input state, when the project has `input.toml`.
    pub input: Option<Rc<RefCell<InputState>>>,
    /// Tilemaps.
    pub tiles: Rc<RefCell<argus_tiles::MapStore>>,
    /// Navigation grids and the searches made on them (phase 3 item 2).
    pub nav: Rc<RefCell<argus_nav::NavStore>>,
    /// Room bounds for the camera.
    pub zones: Rc<RefCell<argus_camera::Zones>>,
    /// Animator machines.
    pub anims: Rc<RefCell<argus_anim::AnimStore>>,
    /// Particle pools.
    pub particles: Rc<RefCell<argus_particles::ParticleStore>>,
    /// The colliders overlapping at the end of the last tick, which a
    /// keyframe keeps.
    pub contacts: Rc<RefCell<argus_physics::Contacts>>,
    /// The rigid bodies' store, when the game uses the module (phase 5
    /// item 7), which a keyframe keeps.
    pub rigid: crate::keyframe::RigidSource,
    /// `platform.toml`: the game's achievements and Steam's app id (phase
    /// 5 item 9).
    pub platform: argus_platform::PlatformConfig,
    /// The Luau host (scripts, defs).
    pub host: ScriptHost,
    /// The Rust plugins `argus.toml` lists, loaded and installed, and what
    /// went wrong with any (phase 5 item 6).
    pub plugins: argus_plugins::Plugins,
    /// The project.
    pub project: Project,
    /// The scene (rooms, streaming), shared with `rooms.stream`, which
    /// streams its rooms around the followed entity inside the tick.
    pub scene: Rc<RefCell<Scene>>,
    /// Problems in the project or the scene; when there are any, nothing
    /// was spawned.
    pub issues: Vec<ArgusError>,
    /// What spawning did.
    pub load: Option<LoadReport>,
    /// The UI's theme (`ui.toml`) and fonts, shared by its systems and
    /// whatever draws them (phase 3 item 1), and the string tables (item 4).
    pub ui: Rc<RefCell<argus_ui::Shared>>,
    /// The dialogue graph, the project's `dialogue` table (phase 3 item 4).
    pub dialogue: Rc<RefCell<argus_dialogue::Graph>>,
    /// The project's timelines, its `timeline` table (phase 3 item 5).
    pub timelines: Rc<RefCell<argus_timeline::Timelines>>,
    /// The imported assets.
    pub assets: argus_assets::Importer,
    /// The import's report.
    pub asset_report: argus_assets::Report,
    /// The mixer, when asked for: its sounds set, no device open.
    #[cfg(feature = "audio")]
    pub audio: Option<Result<argus_audio::AudioEngine>>,
    /// The mods laid over the game, those left out and why, and what they
    /// changed (phase 3 item 8).
    pub mods: crate::mods::Report,
    /// The saves the host keeps, which scripts list (`q:saves`; 2D staples
    /// item 4): the host fills it and keeps it current.
    pub save_list: Rc<RefCell<Vec<argus_luau::SaveInfo>>>,
}

/// Scripts' `q:saves` (2D staples item 4): on a tick's first run, the
/// host's list, recorded with the run's input; on a re-run, what it read.
/// A project without input.toml has no journal to record it in: its
/// scripts read the host's list as it is.
struct HostSaves {
    list: Rc<RefCell<Vec<argus_luau::SaveInfo>>>,
    input: Option<Rc<RefCell<InputState>>>,
}

impl argus_luau::Saves for HostSaves {
    fn list(&self) -> Vec<argus_luau::SaveInfo> {
        let Some(input) = &self.input else {
            return self.list.borrow().clone();
        };
        let mut st = input.borrow_mut();
        if let Some(text) = st.saves_read() {
            return crate::savegame::list_from_json(text);
        }
        let now = self.list.borrow().clone();
        st.record_saves(&crate::savegame::list_json(&now));
        now
    }
}

/// Assemble scene `scene_name` of the project at `root`, with the mods
/// `opts.mods` names laid over it (phase 3 item 8). A mod with any problem
/// (a name outside its namespace, a row or an entity the game's schemas
/// refuse, a script that does not load, a refused `audio.toml`, a string
/// table or conversation that does not read) is left out whole, with
/// every reason, and the scene assembled again without it; so is a mod
/// needing one left out. The game's own problems are its issues, as
/// before.
pub fn assemble(
    root: &Path,
    cfg: &ProjectConfig,
    scene_name: &str,
    seed: Option<u64>,
    opts: &Options,
) -> Result<Assembly> {
    let mut report = opts.mods.clone();
    loop {
        let ids = report.ids();
        let layers = Layers::with_mods(root, &ids);
        let mut drop: BTreeMap<String, Vec<ArgusError>> = BTreeMap::new();
        match assemble_layers(root, cfg, scene_name, seed, opts, &layers) {
            Ok(mut a) => {
                drop = mod_problems(&a, &layers);
                if drop.is_empty() {
                    fill_report(&mut report, &a, &layers);
                    a.mods = report;
                    return Ok(a);
                }
            }
            // Nothing assembled: the mod the error names, else the last
            // one, is left out and the rest tried again.
            Err(e) if !ids.is_empty() => {
                let id = e
                    .r#ref
                    .as_deref()
                    .and_then(|r| layers.owner(r).map(str::to_string))
                    .unwrap_or_else(|| ids[ids.len() - 1].clone());
                drop.entry(id).or_default().push(e);
            }
            Err(e) => return Err(e),
        }
        // A mod needing one left out goes too.
        loop {
            let gone: Vec<String> = drop.keys().cloned().collect();
            let more: Vec<(String, String)> = report
                .loaded
                .iter()
                .filter(|l| !drop.contains_key(&l.manifest.id))
                .filter_map(|l| {
                    l.manifest
                        .requires
                        .iter()
                        .find(|r| gone.contains(r))
                        .map(|r| (l.manifest.id.clone(), r.clone()))
                })
                .collect();
            if more.is_empty() {
                break;
            }
            for (id, r) in more {
                drop.entry(id.clone()).or_default().push(ArgusError::new(
                    layers::codes::LOAD_ORDER,
                    format!("{id} needs {r}, which was left out"),
                ));
            }
        }
        for (id, why) in drop {
            report.loaded.retain(|l| l.manifest.id != id);
            report.left_out.push((id, why));
        }
    }
}

/// Every problem the mods caused, by mod: whatever went wrong in a file
/// under `mods/<id>/`.
fn mod_problems(a: &Assembly, layers: &Layers) -> BTreeMap<String, Vec<ArgusError>> {
    let mut out: BTreeMap<String, Vec<ArgusError>> = BTreeMap::new();
    if !layers.has_mods() {
        return out;
    }
    let mut put = |at: &str, e: ArgusError| {
        if let Some(id) = layers.owner(at) {
            out.entry(id.to_string()).or_default().push(e);
        }
    };
    for e in &a.issues {
        // A schema's refusal points at the schema (the game's) and names
        // the row it refused in its message: "(in mods/<id>/...)".
        // The mod's row is the thing to fix; the schema, a hint.
        match e.r#ref.as_deref() {
            Some(r) if layers.owner(r).is_some() => put(r, e.clone()),
            _ => {
                if let Some(at) = mentioned(&e.message)
                    .into_iter()
                    .find(|m| layers.owner(m).is_some())
                {
                    let mut e = e.clone();
                    if let Some(decl) = e.r#ref.take() {
                        e.hint = format!("{} The field is declared at {decl}.", e.hint)
                            .trim()
                            .to_string();
                    }
                    e.r#ref = Some(at.clone());
                    put(&at, e);
                }
            }
        }
    }
    for e in a.host.errors() {
        let at = match e.line {
            Some(l) => format!("{}:{l}", e.file),
            None => e.file.clone(),
        };
        put(
            &e.file,
            ArgusError::new(argus_luau::codes::SYNTAX, e.message.clone()).at(Some(at)),
        );
    }
    for f in &a.asset_report.failed {
        put(f.error.r#ref.as_deref().unwrap_or(""), f.error.clone());
    }
    #[cfg(feature = "audio")]
    if let Some(Ok(audio)) = &a.audio {
        for e in audio.mod_problems() {
            put(e.r#ref.as_deref().unwrap_or(""), e.clone());
        }
    }
    for p in a.ui.borrow().strings.problems() {
        put(p, ArgusError::new(argus_ui::codes::BAD_STRINGS, p.clone()));
    }
    for (at, m) in &a.dialogue.borrow().problems {
        put(
            at,
            ArgusError::new(argus_dialogue::codes::BAD_DIALOGUE, m.clone()).at(Some(at.clone())),
        );
    }
    for (at, m) in &a.timelines.borrow().problems {
        put(
            at,
            ArgusError::new(argus_timeline::codes::BAD_TIMELINE, m.clone()).at(Some(at.clone())),
        );
    }
    out
}

/// The places a message names as `(in <place>)`.
fn mentioned(message: &str) -> Vec<String> {
    message
        .split("(in ")
        .skip(1)
        .filter_map(|s| s.split(')').next())
        .map(str::to_string)
        .collect()
}

/// What each mod added, and what the mods changed, into the report.
fn fill_report(report: &mut crate::mods::Report, a: &Assembly, layers: &Layers) {
    let mut changes = a.project.changes().to_vec();
    changes.extend(a.scene.borrow().changes().iter().cloned());
    changes.extend(a.assets.changes().iter().cloned());
    #[cfg(feature = "audio")]
    if let Some(Ok(audio)) = &a.audio {
        changes.extend(audio.changes().iter().cloned());
    }
    changes.extend(a.ui.borrow().strings.changes().iter().cloned());
    report.changes = changes;
    let scene = a.scene.borrow();
    for l in &mut report.loaded {
        let id = l.manifest.id.clone();
        let mine = |at: &str| layers.owner(at) == Some(id.as_str());
        let mut adds = BTreeMap::new();
        adds.insert(
            "scripts".to_string(),
            a.host.files().iter().filter(|f| mine(f)).count(),
        );
        adds.insert(
            "components".to_string(),
            a.host.components().iter().filter(|c| mine(&c.file)).count(),
        );
        adds.insert(
            "def rows".to_string(),
            a.project
                .defs
                .tables()
                .flat_map(|t| t.rows.iter())
                .filter(|r| mine(&r.at) && layers::owns(&id, &r.key))
                .count(),
        );
        adds.insert(
            "rooms".to_string(),
            scene.rooms().filter(|r| layers::owns(&id, &r.name)).count(),
        );
        adds.insert(
            "entities".to_string(),
            scene
                .ids()
                .into_iter()
                .filter(|i| {
                    scene
                        .entity_file(i)
                        .is_some_and(|f| mine(&f.rel) && !report_changed(&report.changes, i))
                })
                .count(),
        );
        adds.insert(
            "assets".to_string(),
            a.assets
                .entries()
                .keys()
                .filter(|n| a.assets.owner(n) == Some(id.as_str()) && layers::owns(&id, n))
                .count(),
        );
        #[cfg(feature = "audio")]
        if let Some(Ok(audio)) = &a.audio {
            adds.insert(
                "sounds".to_string(),
                audio
                    .sound_names()
                    .iter()
                    .filter(|n| layers::owns(&id, n))
                    .count(),
            );
        }
        let strings = &a.ui.borrow().strings;
        adds.insert(
            "strings".to_string(),
            strings
                .table(strings.base())
                .map(|t| t.keys().filter(|k| layers::owns(&id, k)).count())
                .unwrap_or(0),
        );
        l.adds = adds;
    }
}

/// Whether an entity is one a mod replaced (not one it added).
fn report_changed(changes: &[layers::Change], id: &str) -> bool {
    changes.iter().any(|c| c.kind == "entity" && c.name == id)
}

/// [`assemble`] with the mods `layers` names, as they are.
fn assemble_layers(
    root: &Path,
    cfg: &ProjectConfig,
    scene_name: &str,
    seed: Option<u64>,
    opts: &Options,
    layers: &Layers,
) -> Result<Assembly> {
    let scene_doc = SceneDoc::load_layers(layers, scene_name)?;
    let clock = Clock::parse(scene_doc.clock.as_deref().unwrap_or(&cfg.clock))?;
    let seed = seed.unwrap_or(match scene_doc.seed {
        SeedPolicy::Fixed(s) => s,
        SeedPolicy::Random => opts.random_seed,
    });
    let sched_cfg = match clock.kind {
        ClockKind::Continuous => SchedulerConfig::continuous(1.0 / clock.timestep),
        ClockKind::Discrete => SchedulerConfig::discrete(),
    }
    .seed(seed);
    let mut world = World::new();
    argus_ecs::hierarchy::register(&mut world)?;
    // The room the followed entity is in, which scripts may read (phase 3
    // item 7: a room's music); its system comes once the scene is open.
    argus_scene::stream::register(&mut world)?;
    let mut sched = Scheduler::new(sched_cfg);
    // The action map's components exist before scripts load, so a system
    // may declare reads = {"Actions"}.
    let input = match ActionMap::load(root)? {
        Some(map) => {
            let state = Rc::new(RefCell::new(InputState::new(
                map,
                clock.kind == ClockKind::Discrete,
            )));
            argus_input::install(&mut world, &mut sched, state.clone())?;
            // Idle ticks between turns are the run's input (2D staples
            // item 5): recorded live, run again before a re-run turn.
            let (due, rec) = (state.clone(), state.clone());
            sched.on_idle(
                Box::new(move |turn| due.borrow().idle_due(turn)),
                Box::new(move |turn, n| rec.borrow_mut().record_idle(turn, n)),
            );
            Some(state)
        }
        None => None,
    };
    #[cfg(feature = "render")]
    argus_render::register(&mut world)?;
    #[cfg(feature = "audio")]
    argus_audio::register(&mut world)?;
    let tiles = Rc::new(RefCell::new(argus_tiles::MapStore::new(root)));
    let contacts = Rc::new(RefCell::new(argus_physics::Contacts::default()));
    argus_physics::install_with(&mut world, &mut sched, tiles.clone(), contacts.clone())?;
    let rigid = install_rigid(&mut world, &mut sched, &tiles, cfg)?;
    // The platform's setup (phase 5 item 9): the game's own, not a mod's.
    let platform = argus_platform::PlatformConfig::load(root)?;
    // Navigation over the tilemaps; its agents move after the physics.
    let nav = Rc::new(RefCell::new(argus_nav::NavStore::new(
        root,
        Some(tiles.clone()),
    )));
    argus_nav::install(&mut world, &mut sched, nav.clone())?;
    // Jobs on top of it: workers claim, walk, work, and carry.
    argus_jobs::install(&mut world, &mut sched, nav.clone())?;
    let zones = Rc::new(RefCell::new(argus_camera::Zones::default()));
    argus_camera::install(&mut world, &mut sched, zones.clone())?;
    argus_post::install(&mut world, &mut sched)?;
    // 2D lighting's components (phase 5 item 3): no systems, the
    // renderer reads them.
    argus_light::register(&mut world)?;
    // After the renderer registered Sprite (which it writes), before
    // scripts, so it runs first in the late stage.
    let anims = Rc::new(RefCell::new(argus_anim::AnimStore::default()));
    argus_anim::install(&mut world, &mut sched, anims.clone())?;
    // Spine skeletons (phase 5 item 8): posed after the animator, from
    // the same store, their slots drawn as sprites.
    argus_skeletal::install(&mut world, &mut sched, anims.clone())?;
    argus_tween::install(&mut world, &mut sched)?;
    let particles = Rc::new(RefCell::new(argus_particles::ParticleStore::default()));
    argus_particles::install(&mut world, &mut sched, particles.clone())?;
    // After the input map (it reads the players' actions) and the camera
    // (a tree hung from the world is placed through it), before scripts,
    // so a system may read or write the UI's components.
    let ui = Rc::new(RefCell::new(argus_ui::Shared {
        theme: argus_ui::Theme::load(root)?,
        ..Default::default()
    }));
    argus_ui::install(&mut world, &mut sched, ui.clone())?;
    // The string tables every UiText key and q:text reads (phase 3 item 4),
    // and conversations played through the UI.
    ui.borrow_mut().strings = argus_ui::Strings::load_layers(layers, &cfg.strings_base);
    let dialogue = Rc::new(RefCell::new(argus_dialogue::Graph::default()));
    argus_dialogue::install(&mut world, &mut sched, dialogue.clone(), ui.clone())?;
    // Timelines over the tweens, animators, cameras, and conversations
    // (phase 3 item 5): first in the late stage.
    let timelines = Rc::new(RefCell::new(argus_timeline::Timelines::default()));
    argus_timeline::install(&mut world, &mut sched, timelines.clone())?;
    // Rebinding, a UI over the action map (after the UI's input).
    if let Some(st) = &input {
        let tables = ui.clone();
        let prompt: argus_input::rebind::Prompt = Rc::new(move |w: &World| {
            let lang = argus_ui::strings::language(w);
            tables.borrow().strings.text(&lang, "input.press_a_key")
        });
        argus_input::rebind::install_with(&mut world, &mut sched, st.clone(), prompt)?;
        // Button prompts for the device in use (phase 5 item 9), after the
        // rebinding, so a rebind shows at once.
        argus_input::glyphs::install(&mut world, &mut sched, st.clone())?;
    }
    let mut host = ScriptHost::new(ScriptConfig {
        day_seconds: cfg.day_seconds,
        ..ScriptConfig::default()
    })?;
    host.set_paths(Rc::new(NavPaths(nav.clone())));
    host.set_tiles(Rc::new(ScriptTiles(tiles.clone())));
    host.set_casts(Rc::new(PhysicsCasts(tiles.clone())));
    let save_list: Rc<RefCell<Vec<argus_luau::SaveInfo>>> = Default::default();
    host.set_saves(Rc::new(HostSaves {
        list: save_list.clone(),
        input: input.clone(),
    }));
    host.set_texts(Rc::new(UiTexts(ui.clone())));
    host.set_screen(Rc::new(UiScreen(ui.clone())));
    host.load_layers(&mut world, &mut sched, layers)?;
    let defs: Vec<DefSchema> = host
        .defs()
        .iter()
        .map(|d| {
            DefSchema::new(d.schema.clone())
                .key(d.key.clone())
                .prefix(d.prefix.clone())
        })
        .collect();
    let project = Project::open_with(root, with_engine_defs(defs), &layers.ids())?;
    host.set_defs(Rc::new(ProjectDefs(project.defs.clone())));
    host.set_prefabs(Rc::new(ProjectPrefabs::new(&project)));
    *dialogue.borrow_mut() = argus_dialogue::Graph::from_defs(&project.defs);
    *timelines.borrow_mut() = read_timelines(&project.defs, &world);
    let mut scene = Scene::open(&project, scene_name, world.registry())?;
    zones.borrow_mut().rects = scene
        .rooms()
        .filter_map(|r| r.doc.bounds.map(|b| (r.name.clone(), b)))
        .collect();
    let mut issues: Vec<ArgusError> = project.issues().to_vec();
    issues.extend(scene.issues().iter().cloned());
    let mut load = None;
    if issues.is_empty() {
        load = Some(if scene.has_rooms() {
            scene.spawn_around(&mut world, None)?
        } else {
            scene.spawn_all(&mut world)?
        });
    }
    // Rooms stream around the followed entity (phase 3 item 6): first in
    // the input stage, before the physics moves anything.
    let scene = Rc::new(RefCell::new(scene));
    argus_scene::stream::install(&mut world, &mut sched, scene.clone())?;
    // The project's Rust plugins (§7), after the scripts whose components
    // they read and whose systems they order themselves after. One that
    // does not build or check is reported and left out, as a script that
    // does not compile is.
    let plugins =
        argus_plugins::load_all(root, &cfg.plugins, &opts.plugins, &mut world, &mut sched);
    apply_system_groups(
        &mut sched,
        &scene_doc.enable,
        &scene_doc.disable,
        world.registry(),
    )?;
    sched.validate()?;
    // A release pack carries the import cache (§15): its sprites come from
    // the catalog, the rest from their sidecars; a project imports them all.
    // So does a mod pack, for its own (phase 3 item 8).
    let mut assets = argus_assets::Importer::packed_layers(layers)?;
    let asset_report = assets.scan();
    set_fonts(&mut ui.borrow_mut().fonts, &assets);
    tiles.borrow_mut().set_tilesets(&assets);
    anims.borrow_mut().set_assets(&assets);
    particles.borrow_mut().set_assets(&assets);
    // Variations come from the run seed, so sound.played replays.
    #[cfg(feature = "audio")]
    let audio = opts.audio.then(|| {
        argus_audio::AudioEngine::load_layers(layers, seed).map(|mut a| {
            a.set_asset_sounds(assets.sidecar_sounds());
            a
        })
    });
    Ok(Assembly {
        world,
        sched,
        clock,
        seed,
        scene_doc,
        input,
        tiles,
        nav,
        zones,
        anims,
        particles,
        contacts,
        rigid,
        platform,
        host,
        plugins,
        project,
        scene,
        issues,
        ui,
        dialogue,
        timelines,
        load,
        assets,
        asset_report,
        #[cfg(feature = "audio")]
        audio,
        mods: crate::mods::Report::default(),
        save_list,
    })
}

/// The rigid-body module, when `argus.toml` lists it (phase 5 item 7).
#[cfg(feature = "rigid")]
fn install_rigid(
    world: &mut World,
    sched: &mut Scheduler,
    tiles: &Rc<RefCell<argus_tiles::MapStore>>,
    cfg: &ProjectConfig,
) -> Result<crate::keyframe::RigidSource> {
    if !cfg.uses("rigid") {
        return Ok(None);
    }
    let store = Rc::new(RefCell::new(argus_rigid::RigidStore::new()));
    argus_rigid::install(world, sched, tiles.clone(), store.clone())?;
    Ok(Some(store))
}

/// A build without the rigid-body module cannot run a game that uses it.
#[cfg(not(feature = "rigid"))]
fn install_rigid(
    _: &mut World,
    _: &mut Scheduler,
    _: &Rc<RefCell<argus_tiles::MapStore>>,
    cfg: &ProjectConfig,
) -> Result<crate::keyframe::RigidSource> {
    if cfg.uses("rigid") {
        return Err(ArgusError::new(
            argus_ecs::codes::BAD_STATE,
            format!("{} uses the rigid-body module, and this build does not have it", cfg.name),
        )
        .hint("argus build includes it for a game whose argus.toml lists modules = [\"rigid\"]; cargo builds it with --features rigid."));
    }
    Ok(())
}

/// Scripts' maps (`q:tiles`, phase 5 item 4): the session's map store, a
/// runtime map's cells written, any map's read.
struct ScriptTiles(Rc<RefCell<argus_tiles::MapStore>>);

/// Shape queries for scripts (`q:raycast`, `q:shape_cast`, `q:overlap`;
/// 2D staples item 0), on argus-physics' geometry over the colliders and
/// the maps' collision cells.
struct PhysicsCasts(Rc<RefCell<argus_tiles::MapStore>>);

fn cast_round(s: &argus_luau::CastShape) -> argus_physics::geom::Round {
    use argus_physics::geom::{Round, V};
    let r = s.radius;
    if !s.points.is_empty() {
        let pts: Vec<V> = s.points.iter().map(|p| V::new(p[0], p[1])).collect();
        return Round::hull(&pts, r);
    }
    if s.size[0] > 0.0 || s.size[1] > 0.0 {
        let mut b = Round::rect(V::default(), s.size[0] * 0.5, s.size[1] * 0.5);
        b.r = r;
        return b;
    }
    let half = s.height * 0.5 - r;
    if half > 0.0 {
        return Round::capsule(V::new(0.0, -half), V::new(0.0, half), r);
    }
    Round::circle(V::default(), r)
}

fn cast_filter(f: &argus_luau::CastFilter) -> argus_physics::query::Filter {
    argus_physics::query::Filter {
        mask: f.mask,
        colliders: f.colliders,
        tiles: f.tiles,
        one_way: f.one_way,
        ignore: f.ignore.clone(),
    }
}

fn cast_hit(h: &argus_physics::query::Hit) -> argus_luau::CastHit {
    use argus_physics::query::Target;
    let (entity, tile) = match h.target {
        Target::Entity(e) => (Some(e), None),
        Target::Tile { map, col, row } => (None, Some((map, col, row))),
    };
    argus_luau::CastHit {
        entity,
        tile,
        point: [h.point.x, h.point.y],
        normal: [h.normal.x, h.normal.y],
        position: [h.position.x, h.position.y],
        fraction: h.fraction,
        distance: h.distance,
    }
}

impl argus_luau::Casts for PhysicsCasts {
    fn raycast(
        &self,
        world: &World,
        from: [f32; 2],
        to: [f32; 2],
        filter: &argus_luau::CastFilter,
        all: bool,
    ) -> Result<Vec<argus_luau::CastHit>> {
        use argus_physics::geom::V;
        let hits = argus_physics::query::raycast(
            world,
            &mut self.0.borrow_mut(),
            V::new(from[0], from[1]),
            V::new(to[0], to[1]),
            &cast_filter(filter),
            all,
        )?;
        Ok(hits.iter().map(cast_hit).collect())
    }

    fn shape_cast(
        &self,
        world: &World,
        shape: &argus_luau::CastShape,
        from: [f32; 2],
        to: [f32; 2],
        filter: &argus_luau::CastFilter,
    ) -> Result<Option<argus_luau::CastHit>> {
        use argus_physics::geom::V;
        let hit = argus_physics::query::shape_cast(
            world,
            &mut self.0.borrow_mut(),
            &cast_round(shape),
            V::new(from[0], from[1]),
            V::new(to[0], to[1]),
            &cast_filter(filter),
        )?;
        Ok(hit.as_ref().map(cast_hit))
    }

    fn overlap(
        &self,
        world: &World,
        shape: &argus_luau::CastShape,
        at: [f32; 2],
        filter: &argus_luau::CastFilter,
    ) -> Result<Vec<argus_luau::CastHit>> {
        use argus_physics::geom::V;
        let hits = argus_physics::query::overlap(
            world,
            &mut self.0.borrow_mut(),
            &cast_round(shape),
            V::new(at[0], at[1]),
            &cast_filter(filter),
        )?;
        Ok(hits.iter().map(cast_hit).collect())
    }
}

impl ScriptTiles {
    /// The map file a Tilemap entity places.
    fn file(world: &World, e: argus_ecs::Entity) -> Result<String> {
        let t = world.get::<argus_tiles::Tilemap>(e).map_err(|_| {
            ArgusError::new(
                argus_ecs::codes::UNKNOWN_FIELD,
                format!(
                    "{} has no Tilemap: q:tiles takes the entity that places a map",
                    argus_ecs::text::entity_id(world, e)
                ),
            )
        })?;
        Ok(world.strings().resolve(t.map))
    }
}

impl argus_luau::Tiles for ScriptTiles {
    fn size(&self, world: &World, map: argus_ecs::Entity) -> Result<(u32, u32, Vec<String>)> {
        let file = Self::file(world, map)?;
        let mut st = self.0.borrow_mut();
        let m = st.loaded(&file)?;
        Ok((
            m.index.width,
            m.index.height,
            m.index.layers.iter().map(|l| l.name.clone()).collect(),
        ))
    }

    fn grid(&self, world: &World, map: argus_ecs::Entity) -> Result<([f32; 2], f32)> {
        let file = Self::file(world, map)?;
        let origin = world
            .get::<argus_ecs::GlobalTransform>(map)
            .map(|g| [g.pos.x, g.pos.y])
            .or_else(|_| {
                world
                    .get::<argus_ecs::Transform>(map)
                    .map(|t| [t.pos.x, t.pos.y])
            })
            .unwrap_or([0.0, 0.0]);
        let mut st = self.0.borrow_mut();
        Ok((origin, st.loaded(&file)?.index.tile_size))
    }

    fn get(
        &self,
        world: &World,
        map: argus_ecs::Entity,
        layer: &str,
        x: u32,
        y: u32,
    ) -> Result<u32> {
        let file = Self::file(world, map)?;
        let mut st = self.0.borrow_mut();
        let m = st.loaded(&file)?;
        let li = m.layer(layer)?;
        if x >= m.index.width || y >= m.index.height {
            return Ok(0);
        }
        m.cell(li, x, y)
    }

    fn set(
        &self,
        world: &World,
        map: argus_ecs::Entity,
        layer: &str,
        rect: [u32; 4],
        cells: &[u32],
    ) -> Result<()> {
        let file = Self::file(world, map)?;
        let mut st = self.0.borrow_mut();
        let m = st.loaded(&file)?;
        if !m.index.runtime {
            return Err(ArgusError::new(
                argus_tiles::codes::BAD_MAP,
                format!("{file} is the project's map: a script writes only a runtime map's tiles"),
            )
            .hint("Mark the map's index runtime = true for tiles the game makes each run; the project's maps change with edit.tiles or in the editor."));
        }
        let li = m.layer(layer)?;
        m.set(
            li,
            argus_tiles::Rect::new(rect[0], rect[1], rect[2], rect[3]),
            cells,
        )
    }
}

/// Scripts' prefabs (`q:spawn_prefab`, GitHub issue 1): the project's as
/// they were when the host gave them, each resolved on its first spawn and
/// kept. A host gives them again when a prefab changes.
pub struct ProjectPrefabs {
    root: std::path::PathBuf,
    prefabs: argus_scene::PrefabLibrary,
    defs: argus_scene::Defs,
    plans: RefCell<std::collections::HashMap<String, Rc<argus_luau::SpawnPlan>>>,
}

impl ProjectPrefabs {
    /// The project's prefabs now.
    pub fn new(project: &Project) -> ProjectPrefabs {
        ProjectPrefabs {
            root: project.root.clone(),
            prefabs: project.prefabs.clone(),
            defs: project.defs.clone(),
            plans: RefCell::default(),
        }
    }

    fn plan_of(re: &argus_scene::ResolvedEntity, name: &str) -> argus_luau::SpawnPlan {
        use argus_scene::Provenance;
        argus_luau::SpawnPlan {
            name: name.to_string(),
            tags: re.tags.iter().map(|(t, _)| t.clone()).collect(),
            // What the prefab chain sets: the rest is the schema's default,
            // which a fresh entity has already.
            components: re
                .components
                .iter()
                .map(|c| {
                    let set = c
                        .fields
                        .iter()
                        .filter(|f| f.from != Provenance::Default)
                        .map(|f| (f.name.clone(), f.value.clone()))
                        .collect();
                    (c.name.clone(), set)
                })
                .collect(),
            children: re
                .children
                .iter()
                .map(|ch| Self::plan_of(ch, ch.id.rsplit('/').next().unwrap_or(&ch.id)))
                .collect(),
            file: re.file.clone(),
        }
    }
}

impl argus_luau::Prefabs for ProjectPrefabs {
    fn plan(&self, world: &World, name: &str) -> Result<Rc<argus_luau::SpawnPlan>> {
        if let Some(p) = self.plans.borrow().get(name) {
            return Ok(p.clone());
        }
        let ctx = argus_scene::resolve::ResolveCtx {
            root: &self.root,
            prefabs: &self.prefabs,
            defs: &self.defs,
            moves: &[],
            registry: world.registry(),
        };
        // An entity whose file names the prefab and nothing more.
        let doc = argus_scene::EntityDoc {
            prefab: Some(name.to_string()),
            at: "q:spawn_prefab".to_string(),
            ..Default::default()
        };
        let mut issues = Vec::new();
        let file = format!("prefabs/{name}.toml");
        let re = argus_scene::resolve::resolve(&ctx, name, &doc, &file, &mut issues)?;
        let plan = Rc::new(Self::plan_of(&re, name));
        self.plans
            .borrow_mut()
            .insert(name.to_string(), plan.clone());
        Ok(plan)
    }
}

/// Scripts' path queries (`q:path` and the rest, phase 3 item 2), answered
/// by the session's navigation.
struct NavPaths(Rc<RefCell<argus_nav::NavStore>>);

impl argus_luau::Paths for NavPaths {
    fn path(
        &self,
        world: &World,
        from: [f32; 2],
        to: [f32; 2],
        grid: argus_ecs::Entity,
    ) -> Result<Option<(Vec<[f32; 2]>, f32)>> {
        match self.0.borrow_mut().walk(world, from, to, grid, false) {
            Ok(Some(w)) => Ok(w
                .route
                .map(|r| (w.points, r.cost as f32 / argus_nav::grid::STRAIGHT as f32))),
            Ok(None) => Ok(None),
            Err(e) if e.code == argus_nav::codes::OFF_GRID => Ok(None),
            Err(e) => Err(e),
        }
    }
    fn walkable(&self, world: &World, at: [f32; 2], grid: argus_ecs::Entity) -> Result<bool> {
        let g = self.0.borrow_mut().grid_for(world, at, grid, false)?;
        Ok(g.and_then(|(_, g)| g.locate(at).map(|i| g.open(i)))
            .unwrap_or(false))
    }
    fn distance(
        &self,
        world: &World,
        field: argus_ecs::Entity,
        at: [f32; 2],
    ) -> Result<Option<f32>> {
        let f = self.0.borrow_mut().field_of(world, field, false)?;
        Ok(f.and_then(|(g, f)| {
            g.locate(at)
                .and_then(|i| f.cost(i))
                .map(|c| c as f32 / argus_nav::grid::STRAIGHT as f32)
        }))
    }
    fn downhill(
        &self,
        world: &World,
        field: argus_ecs::Entity,
        at: [f32; 2],
    ) -> Result<Option<[f32; 2]>> {
        let f = self.0.borrow_mut().field_of(world, field, false)?;
        Ok(f.and_then(|(g, f)| {
            g.locate(at)
                .and_then(|i| argus_nav::search::downhill(&g, &f, i))
                .map(|n| g.centre(n))
        }))
    }
}

/// The def tables the engine declares, beside a project's own: `dialogue`
/// (phase 3 item 4) and `timeline` (item 5), so every project has them and
/// `world.defs` answers them.
pub fn with_engine_defs(mut defs: Vec<DefSchema>) -> Vec<DefSchema> {
    if !defs.iter().any(|d| d.schema.name == argus_dialogue::TABLE) {
        defs.push(argus_dialogue::schema());
    }
    if !defs.iter().any(|d| d.schema.name == argus_timeline::TABLE) {
        defs.push(argus_timeline::schema());
    }
    defs
}

/// A project's timelines, their cues' fields checked against the world's
/// components (every script's declared by now).
pub fn read_timelines(defs: &argus_scene::Defs, world: &World) -> argus_timeline::Timelines {
    let mut t = argus_timeline::Timelines::from_defs(defs);
    t.check(world.registry());
    t
}

/// Scripts' player-facing strings (`q:text`, `q:languages`, phase 3 item 4):
/// the UI's string tables, in the world's language.
pub struct UiTexts(pub Rc<RefCell<argus_ui::Shared>>);

/// Scripts' screen (issue 21): the UI canvas, ui.toml's size, and the
/// world through the drawing camera, as the layout and the renderer use.
pub struct UiScreen(pub Rc<RefCell<argus_ui::Shared>>);

impl argus_luau::Screen for UiScreen {
    fn to_world(&self, world: &World, p: [f32; 2]) -> Option<[f32; 2]> {
        let t = &self.0.borrow().theme;
        argus_ui::canvas_to_world(world, p, [t.width, t.height])
    }
    fn to_canvas(&self, world: &World, p: [f32; 2]) -> Option<[f32; 2]> {
        let t = &self.0.borrow().theme;
        argus_ui::world_to_canvas(world, p, [t.width, t.height])
    }
}

impl argus_luau::Texts for UiTexts {
    fn text(&self, world: &World, key: &str) -> Option<String> {
        let lang = argus_ui::strings::language(world);
        self.0.borrow().strings.get(&lang, key).map(str::to_string)
    }
    fn languages(&self) -> Vec<String> {
        self.0.borrow().strings.languages()
    }
}

/// Scripts' data tables (`q:def`, `q:defs`, phase 3 item 3): the project's
/// as they were read when the scene opened.
pub struct ProjectDefs(pub argus_scene::Defs);

impl ProjectDefs {
    fn fields(
        t: &argus_scene::DefTable,
        r: &argus_scene::DefRow,
    ) -> Vec<(String, argus_ecs::Value)> {
        t.schema
            .fields
            .iter()
            .zip(&r.values)
            .map(|(f, v)| (f.name.clone(), v.clone()))
            .collect()
    }
}

impl argus_luau::DefRows for ProjectDefs {
    fn row(&self, r: &str) -> Option<Vec<(String, argus_ecs::Value)>> {
        self.0.resolve(r).map(|(t, row)| Self::fields(t, row))
    }
    fn table(
        &self,
        name: &str,
    ) -> Option<(String, Vec<(String, Vec<(String, argus_ecs::Value)>)>)> {
        let t = self.0.table(name)?;
        Some((
            t.prefix.clone(),
            t.rows
                .iter()
                .map(|r| (r.key.clone(), Self::fields(t, r)))
                .collect(),
        ))
    }
}

/// The project's font assets as the UI's fonts (phase 3 item 1): each one
/// shaped from its bytes and drawn from its atlas, which the renderer loads
/// under the asset's name; a font no longer imported is dropped.
pub fn set_fonts(fonts: &mut argus_ui::Fonts, assets: &argus_assets::Importer) {
    let names: Vec<String> = assets.fonts().map(|(n, _)| n.clone()).collect();
    fonts.retain(|n| names.iter().any(|m| m == n));
    for (name, f) in assets.fonts() {
        fonts.add(name, Rc::new(argus_ui::SdfFont::from_asset(name, f)));
    }
}

/// `scene.toml`'s `systems.enable` and `systems.disable`: names or
/// `prefix*` patterns; with an enable list, every system not on it is off.
pub fn apply_system_groups(
    sched: &mut Scheduler,
    enable: &[String],
    disable: &[String],
    registry: &argus_ecs::Registry,
) -> Result<()> {
    let names: Vec<String> = sched
        .systems(registry)?
        .into_iter()
        .map(|s| s.name)
        .collect();
    let matches = |pat: &str, name: &str| {
        pat == "*" || pat == name || pat.strip_suffix('*').is_some_and(|p| name.starts_with(p))
    };
    for n in &names {
        if !enable.is_empty() && !enable.iter().any(|p| matches(p, n)) {
            sched.disable(n, "not in scene.toml systems.enable")?;
        }
        if disable.iter().any(|p| matches(p, n)) {
            sched.disable(n, "listed in scene.toml systems.disable")?;
        }
    }
    Ok(())
}
