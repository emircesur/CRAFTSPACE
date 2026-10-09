#!/usr/bin/env bash
# Opens a pull request on microsoft/winget-pkgs with CraftSpace's manifests for one version, from
# the token owner's fork (created if needed). Run by the release workflow with GH_TOKEN set to a
# token that may fork and open pull requests (classic token with `public_repo`).
#
#   packaging/winget/submit.sh 0.1.3 winget/
set -euo pipefail
version="$1"
dir="$2"
id="emircesur.CraftSpace"
upstream="microsoft/winget-pkgs"
path="manifests/e/emircesur/CraftSpace/$version"

if gh api "repos/$upstream/contents/$path" --silent 2>/dev/null; then
  echo "$id $version is already in winget"
  exit 0
fi
open="$(gh pr list --repo "$upstream" --state open --search "$id $version in:title" --json url --jq '.[0].url // empty')"
if [ -n "$open" ]; then
  echo "$id $version is already waiting for review: $open"
  exit 0
fi
if gh api "repos/$upstream/contents/manifests/e/emircesur/CraftSpace" --silent 2>/dev/null; then
  title="New version: $id version $version"
else
  title="New package: $id version $version"
fi

user="$(gh api user --jq .login)"
gh repo fork "$upstream" --clone=false --default-branch-only >/dev/null 2>&1 || true
for _ in $(seq 30); do
  gh api "repos/$user/winget-pkgs" --silent 2>/dev/null && break
  sleep 10
done
# Start from winget-pkgs as it is now.
gh api -X POST "repos/$user/winget-pkgs/merge-upstream" -f branch=master --silent
sha="$(gh api "repos/$user/winget-pkgs/git/ref/heads/master" --jq .object.sha)"
branch="craftspace-$version-$(date +%s)"
gh api -X POST "repos/$user/winget-pkgs/git/refs" -f ref="refs/heads/$branch" -f sha="$sha" --silent
for file in "$dir"/*.yaml; do
  gh api -X PUT "repos/$user/winget-pkgs/contents/$path/$(basename "$file")" \
    -f message="$title" -f branch="$branch" -f content="$(base64 -w0 "$file")" --silent
done

body="$(cat <<EOF
CraftSpace $version: https://github.com/emircesur/CRAFTSPACE/releases/tag/v$version

- [x] Have you checked that there aren't other open [pull requests](https://github.com/microsoft/winget-pkgs/pulls) for the same manifest update/change?
- [x] This PR only modifies one (1) manifest
- [x] Have you validated your manifest locally with \`winget validate --manifest <path>\`? (checked against the 1.9.0 schemas)
- [x] Have you tested your manifest locally with \`winget install --manifest <path>\`? (the release build installs and uninstalls the setup silently)
- [x] Does your manifest conform to the [1.9 schema](https://github.com/microsoft/winget-pkgs/tree/master/doc/manifest/schema/1.9.0)?
EOF
)"
gh pr create --repo "$upstream" --base master --head "$user:$branch" --title "$title" --body "$body"
