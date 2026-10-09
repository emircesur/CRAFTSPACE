#!/bin/sh
# Install CraftSpace for the current user on Linux:
#   curl -fsSL https://raw.githubusercontent.com/emircesur/craftspace/HEAD/install.sh | sh
# Downloads the latest release, checks its SHA-256, and runs `craftspace-cli self-install`,
# which puts CraftSpace in ~/.local/share/craftspace, adds it to the app menu and links
# `craftspace` and `craftspace-cli` into ~/.local/bin.
set -eu

repo="emircesur/craftspace"
case "$(uname -m)" in
    x86_64 | amd64) arch="x86_64" ;;
    aarch64 | arm64) arch="aarch64" ;;
    *) echo "CraftSpace has no build for $(uname -m) yet." >&2; exit 1 ;;
esac
[ "$(uname -s)" = "Linux" ] || { echo "This script is for Linux. On Windows, download the portable zip from https://github.com/$repo/releases" >&2; exit 1; }

# The latest tag, from the redirect GitHub serves for "latest" downloads (no API rate limit).
location=$(curl -fsSI "https://github.com/$repo/releases/latest/download/SHA256SUMS.txt" | tr -d '\r' | sed -n 's/^[Ll]ocation: //p' | tail -n1)
tag=$(printf '%s' "$location" | sed -n 's|.*/releases/download/\([^/]*\)/.*|\1|p')
[ -n "$tag" ] || { echo "Couldn't find the latest CraftSpace release." >&2; exit 1; }
version=${tag#v}
asset="craftspace-$version-linux-$arch.tar.gz"
base="https://github.com/$repo/releases/download/$tag"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
echo "Downloading CraftSpace $version…"
curl -fL --progress-bar -o "$tmp/$asset" "$base/$asset"
curl -fsSL -o "$tmp/SHA256SUMS.txt" "$base/SHA256SUMS.txt"
expected=$(grep " \*\{0,1\}$asset\$" "$tmp/SHA256SUMS.txt" | cut -d' ' -f1)
actual=$(sha256sum "$tmp/$asset" | cut -d' ' -f1)
[ -n "$expected" ] && [ "$expected" = "$actual" ] || { echo "Checksum mismatch for $asset" >&2; exit 1; }

tar -xzf "$tmp/$asset" -C "$tmp"
"$tmp/craftspace-$version-linux-$arch/bin/craftspace-cli" self-install
echo "Done. Start CraftSpace from your app menu, or run: craftspace"
