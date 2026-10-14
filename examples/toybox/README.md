# Toybox

The rigid-body module's example (phase 5 item 7): a physics toy, not one of
the four reference games. Its `argus.toml` names the module
(`modules = ["rigid"]`), and everything that moves is a `RigidBody`:

- a stack of five crates, which settles and falls asleep;
- a pendulum: a ball on a `revolute` joint to a point in the air;
- a bridge of six planks, hinged to each other and pinned to the world at
  both ends;
- balls a Luau system (`systems/drop.luau`) drops over the bridge every
  second and a half, bouncing off whatever they hit.
- a wedge (a polygon collider) the balls roll down, and a pill (a capsule
  collider) standing on the floor;
- a tripwire along the floor (`systems/laser.luau`): a raycast each tick
  that ends at the pill unless a ball rolls through, its beam drawn with
  `q:draw_line` while you develop (a release build draws none).

`argus --project examples/toybox test` runs its four scenarios;
`argus --project examples/toybox daemon run --play --window 1280x720`
plays it in a window.
