//! What players and the relay say to each other, each message one binary
//! WebSocket frame. A player hosts a room or joins one by its code, then
//! sends bytes the relay passes to every other player in the room; what
//! the bytes say (argus-net's match messages) the relay never reads.

use argus_ecs::codec::{Reader, Writer};
use argus_ecs::Result;

/// From a player to the relay.
#[derive(Clone, Debug, PartialEq)]
pub enum Up {
    /// Make a room for `players`, of the game with this fingerprint.
    Host {
        /// Players the room holds, 2 to 4.
        players: u8,
        /// The game's fingerprint: only the same game joins.
        game: u64,
    },
    /// Join the room with `code`. `seat` 0 takes a free slot; a seat token
    /// from an earlier [`Down::Room`] takes that slot back.
    Join {
        /// The room's code.
        code: String,
        /// The game's fingerprint.
        game: u64,
        /// 0, or the seat token of a slot this player had.
        seat: u64,
    },
    /// Bytes for every other player in the room.
    Send(Vec<u8>),
    /// Leave the room for good.
    Leave,
}

/// From the relay to a player.
#[derive(Clone, Debug, PartialEq)]
pub enum Down {
    /// In a room: its code, this player's slot, the players it holds, and
    /// the seat token that takes the slot back after a dropped connection.
    Room {
        /// The code others join with.
        code: String,
        /// This player's slot, from 1.
        slot: u8,
        /// Players the room holds.
        players: u8,
        /// The seat token.
        seat: u64,
    },
    /// The slots whose players are connected now, after any change.
    Present(Vec<u8>),
    /// Bytes another player sent.
    From {
        /// Its slot.
        slot: u8,
        /// The bytes.
        data: Vec<u8>,
    },
    /// The relay will not: why. It closes the connection after.
    Refused(String),
}

impl Up {
    /// As bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        match self {
            Up::Host { players, game } => {
                w.u8(1);
                w.u8(*players);
                w.u64(*game);
            }
            Up::Join { code, game, seat } => {
                w.u8(2);
                w.str(code);
                w.u64(*game);
                w.u64(*seat);
            }
            Up::Send(data) => {
                w.u8(3);
                w.bytes(data);
            }
            Up::Leave => w.u8(4),
        }
        w.into_bytes()
    }

    /// From bytes; an error for anything else.
    pub fn decode(b: &[u8]) -> Result<Up> {
        let mut r = Reader::new(b, "relay message");
        let m = match r.u8()? {
            1 => Up::Host {
                players: r.u8()?,
                game: r.u64()?,
            },
            2 => Up::Join {
                code: r.str()?,
                game: r.u64()?,
                seat: r.u64()?,
            },
            3 => Up::Send(r.bytes()?.to_vec()),
            4 => Up::Leave,
            k => return Err(r.bad(format!("{k} is not a message"))),
        };
        if r.remaining() != 0 {
            return Err(r.bad("bytes after the message"));
        }
        Ok(m)
    }
}

impl Down {
    /// As bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        match self {
            Down::Room {
                code,
                slot,
                players,
                seat,
            } => {
                w.u8(11);
                w.str(code);
                w.u8(*slot);
                w.u8(*players);
                w.u64(*seat);
            }
            Down::Present(slots) => {
                w.u8(12);
                w.count(slots.len());
                for s in slots {
                    w.u8(*s);
                }
            }
            Down::From { slot, data } => {
                w.u8(13);
                w.u8(*slot);
                w.bytes(data);
            }
            Down::Refused(why) => {
                w.u8(14);
                w.str(why);
            }
        }
        w.into_bytes()
    }

    /// From bytes.
    pub fn decode(b: &[u8]) -> Result<Down> {
        let mut r = Reader::new(b, "relay message");
        let m = match r.u8()? {
            11 => Down::Room {
                code: r.str()?,
                slot: r.u8()?,
                players: r.u8()?,
                seat: r.u64()?,
            },
            12 => {
                let mut s = Vec::new();
                for _ in 0..r.count()? {
                    s.push(r.u8()?);
                }
                Down::Present(s)
            }
            13 => Down::From {
                slot: r.u8()?,
                data: r.bytes()?.to_vec(),
            },
            14 => Down::Refused(r.str()?),
            k => return Err(r.bad(format!("{k} is not a message"))),
        };
        if r.remaining() != 0 {
            return Err(r.bad("bytes after the message"));
        }
        Ok(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_reads_back() {
        for m in [
            Up::Host {
                players: 2,
                game: 9,
            },
            Up::Join {
                code: "KQ7XM".into(),
                game: 9,
                seat: 77,
            },
            Up::Send(vec![1, 2, 3]),
            Up::Leave,
        ] {
            assert_eq!(Up::decode(&m.encode()).unwrap(), m);
        }
        for m in [
            Down::Room {
                code: "KQ7XM".into(),
                slot: 2,
                players: 2,
                seat: 77,
            },
            Down::Present(vec![1, 2]),
            Down::From {
                slot: 1,
                data: vec![9; 300],
            },
            Down::Refused("full".into()),
        ] {
            assert_eq!(Down::decode(&m.encode()).unwrap(), m);
        }
        assert!(Up::decode(&[3, 255, 255, 255, 255, 0, 0, 0, 0]).is_err());
        assert!(Up::decode(&[]).is_err());
    }
}
