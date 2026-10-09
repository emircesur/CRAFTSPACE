# CraftSpace add-ons

The add-on registry CraftSpace reads: presets, palettes, LUTs, templates and plug-ins for the
ArtCraft apps. CraftSpace shows them under **Fonts & add-ons** and installs each one where its
app finds it (`craftspace-cli addons list` / `install` from the command line).

- [`registry.json`](registry.json): every add-on, the add-on repositories CraftSpace suggests, and
  other plug-in sources it lists.
- [`packs/`](packs): CraftSpace's own packs, made by [`build.py`](build.py) (CC0).
- [`validate.py`](validate.py): checks the registry; [`pin.py`](pin.py): prints a download's
  SHA-256.
- Add-on repositories: other catalogs CraftSpace lists separately, such as the ArtCraft Store
  ([below](#add-on-repositories)), and how to make your own
  ([CraftSpace-compatible add-on repos](#craftspace-compatible-add-on-repos)).

## Checked and not checked

| Badge | Means |
|---|---|
| **✔ Checked by CraftSpace** | Made by CraftSpace and reviewed here. Only CraftSpace's own packs. |
| **⚠ Not checked for security** | Everything else: open-source plug-ins pinned in this registry, submissions, and every add-on from an add-on repository. CraftSpace asks before installing one. |

CraftSpace checks every download against the SHA-256 pinned here, so a file changed after it was
listed won't install. A checksum says the file is the one that was listed, not that it's safe:
audio plug-ins are programs that run inside SoundCraft with your permissions. PhotoCraft,
VectorCraft and EffectCraft run their WebAssembly plug-ins in a sandbox.

Organizations can allow only checked add-ons with `"block_unchecked_addons": true` in their policy.

## Submitting an add-on

Easiest: [open a "Submit an add-on" issue](https://github.com/emircesur/CRAFTSPACE/issues/new?template=addon.yml).
Or add it yourself in a pull request:

1. Publish the files somewhere permanent, with a version in the name (a GitHub release asset is
   ideal). One download per platform for plug-ins, one for everything for presets. Zip, tar.gz and
   tar.xz archives are unpacked; other files are used as they are.
2. Get each file's SHA-256: `python3 addons/pin.py https://…/my-pack-1.0.zip` (or run the
   "Pin add-ons" workflow).
3. Add an entry to `addons` in [`registry.json`](registry.json):

   ```json
   {
     "id": "my-brush-palettes",
     "name": "My Palettes",
     "kind": "pack",
     "version": "1.0",
     "author": "Your name",
     "license": "CC-BY-4.0",
     "trust": "unchecked",
     "description": "What it is, in a sentence or two.",
     "apps": ["vectorcraft"],
     "tags": ["palettes"],
     "homepage": "https://github.com/you/my-palettes",
     "files": [{ "url": "https://github.com/you/my-palettes/releases/download/v1.0/my-palettes-1.0.zip", "sha256": "…" }],
     "install": [{ "app": "vectorcraft", "to": "app:config/Swatches", "files": ["*.gpl"] }]
   }
   ```

4. `python3 addons/validate.py --download` must pass (CI runs it on your pull request).

Submissions are always `"trust": "unchecked"`. Don't submit anything you may not share, or any
Adobe (or other vendors') presets, brushes or assets.

### Where files go (`install`)

Each step names an app (`app`), where its files go (`to`), optionally which files (`files`:
paths in the download, `*` within a folder, `**` across folders) and a `hint` saying how to use
them in the app.

| `to` | Goes to | Good for |
|---|---|---|
| `app:config/Swatches` | VectorCraft's swatch libraries, read at start | `.gpl`, `.ase`, `.vcswatches` |
| `app:config/Graphic Styles` | VectorCraft's graphic style libraries | `.vcstyles` |
| `app:config/Libraries` | VectorCraft's Libraries panel | `.vclibrary` |
| `app:data/Graphics Templates` | FilmCraft's graphics templates | `.fcgt` |
| `app:data/Templates` | FilmCraft's project templates | `.fcproj` |
| `app:data/Keyboard Shortcuts` | FilmCraft's shortcut presets | FilmCraft shortcut `.json` |
| `app:documents/Presets` | EffectCraft's animation presets (rescanned while it runs) | `.ecpreset` |
| `app:config/Scripts` | EffectCraft's scripts (After Effects-style) | `.jsx`, `.js` |
| `app:config/Scripts/ScriptUI Panels` | EffectCraft's ScriptUI panels | `.jsx` |
| `app:config/Templates` | EffectCraft's project templates | `.ectemplate` |
| `app:config/Presets` | SoundCraft's plug-in presets | per-plug-in folders of `.json` |
| `plugins` | The app's plug-in folder (PhotoCraft, VectorCraft, EffectCraft) | `.wasm` |
| `clap`, `vst3`, `au` | The per-user audio plug-in folders (bundles found anywhere in the download) | SoundCraft plug-ins |
| `library` | `Documents/CraftSpace Add-ons/<name>`, for content the app imports itself; `"open": true` opens it in the app (PhotoCraft imports `.abr`, `.grd`, `.aco`, `.ase` that way) | LUTs, LightCraft presets, brushes |

Apps whose settings live elsewhere (portable copies, Flatpak) are handled: the folders are found
the same way as for workspace profiles.

## Add-on repositories

Besides this registry, CraftSpace lists add-ons from **add-on repositories**, each shown on its own
under **Fonts & add-ons › Repositories** with a link to it. Their add-ons are always shown as not
checked. The [ArtCraft Store](https://github.com/akkk09/artcraft-store) is suggested (listed under
`stores` in `registry.json`); anyone can add more, in the app (type `owner/repo` and click **Add
repository**) or from the command line:

```sh
craftspace-cli addons repos add someone/their-addons     # or the https:// address of a catalog
craftspace-cli addons repos                              # list; remove / enable / disable ID
```

For `owner/repo`, CraftSpace looks for a catalog in this order:

1. `craftspace-addons.json` at the root of the repository's default branch (a CraftSpace-compatible
   add-on repo, below);
2. `catalog.json` on the repository's GitHub Pages site (`https://owner.github.io/repo/`), in the
   ArtCraft Store's format (`plugins` entries with `downloadUrl`, `releaseAsset` or `artifact`);
3. `catalog.json` at the root of the repository.

## CraftSpace-compatible add-on repos

A GitHub repository with a `craftspace-addons.json` at its root, in the same format as
[`registry.json`](registry.json), plus a `name` and `description` for the repository:

```json
{
  "format": "craftspace-addons",
  "version": 1,
  "name": "Sam's Swatches",
  "description": "Palettes for VectorCraft and PhotoCraft.",
  "addons": [
    {
      "id": "sams-palettes",
      "name": "Sam's Palettes",
      "kind": "pack",
      "version": "1.0",
      "author": "Sam",
      "license": "CC0-1.0",
      "description": "Twelve palettes.",
      "apps": ["vectorcraft"],
      "homepage": "https://github.com/sam/swatches",
      "files": [{ "url": "https://github.com/sam/swatches/releases/download/v1.0/palettes-1.0.zip", "sha256": "…" }],
      "install": [{ "app": "vectorcraft", "to": "app:config/Swatches", "files": ["*.gpl"] }]
    }
  ]
}
```

Everything under [Submitting an add-on](#submitting-an-add-on) applies: permanent `https://`
downloads with their SHA-256, and `install` steps from [the table above](#where-files-go-install).
Check it before publishing:

```sh
craftspace-cli addons check craftspace-addons.json    # or: craftspace-cli addons check owner/repo
```

A repository can't mark its own add-ons as checked: CraftSpace shows everything from a
repository as not checked for security and asks before installing it. CraftSpace re-reads the
catalog when it refreshes, so a change you push reaches everyone who added the repository.

## CraftSpace's own packs

`python3 addons/build.py` writes them to `packs/` and prints their SHA-256. They are made the same
way every time; a published pack never changes (a new version gets a new file name), and CI checks
that the committed files match a fresh build.
