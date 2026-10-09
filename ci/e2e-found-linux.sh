#!/usr/bin/env bash
# Apps installed without CraftSpace on Linux: a .deb installed with apt, and a Flatpak bundle.
# CraftSpace finds them, keeps the deb up to date through apt, and leaves the Flatpak to Flatpak.
set -euxo pipefail
cli="$PWD/target/debug/craftspace-cli"
field() { python3 -c "import json; d=json.load(open('$CRAFTSPACE_HOME/installed.json'))['apps']['$1']['current']; print(d$2)"; }

# A .deb installed with apt.
export CRAFTSPACE_HOME="$RUNNER_TEMP/cs-deb"
curl -fsSL -o "$RUNNER_TEMP/photocraft.deb" \
  https://github.com/storytold/photocraft/releases/download/v0.3.0/photocraft-0.3.0-linux-x86_64.deb
sudo apt-get install -y "$RUNNER_TEMP/photocraft.deb"
"$cli" detect | tee "$RUNNER_TEMP/found-deb.txt"
grep -q "Found PhotoCraft 0.3.0" "$RUNNER_TEMP/found-deb.txt"
test "$(field photocraft "['kind']")" = deb
test "$(field photocraft "['system_package']")" = photocraft
"$cli" verify photocraft
# Updates go through apt (run as root here, as pkexec would on a desktop).
sudo -E "$cli" update photocraft
dpkg-query -W -f '${Version}\n' photocraft
! dpkg-query -W -f '${Version}' photocraft | grep -q '^0\.3\.0'
test "$(field photocraft "['kind']")" = deb
sudo -E "$cli" uninstall photocraft --yes
! dpkg -s photocraft >/dev/null 2>&1

# A Flatpak, installed from the release's bundle (its runtime comes from Flathub).
export CRAFTSPACE_HOME="$RUNNER_TEMP/cs-flatpak"
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
curl -fsSL -o "$RUNNER_TEMP/cadcraft.flatpak" \
  https://github.com/storytold/cadcraft/releases/download/v0.3.0/cadcraft-0.3.0-linux-x86_64.flatpak
flatpak install --user -y --noninteractive "$RUNNER_TEMP/cadcraft.flatpak"
flatpak list --app --columns=application,version,name
"$cli" detect | tee "$RUNNER_TEMP/found-flatpak.txt"
grep -q "Found CADCraft" "$RUNNER_TEMP/found-flatpak.txt"
test "$(field cadcraft "['external']['flatpak']")" = ai.storyteller.cadcraft
# Flatpak keeps it up to date, not CraftSpace.
"$cli" list | grep cadcraft
if "$cli" repair cadcraft 2>"$RUNNER_TEMP/repair.txt"; then echo "repair should be left to Flatpak"; exit 1; fi
grep -qi flatpak "$RUNNER_TEMP/repair.txt"
"$cli" uninstall cadcraft --yes
! flatpak info --user ai.storyteller.cadcraft >/dev/null 2>&1
echo "Found-apps checks passed (deb, Flatpak)"
