//! Starting a daemon process: what `argusd` does at start and what
//! `argus daemon run` does in the foreground. One place, so the CLI and
//! the binary cannot drift in how a project is opened.

use crate::daemon::daemon_info_path;
use crate::{serve, socket_name_for, Daemon, Transport};
use serde_json::json;
use std::path::PathBuf;

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
    /// Open a window of this size after the scene opens.
    pub window: Option<(u32, u32)>,
    /// Enter play mode after opening, so a human can play at once.
    pub play: bool,
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
        let mut params = json!({ "project": root.to_string_lossy() });
        if let Some(s) = &l.scene {
            params["scene"] = json!(s);
        }
        if let Some(s) = l.seed {
            params["seed"] = json!(s);
        }
        match daemon.call(0, "session.open", &params) {
            Ok(v) => {
                eprintln!("argusd: opened {}", v["scene"]);
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
    let code = match serve(daemon, transport, root.as_deref()) {
        Ok(()) => 0,
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
