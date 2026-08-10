#!/usr/bin/env bash
# Restore the original system cosmic-files (undo install.sh).
set -euo pipefail
cd "$(dirname "$0")"

[ -f cosmic-files.orig ] || { echo "No backup (cosmic-files.orig) found."; exit 1; }

# Turn off auto-reapply first, or the next package operation would re-patch the
# binary right after we restore the original. Delegating to the script that owns
# those paths rather than repeating them: this used to remove only the golden
# copy, leaving a root-owned APT hook behind for good.
if [ -x ./remove-auto-reapply.sh ]; then
    ./remove-auto-reapply.sh
fi

echo "==> Restoring original /usr/bin/cosmic-files (needs sudo)..."
sudo install -m 0755 cosmic-files.orig /usr/bin/cosmic-files

echo "==> Restored. Close and reopen cosmic-files windows to load the stock binary."
