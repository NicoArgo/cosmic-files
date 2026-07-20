#!/usr/bin/env bash
# Restore the original system cosmic-files (undo install.sh).
set -euo pipefail
cd "$(dirname "$0")"

[ -f cosmic-files.orig ] || { echo "No backup (cosmic-files.orig) found."; exit 1; }

# Remove the auto-reapply golden copy if one was placed for cosmic-files.
if [ -f /usr/local/lib/pop-flow/cosmic-files ]; then
    echo "==> Removing auto-reapply golden copy (needs sudo)..."
    sudo rm -f /usr/local/lib/pop-flow/cosmic-files
fi

echo "==> Restoring original /usr/bin/cosmic-files (needs sudo)..."
sudo install -m 0755 cosmic-files.orig /usr/bin/cosmic-files

echo "==> Restored. Close and reopen cosmic-files windows to load the stock binary."
