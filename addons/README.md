# CraftSpace add-ons

The add-on registry CraftSpace reads: presets, palettes, LUTs, templates and plug-ins for the
ArtCraft apps. CraftSpace shows them under **Fonts & add-ons** and installs each one where its
app finds it (`craftspace-cli addons list` / `install` from the command line).

- [`registry.json`](registry.json): every add-on, the community stores CraftSpace also reads, and
  other plug-in sources it lists.
- [`packs/`](packs): CraftSpace's own packs, made by [`build.py`](build.py) (CC0).
- [`validate.py`](validate.py): checks the registry; [`pin.py`](pin.py): prints a download's
  SHA-256.

## Checked and not checked

| Badge | Means |
|---|---|
| **✔ Checked by CraftSpace** | Made by CraftSpace and reviewed here. Only CraftSpace's own packs. |
| **⚠ Not checked for security** | Everything else: open-source plug-ins pinned in this registry, submissions, and every add-on from a community store. CraftSpace asks before installing one. |

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

## Community stores

CraftSpace also reads other add-on catalogs, listed under `stores`. Their add-ons are always shown
as not checked. Two formats work: this registry's, and the
[ArtCraft Store](https://github.com/akkk09/artcraft-store)'s (`plugins` entries with `downloadUrl`,
`releaseAsset` or `artifact`). People can add more stores in Settings › Add-on sources
(`craftspace-cli addons sources add https://…/catalog.json`).

## CraftSpace's own packs

`python3 addons/build.py` writes them to `packs/` and prints their SHA-256. They are made the same
way every time; a published pack never changes (a new version gets a new file name), and CI checks
that the committed files match a fresh build.
