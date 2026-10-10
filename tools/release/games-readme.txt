The Argus example games
=======================

Four small games made with Argus, the 2D engine you build games in
together with an AI. Each folder is a game as `argus build` released it:
nothing to install. Run the program in the folder (hollow.exe on Windows,
./hollow on Linux, and so on).

Windows may stop the first run with "Windows protected your PC", because
these programs are not signed: choose More info, then Run anyway.

Hollow: a lit cave platformer
  Move: A/D or the arrow keys     Jump: Space or Z (jump again in mid-air
  once you hold the feather)      Talk: E     Restart: R     Pause: Esc

Delve: a turn-based dungeon
  Move or attack: the arrow keys  Wait a turn: .     Drink a potion: Q
  Take the stairs: Enter          Pause: Esc

Swarm: an arena shooter, waves of hundreds
  Move: WASD (the hero fires by themselves)          Pause: Esc

Outpost: a colony sim
  Pan: WASD or the arrow keys     Zoom: - and =      Pause: Space
  Speed: 1, 2, 3                  Inspect a colonist: Tab     Menu: Esc

A gamepad works in all four, and a screen reader reads their menus.
Each game's Esc menu has settings (text size, high contrast, full
screen) and the controls, where a key can be rebound. Hollow also plays
in French.

Hollow and Outpost are built with Argus's Steam support, set up with
Valve's test app (Spacewar, 480): with Steam running, Steam shows you
playing Spacewar. Without Steam they play the same.

The games' source, and Argus itself: https://github.com/KSamsoe/argus
(examples/hollow, examples/delve, examples/swarm, examples/outpost).
