#!/usr/bin/env python3
"""Prints the SHA-256, size and contents of add-on downloads, for pinning them in catalog.json.

    python3 addons/pin.py URL...      (or URLs one per line on stdin)

Run by the "Pin add-ons" workflow, since add-ons are pinned to exact files: CraftSpace checks
every download against the catalog's SHA-256 before installing it.
"""

import hashlib
import io
import json
import sys
import tarfile
import urllib.request
import zipfile


def contents(name, data):
    try:
        if name.endswith(".zip"):
            return zipfile.ZipFile(io.BytesIO(data)).namelist()
        if name.endswith((".tar.gz", ".tgz", ".tar.xz")):
            return tarfile.open(fileobj=io.BytesIO(data)).getnames()
    except Exception as err:  # noqa: BLE001
        return [f"(can't list: {err})"]
    return []


def main():
    urls = sys.argv[1:] or [line.strip() for line in sys.stdin if line.strip()]
    for url in urls:
        req = urllib.request.Request(url, headers={"User-Agent": "craftspace-addons-pin"})
        with urllib.request.urlopen(req, timeout=600) as resp:
            data = resp.read()
        name = url.rsplit("/", 1)[-1]
        names = contents(name, data)
        # Plug-in bundles and other interesting entries, not every file inside a bundle.
        interesting = sorted({n for n in names if n.lower().rstrip("/").endswith((".clap", ".vst3", ".component", ".lv2", ".dll", ".so", ".dylib", ".exe"))})
        print(json.dumps({"url": url, "sha256": hashlib.sha256(data).hexdigest(), "size": len(data),
                          "entries": len(names), "bundles": interesting[:60], "top": sorted({n.split("/")[0] for n in names})[:20]}))
        sys.stdout.flush()


if __name__ == "__main__":
    main()
