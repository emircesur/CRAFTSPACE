# Doom for PhotoCraft

A CraftSpace add-on: Doom (the [Freedoom](https://freedoom.github.io/) game data, episode 1)
played on a PhotoCraft canvas, a move per key press. Install it from CraftSpace › Fonts &
add-ons (`craftspace-cli addons install craftspace-doom`), make a 640 × 400 RGB document and
press Alt+Shift+W, A, S, D. [`photocraft/README.txt`](photocraft/README.txt) has all the keys.

## How it works

PhotoCraft's plug-ins are WebAssembly filters: each run gets the pixels and a few parameters,
in a fresh sandboxed instance with no imports (no files, clock or memory of earlier runs). So:

- **The engine** is [doomgeneric](https://github.com/ozkl/doomgeneric) (vendored in
  [`doomgeneric/`](doomgeneric), GPL-2.0-or-later), compiled to WebAssembly with clang against a
  small C library of our own ([`src/libc`](src/libc)): an allocator, formatting, and files kept in
  memory. The WAD is compiled into the module ([`src/wad.s`](src/wad.s)); sound is left out.
- **A session** ([`src/session.c`](src/session.c)) drives Doom a tic at a time instead of its
  main loop: start a game, play moves, draw a frame, save and load in memory.
- **The game lives in the picture** ([`src/photocraft.c`](src/photocraft.c)): the low two bits of
  each pixel's red, green and blue hold the save game, compressed ([`src/state.c`](src/state.c)).
  Each run reads it, plays one move for a few tics, draws the frame (scaled to the canvas) and
  writes the game back. Undo takes a move back. With an overlap of 256 pixels every band
  PhotoCraft hands the filter sees the whole canvas (up to 1024 × 512), so each band plays the
  same move.
- **The keys** are PhotoCraft actions ([`photocraft/doom-actions.json`](photocraft/doom-actions.json))
  that run the plug-in with a move; CraftSpace merges them into PhotoCraft's Actions panel (the
  `actions` install step) and binds Alt+Shift keys that aren't taken.

### Changes to doomgeneric

Marked `CraftSpace:` in the source:

- `p_saveg.c`: monster targets and tracers, the sector a sound woke, and the player's attacker
  are saved as references (vanilla dropped them, so monsters forgot whom they were chasing
  after every load); wall switches waiting to pop back out are saved too (`P_ArchiveButtons`).
- `p_spec.c`: `P_UpdateAnimations`, the texture animation step of `P_UpdateSpecials`, so a game
  drawn right after loading shows its animations as they are.
- `st_stuff.c`: the status bar face's state at file scope, with `ST_ResetFace`, so a frame
  depends only on the game.
- `g_game.c`: calls the button save and load.

With these, playing on after a save and load gives exactly the same game as playing straight
through, frame for frame.

## Building

```sh
addons/doom/package.sh out/        # out/craftspace-doom-1.0.0.zip
```

It downloads Freedoom 0.13.0 (checked against its SHA-256), keeps episode 1 without sounds and
music ([`wad/strip_wad.py`](wad/strip_wad.py)) so the plug-in stays under PhotoCraft's 32 MiB
limit, builds the module ([`build.sh`](build.sh); any recent clang with `wasm-ld`) and zips it
the same way every time. The "Doom add-on" workflow builds it on every change, plays a few moves
with the latest PhotoCraft, and publishes it to the `addon-doom` release when run with
*publish*. (The tag has no number in it: CraftSpace reads this repository's releases for its own
updates.)

`src/effectcraft.c` and `effectcraft/Doom.jsx` are an EffectCraft version (an effect plus a
controller panel) that isn't published.

## Licences

The engine is GPL-2.0-or-later ([`doomgeneric/LICENSE`](doomgeneric/LICENSE)); this add-on's own
code is GPL-2.0-or-later too. Freedoom is BSD-3-Clause.
