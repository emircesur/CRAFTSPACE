#!/usr/bin/env python3
"""Checks winget manifests against winget's published JSON schemas (what `winget validate` checks
first), so a release never submits a manifest winget-pkgs would reject.

    python3 packaging/winget/validate.py winget/    (needs pyyaml and jsonschema)
"""

import json
import pathlib
import sys
import urllib.request

import jsonschema
import yaml

SCHEMA = "https://aka.ms/winget-manifest.{kind}.{version}.schema.json"
KINDS = {"version": "version", "installer": "installer", "defaultLocale": "defaultLocale"}


def main(folder):
    files = sorted(pathlib.Path(folder).glob("*.yaml"))
    if not files:
        sys.exit(f"no manifests in {folder}")
    seen = set()
    ids = set()
    for path in files:
        # Dates and versions stay strings, as winget reads them.
        doc = yaml.load(path.read_text(), Loader=yaml.BaseLoader)
        kind = doc["ManifestType"]
        seen.add(kind)
        ids.add((doc["PackageIdentifier"], doc["PackageVersion"]))
        url = SCHEMA.format(kind=KINDS[kind], version=doc["ManifestVersion"])
        with urllib.request.urlopen(url) as resp:
            schema = json.load(resp)
        jsonschema.validate(doc, schema)
        print(f"{path.name}: valid {kind} manifest")
    if seen != set(KINDS):
        sys.exit(f"expected {sorted(KINDS)} manifests, found {sorted(seen)}")
    if len(ids) != 1:
        sys.exit(f"the manifests disagree on the package or version: {sorted(ids)}")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else ".")
