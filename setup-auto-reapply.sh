#!/usr/bin/env bash
# Make POP Flow's cosmic-files survive system/package updates.
#
# A package update overwrites /usr/bin/cosmic-files and /usr/bin/cosmic-files-applet
# (the desktop icons) with the stock binaries.
# This installs an APT/dpkg post-invoke hook that runs (as root, no password)
# after every package operation and reinstalls our build whenever the on-disk
# binary no longer matches our "golden" copy.
#
# One-time setup; needs sudo. Undo with ./remove-auto-reapply.sh
set -euo pipefail
cd "$(dirname "$0")"

# --- component-specific settings ------------------------------------------
# One entry per binary this fork replaces, both owned by the cosmic-files package:
#   name under /usr/bin | our build | how to reload it after reapplying
#  - cosmic-files is not session-managed; the user reopens its windows.
#  - cosmic-files-applet draws the desktop and is respawned by cosmic-session.
COMPONENTS=(
    "cosmic-files|target/release/cosmic-files|:"
    "cosmic-files-applet|target/release/cosmic-files-applet|pkill -x cosmic-files-applet 2>/dev/null || true"
)
PKG=cosmic-files
# --------------------------------------------------------------------------

# Body left unindented: it writes heredocs whose terminators must start the line.
setup_one() {
local COMP=$1 BUILT=$2 RELOAD=$3
LIBDIR=/usr/local/lib/pop-flow
GOLDEN="$LIBDIR/$COMP"
REAPPLY="$LIBDIR/reapply-$COMP"
HOOK="/etc/apt/apt.conf.d/99-pop-flow-$COMP"

[ -f "$BUILT" ] || { echo "Build first: ./install.sh (or cargo build --release)"; exit 1; }

echo "==> Installing golden copy + reapply hook for $COMP (needs sudo)..."
sudo install -d -m 0755 "$LIBDIR"
sudo install -m 0755 "$BUILT" "$GOLDEN"

# The reapplier: reinstall our binary if the system one drifted (or vanished).
# `cmp` follows symlinks, so a package that restores the target as a symlink to
# a stock binary also counts as drift. `install` unlinks the destination first,
# so replacing a symlink leaves whatever it pointed at untouched.
sudo tee "$REAPPLY" >/dev/null <<EOS
#!/usr/bin/env bash
set -e
GOLDEN=$GOLDEN
TARGET=/usr/bin/$COMP
PKG=$PKG
[ -f "\$GOLDEN" ] || exit 0
# Survive an update, not a removal. With the package gone the user asked
# for this program to be gone, and putting the binary back would leave a
# file no package owns.
dpkg-query -W -f='\${Status}' "\$PKG" 2>/dev/null | grep -q '^install ok installed$' || exit 0
if [ ! -f "\$TARGET" ] || [ -L "\$TARGET" ] || ! cmp -s "\$GOLDEN" "\$TARGET"; then
    install -m 0755 -o root -g root "\$GOLDEN" "\$TARGET"
    command -v logger >/dev/null 2>&1 && logger -t pop-flow "reapplied $COMP after change"
    $RELOAD
fi
EOS
sudo chmod 0755 "$REAPPLY"

# The hook. `|| true` guarantees a failing reapplier can never break apt.
sudo tee "$HOOK" >/dev/null <<EOS
// POP Flow: reapply our $COMP after any package operation that overwrites
// /usr/bin/$COMP. Remove with ./remove-auto-reapply.sh
DPkg::Post-Invoke { "$REAPPLY || true"; };
EOS

echo "==> Done. POP Flow's $COMP will be reapplied automatically after updates."
echo "    golden: $GOLDEN"
echo "    hook:   $HOOK"
}

for entry in "${COMPONENTS[@]}"; do
    IFS='|' read -r comp built reload <<<"$entry"
    setup_one "$comp" "$built" "$reload"
done
