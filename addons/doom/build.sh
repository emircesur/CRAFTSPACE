#!/usr/bin/env bash
# Builds the Doom add-on's WebAssembly modules with clang (any recent clang with the wasm32
# target, and wasm-ld).
#
#   addons/doom/build.sh WAD OUT_DIR [glue…]     (glue: photocraft effectcraft probe)
#
# WAD is the stripped Freedoom (wad/strip_wad.py). The modules import nothing.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WAD="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
OUT="$2"
shift 2
GLUES=("${@:-photocraft effectcraft}")
CC="${CC:-clang}"
LD="${WASM_LD:-wasm-ld}"
OBJ="$OUT/obj"
mkdir -p "$OBJ"
CFLAGS=(--target=wasm32 -O2 -mbulk-memory -mnontrapping-fptoint -nostdlibinc
  -isystem "$HERE/src/libc/include" -I"$HERE/doomgeneric" -I"$HERE/src"
  -DDOOMGENERIC_RESX=320 -DDOOMGENERIC_RESY=200 -DNORMALUNIX -DLINUX -D_DEFAULT_SOURCE
  -ffile-prefix-map="$HERE"=. -w)

objs=()
for src in "$HERE"/doomgeneric/*.c; do
  name="$(basename "$src" .c)"
  # d_main.c is part of session.c, doomgeneric.c is the stand-alone entry point and
  # sound.c stands in for s_sound.c.
  case "$name" in d_main | doomgeneric | s_sound) continue ;; esac
  "$CC" "${CFLAGS[@]}" -c "$src" -o "$OBJ/$name.o"
  objs+=("$OBJ/$name.o")
done
for src in session sound moves libc/libc state; do
  "$CC" "${CFLAGS[@]}" -c "$HERE/src/$src.c" -o "$OBJ/$(basename "$src").o"
  objs+=("$OBJ/$(basename "$src").o")
done
"$CC" --target=wasm32 -c -x assembler-with-cpp -DWAD_PATH="\"$WAD\"" "$HERE/src/wad.s" -o "$OBJ/wad.o"
objs+=("$OBJ/wad.o")

for glue in ${GLUES[@]}; do
  "$CC" "${CFLAGS[@]}" -c "$HERE/src/$glue.c" -o "$OBJ/glue-$glue.o"
  "$LD" --no-entry --gc-sections --strip-all -z stack-size=1048576 --initial-memory=67108864 \
    "${objs[@]}" "$OBJ/glue-$glue.o" -o "$OUT/doom-$glue.wasm"
  echo "$OUT/doom-$glue.wasm: $(wc -c <"$OUT/doom-$glue.wasm") bytes"
done
