//! `dialogue.update` (sim stage, after the UI's input, unfrozen so a game
//! that stands still while its people talk still talks): each conversation
//! started, a choice taken or a line gone on from, is walked to its next
//! line, its edits made, and its box of UI nodes made again.

use crate::graph::{holds, subject, value_like, Effect, Graph, Lit, Node, Parties};
use crate::{Talk, TalkPart, GO_ON, NOT_A_BUTTON};
use argus_ecs::{Entity, Parent, Result, StrId, Value, Vec2};
use argus_sim::{Scheduler, Stage, SystemCtx, SystemDesc};
use argus_ui::{UiButton, UiNode, UiPanel, UiRoot, UiText};
use std::cell::RefCell;
use std::rc::Rc;

/// The system's name.
pub const UPDATE: &str = "dialogue.update";

/// Nodes a conversation may pass through in one tick without a line (a
/// loop of branches is stopped here, and said).
const MAX_HOPS: usize = 64;

/// What happens to a conversation this tick.
enum Step {
    Start,
    Pick(i32),
    GoOn,
}

/// Where a walk stopped.
enum Stop {
    /// At a node to show, with the rows it offers (by position among the
    /// node's rows).
    Show(String, Vec<usize>),
    /// The conversation is over; the last node it was at.
    End(String),
}

pub(crate) fn install(
    world: &mut argus_ecs::World,
    sched: &mut Scheduler,
    graph: Rc<RefCell<Graph>>,
    ui: Rc<RefCell<argus_ui::Shared>>,
) -> Result<()> {
    let talk = world.component_id_of::<Talk>().expect("registered");
    let part = world.component_id_of::<TalkPart>().expect("registered");
    let mut desc = SystemDesc::new(UPDATE, Stage::Sim)
        .after("ui.input")
        .unfrozen()
        .writes(talk)
        .writes(part)
        .source("argus-dialogue:update")
        .doc("Plays every Talk (section 12's dialogue): from its start, or after a choice taken (a pressed button of its box, or Talk.pick) or a line gone on from (Talk.advance), walks the dialogue table past nodes whose when fails and through branches to the next line, makes each node's and choice's edits, plays a line's sound, and makes its box again: a panel with the speaker's name, the line, and a button a choice (or one to go on), in the world's language. Emits dialogue.started, dialogue.line, dialogue.chose, dialogue.ended, and dialogue.problem.");
    for id in [
        world.component_id_of::<UiRoot>(),
        world.component_id_of::<UiNode>(),
        world.component_id_of::<UiPanel>(),
        world.component_id_of::<UiText>(),
        world.component_id_of::<UiButton>(),
        world.component_id_of::<Parent>(),
    ]
    .into_iter()
    .flatten()
    {
        desc = desc.reads(id).writes(id);
    }
    if let Some(s) = world.component_id_of::<argus_ui::UiSettings>() {
        desc = desc.reads(s);
    }
    sched.add_system(
        desc,
        Box::new(move |ctx| {
            let Some(tid) = ctx.world().component_id_of::<Talk>() else {
                return Ok(());
            };
            // A world without conversations pays nothing, not even a query.
            if !ctx
                .world()
                .archetypes()
                .iter()
                .any(|a| !a.is_empty() && a.has(tid))
            {
                return Ok(());
            }
            let mut talks: Vec<(Entity, Talk)> = Vec::new();
            for e in ctx.query().has(tid).entities() {
                talks.push((e, ctx.get::<Talk>(e)?));
            }
            talks.sort_by_key(|t| t.0);
            // Buttons pressed this tick, by conversation.
            let mut pressed: Vec<(Entity, i32)> = Vec::new();
            if let (Some(pid), Some(bid)) = (
                ctx.world().component_id_of::<TalkPart>(),
                ctx.world().component_id_of::<UiButton>(),
            ) {
                for e in ctx.query().has(pid).has(bid).entities() {
                    let b = ctx.get::<UiButton>(e)?;
                    if b.pressed {
                        let p = ctx.get::<TalkPart>(e)?;
                        pressed.push((p.talk, p.choice));
                    }
                }
            }
            pressed.sort();
            let graph = graph.borrow();
            let ui = ui.borrow();
            for (e, mut t) in talks {
                let step = if t.steps == 0 && t.node == StrId::EMPTY {
                    Step::Start
                } else if t.pick >= 0 {
                    Step::Pick(t.pick)
                } else if let Some(&(_, c)) = pressed.iter().find(|p| p.0 == e) {
                    if c == GO_ON {
                        Step::GoOn
                    } else {
                        Step::Pick(c)
                    }
                } else if t.advance {
                    Step::GoOn
                } else {
                    continue;
                };
                let who = Parties {
                    speaker: t.speaker,
                    listener: t.listener,
                };
                let name =
                    |id: StrId, ctx: &SystemCtx<'_>| ctx.world().strings().resolve(id).to_string();
                let stop = match step {
                    Step::Start => {
                        let start = name(t.start, ctx);
                        ctx.emit(
                            "dialogue.started",
                            vec![
                                ("talk".into(), Value::Entity(e)),
                                ("start".into(), Value::Str(start.clone())),
                                ("speaker".into(), Value::Entity(t.speaker)),
                                ("listener".into(), Value::Entity(t.listener)),
                            ],
                        );
                        let key = start
                            .strip_prefix("dialogue:")
                            .unwrap_or(&start)
                            .to_string();
                        walk(ctx, &graph, e, who, Some(key), String::new())
                    }
                    Step::Pick(n) => {
                        let at = name(t.node, ctx);
                        let rows = graph.children(&at);
                        let offered: Vec<usize> = (0..rows.len().min(32))
                            .filter(|i| t.offered & (1 << i) != 0)
                            .collect();
                        match offered
                            .get(n.max(0) as usize)
                            .and_then(|&i| graph.node(&rows[i]))
                        {
                            Some(c) => {
                                effects(ctx, c, e, who);
                                ctx.emit(
                                    "dialogue.chose",
                                    vec![
                                        ("talk".into(), Value::Entity(e)),
                                        ("node".into(), Value::Str(at.clone())),
                                        ("choice".into(), Value::Str(c.key.clone())),
                                    ],
                                );
                                walk(ctx, &graph, e, who, c.next.clone(), c.key.clone())
                            }
                            None => {
                                problem(
                                    ctx,
                                    e,
                                    &at,
                                    &format!(
                                        "there is no choice {n} here; {} offered",
                                        offered.len()
                                    ),
                                );
                                t.pick = -1;
                                ctx.update::<Talk, _>(e, |x| *x = t)?;
                                continue;
                            }
                        }
                    }
                    Step::GoOn => {
                        let at = name(t.node, ctx);
                        match graph.node(&at) {
                            Some(n) => {
                                let next = branch(ctx, &graph, n, e, who);
                                walk(ctx, &graph, e, who, next, at.clone())
                            }
                            None => Stop::End(at),
                        }
                    }
                };
                // The box goes; a shown node makes it again.
                clear(ctx, e)?;
                match stop {
                    Stop::End(last) => {
                        ctx.emit(
                            "dialogue.ended",
                            vec![
                                ("talk".into(), Value::Entity(e)),
                                ("start".into(), Value::Str(name(t.start, ctx))),
                                ("node".into(), Value::Str(last)),
                                ("speaker".into(), Value::Entity(t.speaker)),
                                ("listener".into(), Value::Entity(t.listener)),
                            ],
                        );
                        ctx.despawn(e)?;
                    }
                    Stop::Show(key, offered) => {
                        let node = graph.node(&key).expect("walked to it");
                        let lang = argus_ui::strings::language(ctx.world());
                        let line = if node.line.is_empty() {
                            String::new()
                        } else {
                            let raw = ui.strings.text(&lang, &node.line);
                            let mut probs = Vec::new();
                            argus_ecs::text::fill(ctx.world(), t.speaker, &raw, &mut probs)
                        };
                        let s = ctx.world().strings();
                        t.node = s.intern(&key);
                        t.line = s.intern(&line);
                        t.who = if node.speaker.is_empty() {
                            StrId::EMPTY
                        } else {
                            s.intern(&ui.strings.text(&lang, &node.speaker))
                        };
                        t.choices = offered.len() as u32;
                        t.offered = offered.iter().fold(0u32, |m, &i| m | (1 << i));
                        t.steps += 1;
                        t.pick = -1;
                        t.advance = false;
                        ctx.update::<Talk, _>(e, |x| *x = t)?;
                        let rows = graph.children(&key);
                        let lines: Vec<String> = offered
                            .iter()
                            .filter_map(|&i| graph.node(&rows[i]).map(|c| c.line.clone()))
                            .collect();
                        make_box(ctx, e, &t, node, &lines)?;
                    }
                }
            }
            Ok(())
        }),
    )
}

/// Walk from `key` to the next node that shows a line or offers choices,
/// passing over nodes whose `when` fails and taking branches, making each
/// shown node's edits on the way.
fn walk(
    ctx: &mut SystemCtx<'_>,
    graph: &Graph,
    talk: Entity,
    who: Parties,
    mut key: Option<String>,
    mut last: String,
) -> Stop {
    for _ in 0..MAX_HOPS {
        let Some(k) = key.take() else {
            return Stop::End(last);
        };
        let Some(n) = graph.node(&k) else {
            problem(ctx, talk, &k, "no dialogue row has this key");
            return Stop::End(last);
        };
        last = n.key.clone();
        if !holds(ctx.world(), n.when.as_ref(), who) {
            key = n.next.clone();
            continue;
        }
        effects(ctx, n, talk, who);
        if !n.sound.is_empty() {
            ctx.emit(
                "sound.play",
                vec![
                    ("entity".into(), Value::Entity(who.speaker)),
                    ("sound".into(), Value::Str(n.sound.clone())),
                ],
            );
        }
        let offered: Vec<usize> = graph
            .children(&n.key)
            .iter()
            .enumerate()
            .take(32)
            .filter(|(_, c)| {
                graph
                    .node(c)
                    .is_some_and(|c| !c.line.is_empty() && holds(ctx.world(), c.when.as_ref(), who))
            })
            .map(|(i, _)| i)
            .collect();
        if !n.line.is_empty() || !offered.is_empty() {
            ctx.emit(
                "dialogue.line",
                vec![
                    ("talk".into(), Value::Entity(talk)),
                    ("node".into(), Value::Str(n.key.clone())),
                    ("line".into(), Value::Str(n.line.clone())),
                    ("speaker".into(), Value::Str(n.speaker.clone())),
                    ("choices".into(), Value::UInt(offered.len() as u64)),
                ],
            );
            return Stop::Show(n.key.clone(), offered);
        }
        key = branch(ctx, graph, n, talk, who);
    }
    problem(
        ctx,
        talk,
        &last,
        &format!("{MAX_HOPS} nodes in a row without a line: a loop?"),
    );
    Stop::End(last)
}

/// Where a node goes on to: its first branch (a row from it without a line)
/// whose `when` holds, its edits made, or its `next`.
fn branch(
    ctx: &mut SystemCtx<'_>,
    graph: &Graph,
    n: &Node,
    talk: Entity,
    who: Parties,
) -> Option<String> {
    for c in graph.children(&n.key) {
        let Some(b) = graph.node(c) else { continue };
        if b.line.is_empty() && holds(ctx.world(), b.when.as_ref(), who) {
            effects(ctx, b, talk, who);
            return b.next.clone();
        }
    }
    n.next.clone()
}

/// A row's edits: fields set or added to, events emitted. The system grows
/// its declared writes by what it edits (as the tweens do), so who_writes
/// stays true.
fn effects(ctx: &mut SystemCtx<'_>, n: &Node, talk: Entity, who: Parties) {
    for eff in &n.effects {
        let (path, lit) = match eff {
            Effect::Emit(name) => {
                ctx.emit(
                    name.clone(),
                    vec![
                        ("talk".into(), Value::Entity(talk)),
                        ("node".into(), Value::Str(n.key.clone())),
                        ("speaker".into(), Value::Entity(who.speaker)),
                        ("listener".into(), Value::Entity(who.listener)),
                    ],
                );
                continue;
            }
            Effect::Set(p, l) => (p, l.clone()),
            Effect::Add(p, d) => (p, Lit::Num(*d)),
        };
        let Some(e) = subject(ctx.world(), path, who) else {
            problem(
                ctx,
                talk,
                &n.key,
                &format!("no one to edit for {}", path.path),
            );
            continue;
        };
        let old = match ctx.world().get_field(e, &path.path) {
            Ok(v) => v,
            Err(err) => {
                problem(ctx, talk, &n.key, &err.message);
                continue;
            }
        };
        let lit = match (eff, &old) {
            (Effect::Add(_, d), v) => match v {
                Value::Int(i) => Lit::Num(*i as f64 + d),
                Value::UInt(u) => Lit::Num((*u as f64 + d).max(0.0)),
                Value::Float(f) => Lit::Num(f + d),
                _ => {
                    problem(
                        ctx,
                        talk,
                        &n.key,
                        &format!("{} is not a number to add to", path.path),
                    );
                    continue;
                }
            },
            _ => lit,
        };
        let Some(v) = value_like(&old, &lit) else {
            problem(
                ctx,
                talk,
                &n.key,
                &format!("{} cannot take {lit:?}", path.path),
            );
            continue;
        };
        // Declared now, joining the system's writes after this run (as a
        // tween's target does), and written through the world.
        if let Ok((c, _)) = ctx.world().registry().resolve_path(&path.path) {
            ctx.declare(c, true);
        }
        if let Err(err) = ctx.world().set_field(e, &path.path, v) {
            problem(ctx, talk, &n.key, &err.message);
        }
    }
}

fn problem(ctx: &mut SystemCtx<'_>, talk: Entity, node: &str, why: &str) {
    ctx.emit(
        "dialogue.problem",
        vec![
            ("talk".into(), Value::Entity(talk)),
            ("node".into(), Value::Str(node.to_string())),
            ("why".into(), Value::Str(why.to_string())),
        ],
    );
}

/// Remove a conversation's box.
fn clear(ctx: &mut SystemCtx<'_>, talk: Entity) -> Result<()> {
    let Some(pid) = ctx.world().component_id_of::<TalkPart>() else {
        return Ok(());
    };
    let mut gone = Vec::new();
    for e in ctx.query().has(pid).entities() {
        if ctx.get::<TalkPart>(e)?.talk == talk {
            gone.push(e);
        }
    }
    gone.sort();
    for e in gone {
        ctx.despawn(e)?;
    }
    Ok(())
}

/// One node of the box.
fn part(ctx: &mut SystemCtx<'_>, talk: Entity, parent: Entity, choice: i32) -> Result<Entity> {
    let e = ctx.spawn();
    ctx.insert(e, TalkPart { talk, choice })?;
    if !parent.is_null() {
        ctx.insert(e, Parent { entity: parent })?;
    }
    Ok(e)
}

/// The box a shown node makes: along the top of the canvas (under a HUD
/// line, clear of the people talking, who stand lower), a panel with
/// the speaker's name, the line, and a button a choice, or one to go on.
/// The words are string keys, so the UI shows them in the world's language.
fn make_box(
    ctx: &mut SystemCtx<'_>,
    talk: Entity,
    t: &Talk,
    node: &Node,
    choices: &[String],
) -> Result<()> {
    let id = |ctx: &SystemCtx<'_>, x: &str| ctx.world().strings().intern(x);
    let (start, center, panel_color, button_color, accent) = (
        id(ctx, "start"),
        id(ctx, "center"),
        id(ctx, "panel"),
        id(ctx, "button"),
        id(ctx, "accent"),
    );
    // (key, colour, button) a line of the panel.
    let mut lines: Vec<(StrId, StrId, Option<i32>)> = Vec::new();
    if !node.speaker.is_empty() {
        lines.push((id(ctx, &node.speaker), accent, None));
    }
    if !node.line.is_empty() {
        lines.push((id(ctx, &node.line), StrId::EMPTY, None));
    }
    if choices.is_empty() {
        lines.push((id(ctx, "dialogue.continue"), StrId::EMPTY, Some(GO_ON)));
    } else {
        for (i, c) in choices.iter().enumerate() {
            lines.push((id(ctx, c), StrId::EMPTY, Some(i as i32)));
        }
    }
    let root = part(ctx, talk, Entity::NULL, NOT_A_BUTTON)?;
    ctx.insert(
        root,
        UiRoot {
            layer: 40,
            slot: t.slot,
            ..UiRoot::default()
        },
    )?;
    ctx.insert(
        root,
        UiNode {
            justify: start,
            align: center,
            pad: Vec2::new(24.0, 52.0),
            ..UiNode::default()
        },
    )?;
    let panel = part(ctx, talk, root, NOT_A_BUTTON)?;
    ctx.insert(
        panel,
        UiNode {
            pad: Vec2::new(22.0, 16.0),
            gap: 8.0,
            min: Vec2::new(560.0, 0.0),
            ..UiNode::default()
        },
    )?;
    ctx.insert(
        panel,
        UiPanel {
            color: panel_color,
            border: 2.0,
            ..UiPanel::default()
        },
    )?;
    for (order, (key, color, button)) in lines.into_iter().enumerate() {
        let e = part(ctx, talk, panel, button.unwrap_or(NOT_A_BUTTON))?;
        ctx.insert(
            e,
            UiNode {
                order: order as i32,
                pad: if button.is_some() {
                    Vec2::new(14.0, 6.0)
                } else {
                    Vec2::ZERO
                },
                ..UiNode::default()
            },
        )?;
        ctx.insert(
            e,
            UiText {
                key,
                color,
                ..UiText::default()
            },
        )?;
        if button.is_some() {
            ctx.insert(
                e,
                UiPanel {
                    color: button_color,
                    ..UiPanel::default()
                },
            )?;
            ctx.insert(e, UiButton::default())?;
        }
    }
    Ok(())
}
