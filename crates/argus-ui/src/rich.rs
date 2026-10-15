//! Rich text (reaching players item 3): markup in a `UiText`'s words.
//!
//! - `[color=accent]words[/color]`: a theme colour's name or `#RRGGBB`(`AA`).
//! - `[b]words[/b]`, `[i]words[/i]`: bold and italic.
//! - `[size=24]words[/size]`: a size in canvas pixels, as `UiText.size`.
//! - `[glyph=jump]`: what the player presses for the action, on the device
//!   they use, as `InputGlyph` shows it (`[glyph=jump:2]` for slot 2). The
//!   layout resolves it into a `[key]Space[/key]` run, so the words shown
//!   (`UiText.shown`) hold no lookup, and the drawing marks the key.
//! - `[img=icons]`, `[img=icons:3]`: a sprite's frame, as tall as the text
//!   and as wide.
//! - `[[` is a bracket. Anything else in brackets is words, as written.
//!
//! Holes (`{Component.field}`) and string tables work as before: the
//! markup may sit in a table's text and around a hole. The semantic frame
//! and the screen reader get the plain words ([`plain`]): no tags, a key's
//! words, nothing for a picture.

use crate::text::{FontFace, LINE};

/// How a run of words is drawn.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Style {
    /// A theme colour's name or `#RRGGBB(AA)`; none: the text's own.
    pub color: Option<String>,
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// A size in canvas pixels; none: the text's own.
    pub size: Option<f32>,
    /// A key's words (a resolved `[glyph]`), drawn on a plate.
    pub key: bool,
}

/// What a span is.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// Words.
    Text(String),
    /// A sprite's frame.
    Img {
        /// The sprite asset.
        sprite: String,
        /// Its frame.
        frame: u32,
    },
}

/// Words or a picture, styled.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    /// What it is.
    pub kind: Kind,
    /// How it is drawn.
    pub style: Style,
}

/// One tag.
enum Tag {
    Open(&'static str, Style),
    Close(&'static str),
    Glyph(String),
    Img(String, u32),
}

/// What a bracketed name is, when it is a tag; `Err` for one that names a
/// tag but does not read (a size that is not a number); `Ok(None)` for
/// words in brackets.
fn tag(body: &str, base: &Style) -> Result<Option<Tag>, String> {
    let open = |name: &'static str, f: &dyn Fn(&mut Style)| {
        let mut s = base.clone();
        f(&mut s);
        Ok(Some(Tag::Open(name, s)))
    };
    match body {
        "b" => return open("b", &|s| s.bold = true),
        "i" => return open("i", &|s| s.italic = true),
        "key" => return open("key", &|s| s.key = true),
        "/b" => return Ok(Some(Tag::Close("b"))),
        "/i" => return Ok(Some(Tag::Close("i"))),
        "/key" => return Ok(Some(Tag::Close("key"))),
        "/color" => return Ok(Some(Tag::Close("color"))),
        "/size" => return Ok(Some(Tag::Close("size"))),
        _ => {}
    }
    let Some((k, v)) = body.split_once('=') else {
        return Ok(None);
    };
    let v = v.trim();
    match k.trim() {
        "color" if !v.is_empty() => {
            let c = v.to_string();
            open("color", &move |s| s.color = Some(c.clone()))
        }
        "size" => match v.parse::<f32>() {
            Ok(n) if n > 0.0 && n.is_finite() => open("size", &move |s| s.size = Some(n)),
            _ => Err(format!("[size={v}]: a size is a number of pixels above 0")),
        },
        "glyph" if !v.is_empty() => Ok(Some(Tag::Glyph(v.to_string()))),
        "img" if !v.is_empty() => {
            let (sprite, frame) = match v.rsplit_once(':') {
                Some((s, f)) => match f.parse::<u32>() {
                    Ok(n) => (s, n),
                    Err(_) => return Err(format!("[img={v}]: a frame is a number, img=icons:3")),
                },
                None => (v, 0),
            };
            Ok(Some(Tag::Img(sprite.to_string(), frame)))
        }
        _ => Ok(None),
    }
}

/// Whether `s` might have markup: most words have no `[`, and are left as
/// they are.
pub fn has_markup(s: &str) -> bool {
    s.contains('[')
}

/// Read markup into spans, and what did not read (each a line saying
/// where). A tag that does not read, or a closing tag with no opening one,
/// is shown as written.
pub fn parse(s: &str) -> (Vec<Span>, Vec<String>) {
    let mut out: Vec<Span> = Vec::new();
    let mut problems = Vec::new();
    // The open tags, innermost last, each with the style it began.
    let mut stack: Vec<(&'static str, Style)> = Vec::new();
    let mut words = String::new();
    let style_now =
        |stack: &[(&'static str, Style)]| stack.last().map(|(_, s)| s.clone()).unwrap_or_default();
    let flush = |words: &mut String, out: &mut Vec<Span>, style: Style| {
        if !words.is_empty() {
            out.push(Span {
                kind: Kind::Text(std::mem::take(words)),
                style,
            });
        }
    };
    let mut rest = s;
    while let Some(i) = rest.find('[') {
        words.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        if let Some(r) = after.strip_prefix('[') {
            words.push('[');
            rest = r;
            continue;
        }
        let Some(j) = after.find(']') else {
            words.push('[');
            rest = after;
            continue;
        };
        let body = &after[..j];
        let base = style_now(&stack);
        match tag(body, &base) {
            Ok(Some(t)) => match t {
                Tag::Open(name, style) => {
                    flush(&mut words, &mut out, base);
                    stack.push((name, style));
                }
                Tag::Close(name) => match stack.iter().rposition(|(n, _)| *n == name) {
                    Some(k) => {
                        flush(&mut words, &mut out, base);
                        stack.truncate(k);
                    }
                    None => {
                        problems.push(format!("[/{name}] with no [{name}] before it"));
                        words.push_str(&format!("[/{name}]"));
                    }
                },
                // Left as written when the layout did not resolve it.
                Tag::Glyph(action) => words.push_str(&format!("[glyph={action}]")),
                Tag::Img(sprite, frame) => {
                    flush(&mut words, &mut out, base);
                    out.push(Span {
                        kind: Kind::Img { sprite, frame },
                        style: style_now(&stack),
                    });
                }
            },
            Ok(None) => words.push_str(&format!("[{body}]")),
            Err(e) => {
                problems.push(e);
                words.push_str(&format!("[{body}]"));
            }
        }
        rest = &after[j + 1..];
    }
    words.push_str(rest);
    flush(&mut words, &mut out, style_now(&stack));
    (out, problems)
}

/// The plain words: no tags, a key's words, nothing for a picture, `[[` a
/// bracket. What the semantic frame and the screen reader say.
pub fn plain(s: &str) -> String {
    if !has_markup(s) {
        return s.to_string();
    }
    parse(s)
        .0
        .into_iter()
        .filter_map(|sp| match sp.kind {
            Kind::Text(t) => Some(t),
            Kind::Img { .. } => None,
        })
        .collect()
}

/// `[glyph=action]` (and `[glyph=action:slot]`) made `[key]label[/key]` by
/// `label(action, slot)`; one it gives no label for is left as written,
/// and said in `problems`.
pub fn resolve_glyphs(
    s: &str,
    label: impl Fn(&str, u8) -> Option<String>,
    problems: &mut Vec<String>,
) -> String {
    if !s.contains("[glyph=") {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("[glyph=") {
        // A doubled bracket before it is a bracket, not a tag.
        if rest[..i].ends_with('[') && !rest[..i].ends_with("[[") {
            out.push_str(&rest[..i + 1]);
            rest = &rest[i + 1..];
            continue;
        }
        out.push_str(&rest[..i]);
        let after = &rest[i + "[glyph=".len()..];
        let Some(j) = after.find(']') else {
            out.push_str(&rest[i..]);
            return out;
        };
        let v = after[..j].trim();
        let (action, slot) = match v.rsplit_once(':') {
            Some((a, n)) => (a, n.parse::<u8>().unwrap_or(1)),
            None => (v, 1),
        };
        match label(action, slot) {
            Some(l) => {
                out.push_str("[key]");
                out.push_str(&l.replace('[', "[["));
                out.push_str("[/key]");
            }
            None => {
                problems.push(format!(
                    "[glyph={v}]: no action {action:?} with a binding to show"
                ));
                out.push_str(&format!("[glyph={v}]"));
            }
        }
        rest = &after[j + 1..];
    }
    out.push_str(rest);
    out
}

/// A piece of a laid-out line.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    /// Words, or a picture.
    pub kind: Kind,
    /// Its style.
    pub style: Style,
    /// Its size in canvas pixels.
    pub size: f32,
    /// Left, from the line's start.
    pub x: f32,
    /// Its width.
    pub width: f32,
}

/// A laid-out line.
#[derive(Clone, Debug, PartialEq)]
pub struct RichLine {
    /// Its pieces, left to right.
    pub pieces: Vec<Piece>,
    /// Its width.
    pub width: f32,
    /// Its height: its largest size's line.
    pub height: f32,
}

/// Spans broken into lines no wider than `max` (a newline always breaks;
/// words wrap at spaces, a word may change style inside; a word wider than
/// a line is cut), at `size` unless a span says otherwise.
pub fn lines(font: &dyn FontFace, spans: &[Span], size: f32, max: Option<f32>) -> Vec<RichLine> {
    // Atoms: a word (its pieces), a space, a newline, a picture.
    enum Atom {
        Word(Vec<(String, Style, f32)>),
        Space(Style, f32),
        Break,
        Img(Span, f32),
    }
    let mut atoms: Vec<Atom> = Vec::new();
    let mut word: Vec<(String, Style, f32)> = Vec::new();
    let end_word = |word: &mut Vec<(String, Style, f32)>, atoms: &mut Vec<Atom>| {
        if !word.is_empty() {
            atoms.push(Atom::Word(std::mem::take(word)));
        }
    };
    for sp in spans {
        let sz = sp.style.size.unwrap_or(size);
        match &sp.kind {
            Kind::Img { .. } => {
                end_word(&mut word, &mut atoms);
                atoms.push(Atom::Img(sp.clone(), sz));
            }
            Kind::Text(t) => {
                let mut cur = String::new();
                for c in t.chars() {
                    match c {
                        ' ' | '\n' => {
                            if !cur.is_empty() {
                                word.push((std::mem::take(&mut cur), sp.style.clone(), sz));
                            }
                            end_word(&mut word, &mut atoms);
                            atoms.push(if c == ' ' {
                                Atom::Space(sp.style.clone(), sz)
                            } else {
                                Atom::Break
                            });
                        }
                        _ => cur.push(c),
                    }
                }
                if !cur.is_empty() {
                    word.push((cur, sp.style.clone(), sz));
                }
            }
        }
    }
    end_word(&mut word, &mut atoms);

    let mut out: Vec<RichLine> = Vec::new();
    let mut line = RichLine {
        pieces: Vec::new(),
        width: 0.0,
        height: size * LINE,
    };
    // Spaces waiting for the next word on the same line.
    let mut pending: Vec<(Style, f32)> = Vec::new();
    let push_line = |line: &mut RichLine, out: &mut Vec<RichLine>| {
        out.push(std::mem::replace(
            line,
            RichLine {
                pieces: Vec::new(),
                width: 0.0,
                height: size * LINE,
            },
        ));
    };
    let place = |line: &mut RichLine, kind: Kind, style: Style, sz: f32, w: f32| {
        line.pieces.push(Piece {
            kind,
            style,
            size: sz,
            x: line.width,
            width: w,
        });
        line.width += w;
        line.height = line.height.max(sz * LINE);
    };
    for atom in atoms {
        match atom {
            Atom::Break => {
                pending.clear();
                push_line(&mut line, &mut out);
            }
            Atom::Space(st, sz) => pending.push((st, sz)),
            Atom::Img(sp, sz) => {
                let spaces: f32 = pending.iter().map(|(_, s)| font.measure(" ", *s)).sum();
                if !line.pieces.is_empty()
                    && max.is_some_and(|m| line.width + spaces + sz > m + 0.01)
                {
                    push_line(&mut line, &mut out);
                    pending.clear();
                }
                if !line.pieces.is_empty() {
                    for (st, s) in pending.drain(..) {
                        place(
                            &mut line,
                            Kind::Text(" ".into()),
                            st,
                            s,
                            font.measure(" ", s),
                        );
                    }
                }
                pending.clear();
                place(&mut line, sp.kind, sp.style, sz, sz);
            }
            Atom::Word(parts) => {
                let w: f32 = parts.iter().map(|(t, _, s)| font.measure(t, *s)).sum();
                let spaces: f32 = pending.iter().map(|(_, s)| font.measure(" ", *s)).sum();
                if !line.pieces.is_empty()
                    && max.is_some_and(|m| line.width + spaces + w > m + 0.01)
                {
                    push_line(&mut line, &mut out);
                    pending.clear();
                }
                if !line.pieces.is_empty() {
                    for (st, s) in pending.drain(..) {
                        place(
                            &mut line,
                            Kind::Text(" ".into()),
                            st,
                            s,
                            font.measure(" ", s),
                        );
                    }
                }
                pending.clear();
                for (t, st, s) in parts {
                    let pw = font.measure(&t, s);
                    match max {
                        // Wider than a line: cut it, as much as fits.
                        Some(m) if pw > m + 0.01 => {
                            let mut chars: Vec<char> = t.chars().collect();
                            while !chars.is_empty() {
                                let room = (m - line.width).max(0.0);
                                let mut n = chars.len();
                                while n > 1
                                    && font.measure(&chars[..n].iter().collect::<String>(), s)
                                        > room + 0.01
                                {
                                    n -= 1;
                                }
                                let piece: String = chars[..n].iter().collect();
                                let w = font.measure(&piece, s);
                                if !line.pieces.is_empty() && w > room + 0.01 {
                                    push_line(&mut line, &mut out);
                                    continue;
                                }
                                place(&mut line, Kind::Text(piece), st.clone(), s, w);
                                chars.drain(..n);
                                if !chars.is_empty() {
                                    push_line(&mut line, &mut out);
                                }
                            }
                        }
                        _ => place(&mut line, Kind::Text(t), st, s, pw),
                    }
                }
            }
        }
    }
    out.push(line);
    out
}

/// How big rich words are, broken as [`lines`] breaks them.
pub fn size_of(font: &dyn FontFace, spans: &[Span], size: f32, max: Option<f32>) -> (f32, f32) {
    let l = lines(font, spans, size, max);
    (
        l.iter().map(|l| l.width).fold(0.0, f32::max),
        l.iter().map(|l| l.height).sum(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::PixelFont;

    #[test]
    fn markup_reads_into_styled_spans_and_words_in_brackets_stay() {
        let (s, problems) =
            parse("Hit [color=accent]the [b]bat[/b][/color] for [size=24]3[/size] [E] [[ok]");
        assert!(problems.is_empty(), "{problems:?}");
        let texts: Vec<(&str, bool, Option<&str>)> = s
            .iter()
            .map(|sp| match &sp.kind {
                Kind::Text(t) => (t.as_str(), sp.style.bold, sp.style.color.as_deref()),
                Kind::Img { .. } => ("<img>", false, None),
            })
            .collect();
        assert_eq!(
            texts,
            [
                ("Hit ", false, None),
                ("the ", false, Some("accent")),
                ("bat", true, Some("accent")),
                (" for ", false, None),
                ("3", false, None),
                (" [E] [ok]", false, None),
            ]
        );
        assert_eq!(s[4].style.size, Some(24.0));
        assert_eq!(
            plain("Hit [color=accent]the [b]bat[/b][/color] [[E]"),
            "Hit the bat [E]"
        );
    }

    #[test]
    fn what_does_not_read_is_shown_as_written_and_said() {
        // A size that is not one, so its closing tag closes nothing either.
        let (s, problems) = parse("a [/b] b [size=big]c[/size]");
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert_eq!(plain("a [/b] b"), "a [/b] b");
        assert!(matches!(&s[0].kind, Kind::Text(t) if t == "a [/b] b [size=big]c[/size]"));
    }

    #[test]
    fn a_glyph_is_resolved_into_a_key_and_a_picture_is_a_span() {
        let mut problems = Vec::new();
        let s = resolve_glyphs(
            "Press [glyph=jump] to jump, [glyph=fly:2] to fly",
            |a, slot| (a == "jump" && slot == 1).then(|| "Space".to_string()),
            &mut problems,
        );
        assert_eq!(s, "Press [key]Space[/key] to jump, [glyph=fly:2] to fly");
        assert_eq!(problems.len(), 1);
        assert_eq!(plain(&s), "Press Space to jump, [glyph=fly:2] to fly");
        let (spans, _) = parse("[img=icons:3] Gold");
        assert_eq!(
            spans[0].kind,
            Kind::Img {
                sprite: "icons".into(),
                frame: 3
            }
        );
        assert_eq!(plain("[img=icons:3] Gold"), " Gold");
    }

    #[test]
    fn lines_wrap_across_styles_and_take_the_largest_size() {
        let font = PixelFont::default();
        let (spans, _) = parse("one [b]two[/b] [size=32]three[/size] four");
        let l = lines(&font, &spans, 16.0, Some(150.0));
        assert!(l.len() >= 2, "{l:?}");
        // A line with the big word is that word's height.
        let big = l
            .iter()
            .find(|l| {
                l.pieces
                    .iter()
                    .any(|p| p.kind == Kind::Text("three".into()))
            })
            .unwrap();
        assert_eq!(big.height, 32.0 * LINE);
        for line in &l {
            assert!(line.width <= 150.01, "{line:?}");
        }
        // Without markup, as the plain text measures.
        let (spans, _) = parse("plain words here");
        assert_eq!(
            size_of(&font, &spans, 16.0, None),
            crate::text::size_of(&font, "plain words here", 16.0, None)
        );
    }
}
