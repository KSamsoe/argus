//! A scene's world, assembled one way (phase 2 item 7): the daemon's
//! `session.open` and `scene.load`, the web page, and a release build all
//! call [`assemble`], so a game runs the same systems in the same order
//! wherever it runs. What each adds around it (the daemon's index, history,
//! and window; a player's devices and recorder) is its own.

use argus_ecs::{ArgusError, Result, World};
use argus_input::{ActionMap, InputState};
use argus_luau::{ScriptConfig, ScriptHost};
use argus_scene::{DefSchema, LoadReport, Project, ProjectConfig, Scene, SceneDoc, SeedPolicy};
use argus_sim::{Clock, ClockKind, Scheduler, SchedulerConfig};
use std::cell::RefCell;
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
}

impl Default for Options {
    fn default() -> Self {
        Options {
            random_seed: 1,
            audio: true,
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
    /// The Luau host (scripts, defs).
    pub host: ScriptHost,
    /// The project.
    pub project: Project,
    /// The scene (rooms, streaming).
    pub scene: Scene,
    /// Problems in the project or the scene; when there are any, nothing
    /// was spawned.
    pub issues: Vec<ArgusError>,
    /// What spawning did.
    pub load: Option<LoadReport>,
    /// The UI's theme (`ui.toml`) and fonts, shared by its systems and
    /// whatever draws them (phase 3 item 1).
    pub ui: Rc<RefCell<argus_ui::Shared>>,
    /// The imported assets.
    pub assets: argus_assets::Importer,
    /// The import's report.
    pub asset_report: argus_assets::Report,
    /// The mixer, when asked for: its sounds set, no device open.
    #[cfg(feature = "audio")]
    pub audio: Option<Result<argus_audio::AudioEngine>>,
}

/// Assemble scene `scene_name` of the project at `root`.
pub fn assemble(
    root: &Path,
    cfg: &ProjectConfig,
    scene_name: &str,
    seed: Option<u64>,
    opts: &Options,
) -> Result<Assembly> {
    let scene_doc = SceneDoc::load(root, scene_name)?;
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
            Some(state)
        }
        None => None,
    };
    #[cfg(feature = "render")]
    argus_render::register(&mut world)?;
    #[cfg(feature = "audio")]
    argus_audio::register(&mut world)?;
    let tiles = Rc::new(RefCell::new(argus_tiles::MapStore::new(root)));
    argus_physics::install(&mut world, &mut sched, tiles.clone())?;
    // Navigation over the tilemaps; its agents move after the physics.
    let nav = Rc::new(RefCell::new(argus_nav::NavStore::new(
        root,
        Some(tiles.clone()),
    )));
    argus_nav::install(&mut world, &mut sched, nav.clone())?;
    let zones = Rc::new(RefCell::new(argus_camera::Zones::default()));
    argus_camera::install(&mut world, &mut sched, zones.clone())?;
    argus_post::install(&mut world, &mut sched)?;
    // After the renderer registered Sprite (which it writes), before
    // scripts, so it runs first in the late stage.
    let anims = Rc::new(RefCell::new(argus_anim::AnimStore::default()));
    argus_anim::install(&mut world, &mut sched, anims.clone())?;
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
    // Rebinding, a UI over the action map (after the UI's input).
    if let Some(st) = &input {
        argus_input::rebind::install(&mut world, &mut sched, st.clone())?;
    }
    let mut host = ScriptHost::new(ScriptConfig {
        day_seconds: cfg.day_seconds,
        ..ScriptConfig::default()
    })?;
    host.set_paths(Rc::new(NavPaths(nav.clone())));
    host.load_project(&mut world, &mut sched, root)?;
    let defs: Vec<DefSchema> = host
        .defs()
        .iter()
        .map(|d| {
            DefSchema::new(d.schema.clone())
                .key(d.key.clone())
                .prefix(d.prefix.clone())
        })
        .collect();
    let project = Project::open(root, defs)?;
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
    apply_system_groups(
        &mut sched,
        &scene_doc.enable,
        &scene_doc.disable,
        world.registry(),
    )?;
    sched.validate()?;
    // A release pack carries the import cache (§15): its sprites come from
    // the catalog, the rest from their sidecars; a project imports them all.
    let mut assets = if argus_os::fs::is_file(root.join(argus_assets::CATALOG)) {
        argus_assets::Importer::packed(root)?
    } else {
        argus_assets::Importer::new(root)
    };
    let asset_report = assets.scan();
    set_fonts(&mut ui.borrow_mut().fonts, &assets);
    tiles.borrow_mut().set_tilesets(&assets);
    anims.borrow_mut().set_assets(&assets);
    particles.borrow_mut().set_assets(&assets);
    // Variations come from the run seed, so sound.played replays.
    #[cfg(feature = "audio")]
    let audio = opts.audio.then(|| {
        argus_audio::AudioEngine::load(root, seed).map(|mut a| {
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
        host,
        project,
        scene,
        issues,
        ui,
        load,
        assets,
        asset_report,
        #[cfg(feature = "audio")]
        audio,
    })
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
