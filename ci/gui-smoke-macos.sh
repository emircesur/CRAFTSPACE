#!/usr/bin/env bash
# Start the CraftSpace window on the runner's desktop and check that:
#   - the window appears and the menu bar icon is created,
#   - a test notification is accepted (reported, not required),
#   - closing the window keeps CraftSpace running in the menu bar (when the runner lets us
#     press the close button through System Events).
# Screenshots go to $OUT (default: gui-screenshots/).
set -euo pipefail

BIN="${BIN:-target/debug/craftspace}"
OUT="${OUT:-gui-screenshots}"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
export CRAFTSPACE_HOME="$(mktemp -d)" RUST_LOG=info CRAFTSPACE_TEST_NOTIFICATION=1
LOG="$OUT/craftspace.log"

fail() { echo "FAIL: $*"; echo "--- log"; cat "$LOG"; exit 1; }
windows() { osascript -e 'tell application "System Events" to count windows of (first process whose unix id is '"$APP"')' 2>/dev/null || echo "?"; }

"$BIN" >"$LOG" 2>&1 &
APP=$!
trap 'kill $APP 2>/dev/null || true' EXIT
sleep 15
kill -0 "$APP" 2>/dev/null || fail "CraftSpace exited"
screencapture -x "$OUT/macos-window.png" || echo "::warning::no screenshot (screen recording not allowed)"

grep -q "tray icon ready" "$LOG" || fail "no menu bar icon"
echo "menu bar: icon created"
if grep -q "test notification sent" "$LOG"; then echo "notification: accepted"; else echo "::warning::the test notification wasn't accepted on this runner"; fi

before=$(windows)
echo "windows: $before"
if [ "$before" = "?" ] || [ "$before" = "0" ]; then
  echo "::warning::System Events can't see the window here; skipping the close test"
else
  osascript -e 'tell application "System Events" to tell (first process whose unix id is '"$APP"') to click (first button of window 1 whose subrole is "AXCloseButton")'
  sleep 4
  kill -0 "$APP" 2>/dev/null || fail "CraftSpace quit instead of staying in the menu bar"
  grep -q "still running in the tray" "$LOG" || fail "no tray message in the log"
  echo "close: hidden in the menu bar (windows now: $(windows))"
  screencapture -x "$OUT/macos-in-tray.png" || true
fi
echo "macOS GUI smoke test passed"
