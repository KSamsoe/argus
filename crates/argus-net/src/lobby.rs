//! Before a match: a room on a relay (docs/multiplayer-checklist.md item 3).
//! The game's menu asks to host or to join; the room's host, once every slot
//! is taken, sends the scene and seed every player then loads afresh, and
//! the match starts from the same world on every machine.
//!
//! [`Lobby`] is the state; a [`Wire`] is how a host reaches the relay (a
//! desktop's `argus_relay::Client`, a page's WebSocket); [`RoomLink`] is the
//! match's [`Link`] over the same wire, which keeps the room's news (who is
//! still there) for the lobby to see.

use crate::link::Link;
use crate::relay::{Down, Up};
use crate::wire::Msg;
use std::cell::RefCell;
use std::rc::Rc;

/// A host's way to a relay: relay messages out and in.
pub trait Wire {
    /// Send one message.
    fn send(&mut self, up: &Up);
    /// What came since the last call.
    fn recv(&mut self) -> Vec<Down>;
}

/// The room as the relay last told it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Room {
    /// Its code.
    pub code: String,
    /// This player's slot.
    pub slot: u8,
    /// Players it holds.
    pub players: u8,
    /// The token that takes the slot back.
    pub seat: u64,
    /// The slots connected now.
    pub present: Vec<u8>,
    /// Why the relay closed the connection, once it has.
    pub closed: Option<String>,
}

struct Shared<W> {
    wire: W,
    room: Room,
    /// Bytes from the others, for the match.
    data: Vec<Vec<u8>>,
}

impl<W: Wire> Shared<W> {
    fn pump(&mut self) {
        for m in self.wire.recv() {
            match m {
                Down::Room {
                    code,
                    slot,
                    players,
                    seat,
                } => {
                    self.room.code = code;
                    self.room.slot = slot;
                    self.room.players = players;
                    self.room.seat = seat;
                }
                Down::Present(p) => self.room.present = p,
                Down::From { data, .. } => self.data.push(data),
                Down::Refused(why) => {
                    if self.room.closed.is_none() {
                        self.room.closed = Some(why);
                    }
                }
            }
        }
    }
}

/// A match's [`Link`] through the lobby's relay.
pub struct RoomLink<W: Wire>(Rc<RefCell<Shared<W>>>);

impl<W: Wire> Link for RoomLink<W> {
    fn send(&mut self, bytes: Vec<u8>) {
        self.0.borrow_mut().wire.send(&Up::Send(bytes));
    }

    fn recv(&mut self) -> Vec<Vec<u8>> {
        let mut s = self.0.borrow_mut();
        s.pump();
        std::mem::take(&mut s.data)
    }
}

/// What the room's host sent every player: start from this.
#[derive(Clone, Debug, PartialEq)]
pub struct Start {
    /// The scene.
    pub scene: String,
    /// The run's seed.
    pub seed: u64,
    /// The input delay.
    pub delay: u8,
    /// This player's slot.
    pub slot: u8,
    /// Players.
    pub players: u8,
}

/// Where the lobby is.
#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    /// No room asked for.
    Alone,
    /// Asked; the relay has not answered.
    Asking,
    /// In a room, waiting for every slot to be taken.
    Room,
    /// The match has started.
    Playing,
    /// Over, and why.
    Ended(String),
}

/// One player's side of a room.
pub struct Lobby<W: Wire> {
    shared: Option<Rc<RefCell<Shared<W>>>>,
    phase: Phase,
    /// The host's: what to start.
    plan: Option<(String, u64, u8)>,
    started: bool,
}

impl<W: Wire> Default for Lobby<W> {
    fn default() -> Lobby<W> {
        Lobby {
            shared: None,
            phase: Phase::Alone,
            plan: None,
            started: false,
        }
    }
}

impl<W: Wire> Lobby<W> {
    /// No room yet.
    pub fn new() -> Lobby<W> {
        Lobby::default()
    }

    /// Host a room for `players` of the game with fingerprint `game`
    /// through `wire`; the match will start on `scene` with `seed` and
    /// `delay` ticks of input delay.
    pub fn host(&mut self, mut wire: W, players: u8, game: u64, scene: &str, seed: u64, delay: u8) {
        wire.send(&Up::Host { players, game });
        self.open(wire);
        self.plan = Some((scene.to_string(), seed, delay));
    }

    /// Join the room with `code`.
    pub fn join(&mut self, mut wire: W, code: &str, game: u64) {
        wire.send(&Up::Join {
            code: code.to_string(),
            game,
            seat: 0,
        });
        self.open(wire);
        self.plan = None;
    }

    fn open(&mut self, wire: W) {
        self.shared = Some(Rc::new(RefCell::new(Shared {
            wire,
            room: Room::default(),
            data: Vec::new(),
        })));
        self.phase = Phase::Asking;
        self.started = false;
    }

    /// Leave the room, or the match, for good.
    pub fn leave(&mut self) {
        if let Some(s) = self.shared.take() {
            s.borrow_mut().wire.send(&Up::Leave);
        }
        self.phase = Phase::Alone;
        self.plan = None;
    }

    /// Where it is.
    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    /// The room, once there is one.
    pub fn room(&self) -> Option<Room> {
        let s = self.shared.as_ref()?;
        let r = s.borrow().room.clone();
        (!r.code.is_empty()).then_some(r)
    }

    /// The match is over (a player left, or the worlds differed): the
    /// lobby says so.
    pub fn end(&mut self, why: &str) {
        self.phase = Phase::Ended(why.to_string());
    }

    /// Why the relay closed the connection, once it has (the match's link
    /// reads the news while a match runs).
    pub fn closed(&self) -> Option<String> {
        self.shared.as_ref()?.borrow().room.closed.clone()
    }

    /// The link a match plays over, once started.
    pub fn link(&self) -> Option<RoomLink<W>> {
        self.shared.as_ref().map(|s| RoomLink(s.clone()))
    }

    /// Take the relay's news. Once every slot is taken the host sends the
    /// start; a player given it (the host too) gets it back, once, to load
    /// the scene and begin.
    pub fn poll(&mut self) -> Option<Start> {
        let shared = self.shared.clone()?;
        if matches!(self.phase, Phase::Playing | Phase::Ended(_)) {
            // The match reads the wire itself; only a closed relay is news.
            let closed = shared.borrow().room.closed.clone();
            if let (Some(why), Phase::Playing) = (closed, &self.phase) {
                self.phase = Phase::Ended(why);
            }
            return None;
        }
        let mut s = shared.borrow_mut();
        s.pump();
        if let Some(why) = s.room.closed.clone() {
            self.phase = Phase::Ended(why);
            return None;
        }
        if !s.room.code.is_empty() && self.phase == Phase::Asking {
            self.phase = Phase::Room;
        }
        let full = s.room.players > 0 && s.room.present.len() == s.room.players as usize;
        // The host starts it.
        if let (Some((scene, seed, delay)), true, false) = (&self.plan, full, self.started) {
            let start = Msg::Start {
                scene: scene.clone(),
                seed: *seed,
                delay: *delay,
            };
            s.wire.send(&Up::Send(start.encode()));
            self.started = true;
            self.phase = Phase::Playing;
            return Some(Start {
                scene: scene.clone(),
                seed: *seed,
                delay: *delay,
                slot: s.room.slot,
                players: s.room.players,
            });
        }
        // A guest is told.
        let data = std::mem::take(&mut s.data);
        let mut start = None;
        for d in data {
            match Msg::decode(&d) {
                Ok(Msg::Start { scene, seed, delay }) if start.is_none() && !self.started => {
                    start = Some(Start {
                        scene,
                        seed,
                        delay,
                        slot: s.room.slot,
                        players: s.room.players,
                    });
                }
                // A match's own message come early: it waits for the match.
                Ok(_) if start.is_some() => s.data.push(d),
                _ => {}
            }
        }
        if start.is_some() {
            self.started = true;
            self.phase = Phase::Playing;
        }
        start
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, VecDeque};

    /// A relay inside the test: rooms by code, every message straight on.
    #[derive(Default)]
    struct Fake {
        rooms: BTreeMap<String, (u8, Vec<usize>)>,
        inbox: Vec<VecDeque<Down>>,
    }

    struct FakeWire(Rc<RefCell<Fake>>, usize);

    impl Wire for FakeWire {
        fn send(&mut self, up: &Up) {
            let mut f = self.0.borrow_mut();
            let me = self.1;
            match up {
                Up::Host { players, .. } => {
                    f.rooms.insert("AAAAA".into(), (*players, vec![me]));
                    f.inbox[me].push_back(Down::Room {
                        code: "AAAAA".into(),
                        slot: 1,
                        players: *players,
                        seat: 1,
                    });
                    f.inbox[me].push_back(Down::Present(vec![1]));
                }
                Up::Join { code, .. } => {
                    let (players, mut who) = f.rooms[code].clone();
                    who.push(me);
                    f.rooms.insert(code.clone(), (players, who.clone()));
                    f.inbox[me].push_back(Down::Room {
                        code: code.clone(),
                        slot: who.len() as u8,
                        players,
                        seat: 2,
                    });
                    let present: Vec<u8> = (1..=who.len() as u8).collect();
                    for w in who {
                        f.inbox[w].push_back(Down::Present(present.clone()));
                    }
                }
                Up::Send(data) => {
                    let who = f.rooms["AAAAA"].1.clone();
                    for (i, w) in who.into_iter().enumerate() {
                        if w != me {
                            f.inbox[w].push_back(Down::From {
                                slot: i as u8 + 1,
                                data: data.clone(),
                            });
                        }
                    }
                }
                Up::Leave => {}
            }
        }

        fn recv(&mut self) -> Vec<Down> {
            let mut f = self.0.borrow_mut();
            f.inbox
                .get_mut(self.1)
                .map(|q| q.drain(..).collect())
                .unwrap_or_default()
        }
    }

    #[test]
    fn the_host_starts_the_match_when_the_room_is_full() {
        let relay = Rc::new(RefCell::new(Fake {
            inbox: vec![VecDeque::new(); 2],
            ..Fake::default()
        }));
        let (mut a, mut b) = (Lobby::new(), Lobby::new());
        a.host(FakeWire(relay.clone(), 0), 2, 7, "arena", 42, 3);
        assert_eq!(a.phase(), &Phase::Asking);
        assert_eq!(a.poll(), None);
        assert_eq!(a.phase(), &Phase::Room);
        assert_eq!(a.room().unwrap().code, "AAAAA");
        b.join(FakeWire(relay.clone(), 1), "AAAAA", 7);
        let start = a.poll().expect("full, so the host starts");
        assert_eq!(
            (start.scene.as_str(), start.seed, start.slot),
            ("arena", 42, 1)
        );
        let got = b.poll().expect("the guest is told");
        assert_eq!(
            (got.scene.as_str(), got.seed, got.delay, got.slot),
            ("arena", 42, 3, 2)
        );
        assert_eq!(a.phase(), &Phase::Playing);
        assert_eq!(b.poll(), None, "once");
        // The match's link reaches the other side.
        let (mut la, mut lb) = (a.link().unwrap(), b.link().unwrap());
        la.send(vec![1, 2]);
        assert_eq!(lb.recv(), vec![vec![1, 2]]);
    }
}
