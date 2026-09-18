//! Transports (§2): a local socket (Windows named pipe or Unix domain
//! socket) for the CLI and editor, or stdio when embedded. One line of
//! JSON per message. Connections run on threads that forward lines to the
//! single daemon thread; the daemon thread also drives real-time frames
//! while the continuous clock is playing.

use crate::daemon::{daemon_info_path, ConnId, Daemon};
use interprocess::local_socket::{
    prelude::*, GenericFilePath, GenericNamespaced, ListenerOptions, Stream, ToFsName, ToNsName,
};
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

enum Msg {
    Open(ConnId, Sender<String>),
    Line(ConnId, String),
    Closed(ConnId),
}

/// The socket name for a project: a namespaced name where the platform
/// has them (Windows pipes, Linux abstract sockets), else a file under
/// `.argus/`.
pub fn socket_name_for(root: &Path) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in root.to_string_lossy().as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    if GenericNamespaced::is_supported() {
        format!("argusd-{h:016x}.sock")
    } else {
        root.join(".argus")
            .join("argusd.sock")
            .to_string_lossy()
            .into_owned()
    }
}

fn to_name(name: &str) -> std::io::Result<interprocess::local_socket::Name<'_>> {
    if GenericNamespaced::is_supported() && !name.contains('/') && !name.contains('\\') {
        name.to_ns_name::<GenericNamespaced>()
    } else {
        name.to_fs_name::<GenericFilePath>()
    }
}

/// Connect to a daemon by socket name (what the CLI does).
pub fn connect(name: &str) -> std::io::Result<Stream> {
    Stream::connect(to_name(name)?)
}

/// Serve until stdin closes (stdio) or forever (socket). `project_root`,
/// when given, gets a `.argus/daemon.json` with the socket name and pid so
/// clients can find the daemon.
pub fn serve(
    mut daemon: Daemon,
    transport: Transport,
    project_root: Option<&Path>,
) -> std::io::Result<()> {
    let (tx, rx) = mpsc::channel::<Msg>();
    match &transport {
        Transport::Stdio => {
            let (out_tx, out_rx) = mpsc::channel::<String>();
            tx.send(Msg::Open(0, out_tx)).ok();
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
                            if tx2.send(Msg::Line(0, l)).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let _ = tx2.send(Msg::Closed(0));
            });
        }
        Transport::Socket(name) => {
            let listener = ListenerOptions::new().name(to_name(name)?).create_sync()?;
            if let Some(root) = project_root {
                let info = format!(
                    "{{\"socket\": {:?}, \"pid\": {}, \"version\": {:?}}}\n",
                    name,
                    std::process::id(),
                    crate::daemon::VERSION
                );
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
    loop {
        let timeout = if daemon.wants_frames() || daemon.has_window() {
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
        if (daemon.wants_frames() || daemon.has_window()) && Instant::now() >= next_frame {
            if daemon.wants_frames() {
                let _ = daemon.frame();
            }
            if daemon.has_window() {
                daemon.present();
            }
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
        if daemon.shutdown {
            // Let the writer threads deliver the reply before the process ends.
            drop(outs);
            std::thread::sleep(Duration::from_millis(150));
            return Ok(());
        }
    }
}
