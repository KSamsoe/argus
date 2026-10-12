//! Distribution item 3: `argus mcp setup <client>` puts the server in each
//! agent host's own file, merged with what is there, idempotent, with a
//! backup. A stand-in home (`ARGUS_SETUP_HOME`) keeps the real ones out.

use serde_json::Value as Json;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A temp directory removed on drop, used as its path.
struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl std::ops::Deref for Temp {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

fn temp(name: &str) -> Temp {
    let d = std::env::temp_dir().join(format!("argus-setup-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    Temp(d)
}

/// A home and a project in it, and the guard that removes them.
fn world(name: &str) -> (Temp, PathBuf, PathBuf) {
    let base = temp(name);
    let home = base.join("home");
    let project = base.join("game");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("argus.toml"),
        "name = \"game\"\nclock = \"continuous@60\"\n",
    )
    .unwrap();
    (base, home, project)
}

fn setup(home: &Path, project: &Path, args: &[&str]) -> Json {
    let o = Command::new(env!("CARGO_BIN_EXE_argus"))
        .args(["--json", "mcp", "setup"])
        .args(args)
        .current_dir(project)
        .env("ARGUS_SETUP_HOME", home)
        .env_remove("ARGUS_PROJECT")
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    serde_json::from_slice(&o.stdout).unwrap()
}

fn read(p: &Path) -> Json {
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn plain(p: &Path) -> String {
    p.canonicalize()
        .unwrap()
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_string()
}

#[test]
fn each_client_gets_its_own_project_file_and_key() {
    let (_t, home, game) = world("project");
    for (client, file, key) in [
        ("claude-code", ".mcp.json", "mcpServers"),
        ("cursor", ".cursor/mcp.json", "mcpServers"),
        ("vscode", ".vscode/mcp.json", "servers"),
        ("gemini", ".gemini/settings.json", "mcpServers"),
    ] {
        let r = setup(&home, &game, &[client]);
        assert_eq!(r["scope"], "project", "{r}");
        assert_eq!(r["files"][0]["status"], "added", "{r}");
        let v = read(&game.join(file));
        let e = &v[key]["argus"];
        assert_eq!(e["args"][0], "mcp", "{client}: {v}");
        assert_eq!(e["args"][1], "--project", "{client}: {v}");
        assert_eq!(e["args"][2], plain(&game), "{client}: {v}");
        assert!(
            e["command"].as_str().unwrap().contains("argus"),
            "{client}: {v}"
        );
        // Again: nothing changes.
        let again = setup(&home, &game, &[client]);
        assert_eq!(
            again["files"][0]["status"], "unchanged",
            "{client}: {again}"
        );
    }
    let toml = std::fs::read_to_string(game.join(".codex/config.toml"));
    assert!(toml.is_err(), "codex was not asked for");
    let r = setup(&home, &game, &["codex"]);
    assert_eq!(r["files"][0]["status"], "added", "{r}");
    let toml = std::fs::read_to_string(game.join(".codex/config.toml")).unwrap();
    assert!(
        toml.starts_with("[mcp_servers.argus]\ncommand = '"),
        "{toml}"
    );
    assert!(
        toml.contains(&format!("args = ['mcp', '--project', '{}']", plain(&game))),
        "{toml}"
    );
}

#[test]
fn a_file_with_other_servers_keeps_them_and_is_backed_up() {
    let (_t, home, game) = world("merge");
    let file = game.join(".mcp.json");
    let before = "{\n    \"mcpServers\": {\n        \"other\": { \"command\": \"x\" }\n    },\n    \"zeta\": true\n}\n";
    std::fs::write(&file, before).unwrap();
    let r = setup(&home, &game, &["claude-code"]);
    assert_eq!(r["files"][0]["status"], "added", "{r}");
    let backup = PathBuf::from(r["files"][0]["backup"].as_str().unwrap());
    assert_eq!(std::fs::read_to_string(backup).unwrap(), before);
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.ends_with("    },\n    \"zeta\": true\n}\n"), "{text}");
    let v = read(&file);
    assert_eq!(v["mcpServers"]["other"]["command"], "x");
    assert_eq!(v["mcpServers"]["argus"]["type"], "stdio");
}

#[test]
fn a_user_entry_opens_no_project_unless_one_is_named() {
    let (_t, home, game) = world("user");
    let r = setup(&home, &game, &["cursor", "--scope", "user"]);
    assert_eq!(r["scope"], "user", "{r}");
    let v = read(&home.join(".cursor/mcp.json"));
    assert_eq!(
        v["mcpServers"]["argus"]["args"],
        serde_json::json!(["mcp"]),
        "{v}"
    );
    let r = setup(
        &home,
        &game,
        &[
            "cursor",
            "--scope",
            "user",
            "--project",
            game.to_str().unwrap(),
        ],
    );
    assert_eq!(r["files"][0]["status"], "updated", "{r}");
    let v = read(&home.join(".cursor/mcp.json"));
    assert_eq!(v["mcpServers"]["argus"]["args"][2], plain(&game), "{v}");
}

#[test]
fn claude_desktop_is_user_scope_and_writes_the_store_apps_copy_too() {
    let (_t, home, game) = world("desktop");
    let (roaming, local) = if cfg!(windows) {
        (home.join("AppData/Roaming"), home.join("AppData/Local"))
    } else {
        (home.join(".config"), home.join(".local/share"))
    };
    if cfg!(windows) {
        std::fs::create_dir_all(local.join("Packages/Claude_abc123")).unwrap();
    }
    let r = setup(&home, &game, &["claude-desktop"]);
    assert_eq!(r["scope"], "user", "{r}");
    let v = read(&roaming.join("Claude/claude_desktop_config.json"));
    assert_eq!(
        v["mcpServers"]["argus"]["args"],
        serde_json::json!(["mcp"]),
        "{v}"
    );
    assert!(v["mcpServers"]["argus"].get("type").is_none(), "{v}");
    if cfg!(windows) {
        let store = local
            .join("Packages/Claude_abc123/LocalCache/Roaming/Claude/claude_desktop_config.json");
        assert_eq!(
            read(&store)["mcpServers"]["argus"],
            v["mcpServers"]["argus"]
        );
        assert_eq!(r["files"].as_array().unwrap().len(), 2, "{r}");
    }
    let o = Command::new(env!("CARGO_BIN_EXE_argus"))
        .args(["mcp", "setup", "claude-desktop", "--scope", "project"])
        .current_dir(&game)
        .env("ARGUS_SETUP_HOME", &home)
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("no project file"));
}

#[test]
fn with_no_client_it_lists_them_and_which_are_here() {
    let (_t, home, game) = world("list");
    std::fs::create_dir_all(home.join(".gemini")).unwrap();
    let r = setup(&home, &game, &[]);
    let list = r["clients"].as_array().unwrap();
    let names: Vec<&str> = list.iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "claude-code",
            "claude-desktop",
            "cursor",
            "vscode",
            "codex",
            "gemini"
        ]
    );
    for c in list {
        assert_eq!(c["found"], c["name"] == "gemini", "{c}");
    }
}

#[test]
fn a_gemini_project_gets_a_gemini_md_that_imports_agents_md() {
    let (_t, home, game) = world("gemini");
    std::fs::write(game.join("AGENTS.md"), "# Working on game\n").unwrap();
    setup(&home, &game, &["gemini"]);
    assert_eq!(
        std::fs::read_to_string(game.join("GEMINI.md")).unwrap(),
        "@AGENTS.md\n"
    );
}
