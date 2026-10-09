//! Building a plugin with cargo (§7: "The agent sees a compile error as a
//! structured `script.errors` response with file:line and the compiler's
//! message, never as a crash"). Cargo's JSON messages are the source: each
//! error with its primary span becomes a [`Diagnostic`] at a
//! project-relative path.
//!
//! A plugin is its own cargo workspace; its build goes to
//! `.argus/plugins/target` under the project, out of the source tree.

use crate::{codes, PluginError};
use serde_json::Value as Json;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// One compiler error.
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    /// Project-relative file: `plugins/swarm-hot/src/lib.rs`.
    pub file: String,
    /// Line, from 1.
    pub line: Option<u32>,
    /// Column, from 1.
    pub column: Option<u32>,
    /// The compiler's one-line message.
    pub message: String,
    /// The compiler's whole text, with the source excerpt.
    pub rendered: String,
}

/// What a build made.
#[derive(Clone, Debug, PartialEq)]
pub struct Build {
    /// The plugin.
    pub plugin: String,
    /// The built library, when it built.
    pub dll: Option<PathBuf>,
    /// Compiler errors.
    pub diagnostics: Vec<Diagnostic>,
    /// Warnings, counted.
    pub warnings: usize,
    /// Nothing needed compiling.
    pub fresh: bool,
    /// Milliseconds cargo took.
    pub ms: u64,
}

impl Build {
    /// The build's errors as `script.errors` rows.
    pub fn errors(&self, plugin: &str) -> Vec<PluginError> {
        self.diagnostics
            .iter()
            .map(|d| PluginError {
                plugin: plugin.to_string(),
                phase: "compile",
                file: d.file.clone(),
                line: d.line,
                message: d.message.clone(),
                code: codes::BUILD,
            })
            .collect()
    }
}

/// Where cargo puts the plugin's library: the last good build is still
/// there when the next one fails.
pub fn library_path(root: &Path, name: &str, release: bool) -> PathBuf {
    let root = plain(root);
    root.join(".argus")
        .join("plugins")
        .join("target")
        .join(if release { "release" } else { "debug" })
        .join(format!(
            "{}{}{}",
            std::env::consts::DLL_PREFIX,
            name.replace('-', "_"),
            std::env::consts::DLL_SUFFIX
        ))
}

/// The cargo to build with: the one running us (`CARGO`), else the one on
/// the path, else rustup's own. A daemon started from a shell without
/// cargo on its path (Kevin's PowerShell) still finds it.
pub fn find_cargo() -> Option<PathBuf> {
    if let Some(c) = std::env::var_os("CARGO") {
        let p = PathBuf::from(c);
        if p.is_file() {
            return Some(p);
        }
    }
    let exe = format!("cargo{}", std::env::consts::EXE_SUFFIX);
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let p = dir.join(&exe);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    let home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(|h| PathBuf::from(h).join(".cargo"))
        })?;
    let p = home.join("bin").join(exe);
    p.is_file().then_some(p)
}

/// A path without Windows' verbatim prefix (`\\?\C:\...`, what
/// canonicalising gives), which cargo and rustc do not take everywhere.
fn plain(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => p.to_path_buf(),
    }
}

/// Build `plugins/<name>/` under `root`.
pub fn build(root: &Path, name: &str, release: bool) -> Build {
    let root = &plain(root);
    let started = Instant::now();
    let rel = format!("plugins/{name}");
    let dir = root.join("plugins").join(name);
    let mut out = Build {
        plugin: name.to_string(),
        dll: None,
        diagnostics: Vec::new(),
        warnings: 0,
        fresh: false,
        ms: 0,
    };
    let fail = |out: &mut Build, file: String, message: String| {
        out.diagnostics.push(Diagnostic {
            file,
            line: None,
            column: None,
            rendered: message.clone(),
            message,
        });
    };
    if cfg!(target_arch = "wasm32") {
        fail(
            &mut out,
            rel,
            "plugins are built and loaded on the desktop; the page cannot".into(),
        );
        return out;
    }
    if !dir.join("Cargo.toml").is_file() {
        fail(
            &mut out,
            format!("{rel}/Cargo.toml"),
            format!("argus.toml lists the plugin {name:?}, but {rel}/Cargo.toml is not there"),
        );
        return out;
    }
    if let Err(e) = crate::sdk::write(root) {
        fail(
            &mut out,
            format!("{rel}/Cargo.toml"),
            format!("cannot write the plugin SDK to .argus/plugins/sdk: {e}"),
        );
        return out;
    }
    let Some(cargo) = find_cargo() else {
        fail(
            &mut out,
            format!("{rel}/Cargo.toml"),
            "no cargo to build the plugin with: install Rust (rustup), or set CARGO".into(),
        );
        return out;
    };
    let mut c = Command::new(cargo);
    c.current_dir(&dir)
        .args(["build", "--message-format=json"])
        .env(
            "CARGO_TARGET_DIR",
            root.join(".argus").join("plugins").join("target"),
        )
        // A daemon running under cargo (a test) passes its own build's
        // settings on; the plugin's build is its own.
        .env_remove("CARGO_MANIFEST_DIR")
        .env_remove("CARGO_PKG_NAME")
        .env_remove("CARGO_BUILD_TARGET_DIR");
    if release {
        c.arg("--release");
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let output = match c.output() {
        Ok(o) => o,
        Err(e) => {
            fail(
                &mut out,
                format!("{rel}/Cargo.toml"),
                format!("cargo did not start: {e}"),
            );
            return out;
        }
    };
    let lib = name.replace('-', "_");
    let mut finished_ok = false;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Ok(msg) = serde_json::from_str::<Json>(line) else {
            continue;
        };
        match msg["reason"].as_str() {
            Some("compiler-message") => {
                let m = &msg["message"];
                match m["level"].as_str() {
                    Some("error") => {
                        if let Some(d) = diagnostic(m, root, &dir) {
                            out.diagnostics.push(d);
                        }
                    }
                    Some("warning") => out.warnings += 1,
                    _ => {}
                }
            }
            Some("compiler-artifact") => {
                let t = &msg["target"];
                let cdylib = t["kind"]
                    .as_array()
                    .is_some_and(|k| k.iter().any(|k| k == "cdylib"));
                if cdylib && t["name"].as_str() == Some(lib.as_str()) {
                    out.fresh = msg["fresh"].as_bool().unwrap_or(false);
                    out.dll = msg["filenames"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|f| f.as_str())
                        .find(|f| f.ends_with(&format!(".{}", std::env::consts::DLL_EXTENSION)))
                        .map(PathBuf::from);
                }
            }
            Some("build-finished") => finished_ok = msg["success"].as_bool().unwrap_or(false),
            _ => {}
        }
    }
    if !finished_ok || !output.status.success() {
        out.dll = None;
        if out.diagnostics.is_empty() {
            // Cargo itself refused (a manifest it cannot read, a missing
            // dependency): its own words, from stderr.
            let err = String::from_utf8_lossy(&output.stderr);
            let text: Vec<&str> = err
                .lines()
                .filter(|l| !l.trim_start().starts_with("Compiling"))
                .collect();
            fail(
                &mut out,
                format!("{rel}/Cargo.toml"),
                text.join("\n").trim().to_string(),
            );
        }
    } else if out.dll.is_none() {
        fail(
            &mut out,
            format!("{rel}/Cargo.toml"),
            format!("{rel} built no library: its Cargo.toml needs [lib] crate-type = [\"cdylib\"]"),
        );
    }
    out.ms = started.elapsed().as_millis() as u64;
    out
}

/// One error message as a diagnostic at its primary span.
fn diagnostic(m: &Json, root: &Path, dir: &Path) -> Option<Diagnostic> {
    let spans = m["spans"].as_array()?;
    let span = spans
        .iter()
        .find(|s| s["is_primary"].as_bool() == Some(true))
        .or(spans.first());
    let mut message = m["message"].as_str().unwrap_or_default().to_string();
    // "aborting due to N previous errors" has no span: the errors it
    // counts are listed.
    let span = span?;
    // The headline alone ("mismatched types") loses what rustc said at the
    // place and after it (found at the phase 5 gate): its label there
    // ("expected `u32`, found `f32`") and its notes and help.
    if let Some(label) = span["label"].as_str().filter(|l| !l.is_empty()) {
        message = format!("{message}: {label}");
    }
    for child in m["children"].as_array().into_iter().flatten() {
        if let (Some(level), Some(text)) = (child["level"].as_str(), child["message"].as_str()) {
            if !text.is_empty() {
                message = format!("{message}; {level}: {text}");
            }
        }
    }
    let file = span["file_name"].as_str().unwrap_or_default();
    let path = Path::new(file);
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        dir.join(path)
    };
    let file = relative(root, &abs);
    Some(Diagnostic {
        file,
        line: span["line_start"].as_u64().map(|l| l as u32),
        column: span["column_start"].as_u64().map(|c| c as u32),
        rendered: m["rendered"].as_str().unwrap_or(&message).to_string(),
        message,
    })
}

/// `abs` from `root`, with forward slashes; as given when outside it.
pub(crate) fn relative(root: &Path, abs: &Path) -> String {
    let clean = |p: &Path| {
        let s = p.to_string_lossy().replace('\\', "/");
        s.strip_prefix("//?/").map(str::to_string).unwrap_or(s)
    };
    let (r, a) = (clean(root), clean(abs));
    let r = r.trim_end_matches('/');
    match a.strip_prefix(r) {
        Some(rest) if rest.starts_with('/') => {
            let mut parts: Vec<&str> = Vec::new();
            for p in rest.trim_start_matches('/').split('/') {
                match p {
                    "." | "" => {}
                    ".." => {
                        parts.pop();
                    }
                    p => parts.push(p),
                }
            }
            parts.join("/")
        }
        _ => a,
    }
}
