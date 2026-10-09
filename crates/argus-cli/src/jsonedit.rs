//! One member set in a JSON config file's text, the rest of the file left
//! as it was: its order, its spacing, its comments. `argus mcp setup`
//! (distribution item 3) edits other programs' files, and a rewrite through
//! a map would reorder them (serde_json's maps are sorted).

use serde_json::Value as Json;

struct Member {
    key: String,
    key_start: usize,
    value_start: usize,
    value_end: usize,
}

fn bad(at: usize, what: &str) -> String {
    format!("not JSON at byte {at}: {what}")
}

/// Past whitespace and comments (`//`, `/* */`, as VS Code's files allow).
fn skip(b: &[u8], mut i: usize) -> Result<usize, String> {
    loop {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if b[i..].starts_with(b"//") {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if b[i..].starts_with(b"/*") {
            let end = b[i + 2..]
                .windows(2)
                .position(|w| w == b"*/")
                .ok_or_else(|| bad(i, "a comment that does not end"))?;
            i += 2 + end + 2;
        } else {
            return Ok(i);
        }
    }
}

/// The end of the string that starts at `i`.
fn string(b: &[u8], i: usize) -> Result<usize, String> {
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 2,
            b'"' => return Ok(j + 1),
            _ => j += 1,
        }
    }
    Err(bad(i, "a string that does not end"))
}

/// The end of the value that starts at `i`.
fn value(b: &[u8], i: usize) -> Result<usize, String> {
    match b.get(i) {
        Some(b'{') => members(b, i).map(|(_, close)| close + 1),
        Some(b'[') => {
            let mut j = skip(b, i + 1)?;
            loop {
                if b.get(j) == Some(&b']') {
                    return Ok(j + 1);
                }
                j = skip(b, value(b, j)?)?;
                match b.get(j) {
                    Some(b',') => j = skip(b, j + 1)?,
                    Some(b']') => return Ok(j + 1),
                    _ => return Err(bad(j, "expected , or ] in a list")),
                }
            }
        }
        Some(b'"') => string(b, i),
        Some(_) => {
            let mut j = i;
            while j < b.len() && !b",}] \t\r\n/".contains(&b[j]) {
                j += 1;
            }
            if j == i {
                Err(bad(i, "expected a value"))
            } else {
                Ok(j)
            }
        }
        None => Err(bad(i, "the file ends where a value should be")),
    }
}

/// The members of the object whose `{` is at `open`, and its `}`.
fn members(b: &[u8], open: usize) -> Result<(Vec<Member>, usize), String> {
    let mut out = Vec::new();
    let mut i = skip(b, open + 1)?;
    loop {
        match b.get(i) {
            Some(b'}') => return Ok((out, i)),
            Some(b'"') => {}
            _ => return Err(bad(i, "expected a key or }")),
        }
        let key_start = i;
        let key_end = string(b, i)?;
        let key: String = serde_json::from_slice(&b[key_start..key_end])
            .map_err(|e| bad(key_start, &e.to_string()))?;
        i = skip(b, key_end)?;
        if b.get(i) != Some(&b':') {
            return Err(bad(i, "expected :"));
        }
        let value_start = skip(b, i + 1)?;
        let value_end = value(b, value_start)?;
        out.push(Member {
            key,
            key_start,
            value_start,
            value_end,
        });
        i = skip(b, value_end)?;
        match b.get(i) {
            Some(b',') => i = skip(b, i + 1)?,
            Some(b'}') => return Ok((out, i)),
            _ => return Err(bad(i, "expected , or }")),
        }
    }
}

/// The indentation of a member: the spaces before its key on its line.
fn indent_of(text: &str, at: usize) -> Option<String> {
    let line = text[..at].rfind('\n').map_or(0, |n| n + 1);
    let lead = &text[line..at];
    lead.chars()
        .all(|c| c == ' ' || c == '\t')
        .then(|| lead.to_string())
        .filter(|l| !l.is_empty())
}

/// An object's text, its fields in order, `depth` levels in.
pub fn render(fields: &[(&str, Json)], unit: &str, depth: usize) -> String {
    let inner = unit.repeat(depth + 1);
    let body: Vec<String> = fields
        .iter()
        .map(|(k, v)| format!("{inner}{}: {}", Json::from(*k), v))
        .collect();
    format!("{{\n{}\n{}}}", body.join(",\n"), unit.repeat(depth))
}

/// What `outer.inner` is in `text`, if it is there.
pub fn get(text: &str, outer: &str, inner: &str) -> Result<Option<Json>, String> {
    let b = text.as_bytes();
    if text.trim().is_empty() {
        return Ok(None);
    }
    let top = skip(b, 0)?;
    if b.get(top) != Some(&b'{') {
        return Err(bad(top, "the file is not an object"));
    }
    let (mems, _) = members(b, top)?;
    let Some(o) = mems.iter().find(|m| m.key == outer) else {
        return Ok(None);
    };
    if b[o.value_start] != b'{' {
        return Ok(None);
    }
    let (inner_mems, _) = members(b, o.value_start)?;
    Ok(inner_mems
        .iter()
        .find(|m| m.key == inner)
        .and_then(|m| serde_json::from_str(&text[m.value_start..m.value_end]).ok()))
}

/// `text` with `outer.inner` set to the object `fields`: replaced where it
/// is, else added at the end of `outer`, which is added at the end of the
/// file's object when missing. An empty file becomes that object alone.
pub fn set(
    text: &str,
    outer: &str,
    inner: &str,
    fields: &[(&str, Json)],
) -> Result<String, String> {
    let b = text.as_bytes();
    let k = |s: &str| Json::from(s).to_string();
    if text.trim().is_empty() {
        let unit = "  ";
        return Ok(format!(
            "{{\n{unit}{}: {{\n{unit}{unit}{}: {}\n{unit}}}\n}}\n",
            k(outer),
            k(inner),
            render(fields, unit, 2)
        ));
    }
    let top = skip(b, 0)?;
    if b.get(top) != Some(&b'{') {
        return Err(bad(top, "the file is not an object"));
    }
    let (mems, close) = members(b, top)?;
    let unit = mems
        .first()
        .and_then(|m| indent_of(text, m.key_start))
        .unwrap_or_else(|| "  ".to_string());
    let splice =
        |at: usize, end: usize, with: &str| format!("{}{with}{}", &text[..at], &text[end..]);
    match mems.iter().find(|m| m.key == outer) {
        Some(o) if b[o.value_start] == b'{' => {
            let (inner_mems, inner_close) = members(b, o.value_start)?;
            let v = render(fields, &unit, 2);
            if let Some(m) = inner_mems.iter().find(|m| m.key == inner) {
                return Ok(splice(m.value_start, m.value_end, &v));
            }
            Ok(match inner_mems.last() {
                Some(last) => splice(
                    last.value_end,
                    last.value_end,
                    &format!(",\n{unit}{unit}{}: {v}", k(inner)),
                ),
                None => splice(
                    o.value_start + 1,
                    inner_close,
                    &format!("\n{unit}{unit}{}: {v}\n{unit}", k(inner)),
                ),
            })
        }
        Some(o) => Err(format!(
            "{outer} is there, and not an object (at byte {})",
            o.value_start
        )),
        None => {
            let member = format!(
                "{}: {{\n{unit}{unit}{}: {}\n{unit}}}",
                k(outer),
                k(inner),
                render(fields, &unit, 2)
            );
            Ok(match mems.last() {
                Some(last) => splice(
                    last.value_end,
                    last.value_end,
                    &format!(",\n{unit}{member}"),
                ),
                None => splice(top + 1, close, &format!("\n{unit}{member}\n")),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry() -> Vec<(&'static str, Json)> {
        vec![("command", json!("argus")), ("args", json!(["mcp"]))]
    }

    #[test]
    fn an_empty_file_becomes_the_entry() {
        let t = set("", "mcpServers", "argus", &entry()).unwrap();
        let v: Json = serde_json::from_str(&t).unwrap();
        assert_eq!(v["mcpServers"]["argus"]["args"], json!(["mcp"]));
    }

    #[test]
    fn the_rest_of_the_file_stays_as_it_was() {
        let text = "{\n    \"zeta\": 1,\n    \"mcpServers\": {\n        \"other\": {\"command\": \"x\"}\n    },\n    \"alpha\": [1, 2]\n}\n";
        let t = set(text, "mcpServers", "argus", &entry()).unwrap();
        assert!(t.starts_with("{\n    \"zeta\": 1,\n    \"mcpServers\": {\n        \"other\": {\"command\": \"x\"},\n        \"argus\": {\n"), "{t}");
        assert!(t.ends_with("    },\n    \"alpha\": [1, 2]\n}\n"), "{t}");
        let v: Json = serde_json::from_str(&t).unwrap();
        assert_eq!(v["mcpServers"]["argus"]["command"], "argus");
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        // Again: replaced, not added twice.
        let again = set(&t, "mcpServers", "argus", &entry()).unwrap();
        assert_eq!(again, t);
        assert_eq!(
            get(&t, "mcpServers", "argus").unwrap().unwrap()["command"],
            "argus"
        );
    }

    #[test]
    fn a_missing_key_is_added_at_the_end_and_comments_survive() {
        let text = "{\n  // the theme\n  \"theme\": \"dark\", /* trailing */\n}\n";
        let t = set(text, "servers", "argus", &entry()).unwrap();
        assert!(
            t.contains("// the theme") && t.contains("/* trailing */"),
            "{t}"
        );
        assert!(
            t.contains("\"theme\": \"dark\",\n  \"servers\": {\n    \"argus\": {"),
            "{t}"
        );
        assert_eq!(
            get(&t, "servers", "argus").unwrap().unwrap()["args"],
            json!(["mcp"])
        );
    }

    #[test]
    fn an_empty_object_gets_the_member_inside() {
        let t = set("{}", "mcpServers", "argus", &entry()).unwrap();
        let v: Json = serde_json::from_str(&t).unwrap();
        assert_eq!(v["mcpServers"]["argus"]["command"], "argus");
        let t = set("{\"mcpServers\": {}}", "mcpServers", "argus", &entry()).unwrap();
        let v: Json = serde_json::from_str(&t).unwrap();
        assert_eq!(v["mcpServers"]["argus"]["command"], "argus");
    }

    #[test]
    fn what_is_not_json_is_refused() {
        assert!(set("[1, 2]", "a", "b", &entry()).is_err());
        assert!(set("{\"a\": ", "a", "b", &entry()).is_err());
        assert!(set("{\"mcpServers\": 3}", "mcpServers", "argus", &entry()).is_err());
    }
}
