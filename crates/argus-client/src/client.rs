//! Connections over the daemon's local socket, one JSON line per message.
//!
//! [`Client`] blocks on each call and sets notifications aside while it
//! waits (the CLI). [`Conn`] never blocks the caller: a thread reads every
//! line and hands responses and notifications over a channel, waking the
//! caller as each arrives (the editor, which draws a frame at a time).

use crate::socket;
use interprocess::local_socket::prelude::*;
use serde_json::{json, Value as Json};
use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::sync::mpsc::{self, Receiver, TryRecvError};

/// One blocking connection.
pub struct Client {
    reader: BufReader<Box<dyn Read + Send>>,
    send: Box<dyn Write + Send>,
    next_id: u64,
    pending: VecDeque<Json>,
}

impl Client {
    /// Connect to a socket by name (as `.argus/daemon.json` records it).
    pub fn connect(socket: &str) -> io::Result<Client> {
        let stream = socket::connect(socket)?;
        let (recv, send) = stream.split();
        Ok(Client {
            reader: BufReader::new(Box::new(recv)),
            send: Box::new(send),
            next_id: 1,
            pending: VecDeque::new(),
        })
    }

    /// Send one request and wait for its response envelope. Notifications
    /// that arrive first are queued for `next_notification`.
    pub fn call(&mut self, method: &str, params: Json) -> io::Result<Json> {
        let id = self.next_id;
        self.next_id += 1;
        write_request(&mut self.send, id, method, params)?;
        loop {
            let msg = read_message(&mut self.reader)?.ok_or_else(closed)?;
            if msg.get("id").is_some_and(|v| *v == json!(id)) {
                return Ok(msg);
            }
            if msg.get("id").is_none() {
                self.pending.push_back(msg);
            }
        }
    }

    /// The next server notification: a queued one, else block for one.
    /// `None` when the connection closes.
    pub fn next_notification(&mut self) -> io::Result<Option<Json>> {
        if let Some(n) = self.pending.pop_front() {
            return Ok(Some(n));
        }
        loop {
            match read_message(&mut self.reader)? {
                None => return Ok(None),
                Some(m) if m.get("id").is_none() => return Ok(Some(m)),
                Some(_) => continue,
            }
        }
    }
}

/// What a [`Conn`]'s reader thread hands over.
#[derive(Clone, Debug, PartialEq)]
pub enum Incoming {
    /// The response to the request with this id: `{result}` or `{error}`.
    Response(u64, Json),
    /// A server notification: `{method, params}`.
    Notification(Json),
    /// The daemon closed the connection, or it broke (the reason).
    Closed(String),
}

/// One connection that never blocks its caller.
pub struct Conn {
    send: Box<dyn Write + Send>,
    next_id: u64,
    rx: Receiver<Incoming>,
    closed: bool,
}

impl Conn {
    /// Connect, and start the thread that reads. `wake` runs after each
    /// message is queued (an editor asks for a repaint).
    pub fn connect(socket: &str, wake: impl Fn() + Send + 'static) -> io::Result<Conn> {
        let stream = socket::connect(socket)?;
        let (recv, send) = stream.split();
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("argus-conn".into())
            .spawn(move || {
                let mut reader: BufReader<Box<dyn Read + Send>> = BufReader::new(Box::new(recv));
                loop {
                    let msg = match read_message(&mut reader) {
                        Ok(Some(m)) => m,
                        Ok(None) => {
                            let _ = tx
                                .send(Incoming::Closed("the daemon closed the connection".into()));
                            wake();
                            return;
                        }
                        Err(e) => {
                            let _ = tx.send(Incoming::Closed(e.to_string()));
                            wake();
                            return;
                        }
                    };
                    let item = match msg.get("id").and_then(Json::as_u64) {
                        Some(id) => Incoming::Response(id, msg),
                        None => Incoming::Notification(msg),
                    };
                    if tx.send(item).is_err() {
                        return;
                    }
                    wake();
                }
            })?;
        Ok(Conn {
            send: Box::new(send),
            next_id: 1,
            rx,
            closed: false,
        })
    }

    /// Send a request; its response comes back through [`Conn::poll`]
    /// with the id returned here.
    pub fn send(&mut self, method: &str, params: Json) -> io::Result<u64> {
        let id = self.next_id;
        self.next_id += 1;
        write_request(&mut self.send, id, method, params)?;
        Ok(id)
    }

    /// Everything that has arrived since the last poll, in order.
    pub fn poll(&mut self) -> Vec<Incoming> {
        let mut out = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(i) => {
                    if matches!(i, Incoming::Closed(_)) {
                        self.closed = true;
                    }
                    out.push(i);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if !self.closed {
                        self.closed = true;
                        out.push(Incoming::Closed("the connection's reader stopped".into()));
                    }
                    break;
                }
            }
        }
        out
    }

    /// Wait up to `timeout` for the next message (tests, and a caller with
    /// nothing else to do).
    pub fn wait(&mut self, timeout: std::time::Duration) -> Option<Incoming> {
        let i = self.rx.recv_timeout(timeout).ok()?;
        if matches!(i, Incoming::Closed(_)) {
            self.closed = true;
        }
        Some(i)
    }

    /// True once the connection has closed.
    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

fn write_request(send: &mut dyn Write, id: u64, method: &str, params: Json) -> io::Result<()> {
    let req = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
    let mut line = req.to_string();
    line.push('\n');
    send.write_all(line.as_bytes())?;
    send.flush()
}

fn read_message(reader: &mut dyn BufRead) -> io::Result<Option<Json>> {
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        if line.trim().is_empty() {
            continue;
        }
        return serde_json::from_str::<Json>(&line)
            .map(Some)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e));
    }
}

fn closed() -> io::Error {
    io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "the daemon closed the connection",
    )
}
