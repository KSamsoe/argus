//! A player's connection to a relay, as an [`argus_net::Link`]: a desktop
//! game's, or the daemon's playing one side. (A web page has its own, in
//! the browser's WebSocket.)

use crate::proto::{Down, Up};
use crate::ws::{self, op};
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often a client tells the relay it is there.
const PING: Duration = Duration::from_secs(5);

/// One player in a room on a relay.
pub struct Client {
    out: Arc<Mutex<TcpStream>>,
    rx: Receiver<Down>,
    code: String,
    slot: u8,
    players: u8,
    seat: u64,
    present: Vec<u8>,
    closed: Option<String>,
    pinged: Instant,
}

fn other(e: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

impl Client {
    /// Make a room on the relay at `url` (`ws://klab:8790`) for `players`
    /// of the game with fingerprint `game`; this player is slot 1.
    pub fn host(url: &str, players: u8, game: u64) -> std::io::Result<Client> {
        Client::open(url, Up::Host { players, game })
    }

    /// Join the room with `code`.
    pub fn join(url: &str, code: &str, game: u64) -> std::io::Result<Client> {
        Client::open(
            url,
            Up::Join {
                code: code.to_string(),
                game,
                seat: 0,
            },
        )
    }

    /// Back into a room after a dropped connection, into the slot `seat`
    /// held ([`Client::seat`]).
    pub fn rejoin(url: &str, code: &str, game: u64, seat: u64) -> std::io::Result<Client> {
        Client::open(
            url,
            Up::Join {
                code: code.to_string(),
                game,
                seat,
            },
        )
    }

    fn open(url: &str, first: Up) -> std::io::Result<Client> {
        let (s, mut reader) = ws::connect(url)?;
        let out = Arc::new(Mutex::new(s));
        let (tx, rx) = mpsc::channel();
        let pong = out.clone();
        std::thread::spawn(move || loop {
            match ws::read_frame(&mut reader, 1 << 20) {
                Ok((op::BINARY, true, data)) => match Down::decode(&data) {
                    Ok(m) => {
                        if tx.send(m).is_err() {
                            return;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Down::Refused(format!("the relay sent {e}")));
                        return;
                    }
                },
                Ok((op::PING, _, data)) => {
                    let mut s = pong.lock().unwrap_or_else(|e| e.into_inner());
                    let _ = std::io::Write::write_all(
                        &mut *s,
                        &ws::frame(op::PONG, &data, Some(mask())),
                    );
                }
                Ok((op::CLOSE, _, _)) | Err(_) => {
                    let _ = tx.send(Down::Refused("the connection to the relay closed".into()));
                    return;
                }
                Ok(_) => {}
            }
        });
        let mut c = Client {
            out,
            rx,
            code: String::new(),
            slot: 0,
            players: 0,
            seat: 0,
            present: Vec::new(),
            closed: None,
            pinged: Instant::now(),
        };
        c.write(op::BINARY, &first.encode())?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match c.rx.recv_timeout(left) {
                Ok(Down::Room {
                    code,
                    slot,
                    players,
                    seat,
                }) => {
                    c.code = code;
                    c.slot = slot;
                    c.players = players;
                    c.seat = seat;
                    return Ok(c);
                }
                Ok(Down::Refused(why)) => return Err(other(why)),
                Ok(Down::Present(p)) => c.present = p,
                Ok(Down::From { .. }) => {}
                Err(RecvTimeoutError::Timeout) => return Err(other("the relay did not answer")),
                Err(RecvTimeoutError::Disconnected) => return Err(other("the relay closed")),
            }
        }
    }

    fn write(&mut self, o: u8, data: &[u8]) -> std::io::Result<()> {
        let mut s = self.out.lock().unwrap_or_else(|e| e.into_inner());
        std::io::Write::write_all(&mut *s, &ws::frame(o, data, Some(mask())))
    }

    /// The room's code, for others to join with.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// This player's slot.
    pub fn slot(&self) -> u8 {
        self.slot
    }

    /// Players the room holds.
    pub fn players(&self) -> u8 {
        self.players
    }

    /// The token that takes this slot back after a dropped connection.
    pub fn seat(&self) -> u64 {
        self.seat
    }

    /// The slots whose players are connected, as the relay last said.
    pub fn present(&self) -> &[u8] {
        &self.present
    }

    /// Why the connection ended, once it has.
    pub fn closed(&self) -> Option<&str> {
        self.closed.as_deref()
    }

    /// Leave the room for good, and close.
    pub fn leave(&mut self) {
        let _ = self.write(op::BINARY, &Up::Leave.encode());
        let _ = self.write(op::CLOSE, &[]);
    }

    /// Drop the connection without leaving (a lost network, for a test).
    pub fn drop_connection(&mut self) {
        let s = self.out.lock().unwrap_or_else(|e| e.into_inner());
        let _ = s.shutdown(std::net::Shutdown::Both);
    }

    /// Wait up to `ms` for the relay's news (the slots present), taking
    /// any bytes that came with it.
    pub fn wait(&mut self, ms: u64) -> Vec<Vec<u8>> {
        let mut got = Vec::new();
        if let Ok(m) = self.rx.recv_timeout(Duration::from_millis(ms)) {
            self.take(m, &mut got);
        }
        got.extend(self.poll());
        got
    }

    fn take(&mut self, m: Down, got: &mut Vec<Vec<u8>>) {
        match m {
            Down::From { data, .. } => got.push(data),
            Down::Present(p) => self.present = p,
            Down::Refused(why) => {
                if self.closed.is_none() {
                    self.closed = Some(why);
                }
            }
            Down::Room { .. } => {}
        }
    }

    fn poll(&mut self) -> Vec<Vec<u8>> {
        let mut got = Vec::new();
        while let Ok(m) = self.rx.try_recv() {
            self.take(m, &mut got);
        }
        got
    }
}

fn mask() -> [u8; 4] {
    ws::noise()[..4].try_into().unwrap()
}

impl argus_net::Link for Client {
    fn send(&mut self, bytes: Vec<u8>) {
        if self.closed.is_some() {
            return;
        }
        if self.write(op::BINARY, &Up::Send(bytes).encode()).is_err() {
            self.closed = Some("the connection to the relay closed".into());
        }
    }

    fn recv(&mut self) -> Vec<Vec<u8>> {
        if self.pinged.elapsed() >= PING {
            self.pinged = Instant::now();
            let _ = self.write(op::PING, &[]);
        }
        self.poll()
    }
}
