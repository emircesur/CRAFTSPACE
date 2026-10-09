#!/usr/bin/env bash
# End-to-end checks against real ArtCraft releases (macOS).
set -euxo pipefail
cli="$PWD/target/debug/craftspace-cli"
export CRAFTSPACE_HOME="$RUNNER_TEMP/cs" CRAFTSPACE_MAC_APPLICATIONS="$RUNNER_TEMP/Applications"
installed() { python3 -c "import json,sys; d=json.load(open('$CRAFTSPACE_HOME/installed.json')); print(d['apps']['$1']['current']['$2'] if '$1' in d['apps'] else '')"; }
plist_version() { /usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$1/Contents/Info.plist"; }

"$cli" list
"$cli" install photocraft --version 0.3.0
app="$(installed photocraft executable)"
echo "$app"
test -d "$app"
case "$app" in "$CRAFTSPACE_MAC_APPLICATIONS"/*) ;; *) echo "not in Applications"; exit 1 ;; esac
plist_version "$app"
"$cli" verify photocraft
"$cli" update photocraft
app="$(installed photocraft executable)"
test -d "$app"
test "$(ls "$CRAFTSPACE_MAC_APPLICATIONS" | grep -c PhotoCraft)" -eq 1
plist_version "$app"
"$cli" rollback photocraft
test "$(installed photocraft version)" = "0.3.0"
test "$(plist_version "$(installed photocraft executable)")" = "0.3.0" || plist_version "$(installed photocraft executable)"
"$cli" verify photocraft
"$cli" uninstall photocraft --yes
test -z "$(ls "$CRAFTSPACE_MAC_APPLICATIONS" | grep PhotoCraft || true)"

# An app CraftSpace didn't install (here: one a different CraftSpace data folder installed) is
# found, and updated where it is instead of installed a second time.
"$cli" install photocraft --version 0.3.0
export CRAFTSPACE_HOME="$RUNNER_TEMP/cs-fresh"
"$cli" detect | tee "$RUNNER_TEMP/detect.txt"
grep -q "Found PhotoCraft 0.3.0" "$RUNNER_TEMP/detect.txt"
test "$(installed photocraft executable)" = "$CRAFTSPACE_MAC_APPLICATIONS/PhotoCraft.app"
"$cli" update photocraft
test "$(ls "$CRAFTSPACE_MAC_APPLICATIONS" | grep -c PhotoCraft)" -eq 1
test "$(plist_version "$CRAFTSPACE_MAC_APPLICATIONS/PhotoCraft.app")" != "0.3.0"
"$cli" uninstall photocraft --yes
test -z "$(ls "$CRAFTSPACE_MAC_APPLICATIONS" | grep PhotoCraft || true)"
export CRAFTSPACE_HOME="$RUNNER_TEMP/cs"

# ArtCraft (a Tauri app with its own release pipeline).
"$cli" install artcraft
test -d "$(installed artcraft executable)"
"$cli" uninstall artcraft --yes

# Fonts.
export CRAFTSPACE_FONTS_DIR="$RUNNER_TEMP/Fonts"
"$cli" fonts install "Noto Sans Arabic"
test -f "$CRAFTSPACE_FONTS_DIR/NotoSansArabic.ttf"
"$cli" fonts uninstall
"$cli" autostart on
test -f "$HOME/Library/LaunchAgents/io.github.emircesur.craftspace.plist"
"$cli" autostart off
echo "macOS end-to-end checks passed"
