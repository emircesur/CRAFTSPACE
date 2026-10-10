#!/usr/bin/env bash
# End-to-end checks against real ArtCraft releases (Linux). Run from the repository root after
# `cargo build -p craftspace-cli`.
set -euxo pipefail
cli="$PWD/target/debug/craftspace-cli"
export CRAFTSPACE_HOME="$RUNNER_TEMP/cs" XDG_DATA_HOME="$RUNNER_TEMP/xdg" XDG_BIN_HOME="$RUNNER_TEMP/bin" XDG_CONFIG_HOME="$RUNNER_TEMP/config"
installed() { python3 -c "import json,sys; d=json.load(open('$CRAFTSPACE_HOME/installed.json')); print(d['apps']['$1']['current']['$2'] if '$1' in d['apps'] else '')"; }

"$cli" list
"$cli" info photocraft

# Tarball: install an older version, update, roll back, verify, uninstall.
"$cli" install photocraft --version 0.3.0
test "$(installed photocraft version)" = "0.3.0"
test -L "$XDG_BIN_HOME/photocraft"
"$XDG_BIN_HOME/photocraft-cli" --version
ls "$XDG_DATA_HOME/applications/" | grep -q photocraft
grep -q "Exec=$CRAFTSPACE_HOME" "$XDG_DATA_HOME"/applications/*photocraft*.desktop
"$cli" verify photocraft
# Every version uses the same folder, so the program keeps its path.
exe="$(installed photocraft executable)"
case "$exe" in "$CRAFTSPACE_HOME/apps/PhotoCraft/"*) ;; *) echo "not in the app's folder: $exe"; exit 1 ;; esac
"$cli" update photocraft
test "$(installed photocraft version)" != "0.3.0"
test "$(installed photocraft executable)" = "$exe"
"$XDG_BIN_HOME/photocraft-cli" --version
"$cli" verify photocraft
"$cli" rollback photocraft
test "$(installed photocraft version)" = "0.3.0"
test "$(installed photocraft executable)" = "$exe"
"$XDG_BIN_HOME/photocraft-cli" --version
"$cli" channel photocraft pin
"$cli" check || test $? -eq 10
"$cli" channel photocraft default
"$cli" uninstall photocraft --yes
test ! -e "$XDG_BIN_HOME/photocraft"
test -z "$(installed photocraft version)"

# AppImage with a delta update from the .zsync file.
"$cli" config prefer_appimage true
"$cli" install photocraft --version 0.3.0
test "$(installed photocraft kind)" = "app-image"
"$cli" -v update photocraft 2>&1 | tee "$RUNNER_TEMP/delta.log"
# Either a delta update, or a full download because too little could be reused.
grep -qiE "delta update" "$RUNNER_TEMP/delta.log"
"$cli" verify photocraft
"$cli" uninstall photocraft --yes
"$cli" config prefer_appimage false

# Fonts.
"$cli" fonts install "Noto Sans Arabic"
fc-list | grep -q "Noto Sans Arabic"
"$cli" fonts uninstall

# Export / import.
"$cli" install gridcraft -q
"$cli" export "$RUNNER_TEMP/apps.json"
"$cli" uninstall gridcraft --yes
"$cli" import "$RUNNER_TEMP/apps.json"
test -n "$(installed gridcraft version)"

# Machine policy: required apps get installed, others hidden.
echo '{"required_apps": ["deckcraft"], "allowed_apps": ["deckcraft", "gridcraft"], "settings": {"keep_previous_version": false}}' > "$RUNNER_TEMP/policy.json"
CRAFTSPACE_POLICY="$RUNNER_TEMP/policy.json" "$cli" apply-policy
test -n "$(installed deckcraft version)"
test "$(CRAFTSPACE_POLICY="$RUNNER_TEMP/policy.json" "$cli" list | tail -n +2 | wc -l)" -eq 2
if CRAFTSPACE_POLICY="$RUNNER_TEMP/policy.json" "$cli" config keep_previous_version true; then echo "policy should lock this"; exit 1; fi

# IT and classrooms: a lab policy (pinned version, no uninstalling, update window, reports) and a
# shared package cache that a second computer installs from.
lab="$RUNNER_TEMP/lab"
mkdir -p "$lab/share/reports" "$lab/share/packages"
cat > "$lab/policy.json" <<POLICY
{
  "organization": "CI Art Lab",
  "support": "https://example.com/help",
  "required_apps": ["photocraft"],
  "pinned_versions": { "photocraft": "0.3.0" },
  "update_window": { "days": [], "from": "03:00", "to": "03:00" },
  "prevent_uninstall": true,
  "report_dir": "$lab/share/reports",
  "settings": { "package_cache": "$lab/share/packages", "package_cache_write": true }
}
POLICY
export CRAFTSPACE_POLICY="$lab/policy.json"
"$cli" policy check "$lab/policy.json"
"$cli" policy show | tee "$lab/show.txt"
grep -q "CI Art Lab" "$lab/show.txt"
export CRAFTSPACE_HOME="$lab/pc1"
"$cli" apply-policy
test "$(installed photocraft version)" = "0.3.0"
ls "$lab/share/packages" | grep -q "photocraft-0.3.0"
ls "$lab/share/reports"/*.json
"$cli" report --json | python3 -c "import json,sys; d=json.load(sys.stdin); a={x['id']:x for x in d['apps']}; assert a['photocraft']['version']=='0.3.0', a; assert d['policy']['organization']=='CI Art Lab', d['policy']"
# Pinned and outside the update window: nothing changes.
"$cli" update --scheduled
test "$(installed photocraft version)" = "0.3.0"
# Students can't uninstall (the runner isn't root).
if "$cli" uninstall photocraft --yes; then echo "prevent_uninstall should stop this"; exit 1; fi
test "$(installed photocraft version)" = "0.3.0"
# Fresh settings between classes; the old ones are kept.
mkdir -p "$XDG_CONFIG_HOME/PhotoCraft"
echo '{"theme":"pink"}' > "$XDG_CONFIG_HOME/PhotoCraft/settings.json"
"$cli" reset photocraft --yes
test ! -e "$XDG_CONFIG_HOME/PhotoCraft"
ls -d "$XDG_CONFIG_HOME"/PhotoCraft.reset-*
# A second computer gets the package from the share.
export CRAFTSPACE_HOME="$lab/pc2"
"$cli" -v apply-policy 2>&1 | tee "$lab/pc2.txt"
grep -q "from the package cache" "$lab/pc2.txt"
test "$(installed photocraft version)" = "0.3.0"
unset CRAFTSPACE_POLICY

# Other sources (optional): an app from any GitHub repository.
export CRAFTSPACE_HOME="$RUNNER_TEMP/sources"
"$cli" config prefer_system_installer false
if "$cli" source add BurntSushi/ripgrep --binary rg; then echo "other sources are off by default"; exit 1; fi
"$cli" source check BurntSushi/ripgrep
"$cli" source enable
"$cli" source add BurntSushi/ripgrep --binary rg
"$cli" list | grep -i ripgrep
"$cli" install ripgrep
"$(installed ripgrep executable)" --version
"$cli" verify ripgrep
"$cli" source list | tee "$RUNNER_TEMP/sources.txt"
grep -q "BurntSushi/ripgrep" "$RUNNER_TEMP/sources.txt"
if "$cli" source remove ripgrep; then echo "installed apps stay on the list"; exit 1; fi
"$cli" uninstall ripgrep --yes
"$cli" source remove ripgrep
"$cli" source set craftspace emircesur/craftspace
"$cli" source set craftspace official
"$cli" source disable
test -z "$("$cli" list | grep -i ripgrep || true)"
export CRAFTSPACE_HOME="$RUNNER_TEMP/cs"

# Workspace sync: a PhotoCraft setup carried to another computer, only the chosen parts, keeping
# that computer's own values; and handed out by a policy.
export CRAFTSPACE_HOME="$RUNNER_TEMP/profiles"
pa="$RUNNER_TEMP/pc-teacher" pb="$RUNNER_TEMP/pc-student"
mkdir -p "$pa/Presets" "$pb"
echo '{"workspaces":{"Painting":{"a":1}},"panelLayout":"wide","shortcuts":{"file.new":"Ctrl+Alt+N"},"general":{"theme":"dark"},"fileHandling":{"recentFiles":["/home/teacher/secret.psd"]}}' > "$pa/preferences.json"
echo '{}' > "$pa/Presets/brushes-1.pcbrushes"
PHOTOCRAFT_CONFIG_DIR="$pa" "$cli" profile export photocraft --out "$RUNNER_TEMP/class.craftprofile"
"$cli" profile show "$RUNNER_TEMP/class.craftprofile" | tee "$RUNNER_TEMP/profile.txt"
grep -q "Presets/brushes-1.pcbrushes" "$RUNNER_TEMP/profile.txt"
if unzip -p "$RUNNER_TEMP/class.craftprofile" files/config/preferences.json | grep -q secret.psd; then echo "recent files travelled"; exit 1; fi
echo '{"general":{"theme":"light"},"fileHandling":{"recentFiles":["/home/student/mine.psd"]}}' > "$pb/preferences.json"
PHOTOCRAFT_CONFIG_DIR="$pb" "$cli" profile import "$RUNNER_TEMP/class.craftprofile" --parts layouts,shortcuts --yes
python3 - "$pb/preferences.json" <<'PY'
import json, sys
p = json.load(open(sys.argv[1]))
assert p["panelLayout"] == "wide" and p["shortcuts"]["file.new"] == "Ctrl+Alt+N", p
assert p["general"]["theme"] == "light", p
assert p["fileHandling"]["recentFiles"] == ["/home/student/mine.psd"], p
PY
test ! -e "$pb/Presets"
PHOTOCRAFT_CONFIG_DIR="$pb" "$cli" profile backups photocraft | grep -q craftprofile
# The organization hands the setup to every computer.
echo "{\"profiles\": {\"photocraft\": {\"source\": \"$RUNNER_TEMP/class.craftprofile\", \"apply\": \"every-start\"}}}" > "$RUNNER_TEMP/profile-policy.json"
pc="$RUNNER_TEMP/pc-lab"
mkdir -p "$pc"
CRAFTSPACE_POLICY="$RUNNER_TEMP/profile-policy.json" PHOTOCRAFT_CONFIG_DIR="$pc" "$cli" policy check "$RUNNER_TEMP/profile-policy.json"
CRAFTSPACE_POLICY="$RUNNER_TEMP/profile-policy.json" PHOTOCRAFT_CONFIG_DIR="$pc" "$cli" apply-policy
test -f "$pc/Presets/brushes-1.pcbrushes"
grep -q '"theme": "dark"' "$pc/preferences.json"

# Add-ons: CraftSpace's own packs, pinned open-source audio plug-ins, and a plug-in from the
# community ArtCraft Store.
export CRAFTSPACE_HOME="$RUNNER_TEMP/addons"
"$cli" addons list | tee "$RUNNER_TEMP/addons.txt"
grep -q "craftspace-palettes" "$RUNNER_TEMP/addons.txt"
grep -q "artcraft-store/" "$RUNNER_TEMP/addons.txt"
"$cli" addons install craftspace-palettes craftspace-looks
test -f "$XDG_CONFIG_HOME/vectorcraft/Swatches/CraftSpace Earth.gpl"
find "$HOME" -path "*CraftSpace Add-ons/CraftSpace Looks/CraftSpace Looks/Vivid.cube" | grep -q .
# Not checked by CraftSpace: refused unless the person agrees.
if "$cli" addons install dexed < /dev/null; then echo "an unchecked add-on installed without agreeing"; exit 1; fi
"$cli" addons install dexed dragonfly-reverb --yes
test -f "$HOME/.clap/Dexed.clap"
test -d "$HOME/.vst3/Dexed.vst3"
ls "$HOME/.clap" | grep -q DragonflyHallReverb.clap
"$cli" addons install artcraft-store/org.photocraft.community.vignette --yes
python3 - "$XDG_CONFIG_HOME/photocraft/preferences.json" <<'PY'
import json, os, sys
p = json.load(open(sys.argv[1]))["plugIns"]
assert p["useAdditionalPluginsFolder"] is True, p
assert os.path.isfile(os.path.join(p["additionalPluginsFolder"], "photocraft_plugin_vignette.wasm")), p
PY
# A policy can allow only checked add-ons.
echo '{"block_unchecked_addons": true}' > "$RUNNER_TEMP/addon-policy.json"
if CRAFTSPACE_POLICY="$RUNNER_TEMP/addon-policy.json" "$cli" addons install six-sines --yes; then echo "the policy should block this"; exit 1; fi
"$cli" addons remove dexed dragonfly-reverb craftspace-palettes craftspace-looks artcraft-store/org.photocraft.community.vignette
test ! -e "$HOME/.clap/Dexed.clap"
test ! -e "$HOME/.vst3/Dexed.vst3"
test ! -e "$XDG_CONFIG_HOME/vectorcraft/Swatches/CraftSpace Earth.gpl"
# Add-on repositories: listed apart from the registry, added by owner/repo.
"$cli" addons repos remove artcraft-store
if "$cli" addons list | grep -q "artcraft-store/"; then echo "a turned-off repository is still listed"; exit 1; fi
"$cli" addons repos add akkk09/artcraft-store | tee "$RUNNER_TEMP/repo.txt"
grep -q "ArtCraft Store (id: artcraft-store)" "$RUNNER_TEMP/repo.txt"
"$cli" addons repos | grep -q "github.com/akkk09/artcraft-store"
if "$cli" addons repos add emircesur/CRAFTSPACE; then echo "a repository without a catalog was added"; exit 1; fi
"$cli" addons check addons/registry.json
"$cli" addons repositories | grep -q "Airwindows"
export CRAFTSPACE_HOME="$RUNNER_TEMP/cs"

# Start at login, self-install.
"$cli" autostart on
test -f "$XDG_CONFIG_HOME/autostart/craftspace.desktop"
"$cli" autostart off
test ! -f "$XDG_CONFIG_HOME/autostart/craftspace.desktop"
"$cli" self-install
"$XDG_BIN_HOME/craftspace-cli" --version
"$cli" news | head
echo "Linux end-to-end checks passed"
