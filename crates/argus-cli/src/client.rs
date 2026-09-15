//! A JSON-RPC client over the daemon's local socket: one line per
//! message, notifications set aside while a call waits for its reply.

use argus_daemon::server;
use interprocess::local_socket::prelude::*;
use serde_json::{json, Value as Json};
use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Read, Write};

/// One connection.
pub struct Client {
    reader: BufReader<Box<dyn Read + Send>>,
    send: Box<dyn Write + Send>,
    next_id: u64,
    pending: VecDeque<Json>,
}

impl Client {
    /// Connect to a socket by name (as `.argus/daemon.json` records it).
    pub fn connect(socket: &str) -> io::Result<Client> {
        let stream = server::connect(socket)?;
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
        let req = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.send.write_all(req.to_string().as_bytes())?;
        self.send.write_all(b"\n")?;
        self.send.flush()?;
        loop {
            let msg = self.read_message()?.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "the daemon closed the connection",
                )
            })?;
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
            match self.read_message()? {
                None => return Ok(None),
                Some(m) if m.get("id").is_none() => return Ok(Some(m)),
                Some(_) => continue,
            }
        }
    }

    fn read_message(&mut self) -> io::Result<Option<Json>> {
        let mut line = String::new();
        loop {
            line.clear();
            if self.reader.read_line(&mut line)? == 0 {
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
}
