# Colony

A colony sim start. Four colonists live on a meadow beside a pond. Each
one picks the most important job they can do and walks to it: picking a
ripe berry bush, felling a tree (only the two woodcutters), carrying what
was picked or felled to the stockpile, or eating when they get hungry.
Bushes and trees grow back, a day passes every minute and the meadow
darkens at night (the lantern at the store comes on), and the line at
the top shows the day, the hour, and what is in store. When there is
nothing to do, colonists wait about the stockpile.

Space pauses, F speeds up (1x, 3x, 10x), WASD or the arrows move the view,
and minus and equals zoom.

## The files that matter

- `components/colony.luau`: the game's components. `Colony` holds the
  rules (hunger, dusk), `Stockpile` what is stored, `Colonist` how fed
  someone is, `Source` a bush or a tree, `Item` goods lying on the ground.
- `systems/`: one file per thing the colony does.
  - `sources.luau`: a ripe bush or tree posts a job.
  - `work.luau`: what a finished job does (goods dropped, stored, eaten).
  - `haul.luau`: goods on the ground get a job to carry them home.
  - `needs.luau`: hunger, the eat job, starving.
  - `idle.luau`: what a colonist with nothing to do does.
  - `day.luau`: the colony's clock, the day turning, the dark at night.
  - `controls.luau`: pause and speed. `hud.luau`: the line on screen.
  - `looks.luau`: how it all looks (below); it decides nothing.
- `scenes/main/`: one file per entity. `jobs.toml` lists the kinds of job
  and their priorities, `colony.toml` is the stockpile and its rules, and
  the bushes, trees, and colonists are small files made from `prefabs/`.
- `prefabs/`: what a colonist, a bush, and a tree are.
- `assets/`, `ui.toml`, and `tools/art.py`: the art (below).
- `tests/`: the scenarios that keep the core loop working. Run them with
  `argus test`.

The job system, the walking, and the pathfinding around the pond are the
engine's (the `Job`, `Worker`, `JobKind`, `NavAgent`, `NavGrid`, and
`NavCost` components; `argus docs` describes them). `argus events
why=job.idle` says why a colonist has nothing to do.

## The art

Everything is drawn by `tools/art.py` (standard-library Python): it
writes the PNGs in `assets/`, with a sidecar `.toml` beside each saying
how it is cut into frames. Run `python tools/art.py` after changing it and
a running game picks the new pictures up. The PNGs are kept here too, so
the game runs without Python, and a release build leaves `tools/` out.

- `sprites/ground.png`: the grass, one picture under the whole meadow.
- `sprites/colonists.png`: a row of 16 poses for each colonist (standing,
  walking, carrying, picking, chopping, eating, walking away).
- `sprites/meadow.png`: everything else in one sheet: bushes and trees in
  each stage of growing back, the goods, the store, the woodpile and
  baskets, the pond, the yard. Its sidecar names every frame.
- `ui/panel.png`: the wooden panel behind the HUD and the pause screen
  (`scenes/main/paused.toml`); `ui.toml` has the words' colours.

The scenes and prefabs name these pictures; `systems/looks.luau` picks
each frame from what the colony's systems decided (a bush's growth, a
colonist's job) and writes only looks, so changing the art never changes
how the colony plays. At night it turns up the lantern's `Light` at the
store (the camera's `Lighting` makes lights count), and when a tree is
chopped it bursts wood chips (`assets/fx/chips.toml`).

To change first: the palette at the top of `tools/art.py` (every colour
has a name) and the colonists' clothes and hair (`PEOPLE` there). A new
colonist with another name gets a row by the length of their name; give
them their own by adding them to `PEOPLE` and to `ROW` in
`systems/looks.luau`.

## Ideas for what to add next

1. Building: spend wood on a house or a bed. A `Blueprint` component on a
   new entity posts a build job (as `sources.luau` posts a gather job),
   and `work.luau` takes the wood from the stockpile when it is done.
2. Sleep: a `rest` need on `Colonist` that falls by day, and a sleep job
   at night (`Colony.dusk`), like the eat job in `needs.luau`. Outpost,
   the full example game, has beds.
3. More colonists: on `day.turned`, while there is plenty of food, a
   newcomer arrives at the edge of the meadow. A script spawns them with
   the components `prefabs/colonist.toml` lists (`q:spawn` takes component
   names, not a prefab), and a scenario checks the colony grows without
   starving.
