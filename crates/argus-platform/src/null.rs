//! The null platform: everything a platform is told, kept in memory, its
//! cloud in memory or a folder. What the daemon, the web page, the tests,
//! and a release without Steam run on, so a game's achievements, presence,
//! and cloud saves are tested without Steam present.

use crate::{codes, CloudFile, Platform};
use argus_ecs::{ArgusError, Result};
use serde_json::{json, Value as Json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// What the null platform remembers of one request, for `platform.status`.
const LOG_KEEP: usize = 64;

/// The null platform.
#[derive(Clone, Debug, Default)]
pub struct NullPlatform {
    achieved: BTreeSet<String>,
    progress: BTreeMap<String, (u32, u32)>,
    presence: BTreeMap<String, String>,
    cloud_dir: Option<PathBuf>,
    cloud: BTreeMap<String, Vec<u8>>,
    overlay: bool,
    pages: Vec<String>,
    log: Vec<String>,
}

impl NullPlatform {
    /// A null platform whose cloud is in memory.
    pub fn new() -> NullPlatform {
        NullPlatform::default()
    }

    /// A null platform whose cloud is a folder: two that share one are two
    /// machines sharing a player's cloud.
    pub fn with_cloud(dir: PathBuf) -> NullPlatform {
        NullPlatform {
            cloud_dir: Some(dir),
            ..Default::default()
        }
    }

    /// Open or close the overlay, as a player's Shift+Tab would (for
    /// tests: the null platform has no overlay of its own).
    pub fn set_overlay(&mut self, open: bool) {
        self.overlay = open;
    }

    /// The overlay pages asked for, oldest first.
    pub fn pages(&self) -> &[String] {
        &self.pages
    }

    /// Rich presence as it stands.
    pub fn presence_now(&self) -> &BTreeMap<String, String> {
        &self.presence
    }

    fn note(&mut self, s: String) {
        self.log.push(s);
        if self.log.len() > LOG_KEEP {
            self.log.remove(0);
        }
    }

    fn path(&self, name: &str) -> Result<Option<PathBuf>> {
        if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
            return Err(ArgusError::new(
                codes::BAD_REQUEST,
                format!("{name:?} is not a cloud file's name: one name, no folders"),
            ));
        }
        Ok(self.cloud_dir.as_ref().map(|d| d.join(name)))
    }
}

fn io(name: &str, e: std::io::Error) -> ArgusError {
    ArgusError::new(codes::PLATFORM, format!("the cloud's {name}: {e}"))
}

impl Platform for NullPlatform {
    fn name(&self) -> &'static str {
        "null"
    }

    fn unlock(&mut self, api: &str) -> Result<bool> {
        let new = self.achieved.insert(api.to_string());
        self.note(format!("unlock {api}"));
        Ok(new)
    }

    fn achieved(&mut self, api: &str) -> Result<bool> {
        Ok(self.achieved.contains(api))
    }

    fn clear_achievement(&mut self, api: &str) -> Result<()> {
        self.achieved.remove(api);
        self.progress.remove(api);
        self.note(format!("clear {api}"));
        Ok(())
    }

    fn progress(&mut self, api: &str, current: u32, max: u32) -> Result<()> {
        self.progress.insert(api.to_string(), (current, max));
        self.note(format!("progress {api} {current}/{max}"));
        Ok(())
    }

    fn presence(&mut self, key: &str, value: Option<&str>) -> Result<()> {
        match value {
            Some(v) => {
                self.presence.insert(key.to_string(), v.to_string());
            }
            None => {
                self.presence.remove(key);
            }
        }
        self.note(format!("presence {key} = {value:?}"));
        Ok(())
    }

    fn cloud_enabled(&self) -> bool {
        true
    }

    fn cloud_write(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        match self.path(name)? {
            Some(p) => {
                if let Some(d) = p.parent() {
                    argus_os::fs::create_dir_all(d).map_err(|e| io(name, e))?;
                }
                argus_os::fs::write(&p, bytes).map_err(|e| io(name, e))?;
            }
            None => {
                self.cloud.insert(name.to_string(), bytes.to_vec());
            }
        }
        self.note(format!("cloud write {name} ({} bytes)", bytes.len()));
        Ok(())
    }

    fn cloud_read(&mut self, name: &str) -> Result<Option<Vec<u8>>> {
        match self.path(name)? {
            Some(p) if argus_os::fs::is_file(&p) => {
                argus_os::fs::read(&p).map(Some).map_err(|e| io(name, e))
            }
            Some(_) => Ok(None),
            None => Ok(self.cloud.get(name).cloned()),
        }
    }

    fn cloud_delete(&mut self, name: &str) -> Result<()> {
        match self.path(name)? {
            Some(p) => {
                if argus_os::fs::is_file(&p) {
                    argus_os::fs::remove_file(&p).map_err(|e| io(name, e))?;
                }
            }
            None => {
                self.cloud.remove(name);
            }
        }
        self.note(format!("cloud delete {name}"));
        Ok(())
    }

    fn cloud_files(&mut self) -> Vec<CloudFile> {
        match &self.cloud_dir {
            Some(d) => {
                let mut v: Vec<CloudFile> = argus_os::fs::read_dir(d)
                    .map(|entries| {
                        entries
                            .into_iter()
                            .filter(|e| !e.is_dir)
                            .map(|e| CloudFile {
                                name: e
                                    .path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default(),
                                size: e.len,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                v.sort_by(|a, b| a.name.cmp(&b.name));
                v
            }
            None => self
                .cloud
                .iter()
                .map(|(n, b)| CloudFile {
                    name: n.clone(),
                    size: b.len() as u64,
                })
                .collect(),
        }
    }

    fn open_overlay(&mut self, page: &str) -> Result<()> {
        self.pages.push(page.to_string());
        self.note(format!("overlay {page}"));
        Ok(())
    }

    fn overlay_open(&self) -> bool {
        self.overlay
    }

    fn status(&mut self) -> Json {
        let files = self.cloud_files();
        json!({
            "achieved": self.achieved,
            "progress": self.progress.iter().map(|(k, (c, m))| (k.clone(), json!([c, m]))).collect::<serde_json::Map<_, _>>(),
            "presence": self.presence,
            "cloud": {
                "where": self.cloud_dir.as_ref().map(|d| d.display().to_string()),
                "files": files.iter().map(|f| json!({ "name": f.name, "size": f.size })).collect::<Vec<_>>(),
            },
            "overlay": { "open": self.overlay, "pages": self.pages },
            "log": self.log,
        })
    }
}
