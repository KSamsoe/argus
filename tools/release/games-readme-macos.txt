The Argus example games, for the Mac
====================================

Three small games made with Argus, the 2D engine you build games in
together with an AI. Each folder is a game as `argus build` released it:
nothing to install. Each runs on Apple Silicon and Intel Macs, macOS 11
or later. In Terminal, from this folder:

  ./hollow/hollow
  ./delve/delve
  ./outpost/outpost

These programs are signed, but not by an Apple developer account, so a
Mac refuses them when they came from a browser ("cannot be opened",
"Apple cannot check it for malicious software"). Clear the mark the
download put on them, once, from this folder:

  xattr -dr com.apple.quarantine .

Swarm, the fourth game, is not here yet: it has a Rust plugin, which
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

A gamepad works in all three, and a screen reader (VoiceOver) reads
their menus. Each game's Esc menu has settings (text size, high
contrast, full screen) and the controls, where a key can be rebound.
Hollow also plays in French.
