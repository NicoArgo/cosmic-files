#!/usr/bin/env bash
# Build and install POP Flow's cosmic-files (hover thumbnail peek) over the
# system cosmic-files. Reversible with ./uninstall.sh
set -euo pipefail
cd "$(dirname "$0")"

echo "==> Building (cargo build --release)..."
cargo build --release

BIN="target/release/cosmic-files"
[ -f "$BIN" ] || { echo "Build failed: $BIN not found"; exit 1; }

if [ ! -f cosmic-files.orig ] && [ -f /usr/bin/cosmic-files ]; then
    echo "==> Backing up current /usr/bin/cosmic-files -> ./cosmic-files.orig"
    cp /usr/bin/cosmic-files cosmic-files.orig
fi

echo "==> Installing to /usr/bin/cosmic-files (needs sudo)..."
sudo install -m 0755 "$BIN" /usr/bin/cosmic-files

# Keep the auto-reapply golden copy in sync, or say out loud that this install
# is temporary — silence here used to hide the fact that a package update wipes
# the feature.
GOLDEN=/usr/local/lib/pop-flow/cosmic-files
if [ -f "$GOLDEN" ]; then
    echo "==> Refreshing auto-reapply golden copy"
    sudo install -m 0755 "$BIN" "$GOLDEN"
else
    echo "!! No auto-reapply hook installed: the next package update of"
    echo "   cosmic-files will silently restore the stock binary."
    echo "   Run ./setup-auto-reapply.sh to make this install stick."
fi

# --- desktop icons (cosmic-files-applet) ------------------------------------
# The desktop is drawn by a separate binary of this same crate. Without it the
# desktop keeps the stock build and none of this fork reaches it (fixed desktop
# columns + scroll, hover peek, folder colors).
echo
echo "==> Building (cargo build --release -p cosmic-files-applet)..."
cargo build --release -p cosmic-files-applet

APPLET_BIN="target/release/cosmic-files-applet"
[ -f "$APPLET_BIN" ] || { echo "Build failed: $APPLET_BIN not found"; exit 1; }

if [ ! -f cosmic-files-applet.orig ] && [ -f /usr/bin/cosmic-files-applet ]; then
    echo "==> Backing up current /usr/bin/cosmic-files-applet -> ./cosmic-files-applet.orig"
    cp /usr/bin/cosmic-files-applet cosmic-files-applet.orig
fi

echo "==> Installing to /usr/bin/cosmic-files-applet (needs sudo)..."
sudo install -m 0755 "$APPLET_BIN" /usr/bin/cosmic-files-applet

APPLET_GOLDEN=/usr/local/lib/pop-flow/cosmic-files-applet
if [ -f "$APPLET_GOLDEN" ]; then
    sudo install -m 0755 "$APPLET_BIN" "$APPLET_GOLDEN"
elif [ -f "$GOLDEN" ]; then
    echo "!! The desktop has no auto-reapply hook yet; run ./setup-auto-reapply.sh"
    echo "   again so a package update can't silently restore the stock desktop."
fi

# -f with an anchored pattern, not -x: the kernel truncates process names to 15
# characters, so `pkill -x cosmic-files-applet` never matches anything.
# cosmic-session respawns it at once, now from the new binary. Only the desktop
# icons blink; no window is closed.
echo "==> Reloading the desktop icons..."
pkill -f '^(/usr/bin/)?cosmic-files-applet( |$)' 2>/dev/null || true

echo "==> Done."
echo "    Note: file-manager windows already open keep the OLD binary. Close all"
echo "    cosmic-files windows and reopen to see the hover thumbnail peek."
echo "    (No process is killed here, so your open windows are left untouched.)"
