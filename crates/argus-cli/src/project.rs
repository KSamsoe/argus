//! Finding the project and its daemon (§2 "one process per project"),
//! starting one when none answers, and the `argus daemon` subcommands.

use crate::client::Client;
use crate::{call_and_print, Invocation, EXIT_CONNECT, EXIT_OK, EXIT_USAGE};
use argus_daemon::{launch, socket_name_for, Launch, Transport};
use serde_json::{json, Value as Json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The project directory: `--project`, else `$ARGUS_PROJECT`, else the
/// nearest directory upward from here holding `argus.toml`.
pub fn find_project(explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(p) = explicit {
        return if p.join("argus.toml").is_file() {
            Ok(p.canonicalize().unwrap_or(p.to_path_buf()))
        } else {
            Err(format!("{} has no argus.toml", p.display()))
        };
    }
    if let Ok(p) = std::env::var("ARGUS_PROJECT") {
        return find_project(Some(Path::new(&p)));
    }
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let mut dir = cwd.as_path();
    loop {
        if dir.join("argus.toml").is_file() {
            return Ok(dir.canonicalize().unwrap_or(dir.to_path_buf()));
        }
        match dir.parent() {
            Some(p) => dir = p,
            None => {
                return Err(format!(
                    "no argus.toml in {} or above; pass --project <dir>",
                    cwd.display()
                ))
            }
        }
    }
}

/// What `.argus/daemon.json` says, if it exists and parses.
pub fn daemon_info(root: &Path) -> Option<Json> {
    let text = std::fs::read_to_string(root.join(".argus").join("daemon.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// Try the recorded socket; `None` when there is no record or nothing
/// answers on it.
pub fn try_connect(root: &Path) -> Option<(Client, Json)> {
    let info = daemon_info(root)?;
    let socket = info["socket"].as_str()?;
    Client::connect(socket).ok().map(|c| (c, info))
}

/// A client for this invocation: `--socket`, or the project's daemon,
/// started if none answers (unless `--no-start`).
pub fn connect(inv: &Invocation, err: &mut dyn Write) -> Result<Client, String> {
    if let Some(s) = &inv.socket {
        return Client::connect(s).map_err(|e| format!("cannot connect to socket {s}: {e}"));
    }
    let root = find_project(inv.project.as_deref())?;
    if let Some((c, _)) = try_connect(&root) {
        return Ok(c);
    }
    if inv.no_start {
        return Err(format!(
            "no daemon is running for {} (start one with `argus daemon start`)",
            root.display()
        ));
    }
    let info = start(&root, inv, err)?;
    let socket = info["socket"].as_str().unwrap_or_default();
    Client::connect(socket).map_err(|e| format!("started a daemon but cannot connect: {e}"))
}

/// Start a daemon for the project in the background and wait until it
/// answers. Returns its `daemon.json`.
pub fn start(root: &Path, inv: &Invocation, err: &mut dyn Write) -> Result<Json, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let log_dir = root.join(".argus");
    let _ = std::fs::create_dir_all(&log_dir);
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("daemon.log"))
        .map_err(|e| format!("cannot open .argus/daemon.log: {e}"))?;
    let log2 = log.try_clone().map_err(|e| e.to_string())?;
    let mut cmd = Command::new(exe);
    cmd.arg("daemon")
        .arg("run")
        .arg("--project")
        .arg(root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log2));
    if let Some(s) = &inv.scene {
        cmd.arg("--scene").arg(s);
    }
    if let Some(s) = inv.seed {
        cmd.arg("--seed").arg(s.to_string());
    }
    if let Some((w, h)) = inv.window {
        cmd.arg("--window").arg(format!("{w}x{h}"));
    }
    detach(&mut cmd);
    let child = cmd
        .spawn()
        .map_err(|e| format!("cannot start the daemon: {e}"))?;
    let pid = child.id();
    let _ = writeln!(
        err,
        "argus: starting a daemon for {} (pid {pid}, log in .argus/daemon.log)",
        root.display()
    );
    // The socket name is a function of the root, so a connection means
    // the new process is up; scene open can take a few hundred ms.
    let socket = socket_name_for(root);
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
        if let Some(info) = daemon_info(root) {
            if info["pid"].as_u64() == Some(pid as u64) && Client::connect(&socket).is_ok() {
                return Ok(info);
            }
        }
    }
    Err(format!(
        "the daemon did not come up within 30 s; see {}",
        log_dir.join("daemon.log").display()
    ))
}

#[cfg(windows)]
fn detach(cmd: &mut Command) {
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const HANDLE_FLAG_INHERIT: u32 = 0x1;
    extern "system" {
        fn SetHandleInformation(handle: *mut std::ffi::c_void, mask: u32, flags: u32) -> i32;
    }
    // Windows inherits every inheritable handle when any stdio is set, so
    // the daemon would hold our stdout pipe open and whoever is waiting on
    // this command (a test, a script) would never see it end. Make our own
    // standard handles non-inheritable for the spawn.
    for h in [
        std::io::stdin().as_raw_handle(),
        std::io::stdout().as_raw_handle(),
        std::io::stderr().as_raw_handle(),
    ] {
        if !h.is_null() {
            // SAFETY: a valid handle owned by this process; clearing the
            // inherit flag changes nothing about how we use it.
            unsafe {
                SetHandleInformation(h, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
    cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
}

#[cfg(unix)]
fn detach(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
}

/// `argus daemon <start|stop|status|run>`.
pub fn daemon_command(
    inv: &Invocation,
    words: &[String],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let sub = words.first().map(String::as_str).unwrap_or("");
    match sub {
        "run" => {
            let root = match find_project(inv.project.as_deref()) {
                Ok(r) => r,
                Err(e) => {
                    let _ = writeln!(err, "argus: {e}");
                    return EXIT_USAGE;
                }
            };
            launch(Launch {
                project: Some(root),
                scene: inv.scene.clone(),
                seed: inv.seed,
                transport: inv.socket.clone().map(Transport::Socket),
                window: inv.window,
            })
        }
        "start" => {
            let root = match find_project(inv.project.as_deref()) {
                Ok(r) => r,
                Err(e) => {
                    let _ = writeln!(err, "argus: {e}");
                    return EXIT_USAGE;
                }
            };
            if let Some((_, info)) = try_connect(&root) {
                let _ = writeln!(
                    out,
                    "already running: pid {} on {}",
                    info["pid"],
                    info["socket"].as_str().unwrap_or("?")
                );
                return EXIT_OK;
            }
            match start(&root, inv, err) {
                Ok(info) => {
                    let _ = writeln!(
                        out,
                        "started: pid {} on {}",
                        info["pid"],
                        info["socket"].as_str().unwrap_or("?")
                    );
                    EXIT_OK
                }
                Err(e) => {
                    let _ = writeln!(err, "argus: {e}");
                    EXIT_CONNECT
                }
            }
        }
        "stop" => {
            let mut client = match connect_no_start(inv) {
                Ok(c) => c,
                Err(e) => {
                    let _ = writeln!(err, "argus: {e}");
                    return EXIT_CONNECT;
                }
            };
            call_and_print(
                &mut client,
                "session.shutdown",
                json!({}),
                inv.json,
                out,
                err,
            )
        }
        "status" => {
            let root = match find_project(inv.project.as_deref()) {
                Ok(r) => r,
                Err(e) => {
                    let _ = writeln!(err, "argus: {e}");
                    return EXIT_USAGE;
                }
            };
            let info = daemon_info(&root);
            let running = try_connect(&root).is_some();
            if inv.json {
                let _ = writeln!(
                    out,
                    "{}",
                    json!({ "project": root.to_string_lossy(), "running": running, "daemon": info })
                );
            } else if running {
                let info = info.unwrap_or(Json::Null);
                let _ = writeln!(
                    out,
                    "running: pid {} on {} (version {})",
                    info["pid"],
                    info["socket"].as_str().unwrap_or("?"),
                    info["version"].as_str().unwrap_or("?")
                );
            } else if info.is_some() {
                let _ = writeln!(out, "not running (stale .argus/daemon.json)");
            } else {
                let _ = writeln!(out, "not running");
            }
            if running {
                EXIT_OK
            } else {
                EXIT_CONNECT
            }
        }
        _ => {
            let _ = write!(err, "{}", crate::help::daemon());
            EXIT_USAGE
        }
    }
}

fn connect_no_start(inv: &Invocation) -> Result<Client, String> {
    let mut i = inv.clone();
    i.no_start = true;
    let mut sink = std::io::sink();
    connect(&i, &mut sink)
}
