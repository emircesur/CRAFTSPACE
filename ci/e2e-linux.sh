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
"$cli" update photocraft
test "$(installed photocraft version)" != "0.3.0"
"$XDG_BIN_HOME/photocraft-cli" --version
"$cli" rollback photocraft
test "$(installed photocraft version)" = "0.3.0"
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

# Start at login, self-install.
"$cli" autostart on
test -f "$XDG_CONFIG_HOME/autostart/craftspace.desktop"
"$cli" autostart off
test ! -f "$XDG_CONFIG_HOME/autostart/craftspace.desktop"
"$cli" self-install
"$XDG_BIN_HOME/craftspace-cli" --version
"$cli" news | head
echo "Linux end-to-end checks passed"
