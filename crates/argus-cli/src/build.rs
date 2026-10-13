//! `argus build` (§15, phase 2 item 7): a project as players get it.
//!
//! `argus build [--target windows|linux|macos|web] [--release] [--out dir]
//! [--since dir]` makes one directory:
//!
//! - desktop: the executable, named for the game (argus-game, which links
//!   no protocol, index, or daemon), `data/` with the game's pack and one
//!   pack per language, and `plugins/` with the game's Rust plugins, each
//!   built optimised (phase 5 item 6);
//! - web: the page, the wasm module, and the same packs in `data/`.
//!
//! Packs come from `argus_player::pack::build`: the import cache and
//! compiled scripts. With `--since` (a previous build's directory) the game
//! goes into a patch holding only what changed, beside the old packs.
//!
//! The executable (or the page's module) is the install's prebuilt runtime
//! (distribution item 1, `crate::runtime`): no compiler, and a Linux
//! release can be made on Windows from the install's Linux runtime. From a
//! checkout, or with `--from-source` or `$ARGUS_ENGINE`, it is built from
//! the engine's source with cargo, for the machine it runs on.
//!
//! Before it says done, the build checks itself: the game from its packs
//! must reach the same world as the project after the same ticks, and a
//! desktop build's executable, run headless, must say so too.

use crate::Invocation;
use argus_player::pack::{self, BuildOptions, Pack};
use serde_json::{json, Value as Json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Ticks the self-check runs.
const CHECK_TICKS: u64 = 300;

/// The most pixels a group of differing ones may have and still be a speck
/// (a texel of pixel art drawn at up to two pixels square).
const SPECK: usize = 4;

/// The targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Windows, x86-64.
    Windows,
    /// Linux, x86-64.
    Linux,
    /// macOS.
    MacOs,
    /// The browser: wasm with WebGPU, WebGL2 where it is missing.
    Web,
}

impl Target {
    fn parse(s: &str) -> Option<Target> {
        Some(match s {
            "windows" => Target::Windows,
            "linux" => Target::Linux,
            "macos" | "mac" => Target::MacOs,
            "web" => Target::Web,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Target::Windows => "windows",
            Target::Linux => "linux",
            Target::MacOs => "macos",
            Target::Web => "web",
        }
    }

    /// The desktop this program runs on.
    fn host() -> Target {
        if cfg!(windows) {
            Target::Windows
        } else if cfg!(target_os = "macos") {
            Target::MacOs
        } else {
            Target::Linux
        }
    }
}

/// The engine's source tree: `$ARGUS_ENGINE`, else the tree this CLI was
/// built from.
pub fn engine_dir() -> Result<PathBuf, String> {
    let dir = match std::env::var("ARGUS_ENGINE") {
        Ok(d) if !d.is_empty() => PathBuf::from(d),
        _ => Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
    };
    let dir = dir.canonicalize().unwrap_or(dir);
    if !dir.join("crates/argus-game/Cargo.toml").is_file() {
        return Err(format!(
            "{} is not the Argus engine's source tree; set ARGUS_ENGINE to it",
            dir.display()
        ));
    }
    Ok(dir)
}

pub(crate) fn plain(p: &Path) -> String {
    p.to_string_lossy().trim_start_matches(r"\\?\").to_string()
}

/// Run a command, its output to `err`; fail with its last lines.
fn run(c: &mut Command, what: &str, err: &mut dyn Write) -> Result<(), String> {
    let out = c
        .output()
        .map_err(|e| format!("{what}: could not start: {e}"))?;
    let _ = err.write_all(&out.stderr);
    if !out.status.success() {
        let text = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = text.lines().rev().take(12).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        return Err(format!("{what} failed:\n{}", tail.join("\n")));
    }
    Ok(())
}

/// The packs in a build's `data/`.
fn read_packs(data: &Path) -> Result<Vec<(String, Pack)>, String> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir(data).map_err(|e| format!("{}: {e}", data.display()))?;
    let mut names: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    names.sort();
    for p in names {
        if p.extension().and_then(|x| x.to_str()) != Some("pack") {
            continue;
        }
        let bytes = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let pack = Pack::decode(&bytes).map_err(|e| format!("{}: {}", p.display(), e.message))?;
        let name = p
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        out.push((name, pack));
    }
    Ok(out)
}

/// The world after `CHECK_TICKS` of the project's own scene: its digest,
/// and a frame of it drawn in software (`None` on a machine with no
/// adapter at all). Its Rust plugins are the build's (`plugins`).
fn after(root: &Path, plugins: &Path) -> Result<(String, Option<argus_render::Image>), String> {
    let mut p = argus_player::Player::open_from(
        root,
        None,
        None,
        argus_player::mods::Report::default(),
        argus_plugins::Source::Prebuilt(plugins.to_path_buf()),
    )
    .map_err(|e| e.message)?;
    if let Some(e) = p.plugins.errors.first() {
        return Err(format!("{}: {}", e.at(), e.message));
    }
    p.step(CHECK_TICKS).map_err(|e| e.message)?;
    let frame = (|| {
        let mut r = argus_render::Renderer::offscreen(640, 360, true).ok()?;
        r.load_catalog(&p.assets).ok()?;
        r.render(&p.world, 0, 1.0).ok()?;
        r.capture().ok()
    })();
    Ok((format!("{:016x}", p.digest()), frame))
}

/// `argus build`.
pub fn build_command(inv: &Invocation, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match build(inv, err) {
        Ok(report) => {
            if inv.json {
                let _ = writeln!(out, "{report}");
            } else {
                let _ = writeln!(out, "{}", summary(&report));
            }
            crate::EXIT_OK
        }
        Err(e) => {
            let _ = writeln!(err, "argus build: {e}");
            crate::EXIT_RPC
        }
    }
}

fn summary(r: &Json) -> String {
    if let Some(id) = r["mod"].as_str() {
        return format!(
            "built mod {id} for {}: {} ({} files, {} bytes, {} scripts compiled)\n  loads with the game; what it adds: {}\n  check: {}",
            r["game"].as_str().unwrap_or("?"),
            r["written"]["file"].as_str().unwrap_or("?"),
            r["pack"]["files"],
            r["written"]["bytes"],
            r["pack"]["scripts_compiled"],
            r["mods"]["loaded"]
                .as_array()
                .and_then(|a| a.iter().find(|l| l["id"] == id))
                .map(|l| l["adds"].clone())
                .unwrap_or_default(),
            r["check"]["said"].as_str().unwrap_or("not run"),
        );
    }
    let mut s = format!(
        "built {} for {} ({}) in {}\n  packs: {} files, {} bytes; this build ships {} new blobs, {} bytes",
        r["game"].as_str().unwrap_or("?"),
        r["target"].as_str().unwrap_or("?"),
        r["profile"].as_str().unwrap_or("?"),
        r["out"].as_str().unwrap_or("?"),
        r["packs"]["files"],
        r["packs"]["bytes"],
        r["packs"]["blobs_new"],
        r["packs"]["bytes_new"],
    );
    let (pages, binary, sounds) = (
        &r["packs"]["pages"],
        &r["packs"]["binary"],
        &r["packs"]["sounds"],
    );
    s.push_str(&format!(
        "\n  release forms: {} sprites on {} pages ({} BC7), {} entity files in {} bundles, {} def tables, {} sounds as Vorbis",
        pages["sprites"],
        pages["pages"].as_array().map_or(0, Vec::len),
        pages["compressed"],
        binary["entities"],
        binary["bundles"],
        binary["def_tables"],
        sounds["encoded"],
    ));
    if let Some(e) = r["executable"].as_str() {
        s.push_str(&format!("\n  executable: {e}"));
    }
    for p in r["plugins"].as_array().into_iter().flatten() {
        s.push_str(&format!(
            "\n  plugin: {} ({} bytes)",
            p["file"].as_str().unwrap_or("?"),
            p["bytes"]
        ));
    }
    s.push_str(&format!(
        "\n  check: {}",
        r["check"]["said"].as_str().unwrap_or("not run")
    ));
    s
}

fn build(inv: &Invocation, err: &mut dyn Write) -> Result<Json, String> {
    if let Some(id) = &inv.mod_id {
        return build_mod(inv, id);
    }
    let target = match &inv.target {
        Some(t) => Target::parse(t)
            .ok_or_else(|| format!("no target {t:?}: windows, linux, macos, or web"))?,
        None => Target::host(),
    };
    let root = crate::project::find_project(inv.project.as_deref())?;
    let root = root.canonicalize().unwrap_or(root);
    let cfg = argus_scene::ProjectConfig::load(&root).map_err(|e| e.message)?;
    // The install's runtime, unless this is engine work.
    let from_source = inv.from_source
        || std::env::var("ARGUS_ENGINE").is_ok_and(|d| !d.is_empty())
        || crate::runtime::root().is_none();
    let runtime = if from_source {
        None
    } else {
        crate::runtime::find(target.name(), &cfg.modules)?
    };
    let native = target == Target::Web || target == Target::host();
    if !native && runtime.is_none() {
        return Err(format!(
            "a {} build from source is made on {}: the executable links that platform's window, audio, and gamepad libraries, so build it there (this machine builds {} and web), or from an install with the {} runtime",
            target.name(),
            target.name(),
            Target::host().name(),
            target.name()
        ));
    }
    if !native && !cfg.plugins.is_empty() {
        return Err(format!(
            "{} has Rust plugins ({}), which are compiled for the machine they run on, so its {} release is made on {}",
            cfg.name,
            cfg.plugins.join(", "),
            target.name(),
            target.name()
        ));
    }
    let engine = match runtime {
        Some(_) => None,
        None => Some(engine_dir()?),
    };
    // From source the runtime is compiled, so cargo is found now, before any
    // pack is written, and in ~/.cargo/bin too, where rustup puts it and
    // Git Bash's PATH does not look (issue 8: the build wrote a 46 MB pack,
    // then said "could not start: program not found").
    let cargo = match &engine {
        Some(_) => Some(argus_plugins::find_cargo().ok_or_else(|| {
            "a build from the engine's source compiles its runtime with cargo, and there is none on the PATH, in $CARGO, or in ~/.cargo/bin: install Rust (https://rustup.rs), or put the folder cargo is in on the PATH, or set CARGO to it. An install of Argus builds games with no Rust".to_string()
        })?),
        None => None,
    };
    let game = argus_player::save::folder_name(&cfg.name);
    let out_dir = inv
        .out
        .clone()
        .unwrap_or_else(|| root.join("dist").join(target.name()));
    let code = argus_daemon::daemon::code_version(&root);
    // A runtime is built once, optimised.
    let profile = if inv.release || runtime.is_some() {
        "release"
    } else {
        "debug"
    };
    if target == Target::Web && !cfg.plugins.is_empty() {
        return Err(format!(
            "{} has Rust plugins ({}), and the page cannot load a library, so the game has no web build: the browser runs its Luau only",
            cfg.name,
            cfg.plugins.join(", ")
        ));
    }
    // The Rust plugins, built optimised whatever the profile (a hot path is
    // why a plugin exists), beside the executable.
    let plugins_dir = out_dir.join("plugins");
    let plugins = plugins(&root, &cfg.plugins, &plugins_dir)?;

    // Packs, and the build they patch.
    let since: Vec<(String, Pack)> = match &inv.since {
        Some(d) => read_packs(&d.join("data"))?,
        None => Vec::new(),
    };
    let built = pack::build(
        &root,
        &BuildOptions {
            code: code.clone(),
            since: since.iter().map(|(_, p)| p.clone()).collect(),
        },
    )
    .map_err(|e| format!("{} {}", e.message, e.hint))?;
    let data = out_dir.join("data");
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).map_err(|e| format!("{}: {e}", data.display()))?;
    let mut written = Vec::new();
    for (name, p) in &since {
        // The old game packs stay; a language is rebuilt whole.
        if matches!(p.kind, pack::Kind::Lang(_)) {
            continue;
        }
        std::fs::write(data.join(name), p.encode()).map_err(|e| e.to_string())?;
    }
    for p in &built.packs {
        let bytes = p.encode();
        std::fs::write(data.join(p.file_name()), &bytes).map_err(|e| e.to_string())?;
        written.push(json!({ "file": format!("data/{}", p.file_name()), "bytes": bytes.len() }));
    }

    // The same world from the packs as from the project, and the same
    // picture of it: the packs' sprites are on pages, some in BC7 (phase 5
    // item 1), which must not show.
    let (expect, project_look) = after(&root, &plugins_dir)?;
    let mount = std::env::temp_dir().join(format!("argus-build-check-{}", std::process::id()));
    pack::mount_dir(&data, &mount, None).map_err(|e| e.message)?;
    let (from_packs, packs_look) = after(&mount, &plugins_dir)?;
    let mut check = json!({
        "ticks": CHECK_TICKS,
        "project": expect,
        "packs": from_packs,
    });
    if let (Some(a), Some(b)) = (&project_look, &packs_look) {
        let c = b.compare_near(a, 0.1).map_err(|e| e.message)?;
        // Specks are forgiven: a texel or two that came out as its
        // neighbour, alone, at most one pixel in 10,000 (0.0.3's release:
        // two highlight texels of Outpost's ground on klab, on a lossless
        // page, 5 pixels in 230,400: where a screen pixel lands between
        // texels differs between a sprite's own image and its place on a
        // page). A sprite gone or a colour changed is a bigger group.
        let specks = c.differing > 0
            && c.differing * 10_000 <= c.pixels
            && c.groups().iter().all(|&n| n <= SPECK);
        check["looks"] = json!({ "pixels": c.pixels, "differing": c.differing, "specks": specks });
        if c.differing > 0 {
            // The two pictures and where they differ, beside build.json.
            let _ = a.save_png(&out_dir.join("look-project.png"));
            let _ = b.save_png(&out_dir.join("look-build.png"));
            let _ = c.diff.save_png(&out_dir.join("look-diff.png"));
        }
    }

    let mut report = json!({
        "game": cfg.name,
        "target": target.name(),
        "profile": profile,
        "out": plain(&out_dir),
        "code": code,
        "engine": argus_daemon::daemon::version_json(),
        "packs": built.report,
        "written": written,
        "plugins": plugins,
        "runtime": match (&runtime, &engine) {
            (Some(rt), _) => rt.report(),
            (None, Some(e)) => json!({ "prebuilt": false, "engine": plain(e) }),
            (None, None) => Json::Null,
        },
    });
    match (target, &runtime, &engine) {
        (Target::Web, Some(rt), _) => {
            crate::runtime::place_web(rt, &out_dir)?;
            packs_json(&out_dir, &built.packs, &code)?;
            game_page(&out_dir, &root, &cfg.name)?;
            report["site"] = json!(plain(&out_dir));
        }
        (Target::Web, None, Some(engine)) => {
            web(
                engine,
                cargo.as_deref().expect("found with the engine"),
                &out_dir,
                &built.packs,
                &code,
                inv.release,
                &cfg.modules,
                err,
            )?;
            game_page(&out_dir, &root, &cfg.name)?;
            report["site"] = json!(plain(&out_dir));
        }
        (_, rt, engine) => {
            let exe = match (rt, engine) {
                (Some(rt), _) => crate::runtime::place_desktop(rt, &out_dir, &game, target.name())?,
                (None, Some(engine)) => desktop(
                    engine,
                    cargo.as_deref().expect("found with the engine"),
                    &out_dir,
                    &game,
                    &code,
                    inv.release,
                    &cfg.modules,
                    err,
                )?,
                (None, None) => unreachable!("a build has a runtime or the engine's source"),
            };
            report["executable"] = json!(plain(&exe));
            if !native {
                // Another platform's executable does not run here: the
                // packs' check stands for it.
                check["executable_run"] = json!(format!(
                    "not run: a {} executable, made on {}",
                    target.name(),
                    Target::host().name()
                ));
            } else {
                // The executable, headless: the same world again.
                let o = Command::new(&exe)
                    .arg("--headless")
                    .arg(CHECK_TICKS.to_string())
                    .output()
                    .map_err(|e| format!("{}: {e}", exe.display()))?;
                let line = String::from_utf8_lossy(&o.stdout);
                let said: Json = serde_json::from_str(line.trim()).map_err(|_| {
                    format!(
                        "{} --headless did not answer: {}",
                        exe.display(),
                        String::from_utf8_lossy(&o.stderr).trim()
                    )
                })?;
                check["executable"] = said["status"]["digest"].clone();
                check["save"] = said["save"].clone();
            }
        }
    }
    let plays = check["packs"] == check["project"]
        && (check["executable"].is_null() || check["executable"] == check["project"]);
    let specks = check["looks"]["specks"] == json!(true);
    let looks = check["looks"]["differing"].as_u64().unwrap_or(0) == 0 || specks;
    let same = plays && looks;
    check["same"] = json!(same);
    check["said"] = json!(match (plays, looks) {
        (true, true) => format!(
            "the build plays and looks as the project does (the same world after {CHECK_TICKS} ticks{})",
            if check["looks"].is_null() {
                String::new()
            } else if specks {
                format!(
                    ", drawn the same but for {} pixels in specks, which look-diff.png beside build.json shows",
                    check["looks"]["differing"]
                )
            } else {
                ", drawn the same".to_string()
            }
        ),
        (false, _) => format!("the build does NOT play as the project does after {CHECK_TICKS} ticks"),
        (true, false) => format!(
            "the build plays as the project does but does NOT look the same after {CHECK_TICKS} ticks: {} of its pixels differ (look-project.png, look-build.png, and look-diff.png beside build.json)",
            check["looks"]["differing"]
        ),
    });
    report["check"] = check;
    let _ = std::fs::write(
        out_dir.join("build.json"),
        serde_json::to_string_pretty(&report).unwrap_or_default(),
    );
    if !same {
        return Err(format!(
            "{}: {}",
            report["check"]["said"].as_str().unwrap_or_default(),
            report["check"]
        ));
    }
    Ok(report)
}

/// Mod `id` with the mods it needs before it, those first, from the
/// project's `mods/`.
fn with_requires(root: &Path, id: &str) -> Vec<String> {
    let (found, _) = argus_player::mods::find(&argus_player::mods::project_dir(root));
    fn visit(id: &str, found: &[argus_player::mods::Found], out: &mut Vec<String>) {
        if out.iter().any(|x| x == id) {
            return;
        }
        if let Some(f) = found.iter().find(|f| f.manifest.id == id) {
            for r in &f.manifest.requires {
                visit(r, found, out);
            }
        }
        if !out.iter().any(|x| x == id) {
            out.push(id.to_string());
        }
    }
    let mut out = Vec::new();
    visit(id, &found, &mut out);
    out
}

/// The world's digest after `CHECK_TICKS` of the project's own scene with
/// mod `id` over it (after the mods it needs), and what became of the
/// mods; an error when the mod was left out.
fn digest_with_mod(root: &Path, id: &str) -> Result<(String, Json), String> {
    let cfg = argus_scene::ProjectConfig::load(root).map_err(|e| e.message)?;
    let (mods, _) = argus_player::mods::choose(
        &cfg,
        &argus_player::mods::project_dir(root),
        argus_player::mods::Order::Given(with_requires(root, id)),
    )
    .map_err(|e| e.message)?;
    let mut p = argus_player::Player::open_with(root, None, None, mods).map_err(|e| e.message)?;
    if let Some((_, why)) = p
        .mods
        .left_out
        .iter()
        .find(|(m, _)| m == id)
        .or(p.mods.left_out.first())
    {
        let said: Vec<String> = why
            .iter()
            .map(|e| match &e.r#ref {
                Some(at) => format!("{at}: {}", e.message),
                None => e.message.clone(),
            })
            .collect();
        return Err(format!(
            "mod {id} does not load with the game: {}",
            said.join("; ")
        ));
    }
    p.step(CHECK_TICKS).map_err(|e| e.message)?;
    Ok((format!("{:016x}", p.digest()), p.mods.to_json()))
}

/// `argus build --mod <id>`: the mod's pack, checked (phase 3 item 8).
fn build_mod(inv: &Invocation, id: &str) -> Result<Json, String> {
    let root = crate::project::find_project(inv.project.as_deref())?;
    let root = root.canonicalize().unwrap_or(root);
    let cfg = argus_scene::ProjectConfig::load(&root).map_err(|e| e.message)?;
    if !cfg.mods_enabled {
        return Err(format!(
            "{} does not take mods ([mods] enabled = false in argus.toml)",
            cfg.name
        ));
    }
    let code = argus_daemon::daemon::code_version(&root);
    // The mod as it is: it loads with the game, nothing left out.
    let (from_dir, mods) = digest_with_mod(&root, id)?;
    let built = pack::build_mod(
        &root,
        id,
        &BuildOptions {
            code: code.clone(),
            since: Vec::new(),
        },
    )
    .map_err(|e| format!("{} {}", e.message, e.hint))?;
    let out_dir = inv
        .out
        .clone()
        .unwrap_or_else(|| root.join("dist").join("mods"));
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let p = &built.packs[0];
    let bytes = p.encode();
    let file = out_dir.join(p.file_name());
    std::fs::write(&file, &bytes).map_err(|e| format!("{}: {e}", file.display()))?;
    // The same game with the mod from its pack: the pack in memory where
    // the directory was, for the rest of this process.
    let back = Pack::decode(&bytes).map_err(|e| e.message)?;
    let files = argus_player::mods::pack_files(&back).map_err(|e| e.message)?;
    argus_os::fs::mount(root.join("mods").join(id), files);
    let (from_pack, _) = digest_with_mod(&root, id)?;
    let same = from_pack == from_dir;
    let report = json!({
        "game": cfg.name,
        "mod": id,
        "code": code,
        "pack": built.report,
        "written": { "file": plain(&file), "bytes": bytes.len() },
        "mods": mods,
        "check": {
            "ticks": CHECK_TICKS,
            "directory": from_dir,
            "pack": from_pack,
            "same": same,
            "said": if same {
                format!("the game plays the same with the mod from its pack as from its directory ({CHECK_TICKS} ticks)")
            } else {
                format!("the game does NOT play the same with the mod from its pack after {CHECK_TICKS} ticks")
            },
        },
    });
    if !same {
        return Err(report["check"]["said"]
            .as_str()
            .unwrap_or_default()
            .to_string());
    }
    Ok(report)
}

/// Build each Rust plugin `names` lists and put its library in `dir` as
/// `<name>.<dll extension>`, where the game's executable looks for it. A
/// compile error stops the build with its file and line.
fn plugins(root: &Path, names: &[String], dir: &Path) -> Result<Vec<Json>, String> {
    let _ = std::fs::remove_dir_all(dir);
    if names.is_empty() {
        return Ok(Vec::new());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut out = Vec::new();
    for name in names {
        let b = argus_plugins::build(root, name, true);
        if let Some(e) = b.errors(name).first() {
            return Err(format!(
                "plugin {name} does not build: {}: {}",
                e.at(),
                e.message
            ));
        }
        let built = b
            .dll
            .ok_or_else(|| format!("plugin {name} built no library"))?;
        let to = dir.join(format!("{name}.{}", std::env::consts::DLL_EXTENSION));
        std::fs::copy(&built, &to)
            .map_err(|e| format!("{} to {}: {e}", built.display(), to.display()))?;
        let bytes = std::fs::metadata(&to).map(|m| m.len()).unwrap_or(0);
        out.push(json!({
            "plugin": name,
            "file": format!("plugins/{name}.{}", std::env::consts::DLL_EXTENSION),
            "bytes": bytes,
            "build_ms": b.ms,
        }));
    }
    Ok(out)
}

/// Build the release executable and put it in `out` under the game's name.
#[allow(clippy::too_many_arguments)]
fn desktop(
    engine: &Path,
    cargo: &Path,
    out: &Path,
    game: &str,
    code: &str,
    release: bool,
    modules: &[String],
    err: &mut dyn Write,
) -> Result<PathBuf, String> {
    let mut c = Command::new(cargo);
    c.current_dir(engine)
        .args(["build", "-q", "-p", "argus-game"])
        .env("ARGUS_CODE", code);
    if release {
        c.args(["--profile", "ship"]);
    }
    // The engine's optional modules the game names, and no others (phase 5
    // item 7): a game without rigid bodies does not carry Rapier.
    if !modules.is_empty() {
        c.args(["--features", &modules.join(",")]);
    }
    run(&mut c, "cargo build -p argus-game", err)?;
    let ext = if cfg!(windows) { ".exe" } else { "" };
    let target_dir = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| engine.join("target"));
    let built = target_dir
        .join(if release { "ship" } else { "debug" })
        .join(format!("argus-game{ext}"));
    let exe = out.join(format!("{game}{ext}"));
    std::fs::copy(&built, &exe)
        .map_err(|e| format!("{} to {}: {e}", built.display(), exe.display()))?;
    // Steam's library beside the executable, as Valve's SDK ships it
    // (phase 5 item 9): the copy the build linked against.
    if modules.iter().any(|m| m == "steam") {
        let profile_dir = target_dir.join(if release { "ship" } else { "debug" });
        let lib = steam_library(&profile_dir)?;
        let to = out.join(lib.file_name().unwrap_or_default());
        std::fs::copy(&lib, &to)
            .map_err(|e| format!("{} to {}: {e}", lib.display(), to.display()))?;
    }
    Ok(exe)
}

/// The Steamworks library a build linked against: steamworks-sys copies
/// it into its build output (the newest, when several builds are there).
fn steam_library(profile_dir: &Path) -> Result<PathBuf, String> {
    let name = if cfg!(windows) {
        "steam_api64.dll"
    } else if cfg!(target_os = "macos") {
        "libsteam_api.dylib"
    } else {
        "libsteam_api.so"
    };
    let build = profile_dir.join("build");
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for e in std::fs::read_dir(&build).map_err(|e| format!("{}: {e}", build.display()))? {
        let Ok(e) = e else { continue };
        if !e
            .file_name()
            .to_string_lossy()
            .starts_with("steamworks-sys-")
        {
            continue;
        }
        let p = e.path().join("out").join(name);
        if let Ok(m) = std::fs::metadata(&p).and_then(|m| m.modified()) {
            if best.as_ref().is_none_or(|(t, _)| m > *t) {
                best = Some((m, p));
            }
        }
    }
    best.map(|(_, p)| p).ok_or_else(|| {
        format!(
            "the build has no {name} under {}: the steam module's build should have made one",
            build.display()
        )
    })
}

/// The environment the wasm build of Luau needs (tools/web/env.sh says why).
fn wasm_env(c: &mut Command) -> Result<(), String> {
    let w = std::env::var("WASI_SDK")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("C:/Users/Kevin/tools/wasi-sdk-34.0-x86_64-windows"));
    if !w.join("share/wasi-sysroot").is_dir() {
        return Err(format!(
            "a web build needs wasi-sdk 33 or later; set WASI_SDK (looked in {})",
            w.display()
        ));
    }
    let bin = |n: &str| {
        let ext = if cfg!(windows) { ".exe" } else { "" };
        w.join("bin")
            .join(format!("{n}{ext}"))
            .to_string_lossy()
            .into_owned()
    };
    let s = w.join("share/wasi-sysroot");
    let s = s.to_string_lossy().replace('\\', "/");
    let flags = "--target=wasm32-wasip1 -fwasm-exceptions -mllvm -wasm-use-legacy-eh=false -D_WASI_EMULATED_PROCESS_CLOCKS";
    c.env("CC_wasm32_unknown_unknown", bin("clang"))
        .env("CXX_wasm32_unknown_unknown", bin("clang++"))
        .env("AR_wasm32_unknown_unknown", bin("ar"))
        .env("CFLAGS_wasm32_unknown_unknown", flags)
        .env(
            "CXXFLAGS_wasm32_unknown_unknown",
            format!("{flags} -include stdlib.h"),
        )
        .env("CXXSTDLIB_wasm32_unknown_unknown", "c++")
        .env(
            "CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS",
            format!(
                "-C link-arg=-L{s}/lib/wasm32-wasip1/eh -C link-arg=-L{s}/lib/wasm32-wasip1 -C link-arg=-lc++abi -C link-arg=-lunwind -C link-arg=-lwasi-emulated-process-clocks -C link-arg=-lc"
            ),
        );
    Ok(())
}

/// The web page: the wasm module, the page, and `data/packs.json` listing
/// the packs the page loads.
#[allow(clippy::too_many_arguments)]
fn web(
    engine: &Path,
    cargo: &Path,
    out: &Path,
    packs: &[Pack],
    code: &str,
    release: bool,
    modules: &[String],
    err: &mut dyn Write,
) -> Result<(), String> {
    let tdir = engine.join("target-web");
    if !release {
        // A page's module from source is a debug build unless asked: about
        // three times the bytes every player's browser downloads.
        let _ = writeln!(
            err,
            "argus build: the web runtime from source without --release is a debug build (about 41 MB of WebAssembly, against 14 MB with --release)"
        );
    }
    let mut c = Command::new(cargo);
    c.current_dir(engine)
        .args([
            "rustc",
            "-q",
            "-p",
            "argus-web",
            "--target",
            "wasm32-unknown-unknown",
            "--crate-type",
            "cdylib",
        ])
        // Without the \\?\ prefix: a build script links with MinGW ld,
        // which cannot open such a path.
        .env("CARGO_TARGET_DIR", plain(&tdir))
        .env("ARGUS_CODE", code);
    if release {
        c.arg("--release");
    }
    // A page has no Steam: it runs on the null platform (phase 5 item 9).
    let modules: Vec<&str> = modules
        .iter()
        .map(String::as_str)
        .filter(|m| *m != "steam")
        .collect();
    if !modules.is_empty() {
        c.args(["--features", &modules.join(",")]);
    }
    wasm_env(&mut c)?;
    run(&mut c, "cargo rustc -p argus-web (wasm)", err)?;
    let wasm = tdir
        .join("wasm32-unknown-unknown")
        .join(if release { "release" } else { "debug" })
        .join("argus_web.wasm");
    let mut b = Command::new("wasm-bindgen");
    b.args(["--target", "web", "--out-dir"])
        .arg(out.join("pkg"))
        .arg(&wasm);
    run(
        &mut b,
        "wasm-bindgen (install wasm-bindgen-cli at the version in Cargo.lock)",
        err,
    )?;
    for f in ["index.html", "main.js"] {
        std::fs::copy(engine.join("tools/web").join(f), out.join(f))
            .map_err(|e| format!("tools/web/{f}: {e}"))?;
    }
    packs_json(out, packs, code)
}

/// The page's title and its line of controls, the game's own: its name,
/// and each action's keyboard keys from its `input.toml`. The engine's page
/// said Hollow's keys for every game (found on the public games site,
/// 2026-10-10).
fn game_page(out: &Path, root: &Path, name: &str) -> Result<(), String> {
    let file = out.join("index.html");
    let page = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let controls = match argus_input::ActionMap::load(root) {
        Ok(Some(map)) => controls_line(&map),
        _ => String::new(),
    };
    let mut title = name.to_string();
    if let Some(c) = title.get_mut(..1) {
        c.make_ascii_uppercase();
    }
    let page = page.replacen(
        "<title>Argus</title>",
        &format!("<title>{}</title>", html_text(&title)),
        1,
    );
    let said = if controls.is_empty() {
        "Click the game first.".to_string()
    } else {
        format!("{}. Click the game first.", html_text(&controls))
    };
    let page = fill_span(&page, "controls", &said);
    std::fs::write(&file, page).map_err(|e| format!("{}: {e}", file.display()))?;
    page_script(out, root)
}

/// A game's own page script (host channel item 2): the project's `web/`
/// folder goes beside the page, and when it has a `game.js`, packs.json
/// names it so the page imports it once the player is ready, with
/// `window.argus` set. That is where a game talks to the world outside it
/// (the Bring Your Own Game Master game calls its model there).
fn page_script(out: &Path, root: &Path) -> Result<(), String> {
    let src = root.join("web");
    if !src.is_dir() {
        return Ok(());
    }
    copy_dir(&src, &out.join("web")).map_err(|e| format!("web/: {e}"))?;
    if !out.join("web").join("game.js").is_file() {
        return Ok(());
    }
    let listed = out.join("data").join("packs.json");
    let mut v: Json = std::fs::read_to_string(&listed)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}));
    v["script"] = json!("web/game.js");
    std::fs::write(&listed, v.to_string()).map_err(|e| format!("{}: {e}", listed.display()))
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

/// What a player presses, from the action map: each action a key plays,
/// moving ones first, the menu's own (`ui_...`) left out:
/// "Move: WASD or the arrow keys · Jump: Space or Z · Pause: Esc".
fn controls_line(map: &argus_input::ActionMap) -> String {
    use argus_input::ActionKind;
    let mut acts: Vec<&argus_input::Action> = map
        .actions
        .iter()
        .filter(|a| {
            !a.name.starts_with("ui_") && a.name != "device" && a.kind != ActionKind::Pointer
        })
        .filter(|a| a.bindings.get("keyboard").is_some_and(|k| !k.is_empty()))
        .collect();
    // By name, a number at the end counted as one (speed_3 before
    // speed_10); then a stable sort puts moving first, pause last.
    acts.sort_by_key(|a| {
        let name = a.name.trim_end_matches(|c: char| c.is_ascii_digit());
        let n: u64 = a.name[name.len()..].parse().unwrap_or(0);
        (name.to_string(), n)
    });
    acts.sort_by_key(|a| {
        (
            !matches!(a.kind, ActionKind::Axis2 | ActionKind::Axis),
            matches!(a.name.as_str(), "pause" | "menu"),
        )
    });
    acts.iter()
        .map(|a| {
            let keys: Vec<String> = a.bindings["keyboard"]
                .iter()
                .map(|k| key_words(k))
                .collect();
            let mut label = a.name.replace('_', " ");
            if let Some(c) = label.get_mut(..1) {
                c.make_ascii_uppercase();
            }
            format!("{label}: {}", keys.join(" or "))
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// A binding as a player reads it: `wasd` is WASD, `arrows` the arrow keys,
/// `minus/equal` - and =, a letter its capital.
fn key_words(k: &str) -> String {
    match k {
        "wasd" => "WASD".into(),
        "arrows" => "the arrow keys".into(),
        "space" => "Space".into(),
        "escape" => "Esc".into(),
        "period" => ".".into(),
        "comma" => ",".into(),
        "minus" => "-".into(),
        "equal" => "=".into(),
        _ if k.contains('/') => k
            .split('/')
            .map(key_words)
            .collect::<Vec<_>>()
            .join(" and "),
        _ if k.len() == 1 => k.to_ascii_uppercase(),
        _ => {
            let mut s = k.to_string();
            if let Some(c) = s.get_mut(..1) {
                c.make_ascii_uppercase();
            }
            s
        }
    }
}

/// Text made safe for a page.
fn html_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// A page's `<span id="...">...</span>` with `inner` inside, or the page as
/// it was when it has no such span (a page from an older runtime).
fn fill_span(page: &str, id: &str, inner: &str) -> String {
    let open = format!("<span id=\"{id}\">");
    let Some(start) = page.find(&open) else {
        return page.to_string();
    };
    let from = start + open.len();
    let Some(len) = page[from..].find("</span>") else {
        return page.to_string();
    };
    format!("{}{inner}{}", &page[..from], &page[from + len..])
}

/// `data/packs.json`: the packs the page loads, its languages, its code.
fn packs_json(out: &Path, packs: &[Pack], code: &str) -> Result<(), String> {
    let names: Vec<String> = std::fs::read_dir(out.join("data"))
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".pack"))
        .collect();
    let langs: Vec<String> = packs
        .iter()
        .filter_map(|p| match &p.kind {
            pack::Kind::Lang(l) => Some(l.clone()),
            _ => None,
        })
        .collect();
    std::fs::write(
        out.join("data/packs.json"),
        json!({ "packs": names, "languages": langs, "code": code }).to_string(),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOLLOW: &str = r#"
[actions.move]
type = "axis2"
[actions.move.bindings]
keyboard = ["wasd", "arrows"]
gamepad = ["left_stick"]

[actions.jump]
type = "button"
[actions.jump.bindings]
keyboard = ["space", "z"]

[actions.pause]
type = "button"
[actions.pause.bindings]
keyboard = ["escape"]

[actions.speed_10]
type = "button"
[actions.speed_10.bindings]
keyboard = ["3"]

[actions.speed_3]
type = "button"
[actions.speed_3.bindings]
keyboard = ["2"]

[actions.zoom]
type = "axis"
[actions.zoom.bindings]
keyboard = ["minus/equal"]

[actions.ui_confirm]
type = "button"
[actions.ui_confirm.bindings]
keyboard = ["enter"]

[actions.pointer]
type = "pointer"
[actions.pointer.bindings]
mouse = ["position"]
"#;

    #[test]
    fn a_page_says_the_games_own_controls() {
        let map = argus_input::ActionMap::parse("input.toml", HOLLOW).unwrap();
        assert_eq!(
            controls_line(&map),
            "Move: WASD or the arrow keys · Zoom: - and = · Jump: Space or Z · Speed 3: 2 · Speed 10: 3 · Pause: Esc"
        );
        let page = "<title>Argus</title><p><span id=\"controls\">Click the game first.</span> <span id=\"f8\">F8</span></p>";
        let filled = fill_span(page, "controls", "Move: WASD. Click the game first.");
        assert_eq!(
            filled,
            "<title>Argus</title><p><span id=\"controls\">Move: WASD. Click the game first.</span> <span id=\"f8\">F8</span></p>"
        );
        assert_eq!(
            fill_span("<p>old page</p>", "controls", "x"),
            "<p>old page</p>"
        );
        assert_eq!(html_text("a <b> & c"), "a &lt;b&gt; &amp; c");
    }
}
