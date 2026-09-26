//! Argus dialogue, phase 3 item 4 (§12 "Dialogue and localization"):
//! "dialogue as a def table (§4) with a small node graph — lines, choices,
//! conditions on world state, effects as edits — runnable from Luau and
//! inspectable through world.defs."
//!
//! The graph is the `dialogue` def table, declared by the engine so every
//! project has one ([`graph::schema`], rows in `defs/dialogue.toml`). Lines
//! and choices are string keys, so a conversation is in the world's
//! language and a translator never touches the graph. A conversation is a
//! [`Talk`] entity: a game spawns one (from Luau, `q:spawn({"Talk"})` with
//! its start, speaker, and listener), `dialogue.update` plays it through a
//! box of ordinary UI nodes (a panel, the speaker's name, the line, a
//! button a choice or one to go on), and the conversation's state is in the
//! component, so a test, a replay, or the agent reads it as it reads
//! anything. When it ends, `dialogue.ended` says so and the entity goes.

#![warn(missing_docs)]

pub mod graph;
mod system;

pub use graph::{schema, Graph, TABLE};
pub use system::UPDATE;

use argus_ecs::{Component, Entity, Result, StrId, World};
use argus_sim::Scheduler;
use std::cell::RefCell;
use std::rc::Rc;

/// A choice's part: no choice (a panel, a name, a line).
pub const NOT_A_BUTTON: i32 = -1;
/// A part's choice: the button that goes on from a line without choices.
pub const GO_ON: i32 = -2;

/// A conversation.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "A conversation (section 12's dialogue): from a node of the dialogue table between a speaker and a listener. dialogue.update shows each line in a box of UI nodes with a button a choice (or one to go on), makes the table's edits, and when it ends emits dialogue.ended and despawns it. A script, the protocol, or a test may take a choice by setting pick, go on by setting advance, or end it by setting stop"
)]
pub struct Talk {
    /// Where it starts.
    #[argus(doc = "The node it starts at (dialogue:hermit.hello, or hermit.hello)")]
    pub start: StrId,
    /// Who speaks.
    #[argus(doc = "The one speaking: the speaker: in conditions and effects")]
    pub speaker: Entity,
    /// Who is spoken to.
    #[argus(doc = "The one spoken to: a condition's or effect's default subject")]
    pub listener: Entity,
    /// Whose hands.
    #[argus(doc = "The player slot whose UI actions drive its box; 0: any")]
    pub slot: u8,
    /// The node shown.
    #[argus(doc = "The node shown now (written by dialogue.update)")]
    pub node: StrId,
    /// The line shown.
    #[argus(
        doc = "The line shown now, in the world's language, holes filled (written by dialogue.update)"
    )]
    pub line: StrId,
    /// Who says it.
    #[argus(doc = "The speaker's name, in the world's language (written by dialogue.update)")]
    pub who: StrId,
    /// Choices offered.
    #[argus(doc = "How many choices are offered now (written by dialogue.update)")]
    pub choices: u32,
    /// Which rows.
    #[argus(
        doc = "Which of the node's rows are offered, bit n the n-th (written by dialogue.update)"
    )]
    pub offered: u32,
    /// A choice to take.
    #[argus(doc = "Set to take a choice, counted from 0 as offered; -1: none")]
    pub pick: i32,
    /// Go on.
    #[argus(doc = "Set to go on from a line without choices")]
    pub advance: bool,
    /// End it.
    #[argus(
        doc = "Set to end the conversation where it is (a skipped cutscene, a game that closes it): dialogue.ended, and the box goes"
    )]
    pub stop: bool,
    /// Nodes shown.
    #[argus(doc = "Nodes shown so far (written by dialogue.update)")]
    pub steps: u32,
}

impl Default for Talk {
    fn default() -> Self {
        Talk {
            start: StrId::EMPTY,
            speaker: Entity::NULL,
            listener: Entity::NULL,
            slot: 0,
            node: StrId::EMPTY,
            line: StrId::EMPTY,
            who: StrId::EMPTY,
            choices: 0,
            offered: 0,
            pick: -1,
            advance: false,
            stop: false,
            steps: 0,
        }
    }
}

/// A node of a conversation's box.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[argus(
    doc = "A UI node of a conversation's box (made and removed by dialogue.update): its button takes a choice, or goes on"
)]
pub struct TalkPart {
    /// Whose box.
    #[argus(doc = "The conversation")]
    pub talk: Entity,
    /// What its button does.
    #[argus(doc = "The choice its button takes, counted from 0; -2 goes on; -1 no button")]
    pub choice: i32,
}

impl Default for TalkPart {
    fn default() -> Self {
        TalkPart {
            talk: Entity::NULL,
            choice: NOT_A_BUTTON,
        }
    }
}

/// Error codes this crate adds (58xx).
pub mod codes {
    /// A dialogue row that does not read.
    pub const BAD_DIALOGUE: u32 = 5801;
}

/// Register the components.
pub fn register(world: &mut World) -> Result<()> {
    world.register::<Talk>()?;
    world.register::<TalkPart>()?;
    Ok(())
}

/// Register the components and add `dialogue.update` (sim stage, after the
/// UI's input). The UI's components must be registered first
/// (`argus_ui::install`).
pub fn install(
    world: &mut World,
    sched: &mut Scheduler,
    graph: Rc<RefCell<Graph>>,
    ui: Rc<RefCell<argus_ui::Shared>>,
) -> Result<()> {
    register(world)?;
    system::install(world, sched, graph, ui)
}
