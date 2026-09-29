//! The CLI's `key=value` words, read into a params object (§16: "a human
//! reading the agent's transcript can rerun any call"):
//!
//! - values are JSON when they parse as JSON (`2`, `true`, `"text"`,
//!   `[1,2]`); bare words are strings;
//! - `[a, b]` and `{k: v}` accept bare words;
//! - dotted keys nest (`where.has=[Enemy]`);
//! - a repeated key becomes a list.
//!
//! Bare values (the method's first parameter without its key) need the
//! method's declared parameters, which the CLI has and this crate does
//! not; [`command::line`](crate::command::line) never writes them.

use serde_json::{Map, Value as Json};
use std::collections::HashSet;

/// Read `key=value` words into a params object. A word that is not one
/// is refused (bare values are the CLI's, which knows the method).
pub fn read(args: &[String]) -> Result<Json, String> {
    let mut obj = Map::new();
    let mut by_word = HashSet::new();
    for a in args {
        let Some((k, v)) = split_kv(a) else {
            return Err(format!("{a:?} is not key=value"));
        };
        insert(&mut obj, k, value(v)?, &mut by_word)?;
    }
    Ok(Json::Object(obj))
}

/// `key=value` if the word is one: a key is an identifier path, so
/// `@player`, `2`, and `[a, b]` are bare values even with an `=` inside.
pub fn split_kv(a: &str) -> Option<(&str, &str)> {
    let i = a.find('=')?;
    let k = &a[..i];
    if !is_key(k) {
        return None;
    }
    Some((k, &a[i + 1..]))
}

/// A key the words can carry: letters, digits, `_`, and `.` between parts.
pub fn is_key(k: &str) -> bool {
    !k.is_empty()
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        && !k.starts_with('.')
}

/// Insert at a dotted path, making objects on the way. A key repeated
/// among the words turns the value into a list; a word replaces what
/// `--params` gave for the same key.
pub fn insert(
    obj: &mut Map<String, Json>,
    key: &str,
    v: Json,
    by_word: &mut HashSet<String>,
) -> Result<(), String> {
    let repeated = !by_word.insert(key.to_string());
    let mut parts = key.split('.').peekable();
    let mut cur = obj;
    while let Some(p) = parts.next() {
        if parts.peek().is_none() {
            match cur.get_mut(p) {
                Some(Json::Array(a)) if repeated => a.push(v),
                Some(old) if repeated => {
                    let first = old.take();
                    *old = Json::Array(vec![first, v]);
                }
                _ => {
                    cur.insert(p.to_string(), v);
                }
            }
            return Ok(());
        }
        let slot = cur
            .entry(p.to_string())
            .or_insert_with(|| Json::Object(Map::new()));
        match slot {
            Json::Object(m) => cur = m,
            _ => return Err(format!("{key}: {p} is already a plain value")),
        }
    }
    Ok(())
}

/// A word's value: JSON when it parses as JSON, else JSON with bare words
/// quoted when it is bracketed, else the string itself.
pub fn value(s: &str) -> Result<Json, String> {
    if let Ok(v) = serde_json::from_str::<Json>(s) {
        return Ok(v);
    }
    let t = s.trim();
    if t.starts_with('[') || t.starts_with('{') {
        let relaxed = quote_bare(t);
        return serde_json::from_str::<Json>(&relaxed)
            .map_err(|e| format!("cannot read {s:?} as a list or object: {e}"));
    }
    Ok(Json::String(s.to_string()))
}

/// Quote bare words in a bracketed literal: `[Enemy, @player]` becomes
/// `["Enemy", "@player"]`; `{of: @player, r: 6}` becomes JSON. Numbers,
/// `true`, `false`, and `null` stay as they are.
pub fn quote_bare(s: &str) -> String {
    let cs: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 16);
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c == '"' {
            // Copy a quoted string through untouched.
            out.push(c);
            i += 1;
            while i < cs.len() {
                out.push(cs[i]);
                if cs[i] == '\\' && i + 1 < cs.len() {
                    out.push(cs[i + 1]);
                    i += 2;
                    continue;
                }
                i += 1;
                if cs[i - 1] == '"' {
                    break;
                }
            }
            continue;
        }
        if is_bare_start(c) {
            let mut word = String::new();
            while i < cs.len() {
                let d = cs[i];
                // A colon stays in the word only when glued on both sides
                // (`item:key.brass`); `of: @player` is a key and a value.
                let glued_colon =
                    d == ':' && i + 1 < cs.len() && is_bare_start(cs[i + 1]) && !word.is_empty();
                // A call glued to a word is part of it: `count()`,
                // `sum(Stockpile.food)`, `dist(@player)` (phase 3 item 3).
                if d == '(' && !word.is_empty() {
                    let mut depth = 0;
                    while i < cs.len() {
                        word.push(cs[i]);
                        depth += match cs[i] {
                            '(' => 1,
                            ')' => -1,
                            _ => 0,
                        };
                        i += 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    continue;
                }
                if is_bare_start(d) || glued_colon {
                    word.push(d);
                    i += 1;
                } else {
                    break;
                }
            }
            if is_literal_word(&word) {
                out.push_str(&word);
            } else {
                out.push('"');
                out.push_str(&word);
                out.push('"');
            }
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// A character a bare word inside brackets may start with or hold.
pub fn is_bare_start(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '@' | '_' | '-' | '+' | '.' | '/' | '#')
}

/// A bare word JSON reads as itself, not as a string: a number, `true`,
/// `false`, or `null`.
pub fn is_literal_word(w: &str) -> bool {
    matches!(w, "true" | "false" | "null") || w.parse::<f64>().is_ok()
}
