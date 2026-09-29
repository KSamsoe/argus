//! Finding the daemon to talk to: `--socket`, or the project's own (its
//! `.argus/daemon.json`), started with the `argus` beside this executable
//! when none answers, as any `argus` command would. The editor never runs
//! a daemon inside itself: it has only the protocol (§11).

use argus_client::{socket, Client};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// What the editor was started with.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Args {
    /// `--project <dir>`: the project, else the nearest `argus.toml` up.
    pub project: Option<PathBuf>,
    /// `--socket <name>`: a daemon to connect to, instead of the project's.
    pub socket: Option<String>,
    /// `--author <name>`: who the person is in the change feed (`human`).
    pub author: String,
}

/// Read the command line.
pub fn parse(argv: &[String]) -> Result<Args, String> {
    let mut a = Args {
        author: std::env::var("ARGUS_AUTHOR")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "human".to_string()),
        ..Args::default()
    };
    let mut it = argv.iter();
    while let Some(w) = it.next() {
        let mut val = |flag: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match w.as_str() {
            "--project" => a.project = Some(PathBuf::from(val("--project")?)),
            "--socket" => a.socket = Some(val("--socket")?),
            "--author" => a.author = val("--author")?,
            "-h" | "--help" => return Err(USAGE.to_string()),
            other => return Err(format!("unknown argument {other:?}\n{USAGE}")),
        }
    }
    Ok(a)
}

/// The usage line.
pub const USAGE: &str = "usage: argus-editor [--project <dir>] [--socket <name>] [--author <name>]\n       (or `argus editor`, which starts the project's daemon if it is not running)";

/// The project root: `dir`, or the nearest directory with an `argus.toml`
/// from here up.
pub fn find_project(dir: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(d) = dir {
        let d = d
            .canonicalize()
            .map_err(|e| format!("{}: {e}", d.display()))?;
        if d.join("argus.toml").exists() {
            return Ok(d);
        }
        return Err(format!("{} has no argus.toml", d.display()));
    }
    let mut cur = std::env::current_dir().map_err(|e| e.to_string())?;
    loop {
        if cur.join("argus.toml").exists() {
            return Ok(cur);
        }
        if !cur.pop() {
            return Err("no argus.toml here or above; pass --project <dir>".into());
        }
    }
}

/// The socket to use: `--socket`, the project's running daemon, or one
/// started for it now.
pub fn socket_for(a: &Args) -> Result<String, String> {
    if let Some(s) = &a.socket {
        return Ok(s.clone());
    }
    let root = find_project(a.project.as_deref())?;
    if let Some(s) = running(&root) {
        return Ok(s);
    }
    let argus = sibling("argus")?;
    let status = Command::new(&argus)
        .arg("--project")
        .arg(&root)
        .args(["daemon", "start"])
        .stdin(Stdio::null())
        .status()
        .map_err(|e| format!("cannot run {}: {e}", argus.display()))?;
    if !status.success() {
        return Err(format!(
            "`argus daemon start` failed; see {}",
            root.join(".argus/daemon.log").display()
        ));
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if let Some(s) = running(&root) {
            return Ok(s);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err("the daemon did not answer within 30 s".into())
}

/// The socket of the project's daemon, if one answers on it.
fn running(root: &Path) -> Option<String> {
    let info = socket::daemon_info(root)?;
    let s = info["socket"].as_str()?.to_string();
    Client::connect(&s).ok().map(|_| s)
}

/// An executable beside this one (`argus`, `argus.exe`).
pub fn sibling(name: &str) -> Result<PathBuf, String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = me.parent().ok_or("the executable has no directory")?;
    let p = dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    if p.exists() {
        Ok(p)
    } else {
        Err(format!(
            "{} is not there; build it with `cargo build -p argus-cli`",
            p.display()
        ))
    }
}
