//! No platform math in the sim (docs/multiplayer-checklist.md item 0).
//!
//! `x.sin()` on an `f32` or `f64` is each platform's own C library, and the
//! libraries disagree in the last bit on up to 17% of inputs
//! (docs/determinism-spike.md). Anything that can reach the world must use
//! `libm`, which gives the same bits everywhere, or two players' worlds
//! drift apart. This reads every crate's and plugin's source and fails on a
//! platform call outside presentation code.

use std::path::{Path, PathBuf};

/// Crates that only draw or play: nothing they compute is world state.
const PRESENTATION: &[&str] = &[
    "argus-audio",
    "argus-light",
    "argus-material",
    "argus-particles",
    "argus-post",
    "argus-render",
];

/// Single files elsewhere, each with why it may.
const ALLOWED: &[(&str, &str)] = &[
    (
        "crates/argus-assets/src/bc7.rs",
        "texture compression's sRGB curve, at import",
    ),
    (
        "crates/argus-daemon/src/daemon.rs",
        "where a map's cells are on screen, for an observation",
    ),
    (
        "crates/argus-editor/src/tiles.rs",
        "the brush under the pointer, in the editor's view",
    ),
    (
        "crates/argus-web/src/mathprobe.rs",
        "compares the platform's library with libm, on purpose",
    ),
];

/// The platform's transcendental functions, as methods and as paths.
const NAMES: &[&str] = &[
    "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sin_cos", "sinh", "cosh", "tanh",
    "asinh", "acosh", "atanh", "exp", "exp2", "exp_m1", "ln", "ln_1p", "log", "log2", "log10",
    "powf", "powi", "hypot", "cbrt", "mul_add",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name();
        if name == "target" || name == ".argus" {
            continue;
        }
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The line with its `//` comment cut (text in quotes is kept whole).
fn code(line: &str) -> &str {
    let b = line.as_bytes();
    let mut quoted = false;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' if quoted => i += 1,
            b'"' => quoted = !quoted,
            b'/' if !quoted && b.get(i + 1) == Some(&b'/') => return &line[..i],
            _ => {}
        }
        i += 1;
    }
    line
}

/// Platform calls on the line: `.sin(` and `f32::sin(`, where `.log(`
/// counts only with an argument (`st.log()` is a journal, not a logarithm).
fn calls(line: &str) -> Vec<&'static str> {
    let mut found = Vec::new();
    for name in NAMES {
        for lead in [".", "f32::", "f64::"] {
            let pat = format!("{lead}{name}(");
            let mut from = 0;
            while let Some(at) = line[from..].find(&pat) {
                let after = from + at + pat.len();
                let before = line[..from + at].chars().next_back();
                let word = before.is_some_and(|c| c.is_alphanumeric() || c == '_');
                let empty = line[after..].starts_with(')');
                // `.` must follow a value, and `f32::` must not be `libm::f32::`.
                if !(lead != "." && word) && !(*name == "log" && empty) {
                    found.push(*name);
                }
                from = after;
            }
        }
    }
    found
}

#[test]
fn the_sim_uses_no_platform_math() {
    let root = root();
    let mut files = Vec::new();
    for dir in ["crates", "examples", "templates"] {
        rust_files(&root.join(dir), &mut files);
    }
    assert!(files.len() > 100, "found only {} files", files.len());
    let mut bad = Vec::new();
    for f in &files {
        let rel = f
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let in_src = rel.contains("/src/");
        let krate = rel.split('/').nth(1).unwrap_or("");
        if !in_src
            || rel.starts_with("crates/") && PRESENTATION.contains(&krate)
            || ALLOWED.iter().any(|(p, _)| *p == rel)
        {
            continue;
        }
        let text = std::fs::read_to_string(f).unwrap();
        // A crate's unit tests compare with the platform freely.
        let body = text.split("#[cfg(test)]").next().unwrap();
        for (n, line) in body.lines().enumerate() {
            for name in calls(code(line)) {
                bad.push(format!("{rel}:{}: {name}: {}", n + 1, line.trim()));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "platform math in sim code; use libm's (libm::sinf, libm::powf, ...), or a plain \
         product for a small power, so every machine gets the same bits:\n{}",
        bad.join("\n")
    );
}

#[test]
fn the_scan_sees_what_it_should() {
    assert_eq!(calls("let (s, c) = self.rot.sin_cos();"), ["sin_cos"]);
    assert_eq!(calls("v * (-dt / tau).exp()"), ["exp"]);
    assert_eq!(calls("let a = f32::atan2(y, x);"), ["atan2"]);
    assert_eq!(calls("(a - b).powi(2)"), ["powi"]);
    assert!(calls("let (s, c) = libm::sincosf(self.rot);").is_empty());
    assert!(calls("for e in st.log() {").is_empty());
    assert_eq!(calls("x.log(2.0)"), ["log"]);
    assert!(calls("let d = (dx * dx + dy * dy).sqrt();").is_empty());
    assert_eq!(code("a.sin() // b.cos()"), "a.sin() ");
    assert_eq!(
        code(r#"let s = "//x"; y.cos()"#),
        r#"let s = "//x"; y.cos()"#
    );
}
