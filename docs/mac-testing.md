# Testing Argus on a Mac

This is for an AI agent with a shell on a Mac (Claude Code, Cursor, Codex, or another). The person whose Mac it is gave it to you. Argus is a 2D game engine you build games in with an AI. Until now it has run only on Windows and Linux, and this is the first build for the Mac. It was cross-compiled on Linux and has never run on a Mac. Your job is to find out what works.

## How to work

- **Go through the checks in order and run every one.** A check you cannot run is SKIPPED, with the reason; never leave one out. The early ones are cheap and tell you whether the later ones can work.
- **Write down what happened, not what should have.** Keep each check's commands and their full output in `~/argus-mac-test/log/<check>.txt`, for example with `2>&1 | tee -a ~/argus-mac-test/log/2.1.txt`. When something fails, the output is the most useful thing you can send.
- **Don't fix Argus, and don't work around a failure** in a way that hides it. Record it and go on. If a failure blocks the checks after it, say which.
- **Ask the person first** before you install anything (Homebrew, the GitHub CLI, Rust, Rosetta), change a system setting, or close their programs. Steps marked **(person)** need them to look at a window or listen.
- About an hour, most of it the example games' scenarios (section 5), which can run while you do other things.

Make the folders first:

```sh
mkdir -p ~/argus-mac-test/log ~/argus-mac-test/runs ~/argus-mac-test/frames ~/argus-mac-test/games
cd ~/argus-mac-test
```

## 0. The machine

```sh
sw_vers
uname -m
sysctl -n machdep.cpu.brand_string
sysctl -n hw.memsize
system_profiler SPDisplaysDataType | grep -E "Chipset Model|Metal|Resolution"
xcode-select -p; echo "developer tools: exit $?"
/usr/bin/arch -x86_64 /usr/bin/true 2>/dev/null && echo "Rosetta: yes" || echo "Rosetta: no"
```

Put all of it in the report. On an Intel Mac `uname -m` says `x86_64`, and the Rosetta line does not apply.

## 1. The release

You need the GitHub CLI, signed in as someone who can read the private repository `KSamsoe/argus` (the person was invited):

```sh
gh --version && gh auth status
```

If `gh` is missing, ask the person whether to install it (`brew install gh`, or from https://cli.github.com). Then:

```sh
gh release download v0.0.5 --repo KSamsoe/argus \
  --pattern install.sh --pattern SHA256SUMS --pattern mac-expected.json --pattern mac-testing.md \
  --pattern 'argus-0.0.5-macos-universal.tar.gz' --pattern 'example-games-0.0.5-macos-universal.tar.gz' \
  --pattern 'argus-0.0.5-macos.mcpb'
grep -E " (install.sh|argus-0.0.5-macos-universal.tar.gz|example-games-0.0.5-macos-universal.tar.gz|argus-0.0.5-macos.mcpb)$" SHA256SUMS | shasum -a 256 -c -
```

**1.1** All four files are `OK`. (`mac-expected.json` and this file are not in `SHA256SUMS`.)

Then the source at that version. The example projects in section 5 come from it:

```sh
gh api repos/KSamsoe/argus/tarball/v0.0.5 > source.tar.gz
mkdir -p source && tar -xzf source.tar.gz -C source --strip-components 1
ls source/examples
```

**1.2** It lists `delve hollow outpost puppet swarm toybox`.

## 2. Installing

```sh
sh install.sh --version 0.0.5
export PATH="$HOME/.local/bin:$PATH"
argus version
```

**2.1** The installer ends `installed argus 0.0.5 (...)`, and `argus version` prints `argus 0.0.5 (<revision>, macos aarch64)` (`macos x86_64` on an Intel Mac).

If it fails, the words matter:
- `Killed: 9` means the signature was refused;
- `dyld: Library not loaded` or `Symbol not found` means a library or symbol is missing;
- "cannot be opened" means Gatekeeper refused it.

Record which one, then try `~/.local/share/argus/app/bin/argus version` directly.

**2.2** The signatures. Apple's tools see what the build's checks could only compute:

```sh
for f in ~/.local/share/argus/app/bin/argus ~/.local/share/argus/app/bin/argus-editor \
         ~/.local/share/argus/app/runtime/macos/standard/argus-game \
         ~/.local/share/argus/app/runtime/macos/steam/argus-game \
         ~/.local/share/argus/app/runtime/macos/steam/libsteam_api.dylib; do
  echo "== $f"; codesign -dv "$f" 2>&1 | grep -E "Format|Signature|Identifier"; codesign --verify --strict "$f" && echo "verify: ok"
done
```

Each says `verify: ok`, and its format is `Mach-O universal (x86_64 arm64)`. Argus's own programs say `Signature=adhoc`. Steam's library is signed by Valve.

**2.3** The other architecture. On Apple Silicon with Rosetta (from section 0) only; otherwise SKIPPED, and do not install Rosetta for this:

```sh
arch -x86_64 ~/.local/share/argus/app/bin/argus version
```

It says `macos x86_64`. This is the part an Intel Mac runs.

**2.4** A download from a browser. A browser marks what it downloads, and Gatekeeper refuses marked programs that Apple has not notarised (this build is not). The installer clears the mark, and this checks that it does:

```sh
cp argus-0.0.5-macos-universal.tar.gz marked.tar.gz
xattr -w com.apple.quarantine "0083;00000000;Safari;" marked.tar.gz
sh install.sh --archive marked.tar.gz && argus version
xattr -lr ~/.local/share/argus/app | grep -c quarantine
```

It installs and runs, and the count is `0`.

**2.5** The docs are inside the program:

```sh
argus docs list | head
argus docs agents | head -20
```

Both print text. `argus docs agents` is the chapter for agents; read it now, since it explains the rest of what you will run.

## 3. New games

**3.1** One game from each template. `argus new` runs the new game's scenarios before it says it is done:

```sh
for t in blank platformer roguelike arena colony; do
  argus new ~/argus-mac-test/games/$t --template $t --json > ~/argus-mac-test/log/new-$t.json; echo "$t: exit $?"
done
grep -h '"passed"' ~/argus-mac-test/log/new-*.json
```

Each exits 0, with every scenario passed (`tests.passed` equals `tests.total`).

**3.2** git. Each game's `git` field is `true` when the developer tools are installed (section 0), else `false`. **(person)** Without the developer tools, did a dialog appear offering to install them? It should not have.

**3.3** A path with a space:

```sh
argus new "$HOME/argus-mac-test/games/with space" --json | grep -E '"passed"|"total"'
```

## 4. The daemon

A game runs in a daemon that holds its world. Every `argus` command and the editor talk to it.

```sh
cd ~/argus-mac-test/games/platformer
argus daemon start
argus daemon status
argus world summary
argus sim step n=120
argus daemon stop
argus daemon start && argus daemon status && argus daemon stop
pgrep -fl "argus" || echo "none left"
cd ~/argus-mac-test
```

**4.1** Each command succeeds. The second `daemon start` matters most. A Mac's daemon socket is a file, and this build fixed one that a stopped daemon left behind, which made the second start fail with "address in use".

**4.2** After the last stop, no `argus` process is left for this game. Another program's `argus mcp` is fine.

## 5. The example games' scenarios, and whether a Mac plays them the same

Each example project has scenarios: scripted playthroughs with checks. Each run ends on a digest of the whole world. The games are made to play the same on every machine, and on Windows, Linux, and in Chrome they give the same digests. Whether a Mac does too is the question here.

```sh
cd ~/argus-mac-test/source
for g in hollow delve outpost toybox puppet; do
  argus --project examples/$g test --json > ~/argus-mac-test/runs/$g.json; echo "$g: exit $?"
  argus --project examples/$g daemon stop > /dev/null 2>&1
done
cd ~/argus-mac-test
```

Outpost is the longest, about a quarter of an hour. Swarm is in section 12; it needs Rust.

**5.1** Every scenario passes, except:
- Hollow's two picture scenarios ("the lit hollow looks as it did", "the cave and the hall look as they did") may fail. They compare a frame with a picture drawn on Windows by Windows' software rasteriser. Linux differs from it at text and sprite edges by about 0.5% of pixels, and a Mac (which draws with its GPU) may too. Record each one's percentage from its message, and copy the pictures it names (under `examples/hollow/.argus/test-images/`) into `~/argus-mac-test/frames/`.
- Puppet's scenarios pass, but their digests are expected to differ between machines.

Record any other failure in full.

**5.2** The digests. `mac-expected.json` has, for each game, scenario, and seed, the digest Windows ended on. Each row in `runs/<game>.json` has the same three and the digest the Mac ended on. Compare them. For each game, record how many runs match and list the ones that do not (game, scenario, seed, both digests). Hollow, Delve, Outpost, and Toybox are expected to match; Puppet is not.

A Mac has no Python until Apple's developer tools are installed, and `jq` only from macOS 15. If you have neither, `osascript -l JavaScript` runs JavaScript and can read the files (`ObjC.import('Foundation')`, then `$.NSString.stringWithContentsOfFileEncodingError`), or write a few lines of shell. Or read them yourself. Say which way you compared.

**5.3** On Apple Silicon with Rosetta only, else SKIPPED. The Intel part on one game. Scenarios run in the project's daemon, so the daemon is the one started as Intel (`daemon run` serves in its own process, with no window):

```sh
cd ~/argus-mac-test/source
arch -x86_64 ~/.local/share/argus/app/bin/argus --project examples/delve daemon run > ~/argus-mac-test/log/5.3-daemon.txt 2>&1 &
sleep 5
argus --project examples/delve daemon status
argus --project examples/delve test --json > ~/argus-mac-test/runs/delve-x86_64.json
argus --project examples/delve daemon stop
cd ~/argus-mac-test
```

`daemon status` names the daemon's process. If you can, confirm it ran as Intel: Activity Monitor's Kind column says "Intel" for it, or ask the person to look. Compare the run's digests with the arm64 run's and with `mac-expected.json`.

## 6. Pictures

```sh
cd ~/argus-mac-test/games/platformer
argus obs frame path=$HOME/argus-mac-test/frames/platformer.png --json | tee ~/argus-mac-test/log/6.1.json
argus obs frame path=$HOME/argus-mac-test/frames/platformer-grey.png see_as=achromatopsia --json > ~/argus-mac-test/log/6.2.json
argus obs frame path=$HOME/argus-mac-test/frames/platformer-software.png software=true --json > ~/argus-mac-test/log/6.3.json
argus obs frame semantic --json > ~/argus-mac-test/log/6.4.json
argus daemon stop
cd ~/argus-mac-test
```

**6.1** A PNG is written, with `sprites` above 0 and an `adapter` naming the Mac's GPU and Metal. Look at the picture if you can read images, and say what you see: a small level with a hero, platforms, and a flag.

**6.2** The grey one is the same scene without colour.

**6.3** `software=true` also draws. A Mac has no software rasteriser, so it uses the GPU, and the answer says `"software": false`. Before this build it was an error.

**6.4** The semantic frame is the scene as data (entities, their bounds, the UI's text); it needs no GPU.

## 7. Art and sound from recipes

A sprite can be a Luau script that draws it, and a sound a recipe of synthesiser settings. Both are made the same on every machine, so the Mac's must match Windows'. `mac-expected.json`'s `generate` part has what Windows made from these same commands.

```sh
cd ~/argus-mac-test/games/platformer
argus generate name=sfx/zap sound=laser seed=3 --json > ~/argus-mac-test/log/7.1.json
argus generate name=creatures/blob sprite=creature seed=2 --json > ~/argus-mac-test/log/7.2.json
argus asset preview creatures/blob --json > ~/argus-mac-test/log/7.3.json
cp .argus/previews/creatures_blob.png ~/argus-mac-test/frames/blob-preview.png
shasum -a 256 assets/sfx/zap.toml assets/creatures/blob.art.luau .argus/assets/creatures/blob.png .argus/previews/creatures_blob.png
argus daemon stop
cd ~/argus-mac-test
```

**7.1** The sound. Its file, `assets/sfx/zap.toml`, has `generate.sound_toml_sha256`. Its description, the answer's `heard`, matches `generate.sound_heard` field for field: the sentence, the length, the loudness, and the pitch over time. The file is the recipe and the description is what the Mac's synthesiser made of it, so a difference in the description alone is in the synthesiser.

**7.2** The sprite. Its script, `assets/creatures/blob.art.luau`, has `generate.sprite_script_sha256`. The sheet the script drew, `.argus/assets/creatures/blob.png`, has `generate.sprite_sheet_sha256`, and the preview has `generate.sprite_preview_sha256`. The answer's `drawn` has 4 frames of 15 x 14 and the clips `idle` and `walk`.

## 8. Windows, sound, and keys (person)

These need the person to look and listen. Ask them each question and write down their answers in their words.

**8.1** A game. From the example games archive:

```sh
mkdir -p ~/argus-mac-test/play && tar -xzf example-games-0.0.5-macos-universal.tar.gz -C ~/argus-mac-test/play
ls ~/argus-mac-test/play/*/
~/argus-mac-test/play/example-games-0.0.5-macos/hollow/hollow
```

Ask the person:
- Does a window open?
- Is it sharp, not blurry, on their screen?
- Does music play?
- Do A/D or the arrow keys walk, and Space jump, with a sound?
- Does Esc open a menu that the arrow keys and Enter work?
- In the menu's settings, does full screen go on and off?
- Does the green button at the window's top left also make it full screen, and does it stay full screen?
- Is any text on screen much smaller than the rest?

If they have VoiceOver (Cmd+F5), does it read the menu's buttons? If they have a gamepad, does it work? Then quit the game, once with Esc and the menu's Quit, and once with Cmd+Q.

**8.2** The editor:

```sh
cd ~/argus-mac-test/games/platformer && argus editor
```

Ask the person:
- Does a window open with the game in its middle and panels around it?
- Does the Play button run the game in the editor?
- Do the panels on the right (the inspector, the tuning panel) show the selected thing's values?
- When they click the hero in the viewport, is it selected?

Close the editor, then `argus daemon stop`.

**8.3** The report key. In a released game, F8 pauses it and asks for a note on the window's bottom line; Enter saves a report under the game's save folder. Mac keyboards send F8 as a media key (play or pause) unless fn is held. Start Hollow again as in 8.1, then:
- Does F8 alone pause the game, or play and pause the person's music?
- Does fn+F8 pause the game and show the note line?
- Is that line's text readable, or much smaller than the rest?

Type a word and press Enter, then:

```sh
ls ~/Library/Application\ Support/hollow/reports/
```

A report file is there.

## 9. Releasing a game

```sh
cp -R ~/argus-mac-test/games/platformer ~/argus-mac-test/games/release-me
cd ~/argus-mac-test/games/release-me
argus build --json > ~/argus-mac-test/log/9.1.json; echo "exit $?"
ls -R dist | head -20
for t in web windows linux; do argus build --target $t --json > ~/argus-mac-test/log/9.2-$t.json; echo "$t: exit $?"; done
argus daemon stop
cd ~/argus-mac-test
```

**9.1** The Mac release is made from the installed runtime (`runtime.prebuilt` true). It plays as the project does (`check.same` true), and its executable, run headless here, reached the same world (`check.executable`). `dist/macos` has the game's executable, named after the game (`platformer`, the name in its `argus.toml`), and its `data/`. **(person)** Run `dist/macos/platformer` and say whether the game opens.

**9.2** The web, Windows, and Linux releases are made too, each with `check.same` true. Their executables are not run on a Mac, and the report says so.

## 10. Agents and the MCP server

**10.1** The server answers, from the shell:

```sh
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"mac-test","version":"0"}}}' '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' | argus mcp | tail -1 | head -c 600
```

The second line lists the tools (`summary`, `query`, `observe`, `run`, `test`, `generate`, and more).

**10.2** Your own host. Connect Argus to the agent you are (`argus mcp setup` on its own lists the clients and which it finds):

```sh
cd ~/argus-mac-test/games/platformer && argus mcp setup <your client>
```

The client is `claude-code`, `cursor`, `vscode`, `codex`, or `gemini`. Most hosts load a new server only when they start, so ask the person to restart you in that folder. Then, through the Argus tools rather than the shell:
- `summary`;
- `observe` (the semantic frame);
- `run` for 60 ticks;
- `test`.

Record whether each worked. If your host cannot be restarted in the middle of this, record SKIPPED and why.

**10.3** Claude Desktop, if the person has it. Check where setup writes, without changing their own settings first:

```sh
ARGUS_SETUP_HOME=$HOME/argus-mac-test/fake-home argus mcp setup claude-desktop --json
ls "$HOME/argus-mac-test/fake-home/Library/Application Support/Claude/"
```

It writes `claude_desktop_config.json` there. Before this build it wrote under `~/.config`, which Claude Desktop on a Mac never reads.

Then, if the person wants Argus in Claude Desktop, install the bundle. In Claude Desktop: Settings, Extensions, Advanced settings, Install Extension, then `argus-0.0.5-macos.mcpb`. In a new chat, ask it to list the Argus projects. **(person)** Did it?

## 11. Updating and removing (last)

```sh
argus self-update 0.0.5 && argus version
argus uninstall
ls ~/.local/share/argus/app 2>&1; ls ~/.local/bin/argus 2>&1
ls ~/argus-mac-test/games
```

**11.1** The update reinstalls 0.0.5 and `argus version` still works.

**11.2** Uninstalling removes the program and its links, and the games are still there.

If the person wants to keep Argus, install it again: `sh install.sh --version 0.0.5`.

## 12. Swarm and Rust plugins (only if the person agrees)

Swarm, the fourth example game, has part of its code in Rust: a plugin, compiled on the machine that runs it. Run this only if `cargo --version` works, or if the person agrees to install Rust (https://rustup.rs) and, if asked, Apple's developer tools. Otherwise SKIPPED.

```sh
cd ~/argus-mac-test/source
argus --project examples/swarm test --json > ~/argus-mac-test/runs/swarm.json; echo "exit $?"
argus --project examples/swarm daemon stop
cd ~/argus-mac-test
```

**12.1** The plugin builds (a `.dylib`), every scenario passes, and Swarm's digests match `mac-expected.json`.

## The report

Write `~/argus-mac-test/report.md`:

1. **The machine:** section 0's answers.
2. **The checks:** a table of every numbered check (1.1 to 12.1), each PASS, FAIL, or SKIPPED, with one line saying what happened.
3. **Each failure:**
   - the commands;
   - the output, the part that says what went wrong, up to 40 lines;
   - what you think it means.
4. **The digests:** for each game, matched and of how many, and the runs that did not match.
5. **What the person saw and heard:** section 8's answers, and 9.1's and 10.3's, in their words.
6. **Anything else:** what surprised you, even outside the checks, such as a slow step, a warning printed, or a doc that was wrong.

File it as an issue on the repository:

```sh
gh issue create --repo KSamsoe/argus --title "Mac test: $(sysctl -n machdep.cpu.brand_string), macOS $(sw_vers -productVersion), Argus 0.0.5" --body-file ~/argus-mac-test/report.md
```

Then zip the logs, runs, and pictures (too big for an issue). The person sends the zip to Kevin:

```sh
cd ~ && zip -qr argus-mac-test-logs.zip argus-mac-test/log argus-mac-test/runs argus-mac-test/frames argus-mac-test/report.md && ls -la argus-mac-test-logs.zip
```

Tell the person the issue's link, and where the zip is.
