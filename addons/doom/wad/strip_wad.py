#!/usr/bin/env python3
"""Makes the WAD the Doom add-on carries: Freedoom: Phase 1 (BSD-3-Clause), episode 1 only, with
no sound effects or music (the ArtCraft apps' plug-ins can't play sound), so it fits the 32 MiB a
PhotoCraft plug-in may be.

    python3 strip_wad.py freedoom1.wad out.wad
"""
import re
import struct
import sys

MAP_LUMPS = {"THINGS", "LINEDEFS", "SIDEDEFS", "VERTEXES", "SEGS", "SSECTORS", "NODES", "SECTORS", "REJECT", "BLOCKMAP"}
DROP_NAMES = {"GENMIDI", "DMXGUS", "DMXGUSC", "DEMO2", "DEMO3", "DEMO4"}


def main(src, dst):
    data = open(src, "rb").read()
    ident, count, table = struct.unpack_from("<4sii", data, 0)
    assert ident == b"IWAD", "not an IWAD"
    lumps = []
    for i in range(count):
        pos, size, raw = struct.unpack_from("<ii8s", data, table + 16 * i)
        lumps.append((raw.rstrip(b"\0").decode("latin1"), data[pos:pos + size]))
    keep = []
    in_section = False
    in_dropped_map = False
    for name, body in lumps:
        if name in ("S_START", "P_START", "F_START"):
            in_section = True
        elif name in ("S_END", "P_END", "F_END"):
            in_section = False
        if re.fullmatch(r"E\dM\d", name):
            in_dropped_map = not name.startswith("E1")
            if in_dropped_map:
                continue
        elif name in MAP_LUMPS:
            if in_dropped_map:
                continue
        else:
            in_dropped_map = False
        if not in_section and (name in DROP_NAMES or re.match(r"^(DS|DP|D_)", name)):
            continue
        keep.append((name, body))
    out = bytearray(b"IWAD" + struct.pack("<ii", len(keep), 0))
    directory = bytearray()
    for name, body in keep:
        directory += struct.pack("<ii8s", len(out), len(body), name.encode("latin1"))
        out += body
    struct.pack_into("<i", out, 8, len(out))
    out += directory
    open(dst, "wb").write(out)
    print(f"{dst}: {len(keep)} of {count} lumps, {len(out)} bytes")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
