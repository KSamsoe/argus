# Puppet

The skeletal module's example (phase 5 item 8): two puppets made of bones,
not one of the four reference games. The skeleton is a Spine 4 JSON export
(`assets/puppet/puppet.json`) with its images unpacked beside it
(`assets/puppet/images/`), which is what the Spine editor writes. The engine
reads it with its own runtime; no Spine code is involved.

- The hero stands, then walks, and waves when the show cues it. Its
  `Animator` (`assets/puppet/moves.toml`) plays the skeleton's animations
  as clips, crossfading between them, and a Luau system
  (`systems/show.luau`) paces it left and right while it walks.
- The knight is the same skeleton in its `armored` skin, mirrored, walking
  on the spot with no animator (`Skeleton.animation = "walk"`).
- Both legs bend by two-bone IK to feet the walk animation moves; the badge
  keeps upright on a turning torso (Spine's `onlyTranslation`); the idle
  blinks by swapping an attachment; the wave tints the badge.
- Every footstep and the wave are Spine events, which the show counts.

`argus --project examples/puppet test` runs its three scenarios;
`argus --project examples/puppet daemon run --play --window 1280x720`
plays it in a window.
