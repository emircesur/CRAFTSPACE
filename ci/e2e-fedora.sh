#!/usr/bin/env bash
# Fedora: CraftSpace installed from its RPM manages the ArtCraft apps as RPMs through dnf.
set -euxo pipefail
export CRAFTSPACE_HOME=/tmp/cs
cli=craftspace-cli
rpm -q craftspace
$cli --version
# Installed from a package: dnf updates CraftSpace, not CraftSpace itself.
$cli self-update | grep -q "up to date"

$cli config prefer_system_installer true
$cli install photocraft --version 0.3.0
rpm -q photocraft | grep -q "0.3.0"
command -v photocraft
test "$(python3 -c "import json; print(json.load(open('$CRAFTSPACE_HOME/installed.json'))['apps']['photocraft']['current']['kind'])")" = rpm
$cli verify photocraft
$cli update photocraft
rpm -q photocraft
! rpm -q photocraft | grep -q "0.3.0"
# Going back to an older version.
$cli install photocraft --version 0.3.0
rpm -q photocraft | grep -q "0.3.0"
$cli uninstall photocraft --yes
! rpm -q photocraft

# Switching an app from the RPM to CraftSpace's own copy removes the RPM.
$cli install gridcraft
rpm -q gridcraft
$cli config prefer_system_installer false
$cli install gridcraft --version "$(rpm -q --qf '%{VERSION}' gridcraft)"
! rpm -q gridcraft
test -x "$CRAFTSPACE_HOME/apps/gridcraft/"*/bin/gridcraft
$cli uninstall gridcraft --yes
echo "Fedora end-to-end checks passed"
