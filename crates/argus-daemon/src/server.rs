//! Transports (§2): a local socket (Windows named pipe or Unix domain
//! socket) for the CLI and editor, or stdio when embedded. One line of
//! JSON per message. Connections run on threads that forward lines to the
//! single daemon thread; the daemon thread also drives real-time frames
//! while the continuous clock is playing.

use crate::daemon::{daemon_info_path, ConnId, Daemon};
use interprocess::local_socket::{prelude::*, ListenerOptions, Stream};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

/// How the daemon listens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Transport {
    /// Lines on stdin, responses on stdout (embedded use).
    Stdio,
    /// A named local socket; the name is what `socket_name_for` produced.
    Socket(String),
}

/// What a connection's threads tell the daemon thread.
pub(crate) enum Msg {
    /// A connection opened, and where its replies go.
    Open(ConnId, Sender<String>),
    /// One request.
    Line(ConnId, String),
    /// It went away.
    Closed(ConnId),
}

use argus_client::socket::to_name;
pub use argus_client::socket::{connect, socket_name_for};

/// The connection id of the stdio transport. Internal calls (the launcher,
/// the test harness) are connection 0 and never appear in the change feed;
/// stdio and socket clients do. The two transports never run together, so
/// stdio can take the first socket id.
const STDIO: ConnId = 1;

/// Serve until stdin closes (stdio) or forever (socket). `project_root`,
/// when given, gets a `.argus/daemon.json` with the socket name and pid so
/// clients can find the daemon.
pub fn serve(
    daemon: Daemon,
    transport: Transport,
    project_root: Option<&Path>,
) -> std::io::Result<()> {
    serve_with(daemon, transport, project_root, None)
}

/// [`serve`], and with `web` given, the web client and the protocol over
/// a WebSocket served there too (phase 5 item 10; `crate::web`), for the
/// project at `project_root`.
pub fn serve_with(
    mut daemon: Daemon,
    transport: Transport,
    project_root: Option<&Path>,
    web: Option<std::net::SocketAddr>,
) -> std::io::Result<()> {
    let (tx, rx) = mpsc::channel::<Msg>();
    let mut web_bound = None;
    if let (Some(addr), Some(root)) = (web, project_root) {
        let bound = crate::web::start(addr, root.to_path_buf(), tx.clone()).map_err(|e| {
            let hint = if e.kind() == std::io::ErrorKind::AddrInUse {
                " (another daemon's web client? --web :PORT picks another port)"
            } else {
                ""
            };
            std::io::Error::new(
                e.kind(),
                format!("the web client cannot listen on {addr}: {e}{hint}"),
            )
        })?;
        daemon.set_web(Some(bound));
        web_bound = Some(bound);
    }
    match &transport {
        Transport::Stdio => {
            let (out_tx, out_rx) = mpsc::channel::<String>();
            tx.send(Msg::Open(STDIO, out_tx)).ok();
            std::thread::spawn(move || {
                let stdout = std::io::stdout();
                for line in out_rx {
                    let mut o = stdout.lock();
                    let _ = o.write_all(line.as_bytes());
                    let _ = o.write_all(b"\n");
                    let _ = o.flush();
                }
            });
            let tx2 = tx.clone();
            std::thread::spawn(move || {
                let stdin = std::io::stdin();
                for line in stdin.lock().lines() {
                    match line {
                        Ok(l) => {
                            if tx2.send(Msg::Line(STDIO, l)).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let _ = tx2.send(Msg::Closed(STDIO));
            });
        }
        Transport::Socket(name) => {
            let listener = match ListenerOptions::new().name(to_name(name)?).create_sync() {
                Ok(l) => l,
                // A socket file a daemon left when it ended without
                // dropping its listener (it exits by process::exit, or a
                // Mac's Quit menu ends it): Linux's sockets are abstract and
                // leave none, but a Mac's is a file in /tmp. Taken over when
                // nothing answers on it; a daemon that still does keeps it.
                Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && connect(name).is_err() => {
                    ListenerOptions::new()
                        .name(to_name(name)?)
                        .try_overwrite(true)
                        .create_sync()?
                }
                Err(e) => return Err(e),
            };
            if let Some(root) = project_root {
                let mut info = serde_json::json!({
                    "socket": name,
                    "pid": std::process::id(),
                    "version": crate::daemon::VERSION,
                });
                if let Some(w) = web_bound {
                    info["web"] = serde_json::json!(format!("http://{w}/"));
                }
                let info = format!("{info}\n");
                let p = daemon_info_path(root);
                if let Some(d) = p.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                let _ = std::fs::write(&p, info);
            }
            let tx2 = tx.clone();
            std::thread::spawn(move || {
                let mut next: ConnId = 1;
                for conn in listener.incoming() {
                    let Ok(conn) = conn else { continue };
                    let id = next;
                    next += 1;
                    spawn_connection(id, conn, tx2.clone());
                }
            });
        }
    }
    run_loop(&mut daemon, rx, matches!(transport, Transport::Stdio))
}

fn spawn_connection(id: ConnId, conn: Stream, tx: Sender<Msg>) {
    let (recv, mut send) = conn.split();
    let (out_tx, out_rx) = mpsc::channel::<String>();
    tx.send(Msg::Open(id, out_tx)).ok();
    std::thread::spawn(move || {
        for line in out_rx {
            if send.write_all(line.as_bytes()).is_err() || send.write_all(b"\n").is_err() {
                break;
            }
            let _ = send.flush();
        }
    });
    std::thread::spawn(move || {
        let reader = BufReader::new(recv);
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    if l.trim().is_empty() {
                        continue;
                    }
                    if tx.send(Msg::Line(id, l)).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = tx.send(Msg::Closed(id));
    });
}

/// The daemon thread: requests in order, frames on time, notifications out.
fn run_loop(
    daemon: &mut Daemon,
    rx: Receiver<Msg>,
    stop_when_stdin_closes: bool,
) -> std::io::Result<()> {
    let mut outs: BTreeMap<ConnId, Sender<String>> = BTreeMap::new();
    let mut next_frame = Instant::now();
    let mut last_refresh = Instant::now();
    let mut last_assets = Instant::now();
    loop {
        let timeout = if daemon.wants_frames() || daemon.has_window() || daemon.has_stream() {
            next_frame.saturating_duration_since(Instant::now())
        } else {
            next_frame = Instant::now();
            Duration::from_millis(250)
        };
        match rx.recv_timeout(timeout) {
            Ok(Msg::Open(id, out)) => {
                outs.insert(id, out);
            }
            Ok(Msg::Line(id, line)) => {
                if let Some(resp) = daemon.handle_line(id, &line) {
                    if let Some(out) = outs.get(&id) {
                        let _ = out.send(resp.to_string());
                    }
                }
            }
            Ok(Msg::Closed(id)) => {
                outs.remove(&id);
                daemon.drop_conn(id);
                if stop_when_stdin_closes && id == 0 {
                    return Ok(());
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                // Idle: pick up files edited outside the daemon (§4 "on every file change").
                if !daemon.wants_frames() && last_refresh.elapsed() >= Duration::from_secs(10) {
                    daemon.refresh_index();
                    last_refresh = Instant::now();
                }
            }
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        }
        if (daemon.wants_frames() || daemon.has_window() || daemon.has_stream())
            && Instant::now() >= next_frame
        {
            // Devices first, so what the human pressed lands on this frame's ticks.
            if daemon.has_window() || daemon.has_stream() {
                daemon.poll_devices();
            }
            if daemon.wants_frames() {
                let _ = daemon.frame();
            }
            if daemon.has_window() {
                daemon.present();
            }
            // A client's viewport (phase 4 item 1).
            daemon.stream_frame();
            // The sound plays only while the game does.
            daemon.sync_audio();
            next_frame += Duration::from_secs_f64(daemon.frame_seconds());
            if next_frame < Instant::now() {
                next_frame = Instant::now();
            }
        }
        for (conn, msg) in daemon.outbox.drain(..) {
            if let Some(out) = outs.get(&conn) {
                let _ = out.send(msg.to_string());
            }
        }
        // The asset watcher: twice a second, playing or not (section 14).
        if last_assets.elapsed() >= Duration::from_millis(500) {
            daemon.refresh_assets();
            last_assets = Instant::now();
        }
        if daemon.shutdown {
            // Let the writer threads deliver the reply before the process ends.
            drop(outs);
            std::thread::sleep(Duration::from_millis(150));
            return Ok(());
        }
    }
}
