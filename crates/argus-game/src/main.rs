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
//! Flags: `--headless <ticks>` runs that many ticks with no window and
//! prints the world's digest (how `argus build` checks what it built);
//! `--lang <code>` plays in a language pack; `--data <dir>` reads packs
//! from elsewhere; `--scene` and `--seed` as in the daemon.

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
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// The code this executable was built from (`argus build` sets it), which
/// every report records.
const CODE: &str = match option_env!("ARGUS_CODE") {
    Some(c) => c,
    None => "dev",
};

/// The last panic's message and place, for a crash report.
static PANIC: Mutex<Option<String>> = Mutex::new(None);

#[derive(Default)]
struct Args {
    data: Option<PathBuf>,
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
    let dir = exe.parent().unwrap_or(Path::new(".")).to_path_buf();
    let data = args.data.clone().unwrap_or_else(|| dir.join("data"));
    // The game's files live in memory, under a root that is not on disk.
    let root = dir.join("game");
    let packs = argus_player::pack::mount_dir(&data, &root, args.lang.as_deref())
        .map_err(|e| format!("{} {}", e.message, e.hint))?;
    let mut p = Player::open(&root, args.scene.as_deref(), args.seed)
        .map_err(|e| format!("{} {}", e.message, e.hint))?;
    p.record();
    let save = argus_player::save::save_dir(&p.name);
    if let Some(n) = args.headless {
        p.step(n).map_err(|e| e.message)?;
        let mut saves = Vec::new();
        for (name, bytes) in p.take_saves(CODE) {
            saves.push(
                write_save(save.as_deref(), &name, &bytes)?
                    .display()
                    .to_string(),
            );
        }
        println!(
            "{}",
            json!({ "packs": packs, "status": p.status(), "code": CODE, "save": save, "saves": saves })
        );
        return Ok(());
    }
    let cfg = argus_scene::ProjectConfig::load(&root).map_err(|e| e.message)?;
    let key = cfg.mark_key.unwrap_or_else(|| "F8".to_string());
    let mut game = Game::open(p, key, save)?;
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
                return Err(format!("The game stopped: {msg}\n\n{saved}"));
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
}

impl Game {
    fn open(mut p: Player, key: String, save: Option<PathBuf>) -> Result<Game, String> {
        let host = WindowHost::open(&p.name, 1280, 720).map_err(|e| e.message)?;
        let window = host.window().ok_or("the window did not open")?;
        let mut r = Renderer::windowed(window, false).map_err(|e| e.message)?;
        r.load_catalog(&p.assets).map_err(|e| e.message)?;
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
        })
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
        if !devs.is_empty() {
            self.p.devices(&devs);
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
        }
        let _ = self.p.sched.present(&mut self.p.world);
        // The game's settings say whether the window fills the screen.
        self.host
            .set_fullscreen(argus_ui::wants_fullscreen(&self.p.world));
        self.overlay();
        let (w, h) = self.r.gpu().size();
        let view = argus_render::view_for(&self.p.world, w, h);
        let extras = self.p.tile_extras(view);
        self.r.set_time(self.p.sched.time() as f32);
        // The player's UI (a load replaces the player, and its UI with it).
        self.r.set_ui(Some(self.p.ui.clone()));
        let store = self.p.particles.borrow();
        let tick = self.p.sched.tick();
        let alpha = self.owed.clamp(0.0, 1.0) as f32;
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
        for (name, bytes) in self.p.take_saves(CODE) {
            if let Err(e) = write_save(self.save.as_deref(), &name, &bytes) {
                self.toast = Some((format!("The game could not be saved: {e}"), Instant::now()));
            }
        }
        for name in self.p.take_loads() {
            let r = read_save(self.save.as_deref(), &name)
                .and_then(|bytes| self.p.load(&bytes).map_err(|e| e.message));
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
            Ok(path) => format!("Report saved: {}", path.display()),
            Err(e) => format!("The report could not be saved: {e}"),
        };
        self.toast = Some((said.clone(), Instant::now()));
        said
    }

    fn write_report(&self, note: &str, kind: &str) -> Result<PathBuf, String> {
        let bytes = self
            .p
            .report(note, kind, CODE)
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

/// A save file under the save location's `saves/` (§15: "a save location
/// per platform convention"; phase 3 item 0).
fn save_file(save: Option<&Path>, name: &str) -> Result<PathBuf, String> {
    let dir = save
        .ok_or("this system has no place for saves")?
        .join("saves");
    Ok(dir.join(argus_player::savegame::file_name(name)))
}

fn write_save(save: Option<&Path>, name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let path = save_file(save, name)?;
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path)
}

fn read_save(save: Option<&Path>, name: &str) -> Result<Vec<u8>, String> {
    let path = save_file(save, name)?;
    std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
}
