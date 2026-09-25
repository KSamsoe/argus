//! Packs: a project carried to where it runs.
//!
//! The spike's pack (phase 2 item 1, [`pack`] and [`unpack`]) is every file
//! of a project, for a dev build of the web page. A release pack (phase 2
//! item 7, §15, [`Pack`]) is what `argus build` ships: built from the import
//! cache and with scripts compiled, its files addressed by content so a
//! patch carries only the blobs that changed, and a separate pack per
//! language, laid over the game's files.

use argus_ecs::codec::{Reader, Writer};
use argus_ecs::{ArgusError, Result};
use serde_json::{json, Value as Json};
use std::collections::BTreeMap;
use std::path::Path;

const MAGIC: &[u8; 8] = b"ARGUSPK1";

/// Every file under `root` but `.argus/` and `tests/`, as a pack: the magic,
/// a count, then each file's `/`-separated relative path and bytes, all
/// lengths little-endian u32. Paths are sorted.
pub fn pack(root: &Path) -> Result<Vec<u8>> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, Vec<u8>)>) -> std::io::Result<()> {
        for e in argus_os::fs::read_dir(dir)? {
            let name = e.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || (dir == root && name == "tests") {
                continue;
            }
            if e.is_dir {
                walk(&e.path, root, out)?;
            } else {
                let rel = e
                    .path
                    .strip_prefix(root)
                    .unwrap_or(&e.path)
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((rel, argus_os::fs::read(&e.path)?));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(root, root, &mut files)
        .map_err(|e| ArgusError::new(1, format!("{}: {e}", root.display())))?;
    let mut out = MAGIC.to_vec();
    out.extend((files.len() as u32).to_le_bytes());
    for (p, b) in files {
        out.extend((p.len() as u32).to_le_bytes());
        out.extend(p.as_bytes());
        out.extend((b.len() as u32).to_le_bytes());
        out.extend(b);
    }
    Ok(out)
}

/// A pack's files: relative path and bytes.
pub fn unpack(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    let bad = || {
        ArgusError::new(1, "not an Argus pack")
            .hint("Make one with `cargo run -p argus-web --example pack -- <project> <out.pack>`.")
    };
    if bytes.len() < 12 || &bytes[..8] != MAGIC {
        return Err(bad());
    }
    let u32_at = |i: usize| -> Result<usize> {
        bytes
            .get(i..i + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
            .ok_or_else(bad)
    };
    let n = u32_at(8)?;
    let mut i = 12;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let pl = u32_at(i)?;
        let path = std::str::from_utf8(bytes.get(i + 4..i + 4 + pl).ok_or_else(bad)?)
            .map_err(|_| bad())?
            .to_string();
        i += 4 + pl;
        let bl = u32_at(i)?;
        let data = bytes.get(i + 4..i + 4 + bl).ok_or_else(bad)?.to_vec();
        i += 4 + bl;
        out.push((path, data));
    }
    Ok(out)
}

// ----- release packs (phase 2 item 7) ---------------------------------------

/// A blob's address: the first sixteen bytes of its SHA-256.
pub type Hash = [u8; 16];

/// Files by path, with their bytes, as a project has them.
pub type Files = Vec<(String, Vec<u8>)>;

/// The address of some bytes.
pub fn address(bytes: &[u8]) -> Hash {
    let h = crate::sha256::sha256(bytes);
    let mut out = [0u8; 16];
    out.copy_from_slice(&h[..16]);
    out
}

const TAG: &[u8; 4] = b"ARGP";
const VERSION: u32 = 2;

/// What a pack is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The game: every file's address and every blob.
    Base,
    /// A later build of the game: every file's address, and only the blobs
    /// the packs before it lack.
    Patch,
    /// A language (`lang/<code>/` in the project): files laid over the
    /// game's, their paths as the game's.
    Lang(String),
}

/// A release pack: a manifest (each file's path, address, and size) and the
/// blobs, each stored once and checked against its address when read.
#[derive(Clone, Debug, PartialEq)]
pub struct Pack {
    /// The game's name (`argus.toml`).
    pub game: String,
    /// The code the build was made from (`git:<rev>`).
    pub code: String,
    /// Base, patch, or language.
    pub kind: Kind,
    /// Builds count up: the newest base or patch's manifest is the game.
    pub seq: u32,
    /// Path, address, size; sorted by path.
    pub files: Vec<(String, Hash, u64)>,
    /// The blobs this pack carries.
    pub blobs: BTreeMap<Hash, Vec<u8>>,
}

fn bad_pack(why: impl std::fmt::Display) -> ArgusError {
    ArgusError::new(1, format!("not a usable Argus pack: {why}"))
        .hint("Rebuild the game with `argus build`, or copy its data/ directory again.")
}

impl Pack {
    /// The file name a build gives it in `data/`.
    pub fn file_name(&self) -> String {
        match &self.kind {
            Kind::Base => "game.pack".to_string(),
            Kind::Patch => format!("patch-{}.pack", self.seq),
            Kind::Lang(l) => format!("lang-{l}.pack"),
        }
    }

    /// Its bytes.
    pub fn encode(&self) -> Vec<u8> {
        let (kind, lang) = match &self.kind {
            Kind::Base => ("base", None),
            Kind::Patch => ("patch", None),
            Kind::Lang(l) => ("lang", Some(l.as_str())),
        };
        let mut w = Writer::new();
        w.magic(TAG, VERSION);
        w.str(
            &json!({ "game": self.game, "code": self.code, "kind": kind, "lang": lang, "seq": self.seq })
                .to_string(),
        );
        w.count(self.files.len());
        for (p, h, n) in &self.files {
            w.str(p);
            w.bytes(h);
            w.u64(*n);
        }
        w.count(self.blobs.len());
        for (h, b) in &self.blobs {
            w.bytes(h);
            w.bytes(b);
        }
        w.into_bytes()
    }

    /// A pack from its bytes; every blob must match its address.
    pub fn decode(bytes: &[u8]) -> Result<Pack> {
        let mut r = Reader::new(bytes, "pack");
        r.magic(TAG, VERSION).map_err(|e| bad_pack(e.message))?;
        let meta: Json = serde_json::from_str(&r.str()?).map_err(bad_pack)?;
        let kind = match (meta["kind"].as_str(), meta["lang"].as_str()) {
            (Some("base"), _) => Kind::Base,
            (Some("patch"), _) => Kind::Patch,
            (Some("lang"), Some(l)) => Kind::Lang(l.to_string()),
            _ => return Err(bad_pack("it says no kind")),
        };
        let hash = |b: &[u8]| -> Result<Hash> {
            b.try_into()
                .map_err(|_| bad_pack("an address is not sixteen bytes"))
        };
        let mut files = Vec::new();
        for _ in 0..r.count()? {
            let p = r.str()?;
            let h = hash(r.bytes()?)?;
            files.push((p, h, r.u64()?));
        }
        let mut blobs = BTreeMap::new();
        for _ in 0..r.count()? {
            let h = hash(r.bytes()?)?;
            let b = r.bytes()?;
            if address(b) != h {
                return Err(bad_pack("a blob does not match its address (damaged)"));
            }
            blobs.insert(h, b.to_vec());
        }
        if r.remaining() != 0 {
            return Err(bad_pack(format!("{} bytes after the end", r.remaining())));
        }
        Ok(Pack {
            game: meta["game"].as_str().unwrap_or_default().to_string(),
            code: meta["code"].as_str().unwrap_or_default().to_string(),
            kind,
            seq: meta["seq"].as_u64().unwrap_or(1) as u32,
            files,
            blobs,
        })
    }
}

/// The files a set of packs makes, in a language: the newest base or
/// patch's manifest, each file from whichever of them holds its blob, then
/// the language pack's files over them. Also says what it used.
pub fn resolve(packs: &[Pack], lang: Option<&str>) -> Result<(Files, Json)> {
    let game: Vec<&Pack> = packs
        .iter()
        .filter(|p| matches!(p.kind, Kind::Base | Kind::Patch))
        .collect();
    let newest = game
        .iter()
        .max_by_key(|p| p.seq)
        .ok_or_else(|| bad_pack("there is no game.pack"))?;
    let blob = |h: &Hash, pool: &[&Pack]| pool.iter().find_map(|p| p.blobs.get(h)).cloned();
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for (path, h, _) in &newest.files {
        let b = blob(h, &game).ok_or_else(|| {
            bad_pack(format!(
                "{path} is in the manifest of build {} but in none of the packs; a patch needs the packs before it",
                newest.seq
            ))
        })?;
        files.insert(path.clone(), b);
    }
    // A language is rebuilt whole with each build: its newest pack is the
    // language (two builds' French packs are one French).
    let mut newest_lang: BTreeMap<&str, &Pack> = BTreeMap::new();
    for p in packs {
        if let Kind::Lang(l) = &p.kind {
            let e = newest_lang.entry(l.as_str()).or_insert(p);
            if p.seq > e.seq {
                *e = p;
            }
        }
    }
    let langs: Vec<&str> = newest_lang.keys().copied().collect();
    let mut used = None;
    // Every language's string table is mounted, whichever language the
    // files are laid out in, so a player can switch in the game.
    for lp in newest_lang.values().copied() {
        for (path, h, _) in &lp.files {
            if path.starts_with("strings/") {
                if let Some(b) = blob(h, &[lp]) {
                    files.insert(path.clone(), b);
                }
            }
        }
    }
    if let Some(l) = lang {
        if let Some(lp) = newest_lang.get(l).copied() {
            for (path, h, _) in &lp.files {
                let b = blob(h, &[lp])
                    .ok_or_else(|| bad_pack(format!("lang-{l}.pack lacks {path}")))?;
                files.insert(path.clone(), b);
            }
            used = Some(l.to_string());
        }
    }
    let report = json!({
        "game": newest.game,
        "code": newest.code,
        "build": newest.seq,
        "packs": packs.len(),
        "files": files.len(),
        "languages": langs,
        "language": used,
    });
    Ok((files.into_iter().collect(), report))
}

/// Read every `*.pack` in a build's `data/` directory, and put the game's
/// files in memory under `root` (in `lang`, when that pack is there), so the
/// loaders read the game as a project there. What the packs made.
pub fn mount_dir(data: &Path, root: &Path, lang: Option<&str>) -> Result<Json> {
    let mut packs = Vec::new();
    let entries =
        argus_os::fs::read_dir(data).map_err(|e| bad_pack(format!("{}: {e}", data.display())))?;
    for e in entries {
        if e.is_dir || e.path.extension().and_then(|x| x.to_str()) != Some("pack") {
            continue;
        }
        let bytes = argus_os::fs::read(&e.path)
            .map_err(|err| bad_pack(format!("{}: {err}", e.path.display())))?;
        packs.push(
            Pack::decode(&bytes)
                .map_err(|err| bad_pack(format!("{}: {}", e.path.display(), err.message)))?,
        );
    }
    let (files, report) = resolve(&packs, lang)?;
    argus_os::fs::mount(root, files);
    Ok(report)
}

/// What `argus build` makes packs from.
#[derive(Clone, Debug, Default)]
pub struct BuildOptions {
    /// The code the build is made from, recorded in each pack.
    pub code: String,
    /// The packs of the build this one follows. With any, the game goes into
    /// a patch holding only the blobs they lack.
    pub since: Vec<Pack>,
}

/// What a build of packs made.
#[derive(Clone, Debug)]
pub struct Built {
    /// The game's pack (a base, or a patch), then one per language.
    pub packs: Vec<Pack>,
    /// What went in: files, scripts compiled, sprites from the import cache,
    /// the sources left out, bytes.
    pub report: Json,
}

/// Top-level directories a build leaves out: the tests, the dev tools, and
/// the builds themselves. Anything whose name starts with a dot is left out
/// too (`.argus/`, `.git/`).
pub const LEFT_OUT: &[&str] = &["tests", "tools", "dist"];

fn walk_files(dir: &Path, root: &Path, out: &mut Vec<String>) -> Result<()> {
    let entries = argus_os::fs::read_dir(dir)
        .map_err(|e| ArgusError::new(1, format!("{}: {e}", dir.display())))?;
    for e in entries {
        let name = e.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with('.') || (dir == root && LEFT_OUT.contains(&name)) {
            continue;
        }
        if e.is_dir {
            walk_files(&e.path, root, out)?;
        } else if let Ok(rel) = e.path.strip_prefix(root) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

fn is_script(rel: &str) -> bool {
    rel.ends_with(".luau")
        && ["components/", "defs/", "systems/"]
            .iter()
            .any(|d| rel.starts_with(d))
}

/// Make a project's packs (§15): scripts compiled to bytecode; sprites from
/// the import cache (their textures and the catalog, not the source images
/// or sidecars); every other file as it is; `lang/<code>/`, and a string
/// table other than the base language's (`strings/<code>.toml`, phase 3
/// item 4), into a pack per language. Fails on any import failure or script syntax error, naming
/// each, since a build that ships them ships a broken game.
pub fn build(root: &Path, opts: &BuildOptions) -> Result<Built> {
    let cfg = argus_scene::ProjectConfig::load(root)?;
    let mut all = Vec::new();
    walk_files(root, root, &mut all)?;
    all.sort();
    // The import cache: import now, so it is current.
    let mut importer = argus_assets::Importer::new(root);
    importer.scan();
    let mut problems: Vec<String> = importer
        .failures()
        .values()
        .map(|f| match &f.error.r#ref {
            Some(at) => format!("{at}: {}", f.error.message),
            None => format!("assets/{}: {}", f.name, f.error.message),
        })
        .collect();
    let sprites = importer.packed_sprites();
    let mut game: Vec<(String, Vec<u8>)> = Vec::new();
    let mut langs: BTreeMap<String, Vec<(String, Vec<u8>)>> = BTreeMap::new();
    let mut compiled = 0usize;
    let mut left_out = Vec::new();
    for rel in &all {
        if sprites.sources.contains(rel) {
            left_out.push(rel.clone());
            continue;
        }
        let bytes = argus_os::fs::read(root.join(rel))
            .map_err(|e| ArgusError::new(1, format!("{rel}: {e}")))?;
        let table = rel
            .strip_prefix("strings/")
            .and_then(|f| f.strip_suffix(".toml"))
            .filter(|code| !code.contains('/') && *code != cfg.strings_base);
        let (target, path) = match rel.strip_prefix("lang/").and_then(|r| r.split_once('/')) {
            Some((code, inner)) => (Some(code.to_string()), inner.to_string()),
            None => match table {
                // A language's strings travel in its own pack (section 15:
                // "a language can be added without a rebuild").
                Some(code) => (Some(code.to_string()), rel.clone()),
                None => (None, rel.clone()),
            },
        };
        let bytes = if is_script(&path) {
            let src = String::from_utf8_lossy(&bytes);
            match argus_luau::compile(rel, &src) {
                Ok(b) => {
                    compiled += 1;
                    b
                }
                Err(e) => {
                    problems.push(e.to_string());
                    continue;
                }
            }
        } else {
            bytes
        };
        match target {
            Some(code) => langs.entry(code).or_default().push((path, bytes)),
            None => game.push((path, bytes)),
        }
    }
    if !problems.is_empty() {
        return Err(ArgusError::new(
            1,
            format!("the build stopped: {}", problems.join("; ")),
        )
        .hint("Fix these and build again; `argus asset list` and `argus script errors` show them in the daemon."));
    }
    game.push((
        argus_assets::CATALOG.to_string(),
        serde_json::to_vec_pretty(&sprites.catalog).unwrap_or_default(),
    ));
    for (rel, disk) in &sprites.textures {
        let bytes = argus_os::fs::read(disk)
            .map_err(|e| ArgusError::new(1, format!("{}: {e}", disk.display())))?;
        game.push((rel.clone(), bytes));
    }
    game.sort_by(|a, b| a.0.cmp(&b.0));

    // Content addressing: a patch carries only what the builds before it lack.
    let old_game: Vec<&Pack> = opts
        .since
        .iter()
        .filter(|p| matches!(p.kind, Kind::Base | Kind::Patch))
        .collect();
    let have: std::collections::BTreeSet<Hash> = old_game
        .iter()
        .flat_map(|p| p.blobs.keys().copied())
        .collect();
    let seq = old_game.iter().map(|p| p.seq).max().unwrap_or(0) + 1;
    let make = |kind: Kind,
                seq: u32,
                files: &[(String, Vec<u8>)],
                skip: &std::collections::BTreeSet<Hash>| {
        let mut manifest = Vec::new();
        let mut blobs = BTreeMap::new();
        for (p, b) in files {
            let h = address(b);
            manifest.push((p.clone(), h, b.len() as u64));
            if !skip.contains(&h) {
                blobs.insert(h, b.clone());
            }
        }
        Pack {
            game: cfg.name.clone(),
            code: opts.code.clone(),
            kind,
            seq,
            files: manifest,
            blobs,
        }
    };
    let kind = if old_game.is_empty() {
        Kind::Base
    } else {
        Kind::Patch
    };
    let main = make(kind, seq, &game, &have);
    let bytes: u64 = game.iter().map(|(_, b)| b.len() as u64).sum();
    let shipped: u64 = main.blobs.values().map(|b| b.len() as u64).sum();
    let mut packs = vec![main];
    for (code, mut files) in langs {
        files.sort_by(|a, b| a.0.cmp(&b.0));
        packs.push(make(
            Kind::Lang(code),
            seq,
            &files,
            &std::collections::BTreeSet::new(),
        ));
    }
    let report = json!({
        "game": cfg.name,
        "build": seq,
        "kind": if packs[0].kind == Kind::Base { "base" } else { "patch" },
        "files": game.len(),
        "bytes": bytes,
        "blobs_new": packs[0].blobs.len(),
        "bytes_new": shipped,
        "scripts_compiled": compiled,
        "sprites": sprites.textures.len(),
        "sources_left_out": left_out,
        "languages": packs[1..].iter().filter_map(|p| match &p.kind { Kind::Lang(l) => Some(l.clone()), _ => None }).collect::<Vec<_>>(),
    });
    Ok(Built { packs, report })
}
