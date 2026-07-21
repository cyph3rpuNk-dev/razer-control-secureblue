#!/usr/bin/env bash
# One-command repo health check.  Mirrors CI (.github/workflows/ci.yml): the
# same formatting, lint, and test gates a pull request has to pass, runnable
# locally before you push.  Runs every step even if an earlier one fails, then
# exits non-zero if any failed so you see the full picture in one pass.
#
#   ./scripts/check.sh
#
# Requires the clippy component (rustup: `rustup component add clippy`;
# Fedora: `sudo dnf install clippy`).  Two checks are optional and skipped
# with a note when the tool is absent:
#   cargo machete  -> unused-dependency scan  (cargo install cargo-machete)
#   cargo audit    -> RUSTSEC advisory scan    (cargo install cargo-audit)
set -u

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT" || exit 1

FAIL=0
step() {
  local label="$1"; shift
  printf '\n== %s ==\n' "$label"
  if "$@"; then
    printf 'PASS  %s\n' "$label"
  else
    printf 'FAIL  %s\n' "$label"
    FAIL=$((FAIL + 1))
  fi
}

step "cargo fmt --check" \
  cargo fmt --all --check

step "cargo clippy (all targets, all features, warnings = errors)" \
  cargo clippy --workspace --all-targets --all-features -- -Dwarnings

step "cargo test (workspace, dry-run default)" \
  cargo test --locked --workspace

step "cargo test (hidraw backend compiled in)" \
  cargo test --locked -p razer-control-secureblue --features hidraw-backend

if cargo machete --help >/dev/null 2>&1; then
  step "cargo machete (unused dependencies)" \
    cargo machete
else
  printf '\nnote  cargo machete not installed; skipping unused-dependency scan\n'
fi

if cargo audit --version >/dev/null 2>&1; then
  step "cargo audit (RUSTSEC advisories)" \
    cargo audit
else
  printf '\nnote  cargo audit not installed; skipping advisory scan\n'
fi

printf '\n'
if [ "$FAIL" -eq 0 ]; then
  echo "All required checks passed."
else
  echo "$FAIL check(s) failed."
fi
exit "$FAIL"
