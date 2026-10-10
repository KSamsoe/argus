//! The docs by topic: the manual's chapters (`docs/manual/`, compiled in so
//! they match this version), the reference's parts, and a project's own
//! ARGUS.md. What the MCP server's `docs` tool and `argus docs <topic>`
//! answer, so an agent with no shell or file system (Claude Desktop) can
//! read everything a coding agent reads.

use serde_json::{json, Value as Json};

/// The manual's chapters, by file name, in the index's order.
pub const CHAPTERS: &[(&str, &str)] = &[
    ("index", include_str!("../../../docs/manual/README.md")),
    ("concepts", include_str!("../../../docs/manual/concepts.md")),
    (
        "first-game",
        include_str!("../../../docs/manual/first-game.md"),
    ),
    (
        "scripting",
        include_str!("../../../docs/manual/scripting.md"),
    ),
    ("scenes", include_str!("../../../docs/manual/scenes.md")),
    ("gameplay", include_str!("../../../docs/manual/gameplay.md")),
    (
        "presentation",
        include_str!("../../../docs/manual/presentation.md"),
    ),
    ("content", include_str!("../../../docs/manual/content.md")),
    ("testing", include_str!("../../../docs/manual/testing.md")),
    (
        "playtesting",
        include_str!("../../../docs/manual/playtesting.md"),
    ),
    (
        "tuning-and-performance",
        include_str!("../../../docs/manual/tuning-and-performance.md"),
    ),
    ("agents", include_str!("../../../docs/manual/agents.md")),
    (
        "releasing",
        include_str!("../../../docs/manual/releasing.md"),
    ),
    (
        "reference",
        include_str!("../../../docs/manual/reference.md"),
    ),
];

/// The reference's parts, with what each lists.
pub const PARTS: &[(&str, &str)] = &[
    (
        "tools",
        "the agent tools: each one's parameters and the methods it calls",
    ),
    (
        "methods",
        "every protocol method, its parameters, and what it returns",
    ),
    (
        "scripting-api",
        "the Luau API: declarations, q, entities, components, vectors, randomness",
    ),
    (
        "scenario-api",
        "the test API: scenario, t, t.input, t.expect",
    ),
    ("files", "every file a project can have, key by key"),
    (
        "components",
        "the engine's components, their fields, and the systems that use them",
    ),
];

/// A topic's text over this, and only its sections are listed, not sent:
/// an agent asks for the one it wants.
const WHOLE: usize = 60_000;

/// A reference part's text.
pub fn part(name: &str) -> Option<String> {
    Some(match name {
        "tools" => crate::agentdoc::tools_reference(),
        "methods" => crate::agentdoc::methods_reference(),
        "scripting-api" => argus_daemon::docs::scripting(2),
        "scenario-api" => argus_daemon::docs::scenarios(2),
        "files" => argus_daemon::formats::files(2),
        "components" => argus_daemon::formats::components(2),
        _ => return None,
    })
}

/// A chapter's or a part's text.
pub fn text(topic: &str) -> Option<String> {
    let topic = if topic == "README" { "index" } else { topic };
    CHAPTERS
        .iter()
        .find(|(n, _)| *n == topic)
        .map(|(_, t)| t.to_string())
        .or_else(|| part(topic))
}

/// A chapter's title: its first heading.
fn title(text: &str) -> String {
    text.lines()
        .find_map(|l| l.strip_prefix("# "))
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// The headings of a text (`##` and `###`, outside code blocks), with the
/// byte range each one's section covers.
pub fn sections(text: &str) -> Vec<(usize, String, usize, usize)> {
    let mut heads = Vec::new();
    let mut at = 0;
    let mut fenced = false;
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
        } else if !fenced {
            let level = line.chars().take_while(|c| *c == '#').count();
            if (2..=3).contains(&level) && line[level..].starts_with(' ') {
                heads.push((level, line[level..].trim().to_string(), at));
            }
        }
        at += line.len();
    }
    let mut out = Vec::new();
    for (i, (level, head, start)) in heads.iter().enumerate() {
        let end = heads[i + 1..]
            .iter()
            .find(|(l, _, _)| l <= level)
            .map_or(text.len(), |(_, _, s)| *s);
        out.push((*level, head.clone(), *start, end));
    }
    out
}

/// The section whose heading is `wanted` (or, failing that, contains it),
/// case blind.
pub fn section(text: &str, wanted: &str) -> Option<String> {
    let w = wanted.to_lowercase();
    let all = sections(text);
    let pick = all
        .iter()
        .find(|(_, h, _, _)| h.to_lowercase() == w)
        .or_else(|| {
            all.iter()
                .find(|(_, h, _, _)| h.to_lowercase().contains(&w))
        })?;
    Some(text[pick.2..pick.3].to_string())
}

/// Every section, in the manual and the reference, whose text mentions
/// `word` (case blind), with the line it is on: where to look next.
pub fn find(word: &str) -> Vec<Json> {
    const MOST: usize = 40;
    let w = word.to_lowercase();
    let mut out = Vec::new();
    let topics: Vec<String> = CHAPTERS
        .iter()
        .map(|(n, _)| n.to_string())
        .chain(PARTS.iter().map(|(n, _)| n.to_string()))
        .collect();
    for t in topics {
        let Some(body) = text(&t) else { continue };
        for (_, head, start, end) in sections(&body) {
            // A section's own lines, not its subsections'.
            let own_end = sections(&body[start..end])
                .get(1)
                .map_or(end, |(_, _, s, _)| start + s);
            if let Some(line) = body[start..own_end]
                .lines()
                .find(|l| l.to_lowercase().contains(&w))
            {
                out.push(json!({ "topic": t, "section": head, "line": line.trim() }));
                if out.len() == MOST {
                    return out;
                }
            }
        }
    }
    out
}

/// The list of topics.
pub fn list() -> Json {
    json!({
        "manual": CHAPTERS.iter().map(|(n, t)| json!({ "topic": n, "title": title(t) })).collect::<Vec<_>>(),
        "reference": PARTS.iter().map(|(n, what)| json!({ "topic": n, "lists": what })).collect::<Vec<_>>(),
        "project": "the open game's own ARGUS.md (its components, systems, prefabs, def tables, and scenes), made from its daemon now",
        "how": "docs topic=<topic> for one; section=<heading> for one section of it; find=<word> for every section that mentions a word",
    })
}

/// The answer to a docs call: the list, a topic (whole, or its sections
/// when it is long), one section, or what mentions a word. `project` is the
/// open game's ARGUS.md when the caller could make one.
pub fn answer(
    topic: Option<&str>,
    section_wanted: Option<&str>,
    word: Option<&str>,
    project: Option<Result<String, String>>,
) -> Result<Json, String> {
    if let Some(w) = word.filter(|w| !w.trim().is_empty()) {
        let found = find(w.trim());
        return Ok(
            json!({ "find": w, "found": found, "more": "docs topic=<topic> section=<section> for one" }),
        );
    }
    let Some(t) = topic.filter(|t| !t.is_empty()) else {
        return Ok(list());
    };
    let body = if t == "project" {
        match project {
            Some(Ok(md)) => md,
            Some(Err(e)) => return Err(format!("the project's ARGUS.md: {e}")),
            None => {
                return Err(
                    "no project is open: open one first (open_project), or read the reference"
                        .into(),
                )
            }
        }
    } else {
        text(t).ok_or_else(|| {
            format!(
                "no topic {t:?}; the manual's are {}, the reference's {}, and project",
                CHAPTERS
                    .iter()
                    .map(|(n, _)| *n)
                    .collect::<Vec<_>>()
                    .join(", "),
                PARTS.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
            )
        })?
    };
    if let Some(s) = section_wanted.filter(|s| !s.is_empty()) {
        return match section(&body, s) {
            Some(text) => Ok(json!({ "topic": t, "section": s, "text": text })),
            None => Err(format!(
                "{t} has no section {s:?}; its sections: {}",
                sections(&body)
                    .iter()
                    .map(|(_, h, _, _)| h.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            )),
        };
    }
    if body.len() > WHOLE {
        return Ok(json!({
            "topic": t,
            "bytes": body.len(),
            "sections": sections(&body).iter().filter(|(l, _, _, _)| *l == 2).map(|(_, h, _, _)| h.clone()).collect::<Vec<_>>(),
            "more": format!("{t} is long: docs topic={t} section=<one of these> for one (find=<word> searches)"),
        }));
    }
    Ok(json!({ "topic": t, "text": body }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_chapter_is_in_the_index_and_the_index_names_only_chapters() {
        let index = text("index").unwrap();
        for (name, body) in CHAPTERS.iter().filter(|(n, _)| *n != "index") {
            assert!(
                index.contains(&format!("({name}.md)")),
                "the index links {name}"
            );
            assert!(!title(body).is_empty(), "{name} has a title");
        }
        for link in index.split("](").skip(1) {
            let target = link.split(')').next().unwrap();
            if let Some(name) = target.strip_suffix(".md").filter(|n| !n.contains('/')) {
                assert!(
                    text(name).is_some(),
                    "the index links {target}, which is no chapter"
                );
            }
        }
    }

    #[test]
    fn a_topic_a_section_and_a_word_are_found() {
        let r = answer(Some("scripting"), None, None, None).unwrap();
        assert!(r["text"].as_str().unwrap().starts_with("# "), "{r}");
        let m = answer(Some("methods"), None, None, None).unwrap();
        assert!(
            m["sections"].is_array(),
            "methods is long, so listed: {}",
            m["bytes"]
        );
        let one = answer(Some("methods"), Some("world.query"), None, None).unwrap();
        assert!(
            one["text"].as_str().unwrap().contains("world.query"),
            "{one}"
        );
        let f = answer(None, None, Some("CharacterController"), None).unwrap();
        assert!(f["found"].as_array().unwrap().len() > 2, "{f}");
        assert!(answer(Some("nope"), None, None, None)
            .unwrap_err()
            .contains("concepts"));
        assert!(answer(Some("project"), None, None, None).is_err());
        let l = answer(None, None, None, None).unwrap();
        assert_eq!(l["manual"].as_array().unwrap().len(), CHAPTERS.len());
    }

    #[test]
    fn a_code_block_line_is_not_a_heading() {
        let t = "# T\n\n## One\n\n```sh\n## not a heading\n```\n\n## Two\n";
        let s: Vec<String> = sections(t).into_iter().map(|(_, h, _, _)| h).collect();
        assert_eq!(s, ["One", "Two"]);
    }
}
