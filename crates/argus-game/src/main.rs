//! The Argus release executable (§15, phase 2 item 7).
//!
//! What a player double-clicks. `argus build` puts it beside the game's
//! `data/` directory under the game's own name. It mounts the packs in
//! memory, opens the game exactly as the daemon does (argus-player's one
//! assembly), and plays it in a window with the keyboard, the mouse, and
//! gamepads. There is no protocol server, no index, and no `.argus/`: this
//! crate does not link the daemon, so nothing can attach to it.
//!
//! Two dev features stay on purpose. The bounded recorder keeps the last
//! thirty to sixty seconds. The report key (F8, or the project's
//! `[marks] key`) pauses the game and asks for a note on the window's
//! bottom line; Enter saves a report, a replay file, under `reports/` in the
//! save location, and `argus mark import` makes it a mark the agent opens.
//! A crash saves a report too.
//!
//! Mods (§15, phase 3 item 8): mod packs, and mod directories, in the
//! `mods/` folder of the save location load over the game in the order its
//! `load-order.toml` declares (by id without one), each at `mods/<id>/`
//! beside the game's files in memory. A mod that cannot load is left out
//! and the bottom line says which and why; a game with `[mods] enabled =
//! false` looks at no folder.
//!
//! Flags: `--headless <ticks>` runs that many ticks with no window and
//! prints the world's digest (how `argus build` checks what it built);
//! `--lang <code>` plays in a language pack; `--data <dir>` reads packs
//! from elsewhere; `--mods <dir>` reads mods from elsewhere, `--no-mods`
//! reads none; `--scene` and `--seed` as in the daemon.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use argus_input::DeviceInput;
use argus_player::desktop::{window_input, Pads, WindowInput};
use argus_player::Player;
use argus_render::window::WindowHost;
use argus_render::Renderer;
use argus_sim::ClockKind;
use serde_json::json;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The code this executable was built from, when `argus build` compiled it
/// for the game (`ARGUS_CODE`).
const BUILT_FOR: Option<&str> = option_env!("ARGUS_CODE");

/// The game's code, which every report and save records: the packs' since
/// a prebuilt runtime serves any game (distribution item 1), else what this
/// executable was built for, else "dev".
static CODE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn code() -> &'static str {
    CODE.get()
        .map_or(BUILT_FOR.unwrap_or("dev"), String::as_str)
}

/// The last panic's message and place, for a crash report.
static PANIC: Mutex<Option<String>> = Mutex::new(None);

#[derive(Default)]
struct Args {
    data: Option<PathBuf>,
    mods: Option<PathBuf>,
    no_mods: bool,
    lang: Option<String>,
    headless: Option<u64>,
    scene: Option<String>,
    seed: Option<u64>,
}

fn parse() -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(w) = it.next() {
        let mut need = |f: &str| it.next().ok_or_else(|| format!("{f} needs a value"));
        match w.as_str() {
            "--data" => a.data = Some(PathBuf::from(need("--data")?)),
            "--mods" => a.mods = Some(PathBuf::from(need("--mods")?)),
            "--no-mods" => a.no_mods = true,
            "--lang" => a.lang = Some(need("--lang")?),
            "--scene" => a.scene = Some(need("--scene")?),
            "--seed" => {
                a.seed = Some(
                    need("--seed")?
                        .parse()
                        .map_err(|_| "--seed takes a number")?,
                )
            }
            "--headless" => {
                a.headless = Some(
                    need("--headless")?
                        .parse()
                        .map_err(|_| "--headless takes a number of ticks")?,
                )
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(a)
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        *PANIC.lock().unwrap_or_else(|e| e.into_inner()) = Some(info.to_string());
    }));
    let args = match parse() {
        Ok(a) => a,
        Err(e) => {
            tell(&e, false);
            std::process::exit(2);
        }
    };
    let headless = args.headless.is_some();
    let code = match catch_unwind(AssertUnwindSafe(|| run(args))) {
        Ok(Ok(())) => 0,
        Ok(Err(e)) => {
            tell(&e, !headless);
            1
        }
        Err(_) => {
            let msg = PANIC.lock().unwrap_or_else(|e| e.into_inner()).take();
            tell(
                &format!("The game stopped: {}", msg.unwrap_or_default()),
                !headless,
            );
            101
        }
    };
    std::process::exit(code);
}

/// Say something went wrong: on the error stream, and in a box where a
/// player launched it from the desktop and has no console to read.
fn tell(msg: &str, boxed: bool) {
    eprintln!("{msg}");
    #[cfg(windows)]
    if boxed {
        #[link(name = "user32")]
        extern "system" {
            fn MessageBoxW(hwnd: isize, text: *const u16, caption: *const u16, kind: u32) -> i32;
        }
        let w = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
        let (t, c) = (w(msg), w("Argus"));
        // MB_ICONERROR.
        unsafe { MessageBoxW(0, t.as_ptr(), c.as_ptr(), 0x10) };
    }
    #[cfg(not(windows))]
    let _ = boxed;
}

fn run(args: Args) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    // A Mac names a program by the link it was started through; the game's
    // data is beside the file the link points to.
    #[cfg(not(windows))]
    let exe = exe.canonicalize().unwrap_or(exe);
    let dir = exe.parent().unwrap_or(Path::new(".")).to_path_buf();
    let data = args.data.clone().unwrap_or_else(|| dir.join("data"));
    // The game's files live in memory, under a root that is not on disk.
    let root = dir.join("game");
    let packs = argus_player::pack::mount_dir(&data, &root, args.lang.as_deref())
        .map_err(|e| format!("{} {}", e.message, e.hint))?;
    if let Some(c) = packs["code"].as_str().filter(|c| !c.is_empty()) {
        let _ = CODE.set(c.to_string());
    }
    let cfg = argus_scene::ProjectConfig::load(&root).map_err(|e| e.message)?;
    // The player's mods, over the game in memory (phase 3 item 8).
    let (mods, mods_said) = player_mods(&root, &cfg, &args);
    // The game's Rust plugins are libraries beside the executable (phase 5
    // item 6), built by `argus build`.
    let plugins = argus_plugins::Source::Prebuilt(dir.join("plugins"));
    let mut p = Player::open_from(&root, args.scene.as_deref(), args.seed, mods, plugins)
        .map_err(|e| format!("{} {}", e.message, e.hint))?;
    if let Some(e) = p.plugins.errors.first() {
        return Err(format!("{}: {}", e.at(), e.message));
    }
    let save = argus_player::save::save_dir(&p.name);
    // The player's settings (phase 5 item 2), before the first tick and the
    // recorder's first keyframe, so a report replays with them.
    let settings = argus_player::settings::Store::open(&mut p, save.as_deref(), None);
    p.record();
    // The saves there are, for the game's own menus (q:saves).
    let saves_dir = save.as_ref().map(|d| d.join("saves"));
    p.list_saves(saves_dir.as_deref());
    if let Some(n) = args.headless {
        p.step(n).map_err(|e| e.message)?;
        let mut saves = Vec::new();
        for (name, bytes) in p.take_saves(code()) {
            let path = save_file(save.as_deref(), &name).ok();
            let went = p
                .store_save(path.as_deref(), &name, &bytes)
                .map_err(|e| e.message)?;
            saves.push(json!({ "name": name, "path": path, "went": went }));
        }
        println!(
            "{}",
            json!({ "packs": packs, "status": p.status(), "code": code(), "engine": env!("CARGO_PKG_VERSION"), "save": save, "saves": saves, "mods": p.mods.to_json() })
        );
        return Ok(());
    }
    // The platform (phase 5 item 9), before the window: Steam's overlay
    // draws into the graphics device made after it starts. A headless run
    // (the build's own check, a script's) has no player and stays on the
    // null platform: it never starts Steam.
    let platform_said = start_platform(&mut p);
    let key = cfg.mark_key.unwrap_or_else(|| "F8".to_string());
    let left_out = p.mods.left_out_line();
    let mut game = Game::open(p, key, save, cfg.reports_url.clone(), settings)?;
    // Reports that did not go last time go now.
    game.send_later();
    // Mods that did not load, or a load order that did not read; Steam
    // that did not start.
    if let Some(line) = left_out.or(mods_said).or(platform_said) {
        game.toast = Some((line, Instant::now()));
    }
    loop {
        match catch_unwind(AssertUnwindSafe(|| game.frame())) {
            Ok(Ok(true)) => {}
            Ok(Ok(false)) => return Ok(()),
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                let msg = PANIC
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take()
                    .unwrap_or_default();
                let saved = game.save_report(&msg, "crash");
                let sent = game.send_now();
                return Err(format!("The game stopped: {msg}\n\n{saved}{sent}"));
            }
        }
    }
}

/// The game in a window. The renderer is declared before the window host so
/// it is dropped first: its surface belongs to the window.
struct Game {
    r: Renderer,
    host: WindowHost,
    p: Player,
    pads: Pads,
    last: Instant,
    owed: f64,
    key: String,
    prompt: Option<String>,
    toast: Option<(String, Instant)>,
    bar: Option<(String, u32)>,
    save: Option<PathBuf>,
    /// Where reports go (`[reports] url`), and the try under way.
    reports: Option<String>,
    sending: Option<mpsc::Receiver<argus_player::reports::Sent>>,
    /// The player's settings, written back when they change.
    settings: argus_player::settings::Store,
    /// The UI the screen reader was last given, to find what it asks for.
    access_ui: Vec<argus_render::semantic::UiSeen>,
}

impl Game {
    fn open(
        mut p: Player,
        key: String,
        save: Option<PathBuf>,
        reports: Option<String>,
        settings: argus_player::settings::Store,
    ) -> Result<Game, String> {
        let host = WindowHost::open(&p.name, 1280, 720).map_err(|e| e.message)?;
        let window = host.window().ok_or("the window did not open")?;
        let mut r = Renderer::windowed(window, false).map_err(|e| e.message)?;
        r.load_catalog(&p.assets).map_err(|e| e.message)?;
        load_glyphs(&mut r, &mut p);
        if let Some(a) = p.audio.as_mut() {
            let _ = a.open_device();
        }
        Ok(Game {
            r,
            host,
            p,
            pads: Pads::open(),
            last: Instant::now(),
            owed: 0.0,
            key,
            prompt: None,
            toast: None,
            bar: None,
            save,
            reports,
            sending: None,
            settings,
            access_ui: Vec::new(),
        })
    }

    /// The UI as the player's screen reader reads it: the semantic frame
    /// as an accessibility tree, made only while one is reading.
    fn give_screen_reader(&mut self) {
        let (w, h) = self.r.gpu().size();
        let canvas = {
            let sh = self.p.ui.borrow();
            [sh.theme.width, sh.theme.height]
        };
        let (world, assets, name) = (&self.p.world, &self.p.assets, &self.p.name);
        let mut seen = None;
        self.host.access(|| {
            let sf = argus_render::semantic::semantic(
                world,
                &argus_render::FrameSpec {
                    width: w,
                    height: h,
                    limit: 0,
                    groups: 0,
                    assets: Some(assets),
                    particles: None,
                    view: None,
                    ui_canvas: Some(canvas),
                },
            );
            let tree = argus_render::access::tree(&sf, name);
            seen = Some(sf.ui);
            tree
        });
        if let Some(ui) = seen {
            self.access_ui = ui;
        }
    }

    /// Where reports wait: the save location's `reports/`.
    fn reports_dir(&self) -> Option<PathBuf> {
        Some(self.save.as_ref()?.join("reports"))
    }

    /// Send the waiting reports in the background; the bottom line says
    /// what became of them ([`Game::sent`]).
    fn send_later(&mut self) {
        let (Some(url), Some(dir)) = (self.reports.clone(), self.reports_dir()) else {
            return;
        };
        if self.sending.is_some() || argus_player::reports::waiting(&dir).is_empty() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(argus_player::reports::send_waiting(
                &dir,
                &url,
                Duration::from_secs(10),
            ));
        });
        self.sending = Some(rx);
    }

    /// Send the waiting reports now, briefly (the game is stopping); what
    /// to add to what the player is told.
    fn send_now(&mut self) -> String {
        let (Some(url), Some(dir)) = (self.reports.clone(), self.reports_dir()) else {
            return String::new();
        };
        let r = argus_player::reports::send_waiting(&dir, &url, Duration::from_secs(3));
        match r.failed {
            None => "\nIt was sent to the team.".to_string(),
            Some(_) => "\nIt is sent the next time the game starts.".to_string(),
        }
    }

    /// What became of a background send, once it is over.
    fn sent(&mut self) {
        let Some(rx) = &self.sending else {
            return;
        };
        let Ok(r) = rx.try_recv() else {
            return;
        };
        self.sending = None;
        let said = match (r.sent.len(), r.failed) {
            (0, Some(e)) => {
                format!("Report not sent yet ({e}); it goes the next time the game starts")
            }
            (1, None) => "Report sent to the team".to_string(),
            (n, None) => format!("{n} reports sent to the team"),
            (n, Some(e)) => format!("{n} reports sent; the rest wait ({e})"),
        };
        self.toast = Some((said, Instant::now()));
    }

    /// One frame: devices in, the ticks the time owes, a picture. False
    /// when the player closed the window.
    fn frame(&mut self) -> Result<bool, String> {
        let open = self.host.pump();
        let mut devs = Vec::new();
        for ev in self.host.take_events() {
            match window_input(ev) {
                Some(WindowInput::Device(DeviceInput::Key { name, pressed })) => {
                    if self.prompt.is_some() {
                        if pressed {
                            self.prompt_key(&name);
                        }
                    } else if name == self.key {
                        if pressed {
                            self.prompt = Some(String::new());
                            devs.push(DeviceInput::ReleaseAll);
                        }
                    } else {
                        devs.push(DeviceInput::Key { name, pressed });
                    }
                }
                Some(WindowInput::Device(d)) if self.prompt.is_none() => devs.push(d),
                Some(WindowInput::Text(t)) => {
                    if let Some(s) = self.prompt.as_mut() {
                        s.extend(t.chars().filter(|c| !c.is_control()));
                    } else {
                        // The game's, for a slot that is typing (issue 23).
                        devs.push(DeviceInput::Text { text: t });
                    }
                }
                Some(WindowInput::Resized(w, h)) => self.r.gpu_mut().resize(w, h),
                Some(WindowInput::Cursor(x, y)) if self.prompt.is_none() => {
                    if let Some(f) = self.r.ui_fit() {
                        let [x, y] = f.to_canvas([x, y]);
                        devs.push(DeviceInput::Pointer { x, y });
                    }
                }
                _ => {}
            }
        }
        if !open {
            // A note half typed when the window closed is kept.
            if let Some(note) = self.prompt.take().filter(|n| !n.trim().is_empty()) {
                self.save_report(&note, "report");
            }
            return Ok(false);
        }
        let pads = self.pads.poll();
        if self.prompt.is_none() {
            devs.extend(pads);
        }
        // What the player's screen reader asked (phase 5 item 2): the
        // pointer to the button, and a click if it pressed it, input the
        // run records as it does a mouse's.
        let mut click = false;
        for ask in self.host.access_asks() {
            let Some((u, press)) = argus_render::access::asked(&ask, &self.access_ui) else {
                continue;
            };
            if let Some(f) = self.r.ui_fit() {
                let b = u.bounds;
                let [x, y] = f.to_canvas([(b[0] + b[2]) as f32 / 2.0, (b[1] + b[3]) as f32 / 2.0]);
                devs.push(DeviceInput::Pointer { x, y });
                click |= press;
            }
        }
        if !devs.is_empty() {
            self.p.devices(&devs);
        }
        // The platform's callbacks, and its overlay as an input (phase 5
        // item 9).
        self.p.poll_platform();
        if click {
            let _ = self
                .p
                .inject("ui_click", Some(&json!(true)), Some(1), false);
        }
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f64();
        self.last = now;
        let d = self.p.sched.driving();
        let (kind, hz) = (d.kind, d.hz());
        if self.prompt.is_none() && kind == ClockKind::Continuous && hz > 0.0 {
            // At 3x or 10x (q:speed, sim.speed) a frame owes that many
            // ticks; a slow frame catches up at most five frames' worth.
            let speed = self.p.sched.speed();
            self.owed = (self.owed + dt * hz * speed).min(5.0 * speed.max(1.0));
            let n = self.owed.floor();
            self.owed -= n;
            if n > 0.0 {
                self.p.step(n as u64).map_err(|e| e.message)?;
                self.saves_and_loads();
            }
        } else if self.prompt.is_none() && kind == ClockKind::Discrete {
            // Between turns the game moves (2D staples item 5): idle ticks
            // at the continuous clock's rate, recorded with the input.
            let hz = self.p.sched.config().hz.max(1.0);
            self.owed = (self.owed + dt * hz).min(5.0);
            let n = self.owed.floor();
            self.owed -= n;
            if n > 0.0 {
                self.p.idle(n as u64).map_err(|e| e.message)?;
            }
        }
        let _ = self.p.sched.present(&mut self.p.world);
        // The game's settings say whether the window fills the screen.
        self.host
            .set_fullscreen(argus_ui::wants_fullscreen(&self.p.world));
        // Settings the player changed are kept for the next launch.
        self.settings.changed(&self.p);
        self.give_screen_reader();
        self.overlay();
        let (w, h) = self.r.gpu().size();
        let view = argus_render::view_for(&self.p.world, w, h);
        let extras = self.p.tile_extras(view);
        let edges = self.p.tile_occluders(view);
        self.r.set_occluders(edges);
        self.r.set_time(self.p.sched.time() as f32);
        // The player's UI (a load replaces the player, and its UI with it).
        self.r.set_ui(Some(self.p.ui.clone()));
        let store = self.p.particles.borrow();
        let tick = self.p.sched.tick();
        // On a discrete clock a frame draws the latest turn (no time owed
        // between turns): with the owed time, 0 there, a turn-based game
        // drew a turn behind (Delve, 2026-10-10). As Scheduler::alpha has it.
        let alpha = match kind {
            ClockKind::Continuous => self.owed.clamp(0.0, 1.0) as f32,
            ClockKind::Discrete => 1.0,
        };
        self.r
            .render_frame(&self.p.world, tick, alpha, &extras, Some(&store))
            .map_err(|e| e.message)?;
        Ok(true)
    }

    fn prompt_key(&mut self, name: &str) {
        match name {
            "Enter" | "NumpadEnter" => {
                let note = self.prompt.take().unwrap_or_default();
                self.save_report(&note, "report");
            }
            "Escape" => self.prompt = None,
            "Backspace" => {
                if let Some(s) = self.prompt.as_mut() {
                    s.pop();
                }
            }
            _ => {}
        }
    }

    /// The saves the game asked for (`q:save`), kept under the save
    /// location's `saves/`, and the loads it asked for (`q:load`), done. A
    /// failure is said on the bottom line; a save that worked is the game's
    /// to mention.
    fn saves_and_loads(&mut self) {
        for (name, bytes) in self.p.take_saves(code()) {
            // On the disk and in the platform's cloud (phase 5 item 9).
            let path = save_file(self.save.as_deref(), &name).ok();
            if let Err(e) = self.p.store_save(path.as_deref(), &name, &bytes) {
                self.toast = Some((
                    format!("The game could not be saved: {}", e.message),
                    Instant::now(),
                ));
            }
        }
        for name in self.p.take_deletes() {
            let path = save_file(self.save.as_deref(), &name).ok();
            if let Err(e) = self.p.delete_save(path.as_deref(), &name) {
                self.toast = Some((
                    format!("The save {name:?} could not be deleted: {}", e.message),
                    Instant::now(),
                ));
            }
        }
        for name in self.p.take_loads() {
            // The newer of the disk's and the cloud's.
            let path = save_file(self.save.as_deref(), &name).ok();
            let r = self
                .p
                .fetch_save(path.as_deref(), &name)
                .map_err(|e| e.message)
                .and_then(|(bytes, _)| self.p.load(&bytes).map_err(|e| e.message));
            if let Err(e) = r {
                self.toast = Some((
                    format!("The save {name:?} could not be loaded: {e}"),
                    Instant::now(),
                ));
            }
        }
    }

    /// Save a report under the save location's `reports/`; what to tell the
    /// player about it.
    fn save_report(&mut self, note: &str, kind: &str) -> String {
        let said = match self.write_report(note, kind) {
            Ok(path) if self.reports.is_some() => {
                self.send_later();
                format!("Report saved: {}; sending it to the team", path.display())
            }
            Ok(path) => format!("Report saved: {}", path.display()),
            Err(e) => format!("The report could not be saved: {e}"),
        };
        self.toast = Some((said.clone(), Instant::now()));
        said
    }

    fn write_report(&self, note: &str, kind: &str) -> Result<PathBuf, String> {
        let bytes = self
            .p
            .report(note, kind, code())
            .ok_or("the recorder is off")?;
        let dir = self
            .save
            .as_ref()
            .ok_or("this system has no place for saves")?
            .join("reports");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut path = dir.join(format!("{secs}-{kind}.argr"));
        let mut n = 1;
        while path.exists() {
            n += 1;
            path = dir.join(format!("{secs}-{kind}-{n}.argr"));
        }
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        Ok(path)
    }

    /// The bottom line: the report prompt, or a few seconds of what became
    /// of a report.
    fn overlay(&mut self) {
        self.sent();
        if self
            .toast
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed().as_secs_f32() > 4.0)
        {
            self.toast = None;
        }
        let want = match (&self.prompt, &self.toast) {
            (Some(note), _) => Some((
                format!("Report: {note}_   (Enter saves, Esc cancels)"),
                [255, 196, 64, 255],
            )),
            (None, Some((t, _))) => Some((t.clone(), [120, 220, 140, 255])),
            (None, None) => None,
        };
        let width = self.r.gpu().size().0;
        let key = want.as_ref().map(|(t, _)| (t.clone(), width));
        if key == self.bar {
            return;
        }
        self.r.set_bar(want.as_ref().map(|(t, c)| (t.as_str(), *c)));
        self.bar = key;
    }
}

/// The mods to load: the folder `--mods` names, else `mods/` in the save
/// location; none with `--no-mods`. Their files are mounted under `root`.
/// Also a line for the player when the folder could not be read.
fn player_mods(
    root: &Path,
    cfg: &argus_scene::ProjectConfig,
    args: &Args,
) -> (argus_player::mods::Report, Option<String>) {
    use argus_player::mods;
    if args.no_mods {
        return (mods::Report::default(), None);
    }
    let Some(dir) = args
        .mods
        .clone()
        .or_else(|| argus_player::save::save_dir(&cfg.name).map(|d| d.join("mods")))
    else {
        return (mods::Report::default(), None);
    };
    match mods::choose(cfg, &dir, mods::Order::Folder) {
        Ok((report, found)) => match mods::mount(root, &found, &report.ids()) {
            Ok(()) => (report, None),
            Err(e) => (
                mods::Report::default(),
                Some(format!("Mods not loaded: {}", e.message)),
            ),
        },
        Err(e) => (
            mods::Report::default(),
            Some(format!("Mods not loaded: {}", e.message)),
        ),
    }
}

/// A save file under the save location's `saves/` (§15: "a save location
/// per platform convention"; phase 3 item 0).
fn save_file(save: Option<&Path>, name: &str) -> Result<PathBuf, String> {
    let dir = save
        .ok_or("this system has no place for saves")?
        .join("saves");
    Ok(dir.join(argus_player::savegame::file_name(name)))
}

/// Start the game's platform (phase 5 item 9): Steam, when the game was
/// built with it, `platform.toml` names its app, and Steam is running;
/// else the null platform, with a line for the player when Steam was meant
/// to be there.
fn start_platform(p: &mut Player) -> Option<String> {
    #[cfg(feature = "steam")]
    if let Some(app) = p.platform_cfg.steam_app_id {
        return match argus_steam::SteamPlatform::init(app) {
            Ok(s) => {
                p.set_platform(Box::new(s));
                None
            }
            Err(e) => Some(format!(
                "Playing without Steam: achievements and cloud saves wait for it. ({})",
                e.message
            )),
        };
    }
    let _ = p;
    None
}

/// The platform's pictures of the pad buttons the action map binds (Steam
/// Input's), as the sprites `glyph/<family>/<input>` that a UI's
/// InputGlyph panels draw. Without them a prompt is its label.
fn load_glyphs(r: &mut Renderer, p: &mut Player) -> usize {
    let Some(map) = p.action_map() else {
        return 0;
    };
    let inputs: std::collections::BTreeSet<String> = map
        .actions
        .iter()
        .filter_map(|a| a.bindings.get("gamepad"))
        .flatten()
        .cloned()
        .collect();
    let mut n = 0;
    for f in [
        argus_platform::Family::Xbox,
        argus_platform::Family::PlayStation,
        argus_platform::Family::Switch,
    ] {
        for i in &inputs {
            if let Some(g) = p.platform.glyph_image(f, i) {
                r.gpu_mut().add_atlas_frames(
                    &format!("glyph/{}/{i}", f.name()),
                    g.width,
                    g.height,
                    &g.rgba,
                    vec![[0.0, 0.0, 1.0, 1.0]],
                    vec![[0.5, 0.5]],
                );
                n += 1;
            }
        }
    }
    n
}
