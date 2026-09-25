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
    let mut host = ScriptHost::new(ScriptConfig {
        day_seconds: cfg.day_seconds,
        ..ScriptConfig::default()
    })?;
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
        zones,
        anims,
        particles,
        host,
        project,
        scene,
        issues,
        load,
        assets,
        asset_report,
        #[cfg(feature = "audio")]
        audio,
    })
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
