//! §15 mods (phase 3 item 8): "A release build can load mod packs —
//! additional defs, scenes, assets, and Luau systems — from a user
//! directory, in a declared load order, with the same sandbox and
//! instruction budgets as the game's own scripts and the same schema
//! validation. Doing this from the start costs a load-order rule and a
//! namespace prefix on mod content ... Games that don't want mods set one
//! flag."
//!
//! A small game made in a temp directory, and mods over it that keep and
//! break each rule: what a mod adds is named with its id first; where two
//! layers give the same thing the later wins, and the change is reported;
//! a mod that breaks a rule, fails the game's validation, or needs one
//! left out is left out whole, and the game runs; a mod's runaway system
//! is stopped by the same budget as the game's; a mod pack from a player's
//! folder is one more layer over the game's packs.

use argus_ecs::layers::codes;
use argus_ecs::Value;
use argus_player::mods::{self, Order, Report};
use argus_player::pack::{self, BuildOptions, Kind, Pack};
use argus_player::{assemble, Assembly, Options, Player};
use argus_scene::ProjectConfig;
use std::path::{Path, PathBuf};

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

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

/// A game: a counter a system counts up, a table of items, a string, a
/// sound, and a scene with one thing in it.
fn game(tag: &str) -> Temp {
    let root = std::env::temp_dir().join(format!("argus-mods-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write(&root, "argus.toml", "name = \"g\"\ndefault_scene = \"s\"\n");
    write(
        &root,
        "components/counter.luau",
        "return component \"Counter\" { n = field.u32 { default = 0 } }\n",
    );
    write(
        &root,
        "systems/count.luau",
        "return system \"sim.count\" { stage = \"sim\", writes = { \"Counter\" }, run = function(q) for _, c in q:each(\"Counter\") do c.n += 1 end end }\n",
    );
    write(
        &root,
        "defs/items.luau",
        "def \"items\" { prefix = \"item\", key = \"id\", id = field.str {}, weight = field.f32 { default = 1, range = {0, 100} }, name = field.str { default = \"\" } }\n",
    );
    write(
        &root,
        "defs/items.toml",
        "[\"sword\"]\nweight = 3\nname = \"Sword\"\n",
    );
    write(&root, "strings/en.toml", "[menu]\ntitle = \"The Game\"\n");
    write(
        &root,
        "audio.toml",
        "[sounds.\"sfx/hit\"]\nvariations = [ { wave = \"square\", freq = 220, ms = 50 } ]\n",
    );
    write(&root, "scenes/s/scene.toml", "seed = 1\n");
    write(&root, "scenes/s/thing.toml", "[Counter]\n");
    Temp(root)
}

fn assemble_with(root: &Path, ids: &[&str]) -> Assembly {
    let cfg = ProjectConfig::load(root).unwrap();
    let (report, _) = mods::choose(
        &cfg,
        &mods::project_dir(root),
        Order::Given(ids.iter().map(|s| s.to_string()).collect()),
    )
    .unwrap();
    assemble(
        root,
        &cfg,
        "s",
        None,
        &Options {
            mods: report,
            ..Options::default()
        },
    )
    .unwrap()
}

fn weight(a: &Assembly, key: &str) -> Option<f64> {
    let t = a.project.defs.table("items")?;
    match t.value(t.get(key)?, "weight")? {
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

fn left_out<'a>(r: &'a Report, id: &str) -> Option<&'a Vec<argus_ecs::ArgusError>> {
    r.left_out.iter().find(|(i, _)| i == id).map(|(_, w)| w)
}

fn modtoml(root: &Path, id: &str, extra: &str) {
    write(
        root,
        &format!("mods/{id}/mod.toml"),
        &format!("id = \"{id}\"\ngame = \"g\"\n{extra}"),
    );
}

#[test]
fn a_mod_adds_under_its_id_and_changes_what_came_before() {
    let root = game("adds");
    modtoml(&root, "a", "version = \"2.1\"\n");
    write(
        &root,
        "mods/a/components/extra.luau",
        "return component \"a_Extra\" { on = field.bool { default = true } }\n",
    );
    write(
        &root,
        "mods/a/systems/tick.luau",
        "return system \"a.tick\" { stage = \"sim\", after = { \"sim.count\" }, writes = { \"Counter\" }, run = function(q) for _, c in q:each(\"Counter\") do c.n += 10 end end }\n",
    );
    write(
        &root,
        "mods/a/defs/items.toml",
        "[\"a.shield\"]\nweight = 5\n\n[\"sword\"]\nweight = 4\n",
    );
    write(
        &root,
        "mods/a/strings/en.toml",
        "[a]\nhello = \"Hello\"\n\n[menu]\ntitle = \"The Game, modded\"\n",
    );
    write(&root, "mods/a/scenes/s/a.lamp.toml", "[a_Extra]\n");
    write(
        &root,
        "mods/a/audio.toml",
        "[sounds.\"a/ding\"]\nvariations = [ { wave = \"sine\", freq = 880, ms = 80 } ]\n",
    );
    write(
        &root,
        "mods/a/assets/a/gem.toml",
        "kind = \"rect\"\nsize = [4, 4]\ncolor = \"#40FF80\"\n",
    );

    let a = assemble_with(&root, &["a"]);
    assert!(a.issues.is_empty(), "{:?}", a.issues);
    assert_eq!(a.mods.ids(), vec!["a"]);
    assert!(a.mods.left_out.is_empty(), "{:?}", a.mods.left_out);
    // Added, in its namespace.
    assert_eq!(weight(&a, "a.shield"), Some(5.0));
    assert!(a.world.component_id("a_Extra").is_some());
    assert!(a.world.entity("@a.lamp").is_ok());
    assert_eq!(a.ui.borrow().strings.get("en", "a.hello"), Some("Hello"));
    assert!(a.assets.entries().contains_key("a/gem"));
    assert_eq!(a.assets.owner("a/gem"), Some("a"));
    assert!(a
        .audio
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap()
        .sound_names()
        .contains(&"a/ding".to_string()));
    // Changed, the later layer winning, and reported with whose it was.
    assert_eq!(weight(&a, "sword"), Some(4.0));
    assert_eq!(
        a.ui.borrow().strings.get("en", "menu.title"),
        Some("The Game, modded")
    );
    let kinds: Vec<(String, String, String)> = a
        .mods
        .changes
        .iter()
        .map(|c| (c.kind.clone(), c.name.clone(), c.over.clone()))
        .collect();
    assert!(
        kinds.contains(&("def row".into(), "item:sword".into(), "the game".into())),
        "{kinds:?}"
    );
    assert!(
        kinds.contains(&("string".into(), "menu.title".into(), "the game".into())),
        "{kinds:?}"
    );
    assert_eq!(kinds.len(), 2, "{kinds:?}");
    let l = &a.mods.loaded[0];
    assert_eq!(l.manifest.version, "2.1");
    assert_eq!(l.adds["scripts"], 2);
    assert_eq!(l.adds["components"], 1);
    assert_eq!(l.adds["def rows"], 1);
    assert_eq!(l.adds["entities"], 1);
    assert_eq!(l.adds["assets"], 1);
    assert_eq!(l.adds["sounds"], 1);
    assert_eq!(l.adds["strings"], 1);

    // The mod's system runs beside the game's, after it.
    let (report, _) = mods::choose(
        &ProjectConfig::load(&root).unwrap(),
        &mods::project_dir(&root),
        Order::Given(vec!["a".into()]),
    )
    .unwrap();
    let mut p = Player::open_with(&root, None, None, report).unwrap();
    p.step(3).unwrap();
    let thing = p.world.entity("@thing").unwrap();
    assert_eq!(
        p.world.get_field(thing, "Counter.n").unwrap(),
        Value::UInt(33)
    );
    // Without the mod, none of it.
    let mut plain = Player::open(&root, None, None).unwrap();
    plain.step(3).unwrap();
    let thing = plain.world.entity("@thing").unwrap();
    assert_eq!(
        plain.world.get_field(thing, "Counter.n").unwrap(),
        Value::UInt(3)
    );
    assert!(plain.world.component_id("a_Extra").is_none());
}

#[test]
fn the_later_mod_wins_and_the_order_is_declared() {
    let root = game("order");
    modtoml(&root, "a", "");
    modtoml(&root, "b", "");
    write(&root, "mods/a/defs/items.toml", "[\"sword\"]\nweight = 4\n");
    write(
        &root,
        "mods/b/defs/items.toml",
        "[\"sword\"]\nweight = 7\nname = \"Big sword\"\n",
    );
    let ab = assemble_with(&root, &["a", "b"]);
    assert_eq!(weight(&ab, "sword"), Some(7.0));
    let ba = assemble_with(&root, &["b", "a"]);
    // a is later: its weight, and b's name (a did not write one).
    assert_eq!(weight(&ba, "sword"), Some(4.0));
    let t = ba.project.defs.table("items").unwrap();
    assert_eq!(
        t.value(t.get("sword").unwrap(), "name"),
        Some(&Value::Str("Big sword".into()))
    );
    let over: Vec<(String, String)> = ba
        .mods
        .changes
        .iter()
        .map(|c| (c.by.clone(), c.over.clone()))
        .collect();
    assert_eq!(
        over,
        vec![
            ("b".to_string(), "the game".to_string()),
            ("a".to_string(), "mod b".to_string())
        ]
    );

    // A player's folder declares the order in load-order.toml; a mod it
    // does not list is left out, and says why.
    let folder = root.join("user-mods");
    for id in ["a", "b"] {
        copy_dir(&root.join("mods").join(id), &folder.join(id));
    }
    write(&folder, "c/mod.toml", "id = \"c\"\n");
    write(&folder, "load-order.toml", "order = [\"b\", \"a\"]\n");
    let cfg = ProjectConfig::load(&root).unwrap();
    let (r, found) = mods::choose(&cfg, &folder, Order::Folder).unwrap();
    assert_eq!(r.rule, "declared");
    assert_eq!(r.ids(), vec!["b", "a"]);
    assert_eq!(found.len(), 3);
    let why = left_out(&r, "c").expect("c is not in the order");
    assert_eq!(why[0].code, codes::LOAD_ORDER);
    // Without the file: every mod, by id, each after what it requires.
    std::fs::remove_file(folder.join("load-order.toml")).unwrap();
    let (r, _) = mods::choose(&cfg, &folder, Order::Folder).unwrap();
    assert_eq!(r.rule, "by id");
    assert_eq!(r.ids(), vec!["a", "b", "c"]);
    write(
        &folder,
        "a/mod.toml",
        "id = \"a\"
game = \"g\"
requires = [\"c\"]
",
    );
    let (r, _) = mods::choose(&cfg, &folder, Order::Folder).unwrap();
    assert_eq!(r.ids(), vec!["b", "c", "a"]);
    // A declared order is the player's: a mod before what it needs is out.
    write(
        &folder,
        "load-order.toml",
        "order = [\"a\", \"c\"]
",
    );
    let (r, _) = mods::choose(&cfg, &folder, Order::Folder).unwrap();
    assert_eq!(r.ids(), vec!["c"]);
    assert!(left_out(&r, "a").unwrap()[0]
        .message
        .contains("needs c, which does not load before it"));
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (a, b) = (e.path(), to.join(e.file_name()));
        if a.is_dir() {
            copy_dir(&a, &b);
        } else {
            std::fs::copy(&a, &b).unwrap();
        }
    }
}

#[test]
fn a_mod_that_breaks_a_rule_is_left_out_whole_and_the_game_runs() {
    let root = game("rules");
    // A system named outside the mod's namespace.
    modtoml(&root, "bad", "");
    write(
        &root,
        "mods/bad/systems/tick.luau",
        "return system \"tick\" { stage = \"sim\", run = function(q) end }\n",
    );
    write(
        &root,
        "mods/bad/defs/items.toml",
        "[\"bad.axe\"]\nweight = 2\n",
    );
    // The game's component, declared again.
    modtoml(&root, "thief", "");
    write(
        &root,
        "mods/thief/components/c.luau",
        "return component \"Counter\" { n = field.u32 {}, m = field.u32 {} }\n",
    );
    // A row the game's schema refuses: the same validation.
    modtoml(&root, "heavy", "");
    write(
        &root,
        "mods/heavy/defs/items.toml",
        "[\"heavy.anvil\"]\nweight = 500\n",
    );
    // A row outside the namespace.
    modtoml(&root, "stray", "");
    write(
        &root,
        "mods/stray/defs/items.toml",
        "[\"axe\"]\nweight = 2\n",
    );
    // Fine on its own, but it needs one left out.
    modtoml(&root, "after", "requires = [\"bad\"]\n");
    // Fine.
    modtoml(&root, "good", "");
    write(
        &root,
        "mods/good/defs/items.toml",
        "[\"good.cup\"]\nweight = 0.5\n",
    );

    let a = assemble_with(&root, &["bad", "thief", "heavy", "stray", "after", "good"]);
    assert!(a.issues.is_empty(), "the game's own issues: {:?}", a.issues);
    assert_eq!(a.mods.ids(), vec!["good"]);
    assert_eq!(weight(&a, "good.cup"), Some(0.5));
    assert_eq!(
        weight(&a, "bad.axe"),
        None,
        "a mod left out is left out whole"
    );
    let bad = left_out(&a.mods, "bad").unwrap();
    assert!(
        bad.iter()
            .any(|e| e.message.contains("\"tick\" is outside its namespace")
                && e.r#ref
                    .as_deref()
                    .is_some_and(|r| r.starts_with("mods/bad/systems/tick.luau"))),
        "{bad:?}"
    );
    let thief = left_out(&a.mods, "thief").unwrap();
    assert!(
        thief.iter().any(|e| e
            .message
            .contains("component \"Counter\", which is the game's")),
        "{thief:?}"
    );
    let heavy = left_out(&a.mods, "heavy").unwrap();
    assert!(
        heavy.iter().any(|e| e
            .r#ref
            .as_deref()
            .is_some_and(|r| r.starts_with("mods/heavy/defs/items.toml:"))),
        "the schema's refusal, at the mod's row: {heavy:?}"
    );
    let stray = left_out(&a.mods, "stray").unwrap();
    assert_eq!(stray[0].code, codes::NAMESPACE, "{stray:?}");
    let after = left_out(&a.mods, "after").unwrap();
    assert!(
        after[0].message.contains("needs bad, which was left out"),
        "{after:?}"
    );
    // The game itself is as it was.
    assert_eq!(weight(&a, "sword"), Some(3.0));
    assert!(a.world.entity("@thing").is_ok());
}

#[test]
fn a_mods_runaway_system_is_stopped_by_the_same_budget_as_the_games() {
    let root = game("budget");
    modtoml(&root, "loop", "");
    write(
        &root,
        "mods/loop/systems/spin.luau",
        "return system \"loop.spin\" { stage = \"sim\", run = function(q) local n = 0 while true do n += 1 end end }\n",
    );
    let (report, _) = mods::choose(
        &ProjectConfig::load(&root).unwrap(),
        &mods::project_dir(&root),
        Order::Given(vec!["loop".into()]),
    )
    .unwrap();
    let mut p = Player::open_with(&root, None, None, report).unwrap();
    assert_eq!(
        p.mods.ids(),
        vec!["loop"],
        "it loads: the loop is only found running"
    );
    let t = std::time::Instant::now();
    p.step(5).unwrap();
    assert!(t.elapsed().as_secs() < 20, "the budget stopped it");
    let systems = p.sched.systems(p.world.registry()).unwrap();
    let spin = systems.iter().find(|s| s.name == "loop.spin").unwrap();
    assert!(!spin.enabled);
    assert!(
        spin.disabled_reason
            .as_deref()
            .is_some_and(|r| r.contains("instruction budget")),
        "{:?}",
        spin.disabled_reason
    );
    // The game's own system kept counting.
    let thing = p.world.entity("@thing").unwrap();
    assert_eq!(
        p.world.get_field(thing, "Counter.n").unwrap(),
        Value::UInt(5)
    );
}

#[test]
fn a_mod_for_another_game_or_before_what_it_needs_is_left_out() {
    let root = game("plan");
    modtoml(&root, "base", "");
    write(
        &root,
        "mods/other/mod.toml",
        "id = \"other\"\ngame = \"not-g\"\n",
    );
    modtoml(&root, "top", "requires = [\"base\"]\n");
    let cfg = ProjectConfig::load(&root).unwrap();
    let dir = mods::project_dir(&root);
    let (r, _) = mods::choose(
        &cfg,
        &dir,
        Order::Given(vec!["top".into(), "base".into(), "other".into()]),
    )
    .unwrap();
    assert_eq!(r.ids(), vec!["base"]);
    let top = left_out(&r, "top").unwrap();
    assert!(
        top[0]
            .message
            .contains("needs base, which does not load before it"),
        "{top:?}"
    );
    assert!(left_out(&r, "other").unwrap()[0]
        .message
        .contains("a mod for \"not-g\""));
    let (r, _) = mods::choose(&cfg, &dir, Order::Given(vec!["base".into(), "top".into()])).unwrap();
    assert_eq!(r.ids(), vec!["base", "top"]);
    // A directory whose mod.toml names another id is not read as that mod.
    write(&root, "mods/wrong/mod.toml", "id = \"right\"\n");
    let (found, problems) = mods::find(&dir);
    assert!(found.iter().all(|f| f.manifest.id != "right"));
    assert!(problems
        .iter()
        .any(|(id, e)| id == "wrong" && e.code == codes::BAD_MOD));
}

#[test]
fn games_that_dont_want_mods_set_one_flag() {
    let root = game("off");
    write(
        &root,
        "argus.toml",
        "name = \"g\"\ndefault_scene = \"s\"\n\n[mods]\nenabled = false\n",
    );
    modtoml(&root, "a", "");
    let cfg = ProjectConfig::load(&root).unwrap();
    assert!(!cfg.mods_enabled);
    let (r, found) = mods::choose(&cfg, &mods::project_dir(&root), Order::Folder).unwrap();
    assert!(!r.enabled);
    assert!(
        r.loaded.is_empty() && found.is_empty(),
        "the folder is not even read"
    );
    let e = mods::choose(
        &cfg,
        &mods::project_dir(&root),
        Order::Given(vec!["a".into()]),
    )
    .unwrap_err();
    assert_eq!(e.code, codes::MODS_OFF);
}

#[test]
fn a_mod_pack_from_a_players_folder_is_one_more_layer_over_the_games_packs() {
    let root = game("pack");
    modtoml(&root, "a", "version = \"1\"\n");
    write(
        &root,
        "mods/a/systems/tick.luau",
        "return system \"a.tick\" { stage = \"sim\", after = { \"sim.count\" }, writes = { \"Counter\" }, run = function(q) for _, c in q:each(\"Counter\") do c.n += 10 end end }\n",
    );
    write(
        &root,
        "mods/a/defs/items.toml",
        "[\"a.shield\"]\nweight = 5\n",
    );
    write(
        &root,
        "mods/a/assets/a/gem.toml",
        "kind = \"rect\"\nsize = [4, 4]\ncolor = \"#40FF80\"\n",
    );
    write(&root, "mods/a/tools/make.py", "print('not shipped')\n");

    // The game's packs never hold its mods.
    let game_packs = pack::build(&root, &BuildOptions::default()).unwrap();
    assert!(game_packs.packs[0]
        .files
        .iter()
        .all(|(p, _, _)| !p.starts_with("mods/")));
    // The mod's pack: its files, the script compiled, the sprite from the
    // import cache, its tools left out.
    let built = pack::build_mod(&root, "a", &BuildOptions::default()).unwrap();
    let mp = &built.packs[0];
    assert_eq!(mp.kind, Kind::Mod("a".into()));
    assert_eq!(mp.file_name(), "a.pack");
    let names: Vec<&str> = mp.files.iter().map(|(p, _, _)| p.as_str()).collect();
    assert!(names.contains(&"mod.toml"));
    assert!(names.contains(&"systems/tick.luau"));
    assert!(names.contains(&argus_assets::CATALOG));
    assert!(!names.iter().any(|n| n.starts_with("tools/")));
    assert!(
        !names.contains(&"assets/a/gem.toml"),
        "the sprite's sidecar is its source: {names:?}"
    );
    assert_eq!(built.report["scripts_compiled"], 1);
    let bytes = mp.encode();
    assert_eq!(Pack::decode(&bytes).unwrap(), *mp);

    // As a release build has it: the game's packs mounted in memory, the
    // mod pack in a player's folder, mounted over them.
    let data = root.join("dist-data");
    std::fs::create_dir_all(&data).unwrap();
    for p in &game_packs.packs {
        std::fs::write(data.join(p.file_name()), p.encode()).unwrap();
    }
    let folder = root.join("player-mods");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("a.pack"), &bytes).unwrap();
    let mem = root.join("mounted-game");
    pack::mount_dir(&data, &mem, None).unwrap();
    let cfg = ProjectConfig::load(&mem).unwrap();
    let (report, found) = mods::choose(&cfg, &folder, Order::Folder).unwrap();
    assert_eq!(report.ids(), vec!["a"]);
    assert_eq!(report.loaded[0].kind, "pack");
    // The pack is the same mod as the directory it was built from.
    let (dir_found, _) = mods::find(&mods::project_dir(&root));
    assert_eq!(found[0].hash, dir_found[0].hash);
    mods::mount(&mem, &found, &report.ids()).unwrap();
    let mut from_pack = Player::open_with(&mem, None, None, report).unwrap();
    assert!(
        from_pack.mods.left_out.is_empty(),
        "{:?}",
        from_pack.mods.left_out
    );
    assert!(
        from_pack.assets.entries().contains_key("a/gem"),
        "the sprite from the mod's catalog"
    );
    from_pack.step(3).unwrap();

    // The same world as the project with the mod's directory.
    let (report, _) = mods::choose(
        &ProjectConfig::load(&root).unwrap(),
        &mods::project_dir(&root),
        Order::Given(vec!["a".into()]),
    )
    .unwrap();
    let mut from_dir = Player::open_with(&root, None, None, report).unwrap();
    from_dir.step(3).unwrap();
    assert_eq!(from_pack.digest(), from_dir.digest());
    let thing = from_pack.world.entity("@thing").unwrap();
    assert_eq!(
        from_pack.world.get_field(thing, "Counter.n").unwrap(),
        Value::UInt(33)
    );
    // A replay, a save, or a mark says which mods it needs.
    let saves = {
        let mark = from_pack.mark("", "test");
        mark["mods"].clone()
    };
    assert_eq!(saves[0]["id"], "a");
    assert_eq!(saves[0]["hash"], found[0].hash.as_str());
}
