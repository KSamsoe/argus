//! Starting a daemon process: what `argusd` does at start and what
//! `argus daemon run` does in the foreground. One place, so the CLI and
//! the binary cannot drift in how a project is opened.

use crate::daemon::daemon_info_path;
use crate::{socket_name_for, Daemon, Transport};
use serde_json::json;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

/// How to start.
#[derive(Clone, Debug, Default)]
pub struct Launch {
    /// Project directory to open before the first client connects.
    pub project: Option<PathBuf>,
    /// Scene to open instead of the project default.
    pub scene: Option<String>,
    /// Run seed instead of the scene's seed policy.
    pub seed: Option<u64>,
    /// Transport; `None` picks the project's socket, or stdio with no project.
    pub transport: Option<Transport>,
    /// Open a window of this size after the scene opens. It is a
    /// person's: the game plays in it at once, and closing it ends the
    /// daemon.
    pub window: Option<(u32, u32)>,
    /// Enter play mode after opening, with or without a window.
    pub play: bool,
    /// Mods from the project's `mods/` to open with, in load order
    /// (phase 3 item 8).
    pub mods: Vec<String>,
    /// Serve the web client and the protocol over a WebSocket here too
    /// (phase 5 item 10): this machine or its tailnet only.
    pub web: Option<std::net::SocketAddr>,
}

/// Open the project (if any), announce on stderr, serve until told to
/// stop. Returns the process exit code. On the socket transport the
/// project's `.argus/daemon.json` is written at start and removed at exit.
pub fn launch(l: Launch) -> i32 {
    let mut daemon = Daemon::new();
    let root = l
        .project
        .as_ref()
        .map(|p| p.canonicalize().unwrap_or(p.clone()));
    if let Some(root) = &root {
        log_panics(root);
    }
    if let Some(root) = &root {
        let mut params = json!({ "project": root.to_string_lossy() });
        if let Some(s) = &l.scene {
            params["scene"] = json!(s);
        }
        if let Some(s) = l.seed {
            params["seed"] = json!(s);
        }
        if !l.mods.is_empty() {
            params["mods"] = json!(l.mods);
        }
        match daemon.call(0, "session.open", &params) {
            Ok(v) => {
                eprintln!("argusd: opened {}", v["scene"]);
                for m in v["mods"]["left_out"].as_array().into_iter().flatten() {
                    eprintln!("argusd: mod {} left out: {}", m["id"], m["why"]);
                }
                if l.play {
                    let _ = daemon.call(0, "sim.play", &json!({}));
                }
                if let Some((w, h)) = l.window {
                    match daemon.call(
                        0,
                        "render.window",
                        &json!({ "open": true, "width": w, "height": h }),
                    ) {
                        Ok(s) => {
                            eprintln!("argusd: window {}x{} on {}", w, h, s["adapter"]);
                            daemon.exit_on_window_close = true;
                            // A person opened it to play. Without this the
                            // game stood still while the window drew it and
                            // answered F8, which looked like the keys doing
                            // nothing (found by Kevin playing item 7's
                            // build, launched without --play).
                            if !l.play {
                                let _ = daemon.call(0, "sim.play", &json!({}));
                            }
                        }
                        Err(e) => eprintln!("argusd: no window: {}", e),
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "argusd: {}",
                    serde_json::to_string(&crate::json::error_json(&e)).unwrap_or_default()
                );
                return 1;
            }
        }
    }
    let transport = match l.transport {
        Some(Transport::Socket(name)) if name.is_empty() => {
            let Some(root) = &root else {
                eprintln!("argusd: --socket without a name needs --project");
                return 2;
            };
            Transport::Socket(socket_name_for(root))
        }
        Some(t) => t,
        None => match &root {
            Some(root) => Transport::Socket(socket_name_for(root)),
            None => Transport::Stdio,
        },
    };
    if let Transport::Socket(name) = &transport {
        eprintln!("argusd: listening on {name}");
    }
    let is_socket = matches!(transport, Transport::Socket(_));
    if let Some(w) = l.web {
        eprintln!(
            "argusd: the web client at http://{w}/ (argus token issue scope=review makes its link)"
        );
    }
    let code = match crate::server::serve_with(daemon, transport, root.as_deref(), l.web) {
        Ok(()) => {
            // So a log that ends without this line is a daemon that died.
            eprintln!("argusd: stopped");
            0
        }
        Err(e) => {
            eprintln!("argusd: {e}");
            1
        }
    };
    if let (Some(root), true) = (&root, is_socket) {
        // Only our own record: a newer daemon may have replaced it.
        let p = daemon_info_path(root);
        if let Ok(text) = std::fs::read_to_string(&p) {
            let mine = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v["pid"].as_u64())
                == Some(std::process::id() as u64);
            if mine {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
    code
}

/// A panic anywhere in the daemon, on any thread, goes to the project's
/// `.argus/daemon.log` with its thread, the time, and a backtrace before
/// the process ends. Once a daemon left mid-test with nothing in its log
/// after "listening" (issue 10). It writes the file itself, since the CLI
/// holds stderr's lock while `argus daemon run` serves, and a hook that
/// printed from another thread would wait on it for ever. When stderr is
/// a terminal (`argus daemon run` by hand), the usual message goes there
/// too.
pub fn log_panics(root: &Path) {
    let log = root.join(".argus").join("daemon.log");
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("unnamed").to_string();
        let when = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let backtrace = std::backtrace::Backtrace::force_capture();
        let written = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .and_then(|mut f| {
                writeln!(
                    f,
                    "argusd: panicked (thread {name}, unix time {when}): {info}\nbacktrace:\n{backtrace}"
                )
            });
        if written.is_err() || std::io::stderr().is_terminal() {
            default(info);
        }
    }));
}
