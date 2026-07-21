#!/usr/bin/env bash
# Build-artifact hygiene.  Two tiers, deliberately separated:
#
#   ./scripts/clean.sh          # SAFE: cargo clean — removes ./target only
#   ./scripts/clean.sh --deep   # DANGEROUS: also git clean -fdx
#
# The default removes the Cargo build directory (./target) and nothing else;
# it only ever costs you a rebuild.
#
# --deep additionally runs `git clean -fdx`, which permanently deletes EVERY
# untracked and git-ignored file in the tree — including anything not yet
# committed.  It requires typing `yes` at the prompt.  There is no undo.
set -u

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT" || exit 1

echo "== cargo clean (safe: removes ./target) =="
cargo clean

if [ "${1:-}" = "--deep" ]; then
  echo
  echo "== git clean -fdx (DANGEROUS) =="
  echo "This will PERMANENTLY delete all untracked and ignored files:"
  git clean -fdxn
  echo
  printf 'Type "yes" to delete the files listed above: '
  read -r reply
  if [ "$reply" = "yes" ]; then
    git clean -fdx
    echo "Done."
  else
    echo "Aborted; nothing deleted."
  fi
fi
