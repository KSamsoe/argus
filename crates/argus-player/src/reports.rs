//! A player's reports reaching the team (section 15: "a player pressing a
//! report key or hitting a crash produces a replay file the team can open
//! with sim.seek. A player bug report becomes a mark"; phase 5 item 1). A
//! desktop build keeps each report in the player's save location and posts
//! it to the project's playtest server (`[reports] url`), where the web
//! page posts its own and `argus mark pull` collects both. A report the
//! server took moves to `sent/`; one it did not waits for the next try.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// What became of a try to send the reports waiting in a folder.
#[derive(Debug, Default)]
pub struct Sent {
    /// Reports the server took, now in `sent/`.
    pub sent: Vec<PathBuf>,
    /// Why the first one that did not go did not.
    pub failed: Option<String>,
}

/// The reports waiting in `dir`, oldest first: `.argr` files directly in it.
pub fn waiting(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "argr"))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// Post every report waiting in `dir` to `<url>marks`, oldest first, until
/// one does not go (the rest would not either).
pub fn send_waiting(dir: &Path, url: &str, timeout: Duration) -> Sent {
    let mut out = Sent::default();
    for path in waiting(dir) {
        let sent = std::fs::read(&path)
            .map_err(|e| format!("{}: {e}", path.display()))
            .and_then(|b| {
                crate::http::post(
                    &format!("{url}marks"),
                    "application/octet-stream",
                    &b,
                    timeout,
                )
            });
        if let Err(e) = sent {
            out.failed = Some(e);
            break;
        }
        let done = dir.join("sent");
        let to = done.join(path.file_name().unwrap_or_default());
        match std::fs::create_dir_all(&done).and_then(|_| std::fs::rename(&path, &to)) {
            Ok(()) => out.sent.push(to),
            Err(e) => {
                out.failed = Some(format!("{}: sent, but not moved: {e}", path.display()));
                break;
            }
        }
    }
    out
}
