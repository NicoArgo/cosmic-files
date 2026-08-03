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

echo "==> Done."
echo "    Note: file-manager windows already open keep the OLD binary. Close all"
echo "    cosmic-files windows and reopen to see the hover thumbnail peek."
echo "    (No process is killed here, so your open windows are left untouched.)"
