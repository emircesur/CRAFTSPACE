#!/usr/bin/env python3
"""Checks addons/registry.json: every add-on is complete, goes somewhere its app reads, and (with
--download) every file matches its pinned SHA-256.

    python3 addons/validate.py [--download]

Run by the "Add-ons" workflow on every change to addons/, so a submission can't break CraftSpace.
"""

import hashlib
import json
import pathlib
import re
import sys
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent
KINDS = {"pack", "plugin", "audio-plugin"}
PLATFORMS = {"linux-x86_64", "linux-aarch64", "windows-x64", "windows-arm64", "macos", "linux", "windows"}
TARGETS = {"library", "plugins", "actions", "clap", "vst3", "au"}
# The app folders add-ons may write to (see crates/craftspace-core/src/profiles/specs.rs).
APP_ROOTS = {
    "photocraft": {"config"}, "lightcraft": {"config", "library"}, "vectorcraft": {"config"},
    "designcraft": {"config"}, "wordcraft": {"config"}, "deckcraft": {"config"}, "gridcraft": {"config"},
    "filmcraft": {"data"}, "effectcraft": {"config", "documents"}, "soundcraft": {"config"}, "artcraft": {"home"},
}
PLUGIN_APPS = {"photocraft", "vectorcraft", "effectcraft"}
SHA = re.compile(r"^[0-9a-f]{64}$")


def main():
    download = "--download" in sys.argv
    reg = json.loads((ROOT / "addons/registry.json").read_text())
    apps = {a["id"] for a in json.loads((ROOT / "catalog.json").read_text())["apps"]}
    errors = []

    def err(where, msg):
        errors.append(f"{where}: {msg}")

    if reg.get("format") != "craftspace-addons" or reg.get("version") != 1:
        err("registry", 'needs "format": "craftspace-addons", "version": 1')
    ids = set()
    for a in reg.get("addons", []):
        where = a.get("id", "?")
        for key in ("id", "name", "description", "author", "license", "homepage", "files", "install", "apps"):
            if not a.get(key):
                err(where, f"needs {key}")
        if not re.match(r"^[a-z0-9][a-z0-9.-]*$", a.get("id", "")):
            err(where, "the id is lower-case letters, digits, dots and dashes")
        if a.get("id") in ids:
            err(where, "the id is already used")
        ids.add(a.get("id"))
        if a.get("kind", "pack") not in KINDS:
            err(where, f"kind is one of {sorted(KINDS)}")
        if a.get("trust", "unchecked") not in {"checked", "unchecked"}:
            err(where, 'trust is "checked" or "unchecked"')
        if a.get("trust") == "checked" and a.get("author") != "CraftSpace":
            err(where, 'only CraftSpace\'s own add-ons are "checked"; submissions are "unchecked"')
        for app in a.get("apps", []):
            if app not in apps:
                err(where, f"unknown app {app}")
        platforms = [f.get("platform") for f in a.get("files", [])]
        if len(platforms) != len(set(platforms)):
            err(where, "one file per platform")
        for f in a.get("files", []):
            if not f.get("url", "").startswith("https://"):
                err(where, "downloads must be https://")
            if not SHA.match(f.get("sha256", "")):
                err(where, f"{f.get('url')}: needs its SHA-256 (python3 addons/pin.py URL)")
            if f.get("platform") is not None and f["platform"] not in PLATFORMS:
                err(where, f"platform is one of {sorted(PLATFORMS)}")
        for step in a.get("install", []):
            app, to = step.get("app"), step.get("to", "")
            if app not in apps:
                err(where, f"install: unknown app {app}")
            if to.startswith("app:"):
                root = to[4:].split("/")[0]
                if root not in APP_ROOTS.get(app, set()):
                    err(where, f"install: {app} has no {root} folder (one of {sorted(APP_ROOTS.get(app, set()))})")
                if ".." in to.split("/"):
                    err(where, "install: no .. in paths")
            elif to not in TARGETS:
                err(where, f"install: to is app:<folder>/<path> or one of {sorted(TARGETS)}")
            if to == "actions" and app != "photocraft":
                err(where, "install: only photocraft takes actions")
            if to == "plugins" and app not in PLUGIN_APPS:
                err(where, f"install: {app} doesn't take plug-ins")
            if to in {"clap", "vst3", "au"} and app != "soundcraft":
                err(where, "install: audio plug-ins are for soundcraft")
    for s in reg.get("stores", []):
        for key in ("id", "name", "url"):
            if not s.get(key):
                err(f"store {s.get('id', '?')}", f"needs {key}")
        if not s.get("url", "").startswith("https://"):
            err(f"store {s.get('id', '?')}", "url must be https://")
        if s.get("repo") is not None and not re.match(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$", s["repo"]):
            err(f"store {s.get('id', '?')}", "repo is owner/repo")
    for r in reg.get("repositories", []):
        if not r.get("url", "").startswith("https://") or not r.get("name"):
            err(f"repository {r.get('name', '?')}", "needs a name and an https:// url")

    if download and not errors:
        for a in reg["addons"]:
            for f in a["files"]:
                req = urllib.request.Request(f["url"], headers={"User-Agent": "craftspace-addons-validate"})
                h = hashlib.sha256()
                with urllib.request.urlopen(req, timeout=600) as resp:
                    for chunk in iter(lambda: resp.read(1 << 20), b""):
                        h.update(chunk)
                ok = h.hexdigest() == f["sha256"]
                print(f"{'ok ' if ok else 'BAD'} {a['id']}: {f['url']}")
                if not ok:
                    err(a["id"], f"{f['url']} doesn't match its SHA-256 (it's {h.hexdigest()})")

    for e in errors:
        print(f"error: {e}", file=sys.stderr)
    print(f"{len(reg.get('addons', []))} add-ons, {len(errors)} problem(s)")
    sys.exit(1 if errors else 0)


if __name__ == "__main__":
    main()
