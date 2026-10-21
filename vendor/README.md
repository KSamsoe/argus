# Vendored crates

Two crates the engine builds from here instead of crates.io, through
`[patch.crates-io]` in the workspace's Cargo.toml. Each is the published
crate with small changes, every one marked `Argus:` in the source.

- **luau0-src 0.12.3+luau663**, Luau's C++ and how to build it. Its math
  functions (`math.sin`, `^`, `vector.angle`, and the compiler's constant
  folding of them) call `argus_libm_*` instead of the platform's C library,
  through `luau/Common/include/argus_libm.h`, and it is built with
  `-ffp-contract=off`. `luau/CodeGen` is left out: the engine does not turn
  on Luau's code generator.
- **mlua-sys 0.6.8**, which builds and links Luau for mlua. It defines the
  `argus_libm_*` functions with the `libm` crate (`src/argus_libm.rs`).

Why: a platform's C library rounds a few results in a hundred differently
in the last bit, so a script on Windows, Linux, a Mac, and in a browser
could compute different worlds, and players in one match must not
(docs/multiplayer-checklist.md, item 0). `crates/argus-luau/tests/math.rs`
checks a script's math against `libm`'s, bit for bit.

To move to a newer mlua: take the new versions of both crates from cargo's
registry, apply the `Argus:` changes again, and run that test.
