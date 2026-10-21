//! What players send each other, as bytes. Small and binary: a match sends
//! one [`Msg::Inputs`] a tick to every other player.

use argus_ecs::codec::{Reader, Writer};
use argus_ecs::Result;
use argus_input::ActionValue;

/// The version of these messages. A player who speaks another is refused
/// at [`Msg::Hello`].
pub const VERSION: u16 = 1;

/// One slot's input for one tick: the actions whose values changed, by
/// their index in the action map (sorted by name, so the same on every
/// player's machine), and the new values. Empty: nothing changed.
pub type Frame = Vec<(u16, ActionValue)>;

/// A message between players.
#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    /// Who a player is, sent until every other player has answered.
    Hello {
        /// [`VERSION`].
        version: u16,
        /// The game's fingerprint: the same game, actions, and scene.
        game: u64,
        /// The sender's slot.
        slot: u8,
        /// Players in the match.
        players: u8,
        /// The input delay, in ticks.
        delay: u8,
    },
    /// The sender's own input, every tick from the first one some other
    /// player has not acknowledged, so a lost message costs nothing.
    Inputs {
        /// The sender's slot.
        from: u8,
        /// For each other slot, the last tick through which the sender has
        /// all of that slot's input.
        acks: Vec<(u8, u64)>,
        /// The tick of the first frame.
        first: u64,
        /// One frame a tick from `first`.
        frames: Vec<Frame>,
    },
    /// The sender's world's digest at the end of a tick every player's
    /// input for is known.
    Check {
        /// The sender's slot.
        from: u8,
        /// The tick.
        tick: u64,
        /// [`argus_ecs::World::column_digest`] there (and the runtime
        /// maps').
        digest: u64,
    },
    /// After a check that differed: a hash of each entity's fields at that
    /// tick, to find which differ without sending the whole world.
    Hashes {
        /// The sender's slot.
        from: u8,
        /// The tick.
        tick: u64,
        /// Entity (`e:12`) and the hash of its rows.
        entities: Vec<(String, u64)>,
    },
    /// The rows (`e:12 Transform.pos = ...`) of the entities whose hashes
    /// differed, so each player can say which fields.
    Rows {
        /// The sender's slot.
        from: u8,
        /// The tick.
        tick: u64,
        /// The rows.
        rows: Vec<String>,
    },
    /// The sender left the match.
    Bye {
        /// The sender's slot.
        from: u8,
    },
    /// From the room's host, once every slot is taken: the scene and the
    /// seed the match starts from, which every player loads afresh.
    Start {
        /// The scene.
        scene: String,
        /// The run's seed.
        seed: u64,
        /// The input delay, in ticks.
        delay: u8,
    },
}

fn value(w: &mut Writer, v: ActionValue) {
    match v {
        ActionValue::Button(b) => w.u8(b as u8),
        ActionValue::Axis(x) => {
            w.u8(2);
            w.f32(x);
        }
        ActionValue::Axis2(x, y) => {
            w.u8(3);
            w.f32(x);
            w.f32(y);
        }
    }
}

fn read_value(r: &mut Reader) -> Result<ActionValue> {
    Ok(match r.u8()? {
        0 => ActionValue::Button(false),
        1 => ActionValue::Button(true),
        2 => ActionValue::Axis(r.f32()?),
        3 => ActionValue::Axis2(r.f32()?, r.f32()?),
        t => return Err(r.bad(format!("{t} is not a value"))),
    })
}

impl Msg {
    /// The message as bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        match self {
            Msg::Hello {
                version,
                game,
                slot,
                players,
                delay,
            } => {
                w.u8(1);
                w.u32(*version as u32);
                w.u64(*game);
                w.u8(*slot);
                w.u8(*players);
                w.u8(*delay);
            }
            Msg::Inputs {
                from,
                acks,
                first,
                frames,
            } => {
                w.u8(2);
                w.u8(*from);
                w.count(acks.len());
                for (s, t) in acks {
                    w.u8(*s);
                    w.u64(*t);
                }
                w.u64(*first);
                w.count(frames.len());
                for f in frames {
                    w.count(f.len());
                    for (a, v) in f {
                        w.u32(*a as u32);
                        value(&mut w, *v);
                    }
                }
            }
            Msg::Check { from, tick, digest } => {
                w.u8(3);
                w.u8(*from);
                w.u64(*tick);
                w.u64(*digest);
            }
            Msg::Hashes {
                from,
                tick,
                entities,
            } => {
                w.u8(4);
                w.u8(*from);
                w.u64(*tick);
                w.count(entities.len());
                for (e, h) in entities {
                    w.str(e);
                    w.u64(*h);
                }
            }
            Msg::Rows { from, tick, rows } => {
                w.u8(5);
                w.u8(*from);
                w.u64(*tick);
                w.count(rows.len());
                for r in rows {
                    w.str(r);
                }
            }
            Msg::Bye { from } => {
                w.u8(6);
                w.u8(*from);
            }
            Msg::Start { scene, seed, delay } => {
                w.u8(7);
                w.str(scene);
                w.u64(*seed);
                w.u8(*delay);
            }
        }
        w.into_bytes()
    }

    /// Read a message; an error for bytes that are not one.
    pub fn decode(bytes: &[u8]) -> Result<Msg> {
        let mut r = Reader::new(bytes, "match message");
        let m = match r.u8()? {
            1 => Msg::Hello {
                version: r.u32()? as u16,
                game: r.u64()?,
                slot: r.u8()?,
                players: r.u8()?,
                delay: r.u8()?,
            },
            2 => {
                let from = r.u8()?;
                let mut acks = Vec::new();
                for _ in 0..r.count()? {
                    acks.push((r.u8()?, r.u64()?));
                }
                let first = r.u64()?;
                let mut frames = Vec::new();
                for _ in 0..r.count()? {
                    let mut f = Vec::new();
                    for _ in 0..r.count()? {
                        let a = r.u32()?;
                        let a = u16::try_from(a).map_err(|_| r.bad("action out of range"))?;
                        f.push((a, read_value(&mut r)?));
                    }
                    frames.push(f);
                }
                Msg::Inputs {
                    from,
                    acks,
                    first,
                    frames,
                }
            }
            3 => Msg::Check {
                from: r.u8()?,
                tick: r.u64()?,
                digest: r.u64()?,
            },
            4 => {
                let (from, tick) = (r.u8()?, r.u64()?);
                let mut entities = Vec::new();
                for _ in 0..r.count()? {
                    entities.push((r.str()?, r.u64()?));
                }
                Msg::Hashes {
                    from,
                    tick,
                    entities,
                }
            }
            5 => {
                let (from, tick) = (r.u8()?, r.u64()?);
                let mut rows = Vec::new();
                for _ in 0..r.count()? {
                    rows.push(r.str()?);
                }
                Msg::Rows { from, tick, rows }
            }
            6 => Msg::Bye { from: r.u8()? },
            7 => Msg::Start {
                scene: r.str()?,
                seed: r.u64()?,
                delay: r.u8()?,
            },
            k => return Err(r.bad(format!("{k} is not a message"))),
        };
        if r.remaining() != 0 {
            return Err(r.bad(format!("{} bytes after the message", r.remaining())));
        }
        Ok(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_reads_back() {
        let all = vec![
            Msg::Hello {
                version: VERSION,
                game: 0xfeed,
                slot: 2,
                players: 2,
                delay: 3,
            },
            Msg::Inputs {
                from: 1,
                acks: vec![(2, 41)],
                first: 40,
                frames: vec![
                    vec![],
                    vec![
                        (0, ActionValue::Button(true)),
                        (3, ActionValue::Axis(-0.5)),
                        (7, ActionValue::Axis2(0.25, -1.0)),
                    ],
                ],
            },
            Msg::Check {
                from: 2,
                tick: 59,
                digest: u64::MAX,
            },
            Msg::Hashes {
                from: 1,
                tick: 59,
                entities: vec![("e:1".into(), 7)],
            },
            Msg::Rows {
                from: 1,
                tick: 59,
                rows: vec!["e:1 Pos.p = [0, 0]".into()],
            },
            Msg::Bye { from: 2 },
            Msg::Start {
                scene: "arena".into(),
                seed: 99,
                delay: 2,
            },
        ];
        for m in all {
            assert_eq!(Msg::decode(&m.encode()).unwrap(), m);
        }
        assert!(Msg::decode(&[]).is_err());
        assert!(Msg::decode(&[9]).is_err());
        let mut long = Msg::Bye { from: 1 }.encode();
        long.push(0);
        assert!(Msg::decode(&long).is_err());
        // A count larger than the bytes left is refused, not allocated.
        assert!(Msg::decode(&[5, 1, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255]).is_err());
    }
}
