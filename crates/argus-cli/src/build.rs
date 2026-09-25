//! `argus build` (§15, phase 2 item 7): a project as players get it.
//!
//! `argus build [--target windows|linux|macos|web] [--release] [--out dir]
//! [--since dir]` makes one directory:
//!
//! - desktop: the executable, named for the game (argus-game, which links
//!   no protocol, index, or daemon), and `data/` with the game's pack and
//!   one pack per language;
//! - web: the page, the wasm module, and the same packs in `data/`.
//!
//! Packs come from `argus_player::pack::build`: the import cache and
//! compiled scripts. With `--since` (a previous build's directory) the game
//! goes into a patch holding only what changed, beside the old packs. The
//! executable is built from the engine's source with cargo, for the machine
//! it runs on: a Linux build is made on Linux, a macOS build on a Mac.
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

fn plain(p: &Path) -> String {
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

/// The world's digest after `CHECK_TICKS` of the project's own scene.
fn digest_after(root: &Path) -> Result<String, String> {
    let mut p = argus_player::Player::open(root, None, None).map_err(|e| e.message)?;
    p.step(CHECK_TICKS).map_err(|e| e.message)?;
    Ok(format!("{:016x}", p.digest()))
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
    if let Some(e) = r["executable"].as_str() {
        s.push_str(&format!("\n  executable: {e}"));
    }
    s.push_str(&format!(
        "\n  check: {}",
        r["check"]["said"].as_str().unwrap_or("not run")
    ));
    s
}

fn build(inv: &Invocation, err: &mut dyn Write) -> Result<Json, String> {
    let target = match &inv.target {
        Some(t) => Target::parse(t)
            .ok_or_else(|| format!("no target {t:?}: windows, linux, macos, or web"))?,
        None => Target::host(),
    };
    if target != Target::Web && target != Target::host() {
        return Err(format!(
            "a {} build is made on {}: the executable links that platform's window, audio, and gamepad libraries, so build it there (this machine builds {} and web)",
            target.name(),
            target.name(),
            Target::host().name()
        ));
    }
    let root = crate::project::find_project(inv.project.as_deref())?;
    let root = root.canonicalize().unwrap_or(root);
    let engine = engine_dir()?;
    let cfg = argus_scene::ProjectConfig::load(&root).map_err(|e| e.message)?;
    let game = argus_player::save::folder_name(&cfg.name);
    let out_dir = inv
        .out
        .clone()
        .unwrap_or_else(|| root.join("dist").join(target.name()));
    let code = argus_daemon::daemon::code_version(&root);
    let profile = if inv.release { "release" } else { "debug" };

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

    // The same world from the packs as from the project.
    let expect = digest_after(&root)?;
    let mount = std::env::temp_dir().join(format!("argus-build-check-{}", std::process::id()));
    pack::mount_dir(&data, &mount, None).map_err(|e| e.message)?;
    let from_packs = digest_after(&mount)?;
    let mut check = json!({
        "ticks": CHECK_TICKS,
        "project": expect,
        "packs": from_packs,
    });

    let mut report = json!({
        "game": cfg.name,
        "target": target.name(),
        "profile": profile,
        "out": plain(&out_dir),
        "code": code,
        "packs": built.report,
        "written": written,
    });
    match target {
        Target::Web => {
            web(&engine, &out_dir, &built.packs, &code, inv.release, err)?;
            report["site"] = json!(plain(&out_dir));
        }
        _ => {
            let exe = desktop(&engine, &out_dir, &game, &code, inv.release, err)?;
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
            report["executable"] = json!(plain(&exe));
        }
    }
    let same = check["packs"] == check["project"]
        && (check["executable"].is_null() || check["executable"] == check["project"]);
    check["same"] = json!(same);
    check["said"] = json!(if same {
        format!("the build plays as the project does (the same world after {CHECK_TICKS} ticks)")
    } else {
        format!("the build does NOT play as the project does after {CHECK_TICKS} ticks")
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

/// Build the release executable and put it in `out` under the game's name.
fn desktop(
    engine: &Path,
    out: &Path,
    game: &str,
    code: &str,
    release: bool,
    err: &mut dyn Write,
) -> Result<PathBuf, String> {
    let mut c = Command::new("cargo");
    c.current_dir(engine)
        .args(["build", "-q", "-p", "argus-game"])
        .env("ARGUS_CODE", code);
    if release {
        c.args(["--profile", "ship"]);
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
    Ok(exe)
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
fn web(
    engine: &Path,
    out: &Path,
    packs: &[Pack],
    code: &str,
    release: bool,
    err: &mut dyn Write,
) -> Result<(), String> {
    let tdir = engine.join("target-web");
    let mut c = Command::new("cargo");
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
        .env("CARGO_TARGET_DIR", &tdir)
        .env("ARGUS_CODE", code);
    if release {
        c.arg("--release");
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
