//! Where a project's daemon listens: a local socket (a Windows named pipe
//! or a Unix domain socket) whose name is a function of the project root,
//! and `.argus/daemon.json`, which the daemon writes with that name and
//! its pid when it starts listening.

use interprocess::local_socket::{
    prelude::*, GenericFilePath, GenericNamespaced, Name, Stream, ToFsName, ToNsName,
};
use serde_json::Value as Json;
use std::path::{Path, PathBuf};

/// The socket name for a project: a namespaced name where the platform
/// has them (Windows pipes, Linux abstract sockets), else a file under
/// `.argus/`.
pub fn socket_name_for(root: &Path) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in root.to_string_lossy().as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    if GenericNamespaced::is_supported() {
        format!("argusd-{h:016x}.sock")
    } else {
        root.join(".argus")
            .join("argusd.sock")
            .to_string_lossy()
            .into_owned()
    }
}

/// A socket name as the platform takes it.
pub fn to_name(name: &str) -> std::io::Result<Name<'_>> {
    if GenericNamespaced::is_supported() && !name.contains('/') && !name.contains('\\') {
        name.to_ns_name::<GenericNamespaced>()
    } else {
        name.to_fs_name::<GenericFilePath>()
    }
}

/// Connect to a daemon by socket name.
pub fn connect(name: &str) -> std::io::Result<Stream> {
    Stream::connect(to_name(name)?)
}

/// `.argus/daemon.json` of a project.
pub fn daemon_info_path(root: &Path) -> PathBuf {
    root.join(".argus").join("daemon.json")
}

/// What `.argus/daemon.json` says, if it exists and parses: the socket
/// name, the pid, and the daemon's version.
pub fn daemon_info(root: &Path) -> Option<Json> {
    let text = std::fs::read_to_string(daemon_info_path(root)).ok()?;
    serde_json::from_str(&text).ok()
}
