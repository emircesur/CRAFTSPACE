#!/usr/bin/env bash
# Start the CraftSpace window on a virtual X desktop with a status-notifier tray (xfce4-panel)
# and a notification service (dunst), then check that:
#   - the tray icon registers, even when the panel starts after CraftSpace (as at login),
#   - a test notification is accepted,
#   - closing the window keeps CraftSpace running in the tray,
#   - activating the tray icon brings the window back.
# Screenshots go to $OUT (default: gui-screenshots/).
set -euo pipefail

BIN="${BIN:-target/debug/craftspace}"
OUT="${OUT:-gui-screenshots}"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
WORK="$(mktemp -d)"
export HOME="$WORK/home" CRAFTSPACE_HOME="$WORK/craftspace"
export DISPLAY=:91 XDG_CURRENT_DESKTOP=XFCE
export RUST_LOG=info CRAFTSPACE_TEST_NOTIFICATION=1
LOG="$OUT/craftspace.log"

# The default Xfce panel layout, which has the system tray.
mkdir -p "$HOME/.config/xfce4/xfconf/xfce-perchannel-xml"
cp /etc/xdg/xfce4/panel/default.xml "$HOME/.config/xfce4/xfconf/xfce-perchannel-xml/xfce4-panel.xml"

Xvfb :91 -screen 0 1600x1000x24 >/dev/null 2>&1 &
sleep 2
eval "$(dbus-launch --sh-syntax)"
cleanup() { kill "${APP:-}" 2>/dev/null || true; kill "$(jobs -p)" 2>/dev/null || true; kill "$DBUS_SESSION_BUS_PID" 2>/dev/null || true; }
trap cleanup EXIT
dunst >/dev/null 2>&1 &

fail() { echo "FAIL: $*"; echo "--- log"; cat "$LOG"; exit 1; }
visible() { xdotool search --onlyvisible --name '^CraftSpace$' 2>/dev/null | head -1; }

"$BIN" >"$LOG" 2>&1 &
APP=$!
for _ in $(seq 30); do [ -n "$(visible)" ] && break; sleep 1; done
[ -n "$(visible)" ] || fail "the window didn't appear"
sleep 4

# The panel starts after CraftSpace; the icon should register once it's up.
grep -q "tray icon ready" "$LOG" || fail "no tray icon"
xfce4-panel >/dev/null 2>&1 &
SERVICE=
for _ in $(seq 20); do
  sleep 1
  items=$(dbus-send --session --print-reply --dest=org.kde.StatusNotifierWatcher /StatusNotifierWatcher \
    org.freedesktop.DBus.Properties.Get string:org.kde.StatusNotifierWatcher string:RegisteredStatusNotifierItems 2>/dev/null || true)
  SERVICE=$(sed -n 's/.*string "\([^/"]*\)\/StatusNotifierItem".*/\1/p' <<<"$items" | head -1)
  [ -n "$SERVICE" ] && break
done
[ -n "$SERVICE" ] || fail "the icon didn't register with the panel"
grep -q "tray icon shown by the panel" "$LOG" || fail "CraftSpace didn't notice the panel"
echo "tray item: $SERVICE"
sleep 2
import -window root "$OUT/linux-window.png"
grep -q "test notification sent" "$LOG" || fail "the test notification wasn't accepted"
echo "notification: accepted"

# Close the window as the window manager would.
python3 "$(dirname "$0")/x11-close-window.py" "$(visible)"
sleep 4
kill -0 "$APP" 2>/dev/null || fail "CraftSpace quit instead of staying in the tray"
[ -z "$(visible)" ] || fail "the window is still showing after closing it"
grep -q "still running in the tray" "$LOG" || fail "no tray message in the log"
echo "close: hidden in the tray"
import -window root "$OUT/linux-in-tray.png"

# Click the tray icon.
dbus-send --session --print-reply --dest="$SERVICE" /StatusNotifierItem org.kde.StatusNotifierItem.Activate int32:0 int32:0 >/dev/null
for _ in $(seq 10); do [ -n "$(visible)" ] && break; sleep 1; done
[ -n "$(visible)" ] || fail "the tray icon didn't bring the window back"
echo "tray: window shown again"
echo "Linux GUI smoke test passed"
