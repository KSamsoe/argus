//! How a match's messages travel. A [`Link`] sends to every other player
//! and hands over what arrived, as the relay does (item 2); [`SimNet`] is
//! a network inside one process, with the delay, jitter, and loss a test
//! asks for, on a clock the test moves.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

/// One player's way to the others.
pub trait Link {
    /// Send to every other player.
    fn send(&mut self, bytes: Vec<u8>);
    /// What arrived since the last call, in arrival order.
    fn recv(&mut self) -> Vec<Vec<u8>>;
}

/// A bad link, one way: each message is delayed by `delay_ms` plus up to
/// `jitter_ms`, and lost with probability `loss`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Conditions {
    /// The least time a message takes.
    pub delay_ms: f64,
    /// Up to this much more, drawn for each message.
    pub jitter_ms: f64,
    /// The chance a message never arrives, 0 to 1.
    pub loss: f64,
}

struct InFlight {
    at: f64,
    seq: u64,
    to: usize,
    bytes: Vec<u8>,
}

struct Net {
    now: f64,
    cond: Conditions,
    rng: u64,
    seq: u64,
    players: usize,
    flying: Vec<InFlight>,
    arrived: Vec<VecDeque<Vec<u8>>>,
    sent: u64,
    lost: u64,
}

impl Net {
    /// splitmix64: the same draws from the same seed.
    fn draw(&mut self) -> f64 {
        self.rng = self.rng.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        (z ^ (z >> 31)) as f64 / u64::MAX as f64
    }

    fn deliver(&mut self) {
        let now = self.now;
        let mut due: Vec<InFlight> = Vec::new();
        let mut i = 0;
        while i < self.flying.len() {
            if self.flying[i].at <= now {
                due.push(self.flying.swap_remove(i));
            } else {
                i += 1;
            }
        }
        due.sort_by(|a, b| a.at.total_cmp(&b.at).then(a.seq.cmp(&b.seq)));
        for m in due {
            self.arrived[m.to].push_back(m.bytes);
        }
    }
}

/// A network of `players` inside one process. Its clock moves only with
/// [`SimNet::advance`], so a test's run is the same every time.
#[derive(Clone)]
pub struct SimNet(Rc<RefCell<Net>>);

impl SimNet {
    /// A network of `players`, every link with `cond`, draws from `seed`.
    pub fn new(players: usize, cond: Conditions, seed: u64) -> SimNet {
        SimNet(Rc::new(RefCell::new(Net {
            now: 0.0,
            cond,
            rng: seed,
            seq: 0,
            players,
            flying: Vec::new(),
            arrived: (0..players).map(|_| VecDeque::new()).collect(),
            sent: 0,
            lost: 0,
        })))
    }

    /// Player `i`'s end (0-based).
    pub fn link(&self, i: usize) -> SimLink {
        assert!(i < self.0.borrow().players);
        SimLink {
            net: self.clone(),
            me: i,
        }
    }

    /// Move the clock on; what is due arrives.
    pub fn advance(&self, ms: f64) {
        let mut n = self.0.borrow_mut();
        n.now += ms;
        n.deliver();
    }

    /// Change the conditions from now on.
    pub fn set(&self, cond: Conditions) {
        self.0.borrow_mut().cond = cond;
    }

    /// The clock, in ms.
    pub fn now(&self) -> f64 {
        self.0.borrow().now
    }

    /// Messages sent to someone, and of those lost.
    pub fn counts(&self) -> (u64, u64) {
        let n = self.0.borrow();
        (n.sent, n.lost)
    }
}

/// One player's end of a [`SimNet`].
pub struct SimLink {
    net: SimNet,
    me: usize,
}

impl Link for SimLink {
    fn send(&mut self, bytes: Vec<u8>) {
        let mut n = self.net.0.borrow_mut();
        for to in 0..n.players {
            if to == self.me {
                continue;
            }
            n.sent += 1;
            if n.draw() < n.cond.loss {
                n.lost += 1;
                continue;
            }
            let at = n.now + n.cond.delay_ms + n.draw() * n.cond.jitter_ms;
            n.seq += 1;
            let seq = n.seq;
            n.flying.push(InFlight {
                at,
                seq,
                to,
                bytes: bytes.clone(),
            });
        }
        // A link with no delay delivers at once.
        n.deliver();
    }

    fn recv(&mut self) -> Vec<Vec<u8>> {
        self.net.0.borrow_mut().arrived[self.me].drain(..).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_arrive_late_and_some_not_at_all() {
        let net = SimNet::new(
            2,
            Conditions {
                delay_ms: 50.0,
                jitter_ms: 20.0,
                loss: 0.25,
            },
            7,
        );
        let (mut a, mut b) = (net.link(0), net.link(1));
        for i in 0..400u32 {
            a.send(i.to_le_bytes().to_vec());
        }
        net.advance(49.0);
        assert!(b.recv().is_empty(), "none before the delay");
        net.advance(21.0);
        let got = b.recv();
        assert!(a.recv().is_empty(), "nothing comes back to the sender");
        let (sent, lost) = net.counts();
        assert_eq!(sent, 400);
        assert_eq!(got.len() as u64, sent - lost);
        assert!((70..130).contains(&lost), "{lost} lost of 400");
        // Jitter reorders.
        let order: Vec<u32> = got
            .iter()
            .map(|b| u32::from_le_bytes(b[..4].try_into().unwrap()))
            .collect();
        assert!(order.windows(2).any(|w| w[0] > w[1]));
    }
}
