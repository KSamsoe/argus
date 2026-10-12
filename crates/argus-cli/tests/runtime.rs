//! Distribution item 1: `argus build` puts a game's packs beside the
//! install's prebuilt runtime, with no compiler. These tests use stand-in
//! runtimes for the other desktop platform (an executable a build does not
//! run here) and for the web, so they need no runtime built; the staged
//! install's real runtimes are checked by building the games with it
//! (tools/release/check.sh).

use serde_json::{json, Value as Json};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The desktop platform this machine does not build from source.
fn other() -> &'static str {
    if cfg!(windows) {
        "linux"
    } else {
        "windows"
    }
}

fn exe(target: &str) -> &'static str {
    if target == "windows" {
        ".exe"
    } else {
        ""
    }
}

/// A temp directory removed on drop, used as its path.
struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl std::ops::Deref for Temp {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

fn temp(name: &str) -> Temp {
    let d = std::env::temp_dir().join(format!("argus-runtime-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    Temp(d)
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let name = e.file_name();
        if name == ".argus" || name == "dist" {
            continue;
        }
        if e.path().is_dir() {
            copy(&e.path(), &to.join(&name));
        } else {
            std::fs::copy(e.path(), to.join(&name)).unwrap();
        }
    }
}

/// A copy of Toybox (it uses the rigid-body module), and its guard.
fn toybox(name: &str) -> (Temp, PathBuf) {
    let t = temp(name);
    let root = t.join("toybox");
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/toybox"),
        &root,
    );
    (t, root)
}

fn argus(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_argus"))
        .args(args)
        .env_remove("ARGUS_ENGINE")
        .output()
        .unwrap()
}

/// This argus's version and revision, which a runtime must match.
fn engine() -> (String, String) {
    let o = argus(&["version", "--json"]);
    let v: Json = serde_json::from_slice(&o.stdout).unwrap();
    (
        v["version"].as_str().unwrap().to_string(),
        v["revision"].as_str().unwrap().to_string(),
    )
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// Stand-in runtimes, as `tools/release/stage.sh` lays them out: the other
/// platform's standard variant and the web's, made by `version`; and
/// their guard.
fn runtimes(name: &str, version: &str) -> (Temp, PathBuf) {
    let t = temp(&format!("{name}-rt"));
    let root = t.join("runtime");
    let (_, rev) = engine();
    let meta = |target: &str, variant: &str| {
        json!({ "version": version, "revision": rev, "target": target, "variant": variant, "modules": ["rigid"] })
            .to_string()
    };
    let os = other();
    let dir = root.join(os).join("standard");
    write(&dir.join(format!("argus-game{}", exe(os))), b"the runtime");
    write(&dir.join("runtime.json"), meta(os, "standard").as_bytes());
    let web = root.join("web");
    write(&web.join("pkg/argus_web.js"), b"glue");
    write(&web.join("pkg/argus_web_bg.wasm"), b"module");
    write(&web.join("index.html"), b"<!doctype html>");
    write(&web.join("main.js"), b"// page");
    write(&web.join("runtime.json"), meta("web", "web").as_bytes());
    (t, root)
}

fn build(project: &Path, runtime: &Path, out: &Path, extra: &[&str]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_argus"));
    c.arg("--project")
        .arg(project)
        .args(["--json", "build", "--out"])
        .arg(out)
        .args(extra)
        .env("ARGUS_RUNTIME", runtime)
        .env_remove("ARGUS_ENGINE");
    c.output().unwrap()
}

fn report(o: &Output) -> Json {
    assert!(
        o.status.success(),
        "argus build: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    serde_json::from_slice(&o.stdout).unwrap()
}

#[test]
fn a_release_for_the_other_platform_is_the_installed_runtime_beside_the_packs() {
    let (_g, game) = toybox("other");
    let (_r, rt) = runtimes("other", &engine().0);
    let out = game.join("dist/other");
    let r = report(&build(&game, &rt, &out, &["--target", other()]));
    assert_eq!(r["runtime"]["prebuilt"], true, "{r}");
    assert_eq!(r["runtime"]["variant"], "standard", "{r}");
    assert_eq!(r["profile"], "release", "a runtime is built optimised: {r}");
    // The runtime, named for the game, and nothing compiled.
    let exe = out.join(format!("toybox{}", exe(other())));
    assert_eq!(std::fs::read(&exe).unwrap(), b"the runtime");
    assert!(
        !out.join("runtime.json").exists(),
        "runtime.json stays in the install"
    );
    assert!(out.join("data/game.pack").is_file());
    // The packs play as the project does; the executable is not run here.
    assert_eq!(r["check"]["same"], true, "{r}");
    assert!(r["check"]["executable"].is_null(), "{r}");
    assert!(
        r["check"]["executable_run"]
            .as_str()
            .unwrap()
            .starts_with("not run"),
        "{r}"
    );
}

#[test]
fn a_web_release_is_the_installed_web_runtime_beside_the_packs() {
    let (_g, game) = toybox("web");
    let (_r, rt) = runtimes("web", &engine().0);
    let out = game.join("dist/web");
    let r = report(&build(&game, &rt, &out, &["--target", "web"]));
    assert_eq!(r["runtime"]["prebuilt"], true, "{r}");
    for f in [
        "pkg/argus_web.js",
        "pkg/argus_web_bg.wasm",
        "index.html",
        "main.js",
    ] {
        assert!(out.join(f).is_file(), "{f} is in the site");
    }
    let packs: Json =
        serde_json::from_str(&std::fs::read_to_string(out.join("data/packs.json")).unwrap())
            .unwrap();
    assert_eq!(packs["code"], r["code"], "{packs}");
    assert!(
        packs["packs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p == "game.pack"),
        "{packs}"
    );
}

#[test]
fn a_runtime_from_another_version_is_refused() {
    let (_g, game) = toybox("version");
    let (_r, rt) = runtimes("version", "9.9.9");
    let o = build(&game, &rt, &game.join("dist/x"), &["--target", other()]);
    assert!(!o.status.success());
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(e.contains("Argus 9.9.9") && e.contains("reinstall"), "{e}");
}

#[test]
fn a_steam_game_needs_the_steam_runtime_and_says_which_are_there() {
    let (_g, game) = toybox("steam");
    let toml = game.join("argus.toml");
    let text = std::fs::read_to_string(&toml)
        .unwrap()
        .replace(r#"modules = ["rigid"]"#, r#"modules = ["rigid", "steam"]"#);
    std::fs::write(&toml, text).unwrap();
    let (_r, rt) = runtimes("steam", &engine().0);
    let o = build(&game, &rt, &game.join("dist/x"), &["--target", other()]);
    assert!(!o.status.success());
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(
        e.contains(&format!("no {} steam runtime", other()))
            && e.contains(&format!("{} standard", other())),
        "{e}"
    );
}

#[test]
fn from_source_the_other_platform_is_built_there() {
    let (_g, game) = toybox("source");
    let (_r, rt) = runtimes("source", &engine().0);
    let o = build(
        &game,
        &rt,
        &game.join("dist/x"),
        &["--target", other(), "--from-source"],
    );
    assert!(!o.status.success());
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(e.contains("from source is made on"), "{e}");
}
