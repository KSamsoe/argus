The Argus example games, for the Mac
====================================

Four small games made with Argus, the 2D engine you build games in
together with an AI. Each folder holds a game as `argus build` released
it, an app: nothing to install. Each runs on Apple Silicon and Intel
Macs, macOS 11 or later. Double-click it:

  hollow/Hollow.app
  delve/Delve.app
  outpost/Outpost.app
  duel/Duel.app

These apps are signed, but not by an Apple developer account, so a Mac
refuses them when they came from a browser ("cannot be opened", "Apple
cannot check it for malicious software"). Clear the mark the download
put on them, once, in Terminal from this folder:

  xattr -dr com.apple.quarantine .

Swarm, the fifth game, is not here yet: it has a Rust plugin, which
is compiled on the machine it runs on, and this Mac build was made
without a Mac.

Hollow: a lit cave platformer
  Move: A/D or the arrow keys     Jump: Space or Z (jump again in mid-air
  once you hold the feather)      Talk: E     Restart: R     Pause: Esc

Delve: a turn-based dungeon
  Move or attack: the arrow keys  Wait a turn: .     Drink a potion: Q
  Take the stairs: Enter          Pause: Esc

Outpost: a colony sim
  Pan: WASD or the arrow keys     Zoom: - and =      Pause: Space
  Speed: 1, 2, 3                  Inspect a colonist: Tab     Menu: Esc

Duel: two players, one arena, first to three
  Player 1: WASD to move, F to swing, Space to dash
  Player 2: the arrows to move, . to swing, / to dash
  Its menu plays two on this machine, or hosts or joins a game online.
  Online needs a relay (`argus relay`, in an Argus install): start Duel
  with --relay ws://<the relay's address>:7879, then Host online and give
  the other player the room's code, or Join online with theirs.

A gamepad works in all four, and a screen reader (VoiceOver) reads
their menus. Each game's Esc menu has settings (text size, high
contrast, full screen) and the controls, where a key can be rebound.
Hollow also plays in French.
