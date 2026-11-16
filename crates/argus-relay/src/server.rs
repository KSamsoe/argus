//! `argus relay`: rooms of players who reach each other through it. A
//! player hosts a room and is told its code; others join with the code;
//! each player's bytes go on to the room's other players. The relay keeps
//! nothing: a room lives while someone is in it, and a little after, so a
//! player whose connection dropped can come back to their slot.
//!
//! It is safe to leave running: rooms, connections, and players have
//! limits, a message has a size and a rate cap, a player who is silent too
//! long or sends what does not read is dropped, and nothing a player sends
//! is read past the relay's own few bytes.

use crate::proto::{Down, Up};
use crate::ws::{self, op};
use std::collections::{BTreeMap, HashMap};
use std::io::{BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What one relay allows.
#[derive(Clone, Debug)]
pub struct Limits {
    /// Rooms at once.
    pub rooms: usize,
    /// Connections at once.
    pub connections: usize,
    /// Players a room may hold, 2 up to this.
    pub players: u8,
    /// Bytes a message may carry.
    pub message: usize,
    /// Messages a second a player may send, kept up.
    pub rate: f64,
    /// Messages a player may send at once beyond the rate.
    pub burst: f64,
    /// A player who sends nothing this long is dropped. The relay pings
    /// each player a third of this apart and a client or page answers, so
    /// only a player gone falls silent.
    pub idle: Duration,
    /// A room with no one connected is kept this long for one coming back.
    pub empty: Duration,
    /// A link made slow on purpose: each message passed on is held this
    /// long first (0 for none), for trying a match over a bad connection.
    pub lag: Duration,
    /// Up to this much more, drawn for each message.
    pub jitter: Duration,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            rooms: 500,
            connections: 2000,
            players: 4,
            message: 64 * 1024,
            rate: 240.0,
            burst: 480.0,
            idle: Duration::from_secs(30),
            empty: Duration::from_secs(60),
            lag: Duration::ZERO,
            jitter: Duration::ZERO,
        }
    }
}

/// The letters a code is made of: none two people read alike.
const CODE: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

#[derive(Clone)]
struct Conn {
    id: u64,
    out: Arc<Mutex<TcpStream>>,
}

struct Seat {
    seat: u64,
    conn: Option<Conn>,
}

struct Room {
    game: u64,
    players: u8,
    seats: BTreeMap<u8, Seat>,
    empty_since: Option<Instant>,
}

#[derive(Default)]
struct State {
    rooms: HashMap<String, Room>,
    connections: usize,
    next: u64,
    rng: u64,
}

impl State {
    fn draw(&mut self) -> u64 {
        self.rng = self.rng.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn sweep(&mut self, empty: Duration) {
        self.rooms
            .retain(|_, r| r.empty_since.is_none_or(|t| t.elapsed() < empty));
    }
}

/// A running relay's shared state, for its counts.
#[derive(Clone)]
pub struct Handle(Arc<Mutex<State>>, SocketAddr);

impl Handle {
    /// Where it listens.
    pub fn addr(&self) -> SocketAddr {
        self.1
    }

    /// Rooms open, and connections.
    pub fn counts(&self) -> (usize, usize) {
        let s = lock(&self.0);
        (s.rooms.len(), s.connections)
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Messages held back for a slow link (`Limits::lag`), each until its time,
/// and the thread that sends them then.
#[derive(Clone)]
struct Later(Arc<(Mutex<Vec<Held>>, std::sync::Condvar)>);

/// A message held back, and when it goes.
type Held = (Instant, Conn, Down);

impl Later {
    fn start() -> Later {
        let later = Later(Arc::new((
            Mutex::new(Vec::new()),
            std::sync::Condvar::new(),
        )));
        let l = later.clone();
        std::thread::spawn(move || {
            let (q, cv) = &*l.0;
            let mut held = lock(q);
            loop {
                let now = Instant::now();
                let (due, rest): (Vec<_>, Vec<_>) = held.drain(..).partition(|m| m.0 <= now);
                *held = rest;
                if !due.is_empty() {
                    drop(held);
                    let mut due = due;
                    due.sort_by_key(|m| m.0);
                    for (_, c, m) in due {
                        send(&c, &m);
                    }
                    held = lock(q);
                    continue;
                }
                let wait = held
                    .iter()
                    .map(|m| m.0.saturating_duration_since(now))
                    .min()
                    .unwrap_or(Duration::from_secs(1));
                held = cv
                    .wait_timeout(held, wait)
                    .map(|r| r.0)
                    .unwrap_or_else(|e| e.into_inner().0);
            }
        });
        later
    }

    fn hold(&self, at: Instant, c: Conn, m: Down) {
        let (q, cv) = &*self.0;
        lock(q).push((at, c, m));
        cv.notify_one();
    }
}

/// A relay bound to its address, not yet answering.
pub struct Relay {
    listener: TcpListener,
    limits: Limits,
    state: Arc<Mutex<State>>,
}

impl Relay {
    /// Listen on `addr` (`127.0.0.1:8790`; port 0 picks one).
    pub fn bind(addr: &str, limits: Limits) -> std::io::Result<Relay> {
        let listener = TcpListener::bind(addr)?;
        let state = State {
            rng: u64::from_le_bytes(ws::noise()[..8].try_into().unwrap()),
            ..State::default()
        };
        Ok(Relay {
            listener,
            limits,
            state: Arc::new(Mutex::new(state)),
        })
    }

    /// Its handle.
    pub fn handle(&self) -> Handle {
        Handle(
            self.state.clone(),
            self.listener.local_addr().expect("a bound address"),
        )
    }

    /// Answer players until the process ends.
    pub fn serve(self) {
        let later =
            (!self.limits.lag.is_zero() || !self.limits.jitter.is_zero()).then(Later::start);
        let limits = Arc::new(self.limits);
        for s in self.listener.incoming() {
            let Ok(s) = s else { continue };
            let (state, limits, later) = (self.state.clone(), limits.clone(), later.clone());
            std::thread::spawn(move || {
                let _ = connection(s, &state, &limits, later.as_ref());
            });
        }
    }

    /// Answer players on a thread of its own; the handle.
    pub fn spawn(self) -> Handle {
        let h = self.handle();
        std::thread::spawn(move || self.serve());
        h
    }
}

fn send(c: &Conn, m: &Down) {
    let mut s = lock(&c.out);
    if ws::write_frame(&mut *s, op::BINARY, &m.encode()).is_err() {
        let _ = s.shutdown(std::net::Shutdown::Both);
    }
}

/// Tell every connected player of a room who is there.
fn present(room: &Room) -> Vec<(Conn, Down)> {
    let slots: Vec<u8> = room
        .seats
        .iter()
        .filter(|(_, s)| s.conn.is_some())
        .map(|(n, _)| *n)
        .collect();
    room.seats
        .values()
        .filter_map(|s| s.conn.clone())
        .map(|c| (c, Down::Present(slots.clone())))
        .collect()
}

fn connection(
    s: TcpStream,
    state: &Arc<Mutex<State>>,
    limits: &Limits,
    later: Option<&Later>,
) -> std::io::Result<()> {
    s.set_read_timeout(Some(limits.idle))?;
    s.set_write_timeout(Some(Duration::from_secs(3)))?;
    s.set_nodelay(true)?;
    let mut reader = BufReader::new(s.try_clone()?);
    let req = ws::read_request(&mut reader)?;
    let Some(accept) = ws::accept(&req) else {
        let body = "This is an Argus relay. A game reaches it with a WebSocket.\n";
        let mut s = s;
        write!(
            s,
            "HTTP/1.1 400 Bad Request\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )?;
        return Ok(());
    };
    let mut w = s.try_clone()?;
    write!(
        w,
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    )?;
    w.flush()?;
    let conn = {
        let mut st = lock(state);
        st.next += 1;
        Conn {
            id: st.next,
            out: Arc::new(Mutex::new(s)),
        }
    };
    // A web page cannot ping (a browser sends no WebSocket pings of its
    // own) but answers one, so the relay pings every player a third of
    // `idle` apart: a page waiting alone in its room stays, and a player
    // gone (no answer either) is still dropped. Stops with the connection.
    let _pinging = {
        let (stop, stopped) = std::sync::mpsc::channel::<()>();
        let out = conn.out.clone();
        let every = limits.idle / 3;
        std::thread::spawn(move || {
            while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = stopped.recv_timeout(every)
            {
                if ws::write_frame(&mut *lock(&out), op::PING, &[]).is_err() {
                    return;
                }
            }
        });
        stop
    };
    let full = {
        let mut st = lock(state);
        st.connections += 1;
        st.connections > limits.connections
    };
    let mut me: Option<(String, u8)> = None;
    let why = if full {
        Some(format!(
            "the relay has all the connections it takes ({})",
            limits.connections
        ))
    } else {
        talk(&mut reader, &conn, state, limits, later, &mut me)
    };
    if let Some(why) = why {
        send(&conn, &Down::Refused(why));
        // Closing with the player's bytes unread would reset the
        // connection and lose the reason: stop writing, and read what is
        // still coming for a moment first.
        let _ = lock(&conn.out).shutdown(std::net::Shutdown::Write);
        let _ = reader
            .get_ref()
            .set_read_timeout(Some(Duration::from_millis(500)));
        let mut sink = [0u8; 4096];
        let mut left = 1 << 20;
        while left > 0 {
            match std::io::Read::read(&mut reader, &mut sink) {
                Ok(0) | Err(_) => break,
                Ok(n) => left -= n.min(left),
            }
        }
    }
    {
        let mut out = Vec::new();
        let mut st = lock(state);
        st.connections -= 1;
        if let Some((code, slot)) = &me {
            if let Some(room) = st.rooms.get_mut(code) {
                // The slot stays, for the player coming back with its seat.
                if let Some(seat) = room.seats.get_mut(slot) {
                    if seat.conn.as_ref().is_some_and(|c| c.id == conn.id) {
                        seat.conn = None;
                    }
                }
                if room.seats.values().all(|s| s.conn.is_none()) {
                    room.empty_since = Some(Instant::now());
                }
                out = present(room);
            }
        }
        drop(st);
        for (c, m) in out {
            send(&c, &m);
        }
    }
    let _ = lock(&conn.out).shutdown(std::net::Shutdown::Both);
    Ok(())
}

/// A player's messages, until it goes or must; why it must, if it must.
fn talk(
    reader: &mut BufReader<TcpStream>,
    conn: &Conn,
    state: &Arc<Mutex<State>>,
    limits: &Limits,
    later: Option<&Later>,
    me: &mut Option<(String, u8)>,
) -> Option<String> {
    let mut tokens = limits.burst;
    let mut last = Instant::now();
    loop {
        let (o, fin, data) = match ws::read_frame(reader, limits.message + 16) {
            Ok(f) => f,
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                return Some(format!("silent for {:.1} s", limits.idle.as_secs_f64()));
            }
            Err(e) if e.to_string().contains("too long") => {
                return Some(format!("a message over {} bytes", limits.message));
            }
            Err(_) => return None,
        };
        tokens = (tokens + last.elapsed().as_secs_f64() * limits.rate).min(limits.burst);
        last = Instant::now();
        if tokens < 1.0 {
            return Some(format!(
                "more than {} messages a second",
                limits.rate as u64
            ));
        }
        tokens -= 1.0;
        match o {
            op::PING => {
                let mut s = lock(&conn.out);
                if ws::write_frame(&mut *s, op::PONG, &data).is_err() {
                    return None;
                }
                continue;
            }
            op::PONG => continue,
            op::CLOSE => return None,
            op::BINARY if fin => {}
            _ => return Some("binary messages, each one frame".into()),
        }
        let Ok(m) = Up::decode(&data) else {
            return Some("a message the relay cannot read".into());
        };
        let mut out: Vec<(Conn, Down)> = Vec::new();
        let refused = {
            let mut st = lock(state);
            handle(&mut st, m, conn, limits, me, &mut out)
        };
        for (c, m) in out {
            match (later, &m) {
                // A slow link holds what one player sends another.
                (Some(l), Down::From { .. }) => {
                    let x = u64::from_le_bytes(ws::noise()[..8].try_into().unwrap());
                    let extra = limits
                        .jitter
                        .mul_f64((x >> 11) as f64 / (1u64 << 53) as f64);
                    l.hold(Instant::now() + limits.lag + extra, c, m);
                }
                _ => send(&c, &m),
            }
        }
        if refused.is_some() {
            return refused;
        }
    }
}

fn handle(
    st: &mut State,
    m: Up,
    conn: &Conn,
    limits: &Limits,
    me: &mut Option<(String, u8)>,
    out: &mut Vec<(Conn, Down)>,
) -> Option<String> {
    match m {
        Up::Host { players, game } => {
            if me.is_some() {
                return Some("already in a room".into());
            }
            if !(2..=limits.players).contains(&players) {
                return Some(format!(
                    "a room holds 2 to {} players, not {players}",
                    limits.players
                ));
            }
            st.sweep(limits.empty);
            if st.rooms.len() >= limits.rooms {
                return Some(format!(
                    "the relay has all the rooms it holds ({})",
                    limits.rooms
                ));
            }
            let code = loop {
                let mut x = st.draw();
                let c: String = (0..5)
                    .map(|_| {
                        let ch = CODE[(x % CODE.len() as u64) as usize] as char;
                        x /= CODE.len() as u64;
                        ch
                    })
                    .collect();
                if !st.rooms.contains_key(&c) {
                    break c;
                }
            };
            let seat = st.draw() | 1;
            let mut seats = BTreeMap::new();
            seats.insert(
                1,
                Seat {
                    seat,
                    conn: Some(conn.clone()),
                },
            );
            let room = Room {
                game,
                players,
                seats,
                empty_since: None,
            };
            out.push((
                conn.clone(),
                Down::Room {
                    code: code.clone(),
                    slot: 1,
                    players,
                    seat,
                },
            ));
            out.extend(present(&room));
            st.rooms.insert(code.clone(), room);
            *me = Some((code, 1));
            None
        }
        Up::Join { code, game, seat } => {
            if me.is_some() {
                return Some("already in a room".into());
            }
            st.sweep(limits.empty);
            let code = code.trim().to_ascii_uppercase();
            let token = st.draw() | 1;
            let Some(room) = st.rooms.get_mut(&code) else {
                return Some(format!("no room {code}: check the code"));
            };
            if room.game != game {
                return Some(format!(
                    "room {code} plays another game, or another build of it"
                ));
            }
            let slot = if seat != 0 {
                let Some((n, s)) = room.seats.iter_mut().find(|(_, s)| s.seat == seat) else {
                    return Some(format!("that seat is not in room {code}"));
                };
                // A connection still open there is this player's old one.
                if let Some(old) = s.conn.replace(conn.clone()) {
                    let _ = lock(&old.out).shutdown(std::net::Shutdown::Both);
                }
                *n
            } else {
                let Some(n) = (1..=room.players).find(|n| !room.seats.contains_key(n)) else {
                    return Some(format!("room {code} is full"));
                };
                room.seats.insert(
                    n,
                    Seat {
                        seat: token,
                        conn: Some(conn.clone()),
                    },
                );
                n
            };
            room.empty_since = None;
            out.push((
                conn.clone(),
                Down::Room {
                    code: code.clone(),
                    slot,
                    players: room.players,
                    seat: room.seats[&slot].seat,
                },
            ));
            out.extend(present(room));
            *me = Some((code, slot));
            None
        }
        Up::Send(data) => {
            let Some((code, slot)) = me.as_ref() else {
                return Some("join a room first".into());
            };
            let Some(room) = st.rooms.get(code) else {
                return Some("the room is gone".into());
            };
            for (n, s) in &room.seats {
                if n != slot {
                    if let Some(c) = &s.conn {
                        out.push((
                            c.clone(),
                            Down::From {
                                slot: *slot,
                                data: data.clone(),
                            },
                        ));
                    }
                }
            }
            None
        }
        Up::Leave => {
            if let Some((code, slot)) = me.take() {
                if let Some(room) = st.rooms.get_mut(&code) {
                    room.seats.remove(&slot);
                    if room.seats.values().all(|s| s.conn.is_none()) {
                        room.empty_since = Some(Instant::now());
                    }
                    out.extend(present(room));
                }
            }
            None
        }
    }
}
