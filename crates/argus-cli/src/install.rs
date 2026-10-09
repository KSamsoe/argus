//! `argus self-update [version]` and `argus uninstall` (distribution item
//! 6): the installer an install carries beside its `bin/` (`install.ps1`,
//! `install.sh`), run on the install this program is in.
//!
//! On Windows a running program holds its folder, so the installer is
//! started to wait for this one to exit, and this one exits: the installer
//! says when it is done. Elsewhere it runs in place.

use crate::Invocation;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

/// The install this program is in (the folder holding `bin/`) and its
/// installer.
fn install() -> Result<(PathBuf, PathBuf), String> {
    let exe = std::env::current_exe().map_err(|e| format!("this program's path: {e}"))?;
    let exe = exe.canonicalize().unwrap_or(exe);
    let app = exe
        .parent()
        .and_then(|b| b.parent())
        .ok_or("this program is not in an install's bin folder")?
        .to_path_buf();
    let script = app.join(if cfg!(windows) {
        "install.ps1"
    } else {
        "install.sh"
    });
    if !script.is_file() {
        return Err(format!(
            "this argus ({}) is not an installed one: there is no installer beside its bin folder. A checkout updates with git pull and cargo build",
            crate::build::plain(&exe)
        ));
    }
    // The installer puts an install in a folder named app; a staged
    // release run where it was unpacked is not one to update in place.
    if app.file_name().is_none_or(|n| n != "app") {
        return Err(format!(
            "this argus runs from {}, which the installer did not make: install it with {} first",
            crate::build::plain(&app),
            crate::build::plain(&script)
        ));
    }
    Ok((app, script))
}

/// Run the installer with `args`, on the install this program is in.
fn run(args: &[String], what: &str, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let (app, script) = match install() {
        Ok(x) => x,
        Err(e) => {
            let _ = writeln!(err, "argus {what}: {e}");
            return crate::EXIT_USAGE;
        }
    };
    // The installer's own folder (it keeps app, app.old, downloads there).
    let into = app.parent().map(|p| p.to_path_buf()).unwrap_or(app.clone());
    if cfg!(windows) {
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(crate::build::plain(&script))
            .arg("-Into")
            .arg(crate::build::plain(&into))
            .arg("-WaitFor")
            .arg(std::process::id().to_string())
            .args(args);
        match c.spawn() {
            Ok(_) => {
                let _ = writeln!(
                    out,
                    "argus {what}: the installer runs once this argus has exited, and says when it is done. Agent hosts and daemons using this install are stopped by it."
                );
                crate::EXIT_OK
            }
            Err(e) => {
                let _ = writeln!(err, "argus {what}: powershell: {e}");
                crate::EXIT_USAGE
            }
        }
    } else {
        let status = Command::new("sh")
            .arg(&script)
            .arg("--into")
            .arg(&into)
            .args(args)
            .status();
        match status {
            Ok(s) if s.success() => crate::EXIT_OK,
            Ok(_) => crate::EXIT_RPC,
            Err(e) => {
                let _ = writeln!(err, "argus {what}: sh: {e}");
                crate::EXIT_USAGE
            }
        }
    }
}

/// `argus self-update [version]`: the latest release, or that one.
pub fn self_update(
    inv: &Invocation,
    words: &[String],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let version = words.first().or_else(|| inv.args.first());
    let args: Vec<String> = match version {
        Some(v) if cfg!(windows) => vec!["-Version".into(), v.trim_start_matches('v').into()],
        Some(v) => vec!["--version".into(), v.trim_start_matches('v').into()],
        None => Vec::new(),
    };
    run(&args, "self-update", out, err)
}

/// `argus uninstall`: the install out, the projects and settings kept.
pub fn uninstall(out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let args = vec![if cfg!(windows) {
        "-Uninstall".to_string()
    } else {
        "--uninstall".to_string()
    }];
    run(&args, "uninstall", out, err)
}
