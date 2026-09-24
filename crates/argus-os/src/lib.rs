//! Argus host services, phase 2 item 1 (the web spike): files and the
//! clock, through one door.
//!
//! Natively this is `std::fs` and `std::time`, nothing more. A browser has
//! no file system and panics on `std::time::Instant`: there, files are an
//! in-memory tree the player mounts from a pack of the project, and the
//! clock is the page's. Every crate that reads the project reads through
//! here, so the loaders that run in the daemon run unchanged in the player.
//! A release build (§15) reads its packs through the same door.

#![warn(missing_docs)]

/// A monotonic clock for timing: `std::time::Instant` natively, the page's
/// clock in a browser (where the standard one panics). Gameplay never reads
/// it (§6); profiling and load reports do.
pub mod time {
    #[cfg(not(target_arch = "wasm32"))]
    pub use std::time::Instant;
    #[cfg(target_arch = "wasm32")]
    pub use web_time::Instant;
}

/// Files.
pub mod fs {
    use std::io;
    use std::path::{Path, PathBuf};
    use std::time::SystemTime;

    /// One entry of a directory.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Entry {
        /// Its full path.
        pub path: PathBuf,
        /// A directory, not a file.
        pub is_dir: bool,
        /// Bytes, for a file.
        pub len: u64,
        /// Last modified (the Unix epoch where there is no clock).
        pub modified: SystemTime,
    }

    /// What `metadata` knows.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Meta {
        /// A directory, not a file.
        pub is_dir: bool,
        /// Bytes, for a file.
        pub len: u64,
        /// Last modified.
        pub modified: SystemTime,
    }

    #[cfg(not(target_arch = "wasm32"))]
    mod imp {
        use super::*;

        pub fn read(p: &Path) -> io::Result<Vec<u8>> {
            std::fs::read(p)
        }
        pub fn write(p: &Path, c: &[u8]) -> io::Result<()> {
            std::fs::write(p, c)
        }
        pub fn create_dir_all(p: &Path) -> io::Result<()> {
            std::fs::create_dir_all(p)
        }
        pub fn remove_file(p: &Path) -> io::Result<()> {
            std::fs::remove_file(p)
        }
        pub fn rename(a: &Path, b: &Path) -> io::Result<()> {
            std::fs::rename(a, b)
        }
        pub fn metadata(p: &Path) -> io::Result<Meta> {
            let m = std::fs::metadata(p)?;
            Ok(Meta {
                is_dir: m.is_dir(),
                len: m.len(),
                modified: m.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            })
        }
        pub fn read_dir(p: &Path) -> io::Result<Vec<Entry>> {
            let mut out = Vec::new();
            for e in std::fs::read_dir(p)?.flatten() {
                let Ok(m) = e.metadata() else { continue };
                out.push(Entry {
                    path: e.path(),
                    is_dir: m.is_dir(),
                    len: m.len(),
                    modified: m.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                });
            }
            Ok(out)
        }
    }

    #[cfg(target_arch = "wasm32")]
    mod imp {
        use super::*;
        use std::cell::RefCell;
        use std::collections::{BTreeMap, BTreeSet};
        use std::path::Component;

        thread_local! {
            static FILES: RefCell<BTreeMap<PathBuf, Vec<u8>>> = const { RefCell::new(BTreeMap::new()) };
            static DIRS: RefCell<BTreeSet<PathBuf>> = const { RefCell::new(BTreeSet::new()) };
        }

        /// `a/./b/../c` is `a/c`; separators are the platform's.
        pub fn norm(p: &Path) -> PathBuf {
            let mut out = PathBuf::new();
            for c in p.components() {
                match c {
                    Component::CurDir => {}
                    Component::ParentDir => {
                        out.pop();
                    }
                    other => out.push(other.as_os_str()),
                }
            }
            out
        }

        fn missing(p: &Path) -> io::Error {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} is not in the pack", p.display()),
            )
        }

        fn is_dir_n(p: &Path) -> bool {
            DIRS.with(|d| d.borrow().contains(p))
                || FILES.with(|f| {
                    f.borrow()
                        .range(p.to_path_buf()..)
                        .next()
                        .is_some_and(|(k, _)| k != p && k.starts_with(p))
                })
        }

        pub fn read(p: &Path) -> io::Result<Vec<u8>> {
            let p = norm(p);
            FILES
                .with(|f| f.borrow().get(&p).cloned())
                .ok_or_else(|| missing(&p))
        }
        pub fn write(p: &Path, c: &[u8]) -> io::Result<()> {
            let p = norm(p);
            FILES.with(|f| f.borrow_mut().insert(p, c.to_vec()));
            Ok(())
        }
        pub fn create_dir_all(p: &Path) -> io::Result<()> {
            let p = norm(p);
            DIRS.with(|d| {
                let mut d = d.borrow_mut();
                for a in p.ancestors() {
                    if !a.as_os_str().is_empty() {
                        d.insert(a.to_path_buf());
                    }
                }
            });
            Ok(())
        }
        pub fn remove_file(p: &Path) -> io::Result<()> {
            let p = norm(p);
            FILES
                .with(|f| f.borrow_mut().remove(&p))
                .map(|_| ())
                .ok_or_else(|| missing(&p))
        }
        pub fn rename(a: &Path, b: &Path) -> io::Result<()> {
            let c = read(a)?;
            remove_file(a)?;
            write(b, &c)
        }
        pub fn metadata(p: &Path) -> io::Result<Meta> {
            let p = norm(p);
            if let Some(len) = FILES.with(|f| f.borrow().get(&p).map(|c| c.len() as u64)) {
                return Ok(Meta {
                    is_dir: false,
                    len,
                    modified: SystemTime::UNIX_EPOCH,
                });
            }
            if is_dir_n(&p) {
                return Ok(Meta {
                    is_dir: true,
                    len: 0,
                    modified: SystemTime::UNIX_EPOCH,
                });
            }
            Err(missing(&p))
        }
        pub fn read_dir(p: &Path) -> io::Result<Vec<Entry>> {
            let p = norm(p);
            if !is_dir_n(&p) {
                return Err(missing(&p));
            }
            let mut names: BTreeMap<PathBuf, (bool, u64)> = BTreeMap::new();
            FILES.with(|f| {
                for (k, c) in f.borrow().range(p.clone()..) {
                    let Ok(rest) = k.strip_prefix(&p) else { break };
                    let mut parts = rest.components();
                    let Some(first) = parts.next() else { continue };
                    let child = p.join(first.as_os_str());
                    let deeper = parts.next().is_some();
                    names
                        .entry(child)
                        .or_insert((deeper, if deeper { 0 } else { c.len() as u64 }));
                }
            });
            DIRS.with(|d| {
                for k in d.borrow().iter() {
                    if k.parent() == Some(p.as_path()) {
                        names.entry(k.clone()).or_insert((true, 0));
                    }
                }
            });
            Ok(names
                .into_iter()
                .map(|(path, (is_dir, len))| Entry {
                    path,
                    is_dir,
                    len,
                    modified: SystemTime::UNIX_EPOCH,
                })
                .collect())
        }

        pub fn mount(root: &Path, files: Vec<(String, Vec<u8>)>) {
            let root = norm(root);
            let _ = create_dir_all(&root);
            FILES.with(|f| {
                let mut f = f.borrow_mut();
                for (rel, bytes) in files {
                    f.insert(norm(&root.join(rel)), bytes);
                }
            });
        }
    }

    /// Read a whole file.
    pub fn read(p: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        imp::read(p.as_ref())
    }
    /// Read a whole file as UTF-8.
    pub fn read_to_string(p: impl AsRef<Path>) -> io::Result<String> {
        String::from_utf8(read(p)?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
    /// Write a whole file, replacing it.
    pub fn write(p: impl AsRef<Path>, c: impl AsRef<[u8]>) -> io::Result<()> {
        imp::write(p.as_ref(), c.as_ref())
    }
    /// Make a directory and its parents.
    pub fn create_dir_all(p: impl AsRef<Path>) -> io::Result<()> {
        imp::create_dir_all(p.as_ref())
    }
    /// Remove a file.
    pub fn remove_file(p: impl AsRef<Path>) -> io::Result<()> {
        imp::remove_file(p.as_ref())
    }
    /// Move a file.
    pub fn rename(a: impl AsRef<Path>, b: impl AsRef<Path>) -> io::Result<()> {
        imp::rename(a.as_ref(), b.as_ref())
    }
    /// What there is at a path.
    pub fn metadata(p: impl AsRef<Path>) -> io::Result<Meta> {
        imp::metadata(p.as_ref())
    }
    /// A directory's entries, sorted by path, so every loader that walks a
    /// tree walks it in the same order on every platform.
    pub fn read_dir(p: impl AsRef<Path>) -> io::Result<Vec<Entry>> {
        let mut v = imp::read_dir(p.as_ref())?;
        v.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(v)
    }
    /// Anything at the path.
    pub fn exists(p: impl AsRef<Path>) -> bool {
        metadata(p).is_ok()
    }
    /// A file at the path.
    pub fn is_file(p: impl AsRef<Path>) -> bool {
        metadata(p).is_ok_and(|m| !m.is_dir)
    }
    /// A directory at the path.
    pub fn is_dir(p: impl AsRef<Path>) -> bool {
        metadata(p).is_ok_and(|m| m.is_dir)
    }

    /// In a browser, put a project's files under `root`: `files` are paths
    /// relative to it, with `/` separators, and their bytes.
    #[cfg(target_arch = "wasm32")]
    pub fn mount(root: impl AsRef<Path>, files: Vec<(String, Vec<u8>)>) {
        imp::mount(root.as_ref(), files)
    }
}
